// ReadBackScopeTests: keys that came read the past back only when they open what waits (a passed over or closed
// item's group and epoch), and a read back takes what this device holds from its cache and the waiting items from
// the bytes kept beside it, without reading the hub's changes again from the start. Before (12 October), every
// handover with a new key (each new session) started a full read back of a big room from the hub.
import XCTest
@testable import TrommiClient

@MainActor
final class ReadBackScopeTests: XCTestCase {
  var base: URL!
  let hubURL = "http://127.0.0.1:9"
  let agent = String(repeating: "ab", count: 32), session = String(repeating: "cd", count: 16), other = String(repeating: "ef", count: 16)

  override func setUp() async throws {
    base = FileManager.default.temporaryDirectory.appendingPathComponent("trommi-read-back-scope-\(UUID().uuidString)")
    Core.tools = FakeTools()
    HubClient.transportForTests = [FakeHub.self]
    FakeHub.shared = FakeHub.Hub()
  }
  override func tearDown() async throws { try? FileManager.default.removeItem(at: base) }

  private func texts(_ room: Room) -> [String] { room.board.timelines.values.flatMap { $0.items.values.compactMap { $0.content?["text"].string } }.sorted() }
  private var fromStart: Int { FakeHub.shared.lock.withLock { FakeHub.shared.changesFromStart } }

  func testOnlyKeysThatOpenWhatWaitsReadBackAndTheReadBackTakesTheCache() async throws {
    let room = try await Room.foundRoom(hubURL: hubURL, base: base).room
    let hub = FakeHub.shared
    hub.commit(group: room.roomId, ["session": session, "agent": agent])
    hub.commit(group: room.roomId, ["session": other, "agent": agent])
    let group = FakeDevice.sessionGroup(room.roomIdHex, session), otherGroup = FakeDevice.sessionGroup(room.roomIdHex, other)
    hub.envelope(["sender": agent, "group": hex(group), "seq": 1, "time": 1001, "kind": KIND.TIMELINE_ITEM, "tk": TIMELINE.CHAT, "ts": TIMELINE_SCOPE.SESSION, "tr": session, "files": [String](), "before": true],
                 payload: ["content_type": "message", "text": "before this device came"])
    hub.envelope(["sender": agent, "group": hex(otherGroup), "seq": 1, "time": 1002, "kind": KIND.TIMELINE_ITEM, "tk": TIMELINE.CHAT, "ts": TIMELINE_SCOPE.SESSION, "tr": other, "files": [String]()],
                 payload: ["content_type": "message", "text": "held whole"])
    _ = try await room.sync()
    XCTAssertEqual(room.record.past?.passedOver, true)
    XCTAssertEqual(room.record.past?.waiting, ["\(hex(group))/1"])
    XCTAssertEqual(texts(room), ["held whole"])
    _ = Spent.take()

    // keys of another group: nothing that waits opens, no read back
    hub.message(group: otherGroup, ["keys": hex(otherGroup)])
    _ = try await room.sync()
    _ = try await room.sync()
    XCTAssertFalse(Spent.take().contains("read backs"), "keys that open nothing that waits read nothing back")
    XCTAssertEqual(room.record.past?.passedOver, true)

    // the keys of before: read back, from the cache and the kept bytes
    let before = fromStart
    hub.message(group: group, ["keys": hex(group)])
    _ = try await room.sync()
    _ = try await room.sync()
    let spent = Spent.take()
    XCTAssertTrue(spent.contains("read backs from the cache"), spent)
    XCTAssertEqual(fromStart, before, "nothing read again from the hub")
    XCTAssertEqual(texts(room), ["before this device came", "held whole"])
    XCTAssertNotEqual(room.record.past?.passedOver, true)
    XCTAssertEqual(room.record.past?.readBack, false)
    await room.saveCacheAndWait()
    room.close()

    // and the board read back is what the next launch restores
    let again = try Room.open(base: base, roomId: room.roomIdHex)
    _ = await again.restore()
    XCTAssertEqual(texts(again), ["before this device came", "held whole"])
    again.close()
  }
}
