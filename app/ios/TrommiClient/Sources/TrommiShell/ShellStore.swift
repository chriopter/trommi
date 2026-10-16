// The two AppCore storage spaces. DeviceStore retains the exclusive lock, durable encrypted journal and
// Keychain revision anchor; the cache contains only encrypted, disposable values. No application decisions.
import Foundation

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
      let key = try LocalKey.get("state", dir: dir, create: fresh)
      device = try locked.rekeyed(key, anchor: LocalAnchor(dir: dir))
      cacheKey = try LocalKey.get("cache", dir: dir, create: true)
      try FileManager.default.createDirectory(at: cache, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
    } catch { locked.close(); throw error }
  }
  func close() { device.close() }
  private func file(_ key: String) -> URL {
    // Core keys can include both a room and a timeline id. Split their reversible encoding so no component
    // exceeds the filesystem limit; the bytes and their authenticated key remain unchanged.
    let encoded = Array(Data(key.utf8).map { String(format: "%02x", $0) }.joined())
    return stride(from: 0, to: encoded.count, by: 120).reduce(cache) { url, at in
      url.appendingPathComponent(String(encoded[at..<min(at + 120, encoded.count)]))
    }.appendingPathComponent("value")
  }
  private func get(_ key: String) throws -> Data? {
    let u = file(key)
    guard FileManager.default.fileExists(atPath: u.path) else { return nil }
    let b = Bytes(try Data(contentsOf: u))
    guard b.count >= 28 else { return nil }
    return try? Data(gcmOpen(key: cacheKey, nonce: Bytes(b.prefix(12)), aad: Bytes(key.utf8), Bytes(b.dropFirst(12))))
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
    case "scan":
      let prefix = op["prefix"] as? String ?? ""
      guard let enumerator = FileManager.default.enumerator(at: cache, includingPropertiesForKeys: [.isRegularFileKey]) else { throw TrommiError("storage") }
      let names = try enumerator.compactMap { entry -> String? in
        guard let url = entry as? URL, url.lastPathComponent == "value", try url.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile == true else { return nil }
        // Enumeration may canonicalize /var to /private/var on Darwin. Its depth, unlike the absolute
        // path prefix, always names exactly the components beneath this cache root.
        return url.pathComponents.suffix(enumerator.level).dropLast().joined()
      }
      let keys = names.compactMap { name -> String? in
        guard name.count % 2 == 0 else { return nil }
        var data = Data(); var at = name.startIndex
        while at < name.endIndex { let end = name.index(at, offsetBy: 2); guard let b = UInt8(name[at..<end], radix: 16) else { return nil }; data.append(b); at = end }
        return String(data: data, encoding: .utf8)
      }.filter { $0.hasPrefix(prefix) }.sorted()
      var entries: [[String: Any]] = []
      for key in keys { if let value = try get(key) { entries.append(["key": key, "value": bytes(value)]) } }
      answer = ["answer": "entries", "entries": entries]
    case "put", "put_durable":
      let durable = op["op"] as? String == "put_durable"
      for key in op["delete"] as? [String] ?? [] { let u = file(key); if FileManager.default.fileExists(atPath: u.path) { try FileManager.default.removeItem(at: u); if durable { try DeviceStore.syncDirectory(u.deletingLastPathComponent()) } } }
      for entry in op["put"] as? [[String: Any]] ?? [] {
        guard let key = entry["key"] as? String else { throw TrommiError("bad-format") }
        let nonce = try LocalKey.random(12)
        let sealed = nonce + (try gcmSeal(key: cacheKey, nonce: nonce, aad: Bytes(key.utf8), Bytes(packet.bytes(entry["value"]))))
        try FileManager.default.createDirectory(at: file(key).deletingLastPathComponent(), withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        try Data(sealed).write(to: file(key), options: .atomic)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file(key).path)
        if durable {
          try DeviceStore.syncFile(file(key))
          var folder = file(key).deletingLastPathComponent()
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
