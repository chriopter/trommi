// NotifyContext: what the Notification Service Extension (TrommiNotify) may know, and nothing more. The app is the
// one owner of the device's state (its MLS groups, its device key, its store): the extension never opens that store.
// The app writes this context into the Keychain (NotifyGroup.swift); the extension reads it.
//
// The extension gets:
//   - the push key: 32 bytes this phone registered with the hub, which open the sealed part of a notification
//     (the room, the change number, the urgency; no content) and the Live Activity's sealed counts;
//   - the ids of the rooms this phone is in;
//   - the hub's address, the app's client name and how far the app has read the room (its cursor): with the
//     push's ticket the extension reads ahead from there what the app would catch up on (NotifyPrefetch), as
//     ciphertext, which only the app opens.
// The content keys it may hold are not in here: they lie in an item of their own that the widget cannot read
// (NotifyKeys.swift). It never gets the device key (it cannot sign, cannot sign in to the hub, cannot send) and no
// MLS state.
import Foundation

public struct NotifyContext: Codable, Equatable {
  /** 4: with the hub, the client name and the cursor for reading ahead. A stored context of another version is not read. */
  public static let currentVersion = 4
  public var version = NotifyContext.currentVersion
  /** The push key (base64url, 32 bytes). */
  public var pushKey: String
  /** The rooms this phone is in (hex ids). */
  public var rooms: [String]
  /** The hub's address (https://…, no trailing slash); nil: no reading ahead. */
  public var hub: String?
  /** The app's `Trommi-Client` header. */
  public var client: String?
  /** How far the app has read the room, by the hub's change number. */
  public var cursor: UInt64?
  public init(pushKey: [UInt8], rooms: [String], hub: String? = nil, client: String? = nil, cursor: UInt64? = nil) {
    self.pushKey = NotifyText.b64u(pushKey); self.rooms = rooms; self.hub = hub; self.client = client; self.cursor = cursor
  }
  /** Whether this phone is in that room. */
  public func holds(room id: [UInt8]) -> Bool { rooms.contains(NotifyText.hex(id)) }
}

/** Byte strings as text, for this module alone (it links nothing else). */
enum NotifyText {
  private static let hexDigits = Array("0123456789abcdef".utf8)
  static func hex(_ b: [UInt8]) -> String {
    var out = [UInt8]()
    out.reserveCapacity(b.count * 2)
    for x in b { out.append(hexDigits[Int(x >> 4)]); out.append(hexDigits[Int(x & 15)]) }
    return String(decoding: out, as: UTF8.self)
  }
  /** nil for anything that is not lower-case hex of whole bytes. */
  static func unhex(_ s: String) -> [UInt8]? {
    let u = Array(s.utf8)
    guard u.count % 2 == 0 else { return nil }
    func nibble(_ c: UInt8) -> UInt8? { c >= 0x30 && c <= 0x39 ? c - 0x30 : c >= 0x61 && c <= 0x66 ? c - 0x61 + 10 : nil }
    var out = [UInt8]()
    out.reserveCapacity(u.count / 2)
    var i = 0
    while i < u.count {
      guard let h = nibble(u[i]), let l = nibble(u[i + 1]) else { return nil }
      out.append(h << 4 | l)
      i += 2
    }
    return out
  }

  /** base64url without padding (RFC 4648 section 5). */
  static func b64u(_ b: [UInt8]) -> String {
    Data(b).base64EncodedString().replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
  }
  /** nil for anything that is not base64url without padding. */
  static func unb64u(_ s: String) -> [UInt8]? {
    guard s.utf8.allSatisfy({ ($0 >= 0x30 && $0 <= 0x39) || ($0 >= 0x41 && $0 <= 0x5a) || ($0 >= 0x61 && $0 <= 0x7a) || $0 == 0x2d || $0 == 0x5f }),
          s.utf8.count % 4 != 1 else { return nil }
    let padded = s.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/") + String(repeating: "=", count: (4 - s.utf8.count % 4) % 4)
    return Data(base64Encoded: padded).map { Array($0) }
  }
}
