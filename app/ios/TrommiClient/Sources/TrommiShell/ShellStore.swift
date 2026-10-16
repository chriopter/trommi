// The two AppCore storage spaces. DeviceStore retains the exclusive lock, durable encrypted journal and
// Keychain revision anchor; the cache holds encrypted values and public logout notices. No application decisions.
import Foundation
#if canImport(Glibc)
import Glibc
#elseif canImport(Darwin)
import Darwin
#endif

public struct NativePacket: @unchecked Sendable {
  public var json: [String: Any]
  public var blobs: [Data]
  public init(_ json: [String: Any], blobs: [Data] = []) { self.json = json; self.blobs = blobs }
  public func bytes(_ index: Any?) throws -> Data {
    guard let n = index as? Int, blobs.indices.contains(n) else { throw TrommiError("bad-format") }
    return blobs[n]
  }
}

final class ShellStore {
  private let dir: URL
  private let cache: URL
  private let device: DeviceStore
  private let cacheKey: Bytes
  init(directory: URL) throws {
    dir = directory; cache = directory.appendingPathComponent("cache")
    let state = directory.appendingPathComponent("state")
    let fresh = !FileManager.default.fileExists(atPath: state.path)
    let locked = try DeviceStore(directory: state, key: LocalKey.random(32))
    do {
      // No state folder: nothing of a device is here (a first start, or the app deleted and installed again, whose
      // Keychain items outlive it). A key and anchor left from before would refuse the new, empty state for ever
      // ("older than this device's last known one"): they go, under the state's lock, and the new state gets a new
      // key, so an old log put back later never opens.
      if fresh { LocalKey.wipe(dir: dir) }
      let key = try LocalKey.get("state", dir: dir, create: fresh)
      device = try locked.rekeyed(key, anchor: LocalAnchor(dir: dir))
      cacheKey = try LocalKey.get("cache", dir: dir, create: true)
      try FileManager.default.createDirectory(at: cache, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
    } catch { locked.close(); throw error }
  }
  func close() { device.close() }
  private static let digits = Array("0123456789abcdef".utf8)
  private static func encoded(_ key: String) -> [UInt8] {
    var bytes = [UInt8](); bytes.reserveCapacity(key.utf8.count * 2)
    for byte in key.utf8 { bytes.append(digits[Int(byte >> 4)]); bytes.append(digits[Int(byte & 15)]) }
    return bytes
  }
  private static func decoded(_ name: String) -> String? {
    let hex = Array(name.utf8)
    guard hex.count % 2 == 0 else { return nil }
    func nibble(_ byte: UInt8) -> UInt8? {
      switch byte { case 48...57: return byte - 48; case 97...102: return byte - 87; case 65...70: return byte - 55; default: return nil }
    }
    var bytes = [UInt8](); bytes.reserveCapacity(hex.count / 2)
    for at in stride(from: 0, to: hex.count, by: 2) {
      guard let high = nibble(hex[at]), let low = nibble(hex[at + 1]) else { return nil }
      bytes.append(high << 4 | low)
    }
    return String(bytes: bytes, encoding: .utf8)
  }
  private func file(_ key: String) -> URL {
    // Keep the existing reversible layout and its 120-byte components, including the empty key.
    let encoded = Self.encoded(key)
    return stride(from: 0, to: encoded.count, by: 120).reduce(cache) { url, at in
      url.appendingPathComponent(String(decoding: encoded[at..<min(at + 120, encoded.count)], as: UTF8.self))
    }.appendingPathComponent("value")
  }
  private func value(at url: URL, key: String) throws -> Data? {
    let b = Bytes(try Data(contentsOf: url))
    guard b.count >= 28 else { return nil }
    return try? Data(gcmOpen(key: cacheKey, nonce: Bytes(b.prefix(12)), aad: Bytes(key.utf8), Bytes(b.dropFirst(12))))
  }
  private func get(_ key: String) throws -> Data? {
    let url = file(key)
    guard FileManager.default.fileExists(atPath: url.path) else { return nil }
    return try value(at: url, key: key)
  }
  private func remove(_ url: URL, durable: Bool) throws {
    guard FileManager.default.fileExists(atPath: url.path) else { return }
    try FileManager.default.removeItem(at: url)
    var folder = url.deletingLastPathComponent()
    if durable { try DeviceStore.syncDirectory(folder) }
    // rmdir removes only an empty directory atomically: a sibling value can never be removed.
    while folder.path != cache.path && rmdir(folder.path) == 0 {
      folder.deleteLastPathComponent()
      if durable { try DeviceStore.syncDirectory(folder) }
    }
  }
  func run(_ packet: NativePacket) throws -> NativePacket {
    let op = packet.json; var blobs: [Data] = []
    func bytes(_ data: Data) -> Int { blobs.append(data); return blobs.count - 1 }
    var answer: [String: Any] = ["answer": "done"]
    switch op["op"] as? String {
    case "device_load":
      let loaded = try device.load()
      answer = ["answer": "device", "revision": loaded.revision, "entries": loaded.entries.map { ["key": bytes(Data($0.key)), "value": bytes(Data($0.value))] }]
    case "device_apply":
      guard let rev = (op["expected_revision"] as? NSNumber)?.uint64Value else { throw TrommiError("bad-format") }
      let put = try (op["put"] as? [[String: Any]] ?? []).map { StoreEntry(key: Bytes(try packet.bytes($0["key"])), value: Bytes(try packet.bytes($0["value"]))) }
      let delete = try (op["delete"] as? [Int] ?? []).map { Bytes(try packet.bytes($0)) }
      try device.apply(expectedRevision: rev, batch: StoreBatch(put: put, delete: delete))
    case "get":
      answer = ["answer": "values", "values": try (op["keys"] as? [String] ?? []).map { key -> Any in
        if let value = try get(key) { return bytes(value) }; return NSNull()
      }]
    case "scan", "keys":
      let prefix = op["prefix"] as? String ?? ""
      guard let enumerator = FileManager.default.enumerator(at: cache, includingPropertiesForKeys: [.isRegularFileKey]) else { throw TrommiError("storage") }
      let wanted = String(decoding: Self.encoded(prefix), as: UTF8.self)
      let found = try enumerator.compactMap { entry -> (String, URL)? in
        guard let url = entry as? URL else { return nil }
        // Depth handles Darwin's /var → /private/var canonicalization without absolute-path guesses.
        let components = url.pathComponents.suffix(enumerator.level)
        if url.lastPathComponent != "value" {
          let branch = components.joined()
          if !wanted.hasPrefix(branch) && !branch.hasPrefix(wanted) { enumerator.skipDescendants() }
          return nil
        }
        let name = components.dropLast().joined()
        guard name.hasPrefix(wanted), try url.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile == true,
              let key = Self.decoded(name) else { return nil }
        return (key, url)
      }.sorted { $0.0 < $1.0 }
      if op["op"] as? String == "keys" { answer = ["answer":"keys", "keys":found.map { $0.0 }]; break }
      var entries: [[String: Any]] = []
      for (key, url) in found {
        // Enumeration already found the file: do not encode its key and stat the same path again.
        if let value = try value(at: url, key: key) { entries.append(["key": key, "value": bytes(value)]) }
      }
      answer = ["answer": "entries", "entries": entries]
    case "put", "put_durable":
      let durable = op["op"] as? String == "put_durable"
      for key in op["delete"] as? [String] ?? [] { try remove(file(key), durable: durable) }
      for entry in op["put"] as? [[String: Any]] ?? [] {
        guard let key = entry["key"] as? String else { throw TrommiError("bad-format") }
        let url = file(key)
        let nonce = try LocalKey.random(12)
        let sealed = nonce + (try gcmSeal(key: cacheKey, nonce: nonce, aad: Bytes(key.utf8), Bytes(packet.bytes(entry["value"]))))
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        try Data(sealed).write(to: url, options: .atomic)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
        if durable {
          try DeviceStore.syncFile(url)
          var folder = url.deletingLastPathComponent()
          while folder.path != dir.path { try DeviceStore.syncDirectory(folder); folder.deleteLastPathComponent() }
          try DeviceStore.syncDirectory(dir)
        }
      }
    default: throw TrommiError("bad-format")
    }
    return NativePacket(answer, blobs: blobs)
  }
  func secret(_ packet: NativePacket) throws -> NativePacket {
    guard let name = packet.json["name"] as? String else { throw TrommiError("bad-format") }
    let account = "secret-\(dir.lastPathComponent)-\(name)"
    switch packet.json["op"] as? String {
    case "get":
      if let b = try LocalKey.read(account, dir: dir) { return NativePacket(["ok": 0], blobs: [Data(b)]) }
    case "put": try LocalKey.write(account, Bytes(packet.bytes(packet.json["value"])), dir: dir)
    case "delete": LocalKey.delete(account, dir: dir)
    default: throw TrommiError("bad-format")
    }
    return NativePacket(["ok": NSNull()])
  }
}
