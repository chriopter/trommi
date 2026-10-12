// Keychain.swift: what the app keeps outside the device's state, one set per state folder:
//   state-<folder>    the key that seals the device's state (DeviceStore.swift)
//   anchor-<folder>   the revision below which that state does not load (DeviceStore.swift, "Rollback")
//   cache-<folder>    the key that seals what this device already opened, kept for a quick start (RecordStore.swift)
//   join-<folder>     the recovery code, only while a sign-in with it has session groups left to join (RoomAccount.swift)
// All are kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly: a push wakes the app while the phone is locked, and the
// item never leaves this device (it can come back with a restore of a backup made ON this device; the state itself
// is left out of backups, so a restored app finds an anchor without a state and asks to sign in again).
//
// They are items of the APP alone. Every query names the app's own access group (<team>.<bundle id>), never the
// App Group, so that an item an extension put under the same name into a shared group is not taken for one of
// these, and a later change of the entitlements cannot move these into a shared group. The app's
// `keychain-access-groups` list its own group FIRST, then the group it shares with the notification extension
// (`<team>.com.trommi.ios.notify-keys`): an item added without naming a group lands in the first.
//
// A key is made only for a NEW state folder. A Keychain that cannot be read is an error, never "no key yet": making
// a new key over an old one would lose the state for good. Elsewhere (Linux: tests) the items are 0600 files beside
// the state, which keeps the logic the same and protects nothing.
//
// On a Mac the items are in the data protection Keychain (kSecUseDataProtectionKeychain, the iPhone's kind of
// Keychain) once the app says so (`LocalKey.systemKeychain = true`, the Mac app's start): it needs an app signed
// with an application identifier, which a test binary of `swift test` on a Mac is not; those keep the files.
import Foundation
#if os(iOS) || os(macOS)
import Security
#endif

/** The Keychain access groups the app names outside this file. */
public enum AppKeychain {
  /**
   * The group the app shares with its notification extension alone (`keychain-access-groups` in both entitlements):
   * `<team>.com.trommi.ios.notify-keys`, from the team of the app's own group. nil when that is not known (and on Linux).
   */
  public static var notifyKeysGroup: String? {
    #if os(iOS)
    guard let own = LocalKey.privateGroup, let team = own.split(separator: ".", maxSplits: 1).first, team.count == 10 else { return nil }
    return "\(team).com.trommi.ios.notify-keys"
    #else
    return nil
    #endif
  }
}

public enum LocalKey {
  #if os(macOS)
  /** The Mac app switches the system Keychain on at its start; until then (tests) the items are files. */
  nonisolated(unsafe) public static var systemKeychain = false
  #endif
  /** The 32-byte key `name` of the state folder `dir`. `create`: the folder is new and may get a new key. */
  static func get(_ name: String, dir: URL, create: Bool) throws -> Bytes {
    let account = "\(name)-\(dir.lastPathComponent)"
    if let k = try read(account, dir: dir) {
      guard k.count == 32 else { throw TrommiError("keychain", "a stored key has the wrong length") }
      return k
    }
    guard create else { throw TrommiError("keychain", "the key of this device's state is missing") }
    let k = try random(32)
    try write(account, k, dir: dir)
    guard try read(account, dir: dir) == k else { throw TrommiError("keychain", "the key is not kept") }
    return k
  }
  /** Forgets everything of that state folder (signing out). */
  static func wipe(dir: URL) {
    for name in ["state", "cache", "anchor", "join"] { delete("\(name)-\(dir.lastPathComponent)", dir: dir) }
  }
  static func random(_ n: Int) throws -> Bytes {
    #if os(iOS) || os(macOS)
    var k = Bytes(repeating: 0, count: n)
    guard SecRandomCopyBytes(kSecRandomDefault, n, &k) == errSecSuccess else { throw TrommiError("keychain", "no random bytes") }
    return k
    #else
    var g = SystemRandomNumberGenerator()
    return (0..<n).map { _ in UInt8.random(in: 0...255, using: &g) }
    #endif
  }

  #if os(iOS) || os(macOS)
  private static let service = "com.trommi.local-key"
  /**
   * The app's own access group, asked from the system once: the group of an item added without naming one is the
   * first of the app's groups, which for an app is its application identifier.
   */
  static let privateGroup: String? = {
    var probe: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: "group-probe",
                                kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly, kSecReturnAttributes as String: true]
    #if os(macOS)
    probe[kSecUseDataProtectionKeychain as String] = true
    #endif
    var out: AnyObject?
    var rc = SecItemCopyMatching(probe.merging([kSecMatchLimit as String: kSecMatchLimitOne]) { a, _ in a } as CFDictionary, &out)
    if rc == errSecItemNotFound { rc = SecItemAdd(probe.merging([kSecValueData as String: Data([0])]) { a, _ in a } as CFDictionary, &out) }
    guard rc == errSecSuccess, let group = (out as? [String: Any])?[kSecAttrAccessGroup as String] as? String, !group.hasPrefix("group."),
          !group.hasSuffix(".notify-keys") else { return nil }
    return group
  }()
  private static func query(_ account: String) throws -> [String: Any] {
    guard let group = privateGroup else { throw TrommiError("keychain", "the app's own Keychain group is not known") }
    var q: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: account, kSecAttrAccessGroup as String: group]
    #if os(macOS)
    q[kSecUseDataProtectionKeychain as String] = true
    #endif
    return q
  }
  private static func keychainRead(_ account: String) throws -> Bytes? {
    var q = try query(account)
    q[kSecReturnData as String] = true; q[kSecMatchLimit as String] = kSecMatchLimitOne
    var out: AnyObject?
    let rc = SecItemCopyMatching(q as CFDictionary, &out)
    if rc == errSecItemNotFound { return nil }
    guard rc == errSecSuccess, let d = out as? Data else { throw TrommiError("keychain", "the Keychain could not be read (\(rc))") }
    return Bytes(d)
  }
  /** Adds the item, or changes its value in place; an existing item is never deleted to make room. */
  private static func keychainWrite(_ account: String, _ value: Bytes) throws {
    var add = try query(account)
    add[kSecValueData as String] = Data(value)
    add[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
    var rc = SecItemAdd(add as CFDictionary, nil)
    if rc == errSecDuplicateItem { rc = SecItemUpdate(try query(account) as CFDictionary, [kSecValueData as String: Data(value)] as CFDictionary) }
    if rc != errSecSuccess { throw TrommiError("keychain", "the Keychain could not be written (\(rc))") }
  }
  private static func keychainDelete(_ account: String) { if let q = try? query(account) { SecItemDelete(q as CFDictionary) } }
  #endif

  #if os(iOS)
  static func read(_ account: String, dir: URL) throws -> Bytes? { try keychainRead(account) }
  static func write(_ account: String, _ value: Bytes, dir: URL) throws { try keychainWrite(account, value) }
  static func delete(_ account: String, dir: URL) { keychainDelete(account) }
  #else
  #if os(macOS)
  static func read(_ account: String, dir: URL) throws -> Bytes? { try (systemKeychain ? keychainRead(account) : fileRead(account, dir: dir)) }
  static func write(_ account: String, _ value: Bytes, dir: URL) throws { try (systemKeychain ? keychainWrite(account, value) : fileWrite(account, value, dir: dir)) }
  static func delete(_ account: String, dir: URL) { if systemKeychain { keychainDelete(account) } else { fileDelete(account, dir: dir) } }
  #else
  static func read(_ account: String, dir: URL) throws -> Bytes? { try fileRead(account, dir: dir) }
  static func write(_ account: String, _ value: Bytes, dir: URL) throws { try fileWrite(account, value, dir: dir) }
  static func delete(_ account: String, dir: URL) { fileDelete(account, dir: dir) }
  #endif
  private static func url(_ account: String, _ dir: URL) -> URL { dir.appendingPathComponent(account.replacingOccurrences(of: "-\(dir.lastPathComponent)", with: "") + ".key") }
  private static func fileRead(_ account: String, dir: URL) throws -> Bytes? {
    let u = url(account, dir)
    guard FileManager.default.fileExists(atPath: u.path) else { return nil }
    return Bytes(try Data(contentsOf: u))
  }
  private static func fileWrite(_ account: String, _ value: Bytes, dir: URL) throws {
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
    try Data(value).write(to: url(account, dir), options: .atomic)
    try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url(account, dir).path)
  }
  private static func fileDelete(_ account: String, dir: URL) { try? FileManager.default.removeItem(at: url(account, dir)) }
  #endif
}

/** The state's anchor as an item beside its key (Keychain on the phone). */
final class LocalAnchor: StateAnchor {
  let dir: URL
  init(dir: URL) { self.dir = dir }
  private var account: String { "anchor-\(dir.lastPathComponent)" }
  func read() throws -> UInt64? {
    guard let b = try LocalKey.read(account, dir: dir) else { return nil }
    guard b.count == 8 else { throw TrommiError("keychain", "the anchor has the wrong length") }
    return readBe64(b, 0)
  }
  func write(_ revision: UInt64) throws { try LocalKey.write(account, be64(revision), dir: dir) }
}
