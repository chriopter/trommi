// NotifyCoreLive: PushNotify's `NotifyCore` on trommi-core, for the Notification Service Extension and the widget. It
// opens the sealed part of one push with the push key the app handed over, and the envelope that rang with the
// read-only copy of a session group's current content key; it holds no device and no group state.
//
//   real      version, openPush (`openApnsPush`), openLive (`openLiveActivity`), openEnvelope (`openNotifyEnvelope`:
//             the sender's signature and the body; the sender's membership and chain are the app's to check)
//   stubbed   nothing
import Foundation
import PushNotify
import TrommiCoreRust

/// A refusal of the core, with the specification's code as text ("decrypt-failed", "bad-format", …). The message is
/// the core's and never holds key material or content.
public struct NotifyCoreError: Error, Equatable, CustomStringConvertible {
  public let code: String
  public let message: String
  public var description: String { message.isEmpty || message == code ? code : "\(code): \(message)" }
}

public final class NotifyCoreLive: NotifyCore {
  public init() {}
  /** The version of the core this extension links: proof that the library is in the binary. */
  public var version: String { versions().core }

  public func openLive(key: [UInt8], sealed: [UInt8]) throws -> (working: UInt64, waiting: UInt64) {
    do {
      let counts = try openLiveActivity(key: Data(key), sealed: Data(sealed))
      return (counts.working, counts.waiting)
    } catch let CoreError.Refused(code, message) {
      throw NotifyCoreError(code: errorCodeText(code: code), message: message)
    }
  }

  public func openEnvelope(keys: [PushNotify.NotifyKey], envelope: [UInt8]) throws -> NotifyEnvelope {
    let held = keys.compactMap { k -> TrommiCoreRust.NotifyKey? in
      guard let g = k.groupBytes, let key = k.keyBytes else { return nil }
      return TrommiCoreRust.NotifyKey(group: Data(g), epoch: k.epoch, key: Data(key))
    }
    do {
      let opened = try openNotifyEnvelope(keys: held, envelope: Data(envelope))
      let h = opened.header
      return NotifyEnvelope(group: [UInt8](h.group), epoch: h.epoch, isCard: h.kind == .version && h.object?.objectType == .card,
                            push: h.push, payload: [UInt8](opened.payload))
    } catch let CoreError.Refused(code, message) {
      throw NotifyCoreError(code: errorCodeText(code: code), message: message)
    }
  }

  public func openPush(key: [UInt8], sealed: [UInt8]) throws -> ApnsPush {
    do {
      let note = try openApnsPush(key: Data(key), sealed: Data(sealed))
      return ApnsPush(roomId: [UInt8](note.roomId), change: note.change, urgency: note.urgency, ticket: [UInt8](note.ticket))
    } catch let CoreError.Refused(code, message) {
      throw NotifyCoreError(code: errorCodeText(code: code), message: message)
    }
  }
}
