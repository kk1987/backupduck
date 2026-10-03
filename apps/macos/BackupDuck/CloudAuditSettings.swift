import AppKit
import SwiftUI

/// One Rust `cloud_audit` run. Counts are Google verdicts from that run.
struct CloudAuditResult: Decodable, Equatable {
  var checked = 0
  var found = 0
  var verified = 0
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

struct CloudAuditStatus: Decodable, Equatable {
  var configured = false
  var last_run_ms: Int64? = nil
  var last_result: CloudAuditResult? = nil
  var session_expired = false
}

/// Settings keys. The cookies file is referenced only by a bookmark; its
/// contents never pass through Swift.
enum CloudAuditKeys {
  static let enabled = "cloudAuditEnabled"
  static let autoPause = "cloudAuditAutoPause"
  static let accountIndex = "cloudAuditAccountIndex"
  static let cookiesBookmark = "cloudAuditCookiesBookmark"
  static let readme = URL(string: "https://github.com/qhhonx/backupduck/blob/main/crates/cloud-audit/README.md")!
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
    let defaults = UserDefaults.standard
    let outcome: Result<Data, Error> = await withCloudCookies { cookies in
      guard let cookies else { return .failure(Bridge.Failure(code: "cloud_cookies_invalid")) }
      do {
        return .success(try await Bridge.call(["op": "cloud_audit", "command": ["run": [
          "pairing": try JSONSerialization.jsonObject(with: JSONEncoder().encode(pairing)),
          "cookies_path": cookies.path,
          "account_index": min(max(defaults.integer(forKey: CloudAuditKeys.accountIndex), 0), 9),
          "dry_run": dryRun, "max_items": 600,
        ]]]))
      } catch { return .failure(error) }
    }
    switch outcome {
    case .success(let data):
      cloudAuditError = nil
      if let result = try? JSONDecoder().decode(CloudAuditResult.self, from: data), result.quota > 0 {
        await handleCloudQuota(result, dryRun: dryRun)
      }
    case .failure(let error):
      cloudAuditError = (error as? Bridge.Failure)?.code ?? "internal"
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
  @State private var chooseError: String?

  private var lastRun: String? {
    guard let status = model.cloudAudit, let at = status.last_run_ms, let result = status.last_result else { return nil }
    let date = Date(timeIntervalSince1970: Double(at) / 1000).formatted(date: .abbreviated, time: .shortened)
    return String(format: NSLocalizedString("cloud_audit_last_run", comment: ""), date, result.checked,
      result.verified, result.quota, result.not_found, result.unknown)
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
    }.formStyle(.grouped)
      .task(id: model.pairing?.receiverID) { await model.refreshCloudAuditStatus() }
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
