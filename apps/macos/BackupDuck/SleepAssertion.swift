import Combine
import Foundation

/// Keeps the Mac from idle-sleeping while backup work is pending. A default
/// URLSession does not survive sleep, so an unattended library migration would
/// otherwise stall whenever the display timer fires. Lid-closed sleep on battery
/// is outside this assertion's reach; it needs mains power or `pmset`.
@MainActor final class SleepAssertion: ObservableObject {
  static let shared = SleepAssertion()
  @Published private(set) var active = false
  private var activity: NSObjectProtocol?
  private var observer: AnyCancellable?

  /// Re-evaluate after every model publication. The model publishes before it
  /// mutates, so defer the read to the next main-loop turn.
  func bind(_ model: BackupModel) {
    observer = model.objectWillChange.sink { [weak self, weak model] _ in
      DispatchQueue.main.async {
        guard let self, let model else { return }
        self.update(model.transferActive)
      }
    }
    update(model.transferActive)
  }

  func update(_ wanted: Bool) {
    guard wanted != active else { return }
    if wanted {
      activity = ProcessInfo.processInfo.beginActivity(
        options: [.userInitiated, .idleSystemSleepDisabled], reason: "BackupDuck transfer")
    } else if let activity {
      ProcessInfo.processInfo.endActivity(activity)
      self.activity = nil
    }
    active = wanted
  }
}
