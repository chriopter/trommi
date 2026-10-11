// NotifyOpen: the Notification Service Extension's work without UIKit, so it is tested on Linux. A notification
// carries the one neutral text of every push ("Something new.", which is all Apple sees) and `e`, a sealed blob
// (spec/v1.md 15.2). `e` is opened with the push key (NotifyCore.openPush): the room, the change number and the
// urgency. That is all the extension learns; from it, on the phone, it picks the text shown ([`text`]): a
// question, an urgent one, a lost agent. The envelope the push names is not fetched, because nothing here could
// open it (a content key never leaves the core, NotifyCore.swift). A push that does not open, or names a room this
// phone is not in, gives nil, and the notification keeps the neutral text. No error is shown.
import Foundation

public enum NotifyOpen {
  /** The longest sealed blob (core `push::MAX_APNS_LEN`). */
  static let maxSealed = 512

  /** `e` opened with the context's push key; nil when it does not open or names a room that is not in the context. */
  public static func push(_ e: String, context: NotifyContext, core: NotifyCore) -> ApnsPush? {
    guard let sealed = NotifyText.unb64u(e), sealed.count <= maxSealed, let key = NotifyText.unb64u(context.pushKey), key.count == 32,
          let push = try? core.openPush(key: key, sealed: sealed), context.holds(room: push.roomId) else { return nil }
    return push
  }

  /** The neutral text every push carries in the clear, and what is shown when `e` does not open. */
  public static let neutral = "Something new."
  /** The text shown for an opened push: without a ticket the hub's word that an agent was lost; urgency 2 or 3 knocks. */
  public static func text(_ push: ApnsPush) -> String {
    push.ticket.isEmpty ? "An agent lost its connection." : push.urgency >= 2 ? "Urgent: a new question." : "A new question."
  }

  /** What the notifications of one room are threaded by: the start of its id, no content. */
  public static func thread(_ push: ApnsPush) -> String { String(NotifyText.hex(push.roomId).prefix(16)) }
}
