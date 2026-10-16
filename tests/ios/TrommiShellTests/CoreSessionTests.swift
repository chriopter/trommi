import XCTest
@testable import TrommiClient

private actor Drawn {
  var model = CoreView.ViewModel()
  var error = false
  func take(_ packet: NativePacket) {
    guard packet.json["type"] as? String == "render" else { return }
    do { let data = try JSONSerialization.data(withJSONObject: packet.json["delta"]!); try model.apply(JSONDecoder().decode(CoreView.ViewDelta.self, from: data)) }
    catch { self.error = true }
  }
  func value() -> CoreView.ViewModel { model }
  func failed() -> Bool { error }
}
final class CoreSessionTests: XCTestCase {
  func testTheNativeExecutorFoldsExactlyTheCoresViewAndKeepsOneOwner() async throws {
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent("trommi-shell-\(UUID().uuidString)")
    defer { try? FileManager.default.removeItem(at: directory) }
    let drawn = Drawn()
    let session = try CoreSession(directory: directory) { packet in await drawn.take(packet) }
    await session.send(NativePacket(["type": "start", "config": ["platform": "test", "version": "0", "seed": 7, "can": ["secrets": true]]]))
    let view = await drawn.value()
    XCTAssertEqual(view.phase, .signedOut)
    let json = try await session.viewJSON()
    XCTAssertEqual(view, try JSONDecoder().decode(CoreView.ViewModel.self, from: Data(json.utf8)))
    let failed = await drawn.failed(); XCTAssertFalse(failed)
    XCTAssertThrowsError(try CoreSession(directory: directory) { _ in })
    await session.close()
    let again = try CoreSession(directory: directory) { _ in }
    await again.close()
  }
  func testTheBundledDemoRunsWithoutADeviceStore() async throws {
    var root = URL(fileURLWithPath: #filePath)
    for _ in 0..<4 { root.deleteLastPathComponent() }
    let fixture = try JSONSerialization.jsonObject(with: Data(contentsOf: root.appendingPathComponent("demo/data/fixture.json")))
    let drawn = Drawn()
    let session = try CoreSession(directory: nil) { packet in await drawn.take(packet) }
    await session.send(NativePacket(["type": "start", "config": ["platform": "test", "version": "0", "seed": 7, "demo": fixture]]))
    let view = await drawn.value()
    XCTAssertEqual(view.phase, .live)
    XCTAssertFalse(view.desk.rows.isEmpty)
    let failed = await drawn.failed(); XCTAssertFalse(failed)
    await session.close()
  }
  func testCacheKeysLongerThanAFilesystemComponentCanBeReadScannedAndDeleted() throws {
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent("trommi-long-cache-\(UUID().uuidString)")
    defer { try? FileManager.default.removeItem(at: directory) }
    let store = try ShellStore(directory: directory); defer { store.close() }
    let key = "c1/" + String(repeating: "ab", count: 32) + "/model/item/s:" + String(repeating: "cd", count: 32) + "/n123"
    _ = try store.run(NativePacket(["op": "put", "put": [["key": key, "value": 0]]], blobs: [Data("private".utf8)]))
    let read = try store.run(NativePacket(["op": "get", "keys": [key]]))
    XCTAssertEqual(read.blobs, [Data("private".utf8)])
    let scan = try store.run(NativePacket(["op": "scan", "prefix": "c1/"]))
    XCTAssertEqual((scan.json["entries"] as? [[String: Any]])?.first?["key"] as? String, key)
    _ = try store.run(NativePacket(["op": "put", "delete": [key]]))
    XCTAssertTrue(try store.run(NativePacket(["op": "scan", "prefix": ""])).blobs.isEmpty)
  }
  func testCachePrefixKeysCanCoexistAtAPathComponentBoundary() throws {
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent("trommi-prefix-cache-\(UUID().uuidString)")
    defer { try? FileManager.default.removeItem(at: directory) }
    let store = try ShellStore(directory: directory); defer { store.close() }
    let key = "c1/" + String(repeating: "a", count: 57)
    let keys = [key, key + "/child", ""]
    for key in keys { _ = try store.run(NativePacket(["op": "put", "put": [["key": key, "value": 0]]], blobs: [Data(key.utf8)])) }
    let read = try store.run(NativePacket(["op": "get", "keys": keys]))
    XCTAssertEqual(read.blobs, keys.map { Data($0.utf8) })
    let scan = try store.run(NativePacket(["op": "scan", "prefix": ""]))
    XCTAssertEqual(Set((scan.json["entries"] as? [[String: Any]] ?? []).compactMap { $0["key"] as? String }), Set(keys))
  }
  func testCacheNamesIncludeUnreadableValuesWithoutReadingOrReturningThem() throws {
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent("trommi-cache-names-\(UUID().uuidString)")
    defer { try? FileManager.default.removeItem(at: directory) }
    let store = try ShellStore(directory: directory); defer { store.close() }
    let key = "c1/private/corrupt"
    _ = try store.run(NativePacket(["op": "put", "put": [["key": key, "value": 0]]], blobs: [Data("private cached value".utf8)]))
    let cache = directory.appendingPathComponent("cache")
    for case let file as URL in FileManager.default.enumerator(at: cache, includingPropertiesForKeys: [.isRegularFileKey])! {
      if file.lastPathComponent == "value" { try Data([1, 2, 3]).write(to: file) }
    }
    let answer = try store.run(NativePacket(["op": "keys", "prefix": "c1/"]))
    XCTAssertEqual(answer.json["answer"] as? String, "keys")
    XCTAssertEqual(answer.json["keys"] as? [String], [key])
    XCTAssertTrue(answer.blobs.isEmpty)
    _ = try store.run(NativePacket(["op": "put", "delete": [key]]))
    XCTAssertEqual(try store.run(NativePacket(["op": "keys", "prefix": ""])).json["keys"] as? [String], [])
  }
  func testCachePrefixesDistinguishUtf8NamesAndCrossLongPathBoundaries() throws {
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent("trommi-unicode-prefix-\(UUID().uuidString)")
    defer { try? FileManager.default.removeItem(at: directory) }
    let store = try ShellStore(directory: directory); defer { store.close() }
    let keys = ["é/one", "e\u{301}/two", String(repeating: "雪", count: 80) + "/child"]
    for (index, key) in keys.enumerated() {
      _ = try store.run(NativePacket(["op": "put", "put": [["key": key, "value": 0]]], blobs: [Data([UInt8(index)])]))
    }
    XCTAssertEqual(try store.run(NativePacket(["op": "scan", "prefix": "é/"])).blobs, [Data([0])])
    XCTAssertEqual(try store.run(NativePacket(["op": "scan", "prefix": "e\u{301}/"])).blobs, [Data([1])])
    XCTAssertEqual(try store.run(NativePacket(["op": "scan", "prefix": String(repeating: "雪", count: 61)])).blobs, [Data([2])])
    XCTAssertTrue(try store.run(NativePacket(["op": "scan", "prefix": String(repeating: "雪", count: 81)])).blobs.isEmpty)
  }
  func testCachePrefixScansAndEmptyDeviceStartupWithUnrelatedRecords() async throws {
    let rows = max(1, min(100_000, Int(ProcessInfo.processInfo.environment["TROMMI_STORE_BENCH_ROWS"] ?? "128") ?? 128))
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent("trommi-prefix-bench-\(UUID().uuidString)")
    defer { try? FileManager.default.removeItem(at: directory) }
    let store = try ShellStore(directory: directory)
    let privatePrefix = "c1/" + String(repeating: "ab", count: 32) + "/model/card/"
    let publicKey = "public-departure/fixture-room.device"
    let entries: [[String: Any]] = (0..<rows).map { ["key": privatePrefix + String(format: "%08d", $0), "value": 0] }
    _ = try store.run(NativePacket(["op": "put", "put": entries + [["key": publicKey, "value": 0]]], blobs: [Data("isolated benchmark record".utf8)]))
    func elapsed(_ since: UInt64) -> Double { Double(DispatchTime.now().uptimeNanoseconds - since) / 1_000_000 }
    var measurements: [String: Any] = ["rows": rows]
    for (name, prefix, count) in [("public_hit_ms", "public-departure/", 1), ("public_miss_ms", "public-departure/no-such-room", 0), ("full_scan_ms", "", rows + 1), ("names_only_ms", "", rows + 1)] {
      var samples = [Double]()
      for _ in 0..<5 {
        let began = DispatchTime.now().uptimeNanoseconds
        let answer = try store.run(NativePacket(["op": name == "names_only_ms" ? "keys" : "scan", "prefix": prefix]))
        samples.append(elapsed(began))
        if name == "names_only_ms" {
          XCTAssertEqual((answer.json["keys"] as? [String])?.count, count)
          XCTAssertTrue(answer.blobs.isEmpty)
        } else { XCTAssertEqual((answer.json["entries"] as? [[String: Any]])?.count, count) }
      }
      measurements[name] = samples
    }
    store.close()
    let drawn = Drawn()
    let began = DispatchTime.now().uptimeNanoseconds
    let session = try CoreSession(directory: directory) { packet in await drawn.take(packet) }
    measurements["construct_ms"] = elapsed(began)
    await session.send(NativePacket(["type": "start", "config": ["platform": "test", "version": "0", "seed": 7]]))
    measurements["construct_to_signed_out_ms"] = elapsed(began)
    let model = await drawn.value()
    XCTAssertEqual(model.phase, .signedOut)
    await session.close()
    let data = try JSONSerialization.data(withJSONObject: measurements, options: [.sortedKeys])
    print("TROMMI_CACHE_PREFIX_BENCH " + String(decoding: data, as: UTF8.self))
  }
  func testDeletingCacheRecordsDoesNotLeaveAScanOfTheirEmptyPaths() throws {
    let rows = Int(ProcessInfo.processInfo.environment["TROMMI_DELETE_BENCH_ROWS"] ?? "32") ?? 32
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent("trommi-delete-bench-\(UUID().uuidString)")
    defer { try? FileManager.default.removeItem(at: directory) }
    let store = try ShellStore(directory: directory); defer { store.close() }
    let prefix = "private/" + String(repeating: "ab", count: 80) + "/"
    let keys = (0..<rows).map { prefix + String(format: "%08d", $0) }
    let keep = prefix + "keep"
    _ = try store.run(NativePacket(["op": "put", "put": (keys + [keep]).map { ["key": $0, "value": 0] }], blobs: [Data([42])]))
    _ = try store.run(NativePacket(["op": "put", "delete": keys]))
    var scans = [Double]()
    for _ in 0..<5 {
      let began = DispatchTime.now().uptimeNanoseconds
      let answer = try store.run(NativePacket(["op": "keys", "prefix": ""]))
      scans.append(Double(DispatchTime.now().uptimeNanoseconds - began) / 1_000_000)
      XCTAssertEqual(answer.json["keys"] as? [String], [keep])
    }
    XCTAssertEqual(try store.run(NativePacket(["op": "get", "keys": [keep]])).blobs, [Data([42])])
    let data = try JSONSerialization.data(withJSONObject: ["rows": rows, "scan_ms": scans], options: [.sortedKeys])
    print("TROMMI_DELETED_CACHE_BENCH " + String(decoding: data, as: UTF8.self))
    _ = try store.run(NativePacket(["op": "put_durable", "delete": [keep]]))
    let paths = FileManager.default.enumerator(at: directory.appendingPathComponent("cache"), includingPropertiesForKeys: nil)!
    XCTAssertNil(paths.nextObject(), "Deleting the last value must prune empty encoded paths, keeping the cache root.")
  }
  func testDurableCacheWritesAndDeletesSurviveReopening() throws {
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent("trommi-durable-cache-\(UUID().uuidString)")
    defer { try? FileManager.default.removeItem(at: directory) }
    let key = "public-departure/" + String(repeating: "a", count: 87)
    let value = Data("public signed retirement".utf8)
    var store = try ShellStore(directory: directory)
    _ = try store.run(NativePacket(["op": "put_durable", "put": [["key": key, "value": 0]]], blobs: [value]))
    store.close(); store = try ShellStore(directory: directory)
    XCTAssertEqual(try store.run(NativePacket(["op": "get", "keys": [key]])).blobs, [value])
    _ = try store.run(NativePacket(["op": "put_durable", "delete": [key]]))
    store.close(); store = try ShellStore(directory: directory)
    XCTAssertTrue(try store.run(NativePacket(["op": "scan", "prefix": "public-departure/"])).blobs.isEmpty)
    store.close()
  }
  func testTheDeviceSpaceIsDurableAndTheCacheIsEncrypted() throws {
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent("trommi-shell-store-\(UUID().uuidString)")
    defer { try? FileManager.default.removeItem(at: directory) }
    var store = try ShellStore(directory: directory)
    _ = try store.run(NativePacket(["op": "device_load"]))
    _ = try store.run(NativePacket(["op": "device_apply", "expected_revision": 0, "put": [["key": 0, "value": 1]]], blobs: [Data([1]), Data("private device state".utf8)]))
    _ = try store.run(NativePacket(["op": "put", "put": [["key": "sensitive/cache", "value": 0]]], blobs: [Data("private cached title".utf8)]))
    let cache = directory.appendingPathComponent("cache")
    for case let file as URL in FileManager.default.enumerator(at: cache, includingPropertiesForKeys: [.isRegularFileKey])! {
      guard try file.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile == true else { continue }
      let bytes = try Data(contentsOf: file); XCTAssertNil(bytes.range(of: Data("private cached title".utf8)))
    }
    store.close(); store = try ShellStore(directory: directory)
    let loaded = try store.run(NativePacket(["op": "device_load"]))
    XCTAssertEqual((loaded.json["revision"] as? NSNumber)?.uint64Value, 1)
    XCTAssertTrue(loaded.blobs.contains(Data("private device state".utf8)))
    let read = try store.run(NativePacket(["op": "get", "keys": ["sensitive/cache"]]))
    XCTAssertEqual(read.blobs, [Data("private cached title".utf8)])
    XCTAssertThrowsError(try store.run(NativePacket(["op": "device_apply", "expected_revision": 0])))
    store.close()
  }
}
