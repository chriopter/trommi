// NotifyTitle: the title of the card that rang, for the notification, opened on the phone (spec/v1.md 15.2). With the
// keys the app handed over (NotifyKeys.swift) the extension finds the envelope of the push's change among the pages it
// read ahead (or reads that one change from the hub), opens it (NotifyCore.openEnvelope) and takes the card's
// `title`. Anything else gives nil, and the notification keeps the text the urgency calls for: no keys, keys of
// another room, a key of another epoch (the group moved on and the app has not written the new one yet), a page
// without the change, an envelope that does not open, something that is not a card or did not ask for a push, a body
// of a newer schema, an empty title.
//
// The sender is not checked here beyond its signature: the extension holds no group state. The content key itself
// says a member of the group wrote it; which member, and whether in its chain's order, the app checks when it opens.
import Foundation

public enum NotifyTitle {
  /** The longest title shown; a longer one is cut and ends in "…". */
  public static let maxLength = 120
  /** The newest body schema this reads (spec/v1.md 9.1.2). */
  static let schemaVersion = 2

  /** The envelope of `change` in a page as `GET /v1/push-changes` gives it; nil when the page does not hold it. */
  public static func envelope(change: UInt64, page: Data) -> [UInt8]? {
    guard let p = try? JSONSerialization.jsonObject(with: page) as? [String: Any], let items = p["items"] as? [[String: Any]] else { return nil }
    for item in items where (item["change"] as? NSNumber)?.uint64Value == change {
      guard (item["kind"] as? String ?? "envelope") == "envelope", let e = item["envelope"] as? String else { return nil }
      return NotifyText.unb64u(e)
    }
    return nil
  }

  /** The title of an opened envelope of `room`: a card that asked for a push, with a title; nil otherwise. */
  public static func title(_ e: NotifyEnvelope, room: [UInt8]) -> String? {
    guard e.isCard, e.push, e.group.count == 48, Array(e.group.prefix(32)) == room,
          let body = try? JSONSerialization.jsonObject(with: Data(e.payload)) as? [String: Any],
          ((body["schema_version"] as? NSNumber)?.intValue ?? schemaVersion) <= schemaVersion,
          let raw = body["title"] as? String else { return nil }
    let one = raw.split(whereSeparator: \.isNewline).joined(separator: " ").trimmingCharacters(in: .whitespacesAndNewlines)
    guard !one.isEmpty else { return nil }
    return one.count > maxLength ? String(one.prefix(maxLength - 1)) + "…" : one
  }

  /**
   * The title for a push: from the keys of its room and the envelope of its change (a page kept in `store`, else that
   * one change read from the hub with the push's ticket). nil when anything is missing or does not open.
   */
  public static func find(_ push: ApnsPush, context: NotifyContext, keys: NotifyKeySet?, store: PrefetchStore?, core: NotifyCore,
                          get: (URL, [String: String]) async -> Data?) async -> String? {
    guard let keys, keys.room == NotifyText.hex(push.roomId), !keys.keys.isEmpty else { return nil }
    var page = store?.page(holding: push.change)
    if page.flatMap({ envelope(change: push.change, page: $0) }) == nil { page = await one(push, context: context, get: get) }
    guard let page, let bytes = envelope(change: push.change, page: page),
          let opened = try? core.openEnvelope(keys: keys.keys, envelope: bytes) else { return nil }
    return title(opened, room: push.roomId)
  }

  private static func one(_ push: ApnsPush, context: NotifyContext, get: (URL, [String: String]) async -> Data?) async -> Data? {
    await NotifyPrefetch.one(push, context: context, get: get)
  }
}
