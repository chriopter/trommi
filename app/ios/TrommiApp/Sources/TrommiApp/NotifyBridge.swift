// NotifyBridge: the app's half of the Notification Service Extension (Sources/TrommiNotify). After a change of the
// board (at most every 2 s) it writes the context into the Keychain item the extension reads (PushNotify:
// NotifyContext says what is in it, NotifyGroup where it lies): the push key, the room's id, the hub's address and
// the room's cursor, from which the extension reads ahead (NotifyPrefetch); the app takes those pages back
// (`takeAhead`). No content key and
// no name of a session: a content key never leaves the core. The app stays the one owner of the device's state.
// It writes only when something changed. Signed out: the context goes.
import Foundation
import SwiftUI
import PushNotify
import TrommiClient

@MainActor
final class NotifyBridge {
  static let shared = NotifyBridge()
  private var due = false
  private var lastContext: NotifyContext?

  /** After a change of the board: write within 2 s (a catch-up brings thousands of changes). */
  func boardChanged(_ m: BoardModel) {
    guard !due else { return }
    due = true
    Task { @MainActor [weak m] in
      try? await Task.sleep(nanoseconds: 2_000_000_000)
      self.due = false
      if let m = m { self.write(m) }
    }
  }

  /** Going to the background: written at once. */
  func writeNow(_ m: BoardModel) { write(m) }

  private func write(_ m: BoardModel) {
    #if os(iOS)
    guard !m.demo, let room = m.room, let pushKey = Push.key() else { return }
    let c = NotifyContext(pushKey: pushKey, rooms: [room.roomIdHex], hub: room.hub.hubURL, client: HubClient.clientName, cursor: room.cursor)
    guard c != lastContext else { return }
    // the error names a Keychain status at most, never a key
    do { try NotifyGroup.writeContext(c); lastContext = c } catch { NSLog("trommi notify: context not written: %@", loggable(error)) }
    #endif
  }

  /** Signed out: nothing of the room stays for the extension, nor what it read ahead. */
  func signedOut() {
    #if os(iOS)
    for room in lastContext?.rooms ?? [] { NotifyGroup.prefetch(room: room)?.clear() }
    NotifyGroup.clear()
    #endif
    lastContext = nil
  }

  /** The room's pages read ahead, taken by their start; a page that is not JSON is dropped. */
  static func takeAhead(room hex: String) -> (UInt64) -> JSON? {
    { after in
      #if os(iOS)
      guard let data = NotifyGroup.prefetch(room: hex)?.take(after: after) else { return nil }
      return (try? JSONSerialization.jsonObject(with: data)) as? JSON
      #else
      return nil
      #endif
    }
  }
}
