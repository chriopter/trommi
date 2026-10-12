// NotifyKeys: the read-only copy of content keys that lets the Notification Service Extension show the title of the
// card that rang (spec/v1.md 6.4, 15.2). The app is still the one owner of the device's state; it hands the extension,
// of each session group of the room, the content key of the group's CURRENT epoch (core `notify_keys`), and writes
// them again whenever an epoch changes. Never the room group's key, never an older epoch, never the device key.
//
// Where they lie: ONE Keychain item in an access group of its own, `<team>.com.trommi.ios.notify-keys`, which only
// the app and the notification extension are entitled to (AppStore/TrommiApp.entitlements and
// TrommiNotify.entitlements, `keychain-access-groups`). Not the App Group: the Share Extension and the Live Activity
// widget, which are members of that, cannot read this item. kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly, like
// the context: readable after the first unlock since the phone started, never on another device, never in a backup
// that leaves it. The extension only reads; it writes and signs nothing.
//
// What a key allows: everything written in that session group during that epoch opens with it (cards, chat, answers),
// not only the card that rang. The extension opens one envelope per push, checks its signature against the sender its
// header names, and shows a card's title; whether that sender is a member, and where the envelope stands in its chain,
// it does not check: the app checks everything again when it opens. Whatever does not open shows the neutral text.
import Foundation
#if canImport(Security)
import Security
#endif

/** The content key of one session group at its current epoch. Never log it. */
public struct NotifyKey: Codable, Equatable {
  /** The session group (hex): the room's 32 bytes, then the session's 16. */
  public var group: String
  public var epoch: UInt64
  /** base64url, 32 bytes. */
  public var key: String
  public init(group: [UInt8], epoch: UInt64, key: [UInt8]) {
    self.group = NotifyText.hex(group); self.epoch = epoch; self.key = NotifyText.b64u(key)
  }
  /** The key's bytes; nil when they are not 32. */
  public var keyBytes: [UInt8]? { NotifyText.unb64u(key).flatMap { $0.count == 32 ? $0 : nil } }
  /** The group's bytes; nil when it is not hex. */
  public var groupBytes: [UInt8]? { NotifyText.unhex(group) }
}

/** What the item holds: the room and its session groups' current keys. */
public struct NotifyKeySet: Codable, Equatable, CustomStringConvertible {
  /** 1. A stored set of another version is not read. */
  public static let currentVersion = 1
  public var version = NotifyKeySet.currentVersion
  /** The room (hex). */
  public var room: String
  public var keys: [NotifyKey]
  public init(room: String, keys: [NotifyKey]) { self.room = room; self.keys = keys.sorted { ($0.group, $0.epoch) < ($1.group, $1.epoch) } }
  /** Never the keys themselves. */
  public var description: String { "NotifyKeySet(\(keys.count) keys)" }
}

/** The key set over a storage: the app writes, the extension reads. */
public struct NotifyKeyStore {
  let storage: NotifyStorage
  public init(storage: NotifyStorage) { self.storage = storage }

  public func write(_ k: NotifyKeySet) throws { try storage.write(Array(try JSONEncoder().encode(k))) }
  /** nil when nothing is stored, or it is not a set of this version. */
  public func read() -> NotifyKeySet? {
    guard let b = storage.read(), let k = try? JSONDecoder().decode(NotifyKeySet.self, from: Data(b)), k.version == NotifyKeySet.currentVersion else { return nil }
    return k
  }
  public func clear() { storage.clear() }
}

public enum NotifyKeyGroup {
  /** What follows the team in the access group's name. */
  public static let suffix = ".com.trommi.ios.notify-keys"
  /** The access group of a team (`<team>.com.trommi.ios.notify-keys`), from the app's own group (`<team>.<bundle id>`). */
  public static func of(appGroup: String) -> String? {
    guard let team = appGroup.split(separator: ".", maxSplits: 1).first, team.count == 10, !appGroup.hasPrefix("group.") else { return nil }
    return "\(team)\(suffix)"
  }
  /** Whether an item's access group is this one: never an App Group's. */
  public static func isOurs(_ accessGroup: String) -> Bool { accessGroup.hasSuffix(suffix) && !accessGroup.hasPrefix("group.") && of(appGroup: accessGroup) == accessGroup }

  #if os(iOS)
  /** The app writes the set into the group it names (`of(appGroup:)`). */
  public static func write(_ k: NotifyKeySet, accessGroup: String) throws { try NotifyKeyStore(storage: NotifyKeysKeychain(accessGroup: accessGroup)).write(k) }
  /** The extension reads it, from whichever entitled group holds it, as long as that is the keys' own group. */
  public static func read() -> NotifyKeySet? { NotifyKeyStore(storage: NotifyKeysKeychain(accessGroup: nil)).read() }
  /** Signed out: the keys go. */
  public static func clear(accessGroup: String?) { NotifyKeysKeychain(accessGroup: accessGroup).clear() }
  #endif
}

#if os(iOS)
/** The keys' one Keychain item. `accessGroup` nil: read from any group this process may read that is the keys' own. */
public struct NotifyKeysKeychain: NotifyStorage {
  static let service = "com.trommi.notify-keys"
  static let account = "keys"
  let accessGroup: String?
  public init(accessGroup: String?) { self.accessGroup = accessGroup }

  private var item: [String: Any] {
    var q: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: Self.service, kSecAttrAccount as String: Self.account]
    if let g = accessGroup { q[kSecAttrAccessGroup as String] = g }
    return q
  }

  public func read() -> [UInt8]? {
    // Which groups hold an item of this name (attributes only: data cannot be asked of all matches at once); only
    // the keys' own group counts, one of the same name elsewhere is not taken for it.
    var group = accessGroup
    if group == nil {
      var q = item
      q[kSecReturnAttributes as String] = true
      q[kSecMatchLimit as String] = kSecMatchLimitAll
      var out: AnyObject?
      guard SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess, let found = out as? [[String: Any]] else { return nil }
      group = found.compactMap { $0[kSecAttrAccessGroup as String] as? String }.first(where: NotifyKeyGroup.isOurs)
    }
    guard let group, NotifyKeyGroup.isOurs(group) else { return nil }
    var q = NotifyKeysKeychain(accessGroup: group).item
    q[kSecReturnData as String] = true
    q[kSecMatchLimit as String] = kSecMatchLimitOne
    var out: AnyObject?
    guard SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess, let d = out as? Data else { return nil }
    return Array(d)
  }

  public func write(_ bytes: [UInt8]) throws {
    guard accessGroup != nil else { throw NotifyError("no-access-group") }
    let fresh: [String: Any] = [kSecValueData as String: Data(bytes), kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly]
    var rc = SecItemUpdate(item as CFDictionary, fresh as CFDictionary)
    if rc == errSecItemNotFound { rc = SecItemAdd(item.merging(fresh) { $1 } as CFDictionary, nil) }
    if rc != errSecSuccess { throw NotifyError("keychain-\(rc)") }
  }

  public func clear() { SecItemDelete(item as CFDictionary) }
}
#endif
