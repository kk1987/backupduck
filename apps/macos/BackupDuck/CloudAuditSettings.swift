import AppKit
import Photos
import SwiftUI

/// One Rust `cloud_audit` run. Counts are Google verdicts from that run.
struct CloudAuditResult: Decodable, Equatable {
  var checked = 0
  var found = 0
  var verified = 0
  /// Counted against quota, but another device uploaded the bytes first.
  var already_in_cloud = 0
  /// Genuine quota use by our uploads.
  var quota = 0
  var new_quota = 0
  var not_found = 0
  var unknown = 0
  var posted = 0
  var rejected = 0
  var dry_run = false
  var session_expired = false
  var stopped: String? = nil
}

/// One Rust `lock_hidden` run (Google Photos Locked Folder moves).
struct LockHiddenResult: Decodable, Equatable {
  var verified_hidden = 0
  var candidates = 0
  var deferred = 0
  var checked = 0
  var found = 0
  var moved = 0
  var failed = 0
  var not_in_library = 0
  var already_moved = 0
  var attempts_exhausted = 0
  var skipped_quota = 0
  var skipped_unverified = 0
  var skipped_no_sha1 = 0
  var dry_run = false
  var session_expired = false
  var stopped: String? = nil
  /// Hidden copies known to be in the Locked Folder after this run.
  var locked: Int { already_moved + (dry_run ? 0 : moved) }
}

struct CloudAuditStatus: Decodable, Equatable {
  var configured = false
  var last_run_ms: Int64? = nil
  var last_result: CloudAuditResult? = nil
  var session_expired = false
  var last_lock_run_ms: Int64? = nil
  var last_lock_result: LockHiddenResult? = nil
}

/// Settings keys. The cookies file is referenced only by a bookmark; its
/// contents never pass through Swift.
enum CloudAuditKeys {
  static let enabled = "cloudAuditEnabled"
  static let autoPause = "cloudAuditAutoPause"
  static let accountIndex = "cloudAuditAccountIndex"
  static let cookiesBookmark = "cloudAuditCookiesBookmark"
  static let readme = URL(string: "https://github.com/qhhonx/backupduck/blob/main/crates/cloud-audit/README.md")!
  /// Move hidden photos into the Locked Folder after they are verified.
  static let lockHidden = "cloudLockHidden"
  /// Account index for which the user confirmed Locked Folder backup is on.
  static let lockHiddenAccount = "cloudLockHiddenAccount"
  static let lockMaxItems = 200

  static var currentAccountIndex: Int {
    min(max(UserDefaults.standard.integer(forKey: accountIndex), 0), 9)
  }
  /// On and confirmed for the current account; a changed index needs a new confirmation.
  static var lockHiddenActive: Bool {
    let defaults = UserDefaults.standard
    return defaults.bool(forKey: lockHidden)
      && (defaults.object(forKey: lockHiddenAccount) as? Int ?? -1) == currentAccountIndex
  }
  /// Opening it asks for the account's fingerprint or password; only the user can check it.
  static func lockedFolderURL(_ index: Int) -> URL {
    URL(string: "https://photos.google.com/u/\(index)/lockedfolder")!
  }
}

extension BackupModel {
  private static let cloudAuditInterval: UInt64 = 600 * 1_000_000_000

  func startCloudAudit() {
    guard cloudAuditTask == nil else { return }
    cloudAuditTask = Task {
      await refreshCloudAuditStatus()
      while !Task.isCancelled {
        try? await Task.sleep(nanoseconds: Self.cloudAuditInterval)
        if ready, !paused, pairing != nil, UserDefaults.standard.bool(forKey: CloudAuditKeys.enabled) {
          await runCloudAudit(dryRun: false)
        }
      }
    }
  }

  nonisolated static func saveCloudCookies(_ url: URL) throws {
    let bookmark = try url.bookmarkData(options: [.withSecurityScope, .securityScopeAllowOnlyReadAccess],
      includingResourceValuesForKeys: nil, relativeTo: nil)
    UserDefaults.standard.set(bookmark, forKey: CloudAuditKeys.cookiesBookmark)
  }

  /// Resolves the bookmark and keeps the file accessible for `body` only.
  private func withCloudCookies<T>(_ body: (URL?) async -> T) async -> T {
    guard let bookmark = UserDefaults.standard.data(forKey: CloudAuditKeys.cookiesBookmark),
      !bookmark.isEmpty else { return await body(nil) }
    var stale = false
    guard let url = try? URL(resolvingBookmarkData: bookmark,
      options: [.withSecurityScope, .withoutUI, .withoutMounting], relativeTo: nil,
      bookmarkDataIsStale: &stale) else { return await body(nil) }
    let accessing = url.startAccessingSecurityScopedResource()
    defer { if accessing { url.stopAccessingSecurityScopedResource() } }
    if stale { try? Self.saveCloudCookies(url) }
    return await body(url)
  }

  func refreshCloudAuditStatus() async {
    guard ready, let pairing else {
      cloudAudit = nil
      return
    }
    let data: Data? = await withCloudCookies { cookies in
      var command: [String: Any] = ["receiver_id": pairing.receiverID]
      if let cookies { command["cookies_path"] = cookies.path }
      return try? await Bridge.call(["op": "cloud_audit", "command": ["status": command]])
    }
    guard pairing.receiverID == self.pairing?.receiverID else { return }
    cloudAudit = data.flatMap { try? JSONDecoder().decode(CloudAuditStatus.self, from: $0) }
  }

  func runCloudAudit(dryRun: Bool) async {
    guard ready, let pairing, !cloudAuditRunning else { return }
    cloudAuditRunning = true
    defer { cloudAuditRunning = false }
    let outcome: Result<Data, Error> = await withCloudCookies { cookies in
      guard let cookies else { return .failure(Bridge.Failure(code: "cloud_cookies_invalid")) }
      do {
        return .success(try await Bridge.call(["op": "cloud_audit", "command": ["run": [
          "pairing": try JSONSerialization.jsonObject(with: JSONEncoder().encode(pairing)),
          "cookies_path": cookies.path,
          "account_index": CloudAuditKeys.currentAccountIndex,
          "dry_run": dryRun, "max_items": 600,
        ]]]))
      } catch { return .failure(error) }
    }
    var lockAfterwards = false
    switch outcome {
    case .success(let data):
      cloudAuditError = nil
      let result = try? JSONDecoder().decode(CloudAuditResult.self, from: data)
      if let result, result.quota > 0 {
        await handleCloudQuota(result, dryRun: dryRun)
      }
      lockAfterwards = !dryRun && result?.session_expired == false && CloudAuditKeys.lockHiddenActive
    case .failure(let error):
      cloudAuditError = (error as? Bridge.Failure)?.code ?? "internal"
    }
    if lockAfterwards { await runLockHidden(dryRun: false) }
    await refreshCloudAuditStatus()
    await refresh()
  }

  /// PhotoKit identifiers of the Hidden album, or nil without full Photos
  /// access. A locked Hidden album (Touch ID or password) reads as empty.
  nonisolated static func hiddenSourceIDs() async -> [String]? {
    guard PHPhotoLibrary.authorizationStatus(for: .readWrite) == .authorized else { return nil }
    return await Task.detached(priority: .utility) {
      guard let album = PHAssetCollection.fetchAssetCollections(
        with: .smartAlbum, subtype: .smartAlbumAllHidden, options: nil).firstObject
      else { return [] }
      let options = PHFetchOptions()
      options.includeHiddenAssets = true
      options.includeAllBurstAssets = true
      var ids: [String] = []
      PHAsset.fetchAssets(in: album, options: options).enumerateObjects { asset, _, _ in
        ids.append(asset.localIdentifier)
      }
      return ids
    }.value
  }

  /// Moves cloud-verified hidden photos into the Google Photos Locked Folder.
  /// A real run needs the confirmed setting; a dry run only looks them up.
  func runLockHidden(dryRun: Bool) async {
    guard ready, let pairing, !lockHiddenRunning, dryRun || CloudAuditKeys.lockHiddenActive else { return }
    lockHiddenRunning = true
    defer { lockHiddenRunning = false }
    guard let ids = await Self.hiddenSourceIDs() else {
      lockHiddenError = "photos_access"
      return
    }
    hiddenVisible = ids.count
    // Zero usually means the Hidden album is locked; never send an empty list.
    guard !ids.isEmpty else {
      lockHiddenError = nil
      return
    }
    let outcome: Result<Data, Error> = await withCloudCookies { cookies in
      guard let cookies else { return .failure(Bridge.Failure(code: "cloud_cookies_invalid")) }
      do {
        return .success(try await Bridge.call(["op": "cloud_audit", "command": ["lock_hidden": [
          "pairing": try JSONSerialization.jsonObject(with: JSONEncoder().encode(pairing)),
          "cookies_path": cookies.path,
          "account_index": CloudAuditKeys.currentAccountIndex,
          "source_ids": ids, "dry_run": dryRun, "max_items": CloudAuditKeys.lockMaxItems,
        ]]]))
      } catch { return .failure(error) }
    }
    switch outcome {
    case .success: lockHiddenError = nil
    case .failure(let error): lockHiddenError = (error as? Bridge.Failure)?.code ?? "internal"
    }
    await refreshCloudAuditStatus()
    await refresh()
  }

  /// Copies that count against Google storage defeat the point of the Pixel
  /// relay. Only newly found ones pause, so a resume is not undone by re-checks.
  private func handleCloudQuota(_ result: CloudAuditResult, dryRun: Bool) async {
    let autoPause = UserDefaults.standard.object(forKey: CloudAuditKeys.autoPause) as? Bool ?? true
    if !dryRun, result.new_quota > 0, !paused, autoPause {
      await setPaused(true)
      _ = try? await Bridge.call(["op": "record_event", "receiver": false, "code": "cloud_quota_pause"])
      message = String(format: NSLocalizedString("cloud_quota_pause", comment: ""), result.new_quota)
    } else {
      message = String(format: NSLocalizedString("cloud_quota_warning", comment: ""), result.quota)
    }
  }
}

struct CloudAuditSettings: View {
  @ObservedObject var model: BackupModel
  @AppStorage(CloudAuditKeys.enabled) private var enabled = false
  @AppStorage(CloudAuditKeys.autoPause) private var autoPause = true
  @AppStorage(CloudAuditKeys.accountIndex) private var accountIndex = 0
  @AppStorage(CloudAuditKeys.cookiesBookmark) private var bookmark = Data()
  @AppStorage(CloudAuditKeys.lockHidden) private var lockHidden = false
  @AppStorage(CloudAuditKeys.lockHiddenAccount) private var lockHiddenAccount = -1
  @State private var chooseError: String?
  @State private var confirmingLock = false

  private var lockActive: Bool { lockHidden && lockHiddenAccount == accountIndex }

  private var lockStatus: String? {
    guard let result = model.cloudAudit?.last_lock_result else { return nil }
    return String(format: NSLocalizedString("cloud_lock_hidden_status", comment: ""),
      result.locked, result.verified_hidden)
  }

  private var lockLastRun: String? {
    guard let status = model.cloudAudit, let at = status.last_lock_run_ms,
      let result = status.last_lock_result else { return nil }
    let date = Date(timeIntervalSince1970: Double(at) / 1000).formatted(date: .abbreviated, time: .shortened)
    var text = result.dry_run
      ? String(format: NSLocalizedString("cloud_lock_hidden_last_dry_run", comment: ""), date,
        result.found, result.not_in_library, result.skipped_quota, result.skipped_unverified)
      : String(format: NSLocalizedString("cloud_lock_hidden_last_run", comment: ""), date,
        result.moved, result.failed, result.not_in_library, result.skipped_quota, result.skipped_unverified)
    if result.deferred > 0 {
      text += " · " + String(format: NSLocalizedString("cloud_lock_hidden_deferred", comment: ""), result.deferred)
    }
    return text
  }

  private var lastRun: String? {
    guard let status = model.cloudAudit, let at = status.last_run_ms, let result = status.last_result else { return nil }
    let date = Date(timeIntervalSince1970: Double(at) / 1000).formatted(date: .abbreviated, time: .shortened)
    return String(format: NSLocalizedString("cloud_audit_last_run", comment: ""), date, result.checked,
      result.verified, result.already_in_cloud, result.quota, result.not_found, result.unknown)
      + (result.dry_run ? " · " + NSLocalizedString("cloud_audit_dry_run", comment: "") : "")
  }

  var body: some View {
    Form {
      Section {
        Text("cloud_audit_explanation").foregroundStyle(.secondary)
        Link("cloud_audit_readme", destination: CloudAuditKeys.readme)
      } header: { Text("cloud_audit_title") }
      Section {
        LabeledContent("cloud_audit_cookies") {
          Text(bookmark.isEmpty ? "cloud_audit_cookies_unset" : "cloud_audit_cookies_set")
            .foregroundStyle(.secondary)
        }
        Button("cloud_audit_choose_cookies", action: choose)
        if let chooseError { Text(chooseError).foregroundStyle(.orange) }
        Stepper(value: $accountIndex, in: 0...9) {
          LabeledContent("cloud_audit_account_index", value: accountIndex.formatted())
        }
        Toggle("cloud_audit_enabled", isOn: $enabled)
        Toggle("cloud_audit_auto_pause", isOn: $autoPause)
      }
      Section {
        HStack {
          Button("cloud_audit_check_now") { Task { await model.runCloudAudit(dryRun: false) } }
          Button("cloud_audit_dry_run") { Task { await model.runCloudAudit(dryRun: true) } }
          if model.cloudAuditRunning { ProgressView().controlSize(.small) }
        }.disabled(!model.ready || model.pairing == nil || bookmark.isEmpty || model.cloudAuditRunning)
        if model.pairing == nil {
          Label("mac_pair_first", systemImage: "externaldrive.badge.plus").foregroundStyle(.secondary)
        }
        if let lastRun { Text(lastRun).foregroundStyle(.secondary).monospacedDigit() }
        if model.cloudAudit?.session_expired == true || model.cloudAuditError == "cloud_session_expired" {
          Text("cloud_audit_session_expired").foregroundStyle(.red)
        } else if let error = model.cloudAuditError {
          Text(NSLocalizedString("error_" + error, comment: "")).foregroundStyle(.orange)
        }
      }
      lockHiddenSection
    }.formStyle(.grouped)
      .task(id: model.pairing?.receiverID) { await model.refreshCloudAuditStatus() }
      .sheet(isPresented: $confirmingLock) {
        LockHiddenConfirmation(accountIndex: accountIndex) {
          lockHiddenAccount = accountIndex
          lockHidden = true
        }
      }
  }

  @ViewBuilder private var lockHiddenSection: some View {
    Section {
      Text("cloud_lock_hidden_explanation").foregroundStyle(.secondary)
      Toggle("cloud_lock_hidden_enabled", isOn: Binding(get: { lockActive }, set: { on in
        if on { confirmingLock = true } else { lockHidden = false }
      }))
      if lockHidden && !lockActive {
        Text("cloud_lock_hidden_reconfirm").foregroundStyle(.orange)
      }
      HStack {
        Button("cloud_lock_hidden_now") { Task { await model.runLockHidden(dryRun: false) } }
          .disabled(!lockActive)
        Button("cloud_lock_hidden_dry_run") { Task { await model.runLockHidden(dryRun: true) } }
        if model.lockHiddenRunning { ProgressView().controlSize(.small) }
      }.disabled(!model.ready || model.pairing == nil || bookmark.isEmpty
        || model.lockHiddenRunning || model.cloudAuditRunning)
      if let lockStatus { Text(lockStatus).monospacedDigit() }
      if let lockLastRun { Text(lockLastRun).foregroundStyle(.secondary).monospacedDigit() }
      if model.hiddenVisible == 0 {
        Text("hidden_photos_locked_warning_mac").font(.callout).foregroundStyle(.orange)
      }
      if model.lockHiddenError == "cloud_session_expired"
        || model.cloudAudit?.last_lock_result?.session_expired == true {
        Text("cloud_audit_session_expired").foregroundStyle(.red)
      } else if let error = model.lockHiddenError {
        Text(NSLocalizedString(error == "photos_access" ? "photos_access_denied_mac" : "error_" + error,
          comment: "")).foregroundStyle(.orange)
      }
      Link(String(format: NSLocalizedString("cloud_lock_hidden_open", comment: ""), accountIndex),
        destination: CloudAuditKeys.lockedFolderURL(accountIndex))
      Text("cloud_lock_hidden_browser_note").font(.callout).foregroundStyle(.secondary)
    } header: { Text("cloud_lock_hidden_title") }
  }

  private func choose() {
    let panel = NSOpenPanel()
    panel.canChooseDirectories = false
    panel.canChooseFiles = true
    panel.allowsMultipleSelection = false
    panel.prompt = NSLocalizedString("cloud_audit_choose_cookies", comment: "")
    panel.begin { result in
      guard result == .OK, let url = panel.url else { return }
      do {
        try BackupModel.saveCloudCookies(url)
        bookmark = UserDefaults.standard.data(forKey: CloudAuditKeys.cookiesBookmark) ?? Data()
        chooseError = nil
        Task { await model.refreshCloudAuditStatus() }
      } catch { chooseError = NSLocalizedString("error_cloud_cookies_invalid", comment: "") }
    }
  }
}

/// Moving items into the Locked Folder is only safe when Locked Folder backup
/// is on: otherwise Google deletes the cloud copies of moved items.
private struct LockHiddenConfirmation: View {
  let accountIndex: Int
  let confirm: () -> Void
  @Environment(\.dismiss) private var dismiss
  @State private var acknowledged = false

  var body: some View {
    VStack(alignment: .leading, spacing: 14) {
      Text("cloud_lock_hidden_confirm_title").font(.headline)
      Text("cloud_lock_hidden_confirm_body").fixedSize(horizontal: false, vertical: true)
      Text("cloud_lock_hidden_confirm_warning").foregroundStyle(.orange)
        .fixedSize(horizontal: false, vertical: true)
      Toggle(isOn: $acknowledged) {
        Text(String(format: NSLocalizedString("cloud_lock_hidden_confirm_ack", comment: ""), accountIndex))
          .fixedSize(horizontal: false, vertical: true)
      }.toggleStyle(.checkbox)
      HStack {
        Spacer()
        Button("cancel") { dismiss() }.keyboardShortcut(.cancelAction)
        Button("cloud_lock_hidden_confirm_turn_on") {
          confirm()
          dismiss()
        }.keyboardShortcut(.defaultAction).disabled(!acknowledged)
      }
    }.padding(20).frame(width: 460)
  }
}
