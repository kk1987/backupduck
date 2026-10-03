import Foundation
import Photos

/// PhotoKit-specific discovery state. The token and pending native identifiers
/// are atomically persisted together before any originals are exported to Rust.
@MainActor final class PhotoLibraryChanges: NSObject, PHPhotoLibraryChangeObserver {
  struct Pending: Codable {
    let id: String
    var nextAttempt: Date = .distantPast
    var createdAtMS: Int64? = nil
    var attempts = 0
  }
  struct State: Codable {
    var enabled = false
    var receiverID: String?
    var token: Data?
    var closingToken: Data? = nil
    var pending: [Pending] = []
    var historyUnavailable = false
  }
  private let file: URL
  private(set) var state = State()
  private var scanning = false
  private var revision = 0
  init(root: URL) throws {
    file = root.appendingPathComponent("photo-library-changes.json")
    super.init()
    if FileManager.default.fileExists(atPath: file.path) {
      state = try JSONDecoder().decode(State.self, from: Data(contentsOf: file))
    }
    PHPhotoLibrary.shared().register(self)
  }
  deinit { PHPhotoLibrary.shared().unregisterChangeObserver(self) }
  private func persist(_ next: State) throws {
    #if os(iOS)
      let options: Data.WritingOptions = [
        .atomic, .completeFileProtectionUntilFirstUserAuthentication,
      ]
    #else
      let options: Data.WritingOptions = [.atomic]
    #endif
    try JSONEncoder().encode(next).write(to: file, options: options)
    state = next
  }
  func setEnabled(_ enabled: Bool, receiverID: String?) async throws {
    var next = state
    if next.receiverID != receiverID {
      next = State()
      next.receiverID = receiverID
      try persist(next)
      revision += 1
    }
    if next.enabled == enabled { return }
    if enabled {
      // Finish the interval that was enabled before resetting the baseline.
      if next.closingToken != nil { try await reconcileClosingWindow() }
      next = state
      // Start from now, including newly imported photos with old capture dates.
      next.token = try NSKeyedArchiver.archivedData(
        withRootObject: PHPhotoLibrary.shared().currentChangeToken, requiringSecureCoding: true)
      next.historyUnavailable = false
    } else if next.token != nil {
      next.closingToken = try NSKeyedArchiver.archivedData(
        withRootObject: PHPhotoLibrary.shared().currentChangeToken, requiringSecureCoding: true)
    }
    next.enabled = enabled
    try persist(next)
    revision += 1
  }
  func matchReceiver(_ receiverID: String?) throws {
    if state.receiverID != receiverID {
      var next = State()
      next.receiverID = receiverID
      try persist(next)
      revision += 1
    }
  }

  nonisolated static func changes(since saved: Data, through ending: Data? = nil)
    throws -> (Data?, Set<String>, Set<String>)
  {
    guard let start = try NSKeyedUnarchiver.unarchivedObject(
      ofClass: PHPersistentChangeToken.self, from: saved)
    else { throw Bridge.Failure(code: "history_unavailable") }
    let cutoff = try ending.flatMap {
      try NSKeyedUnarchiver.unarchivedObject(ofClass: PHPersistentChangeToken.self, from: $0)
    }
    if ending != nil && cutoff == nil { throw Bridge.Failure(code: "history_unavailable") }
    if let cutoff, start == cutoff { return (ending, [], []) }
    var latest: PHPersistentChangeToken?
    var inserted = Set<String>()
    var deleted = Set<String>()
    for change in try PHPhotoLibrary.shared().fetchPersistentChanges(since: start) {
      let details = try change.changeDetails(for: .asset)
      inserted.formUnion(details.insertedLocalIdentifiers)
      inserted.subtract(details.deletedLocalIdentifiers)
      deleted.formUnion(details.deletedLocalIdentifiers)
      deleted.subtract(details.insertedLocalIdentifiers)
      latest = change.changeToken
      if let cutoff, latest == cutoff { break }
    }
    if let cutoff, latest != cutoff { throw Bridge.Failure(code: "history_unavailable") }
    return (try latest.map {
      try NSKeyedArchiver.archivedData(withRootObject: $0, requiringSecureCoding: true)
    }, inserted, deleted)
  }
  private func reconcileClosingWindow() async throws {
    guard let saved = state.token, let cutoff = state.closingToken else { return }
    let version = revision
    let changes = try await Task.detached(priority: .utility) {
      try Self.changes(since: saved, through: cutoff)
    }.value
    guard version == revision, state.closingToken == cutoff else {
      throw Bridge.Failure(code: "conflict")
    }
    var next = state
    next.pending.removeAll { changes.2.contains($0.id) }
    let known = Set(next.pending.map(\.id))
    next.pending.append(contentsOf: changes.1.subtracting(known).map { Pending(id: $0) })
    next.token = cutoff
    next.closingToken = nil
    try persist(next)
  }
  static let attemptLimit = 5
  private static func handOff(_ source: String, receiver: String?, reason: String?) async -> Bool {
    guard let receiver else { return false }
    var result: [String: Any] = [
      "op": "source_result", "receiver_id": receiver, "source": source, "complete": false,
    ]
    if let reason { result["error"] = reason }
    do {
      _ = try await Bridge.call(["op": "schedule_sources", "receiver_id": receiver, "sources": [source]])
      _ = try await Bridge.call(result)
      return true
    } catch { return false }
  }
  nonisolated func photoLibraryDidChange(_ changeInstance: PHChange) {
    Task { @MainActor in await BackupModel.shared.discoverPhotos() }
  }
  func discoverAndExport(using model: BackupModel) async {
    guard (state.enabled || state.closingToken != nil || !state.pending.isEmpty),
      !scanning, !model.paused, !model.importing,
      model.pairing?.receiverID == state.receiverID
    else { return }
    let authorization = PHPhotoLibrary.authorizationStatus(for: .readWrite)
    guard authorization == .authorized || authorization == .limited else {
      model.message = NSLocalizedString("photos_permission_needed", comment: "")
      return
    }
    scanning = true
    defer {
      scanning = false
      model.updateDiscoveryStatus()
    }
    guard await model.canPrepareForReceiver() else { return }
    let version = revision
    do {
      if state.closingToken != nil { try await reconcileClosingWindow() }
      guard version == revision else { return }
      if state.enabled, !state.historyUnavailable, let savedToken = state.token {
        do {
          // PhotoKit fetches can block; keep change-history work off the UI thread.
          let changes = try await Task.detached(priority: .utility) {
            try Self.changes(since: savedToken)
          }.value
          guard version == revision, !Task.isCancelled else { return }
          if let token = changes.0 {
            var next = state
            next.pending.removeAll { changes.2.contains($0.id) }
            let known = Set(next.pending.map(\.id))
            next.pending.append(
              contentsOf: changes.1.subtracting(known).map { Pending(id: $0) })
            next.token = token
            try persist(next)
          }
        } catch {
          guard version == revision else { return }
          var next = state
          next.historyUnavailable = true
          try persist(next)
          model.message = NSLocalizedString("error_history_unavailable", comment: "")
        }
      }
      // Resolve new and legacy pending identifiers once; identifier order has
      // no relationship to capture time. Keep retry deadlines during reordering.
      let missingDates = state.pending.filter { $0.createdAtMS == nil }.map(\.id)
      if !missingDates.isEmpty {
        let dates = await PhotoBackupOrder.captureDates(missingDates)
        guard version == revision, !Task.isCancelled else { return }
        var next = state
        for index in next.pending.indices where next.pending[index].createdAtMS == nil {
          next.pending[index].createdAtMS = dates[next.pending[index].id] ?? Int64.min
        }
        next.pending.sort {
          PhotoBackupOrder.precedes($0.id, $0.createdAtMS ?? Int64.min, $1.id, $1.createdAtMS ?? Int64.min)
        }
        try persist(next)
      }
      // Small bounded batches. A crash before removing an identifier safely
      // re-exports it; the Rust asset identity prevents duplicate transfer.
      let due = state.pending.filter { $0.nextAttempt <= Date() }.prefix(5)
      for pending in due {
        guard version == revision, !model.paused, !Task.isCancelled else { break }
        let completed = await model.importAssets([pending.id], requestAuthorization: false)
        guard version == revision, !Task.isCancelled else { break }
        let attempts = (state.pending.first { $0.id == pending.id }?.attempts ?? 0) + 1
        // After the cap, the durable Rust queue owns the source: it keeps
        // environmental retries going and parks item-specific failures.
        var handedOff = false
        if !completed && attempts >= Self.attemptLimit {
          handedOff = await Self.handOff(
            pending.id, receiver: state.receiverID, reason: model.preparationReason)
        }
        guard version == revision, !Task.isCancelled else { break }
        var next = state
        if completed || handedOff {
          next.pending.removeAll { $0.id == pending.id }
        } else if let index = next.pending.firstIndex(where: { $0.id == pending.id }) {
          next.pending[index].nextAttempt = Date().addingTimeInterval(300)
          next.pending[index].attempts = attempts
        }
        try persist(next)
      }
    } catch { model.message = error.localizedDescription }
  }
}
extension PhotoLibraryChanges.Pending {
  // Files written before retry counting have no `attempts` key.
  init(from decoder: Decoder) throws {
    let values = try decoder.container(keyedBy: CodingKeys.self)
    id = try values.decode(String.self, forKey: .id)
    nextAttempt = try values.decodeIfPresent(Date.self, forKey: .nextAttempt) ?? .distantPast
    createdAtMS = try values.decodeIfPresent(Int64.self, forKey: .createdAtMS)
    attempts = try values.decodeIfPresent(Int.self, forKey: .attempts) ?? 0
  }
}
