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
