// NotifyCore: the calls of trommi-core that the Notification Service Extension and the widget make, as a Swift
// protocol. This module holds no cryptography and does not link the Rust library: the real implementation is a thin
// adapter over the core (UniFFI), handed in by whoever links it; the tests hand in a fake.
//
// It opens what the hub sealed for this phone's push key (a notification, a Live Activity's counts) and, with the
// read-only copy of a session group's current content key (NotifyKeys.swift), the one envelope that rang.
import Foundation

/** What a sealed APNs notification says (spec/v1.md 15.2, core `push::ApnsPush`). No content. */
public struct ApnsPush: Equatable {
  /** The room something changed in (32 bytes). */
  public var roomId: [UInt8]
  /** The hub's change number of the envelope that rang. */
  public var change: UInt64
  /** 0 low, 1 normal, 2 high, 3 critical. */
  public var urgency: UInt8
  /** The hub's ticket for `GET /v1/push-changes` (in a header); empty when there is no envelope to fetch. It reads for a day: never log it. */
  public var ticket: [UInt8]
  public init(roomId: [UInt8], change: UInt64, urgency: UInt8, ticket: [UInt8]) {
    self.roomId = roomId; self.change = change; self.urgency = urgency; self.ticket = ticket
  }
}

/** One envelope the extension opened (core `open_notify_envelope`): signed by the sender its header names, nothing more checked. */
public struct NotifyEnvelope: Equatable {
  /** The group (48 bytes for a session group: the room's 32, then the session's 16). */
  public var group: [UInt8]
  public var epoch: UInt64
  /** Whether it is a version of a card (a Decision or an Info card). */
  public var isCard: Bool
  /** Whether its sender asked for a push. */
  public var push: Bool
  /** The body's payload: one JSON object. Content: never log it. */
  public var payload: [UInt8]
  public init(group: [UInt8], epoch: UInt64, isCard: Bool, push: Bool, payload: [UInt8]) {
    self.group = group; self.epoch = epoch; self.isCard = isCard; self.push = push; self.payload = payload
  }
}

public protocol NotifyCore {
  /**
   * Opens `nonce(12) ‖ AEAD` of a notification under the 32-byte push key this phone registered (core `push::open`).
   * Throws when it does not open or is not the one spelling of the four fields.
   */
  func openPush(key: [UInt8], sealed: [UInt8]) throws -> ApnsPush
  /** Opens a Live Activity's sealed counts (core `push::open_live`): `(working, waiting)`. Throws when they do not open. */
  func openLive(key: [UInt8], sealed: [UInt8]) throws -> (working: UInt64, waiting: UInt64)
  /**
   * Opens one envelope with the key among `keys` of its group and epoch (core `open_notify_envelope`): its signature
   * against the sender its header names, then its body. Throws when none of the keys is the one, and when anything
   * does not hold.
   */
  func openEnvelope(keys: [NotifyKey], envelope: [UInt8]) throws -> NotifyEnvelope
}
