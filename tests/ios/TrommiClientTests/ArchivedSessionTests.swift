// Only active sessions are loaded (spec/hub-api.md point 46): the catch-up and the Desk ask with `active=1`, a session
// archived while the device was away is held as archived from the hub's list, and its envelopes come when its page is
// opened, and again after a read back. FakeCore and a hub in the process.
import XCTest
@testable import TrommiClient

@MainActor
final class ArchivedSessionTests: XCTestCase {
  var base: URL!
  let hubURL = "http://127.0.0.1:9"
  let agent = String(repeating: "ab", count: 32), session = String(repeating: "cd", count: 16)

  override func setUp() async throws {
    base = FileManager.default.temporaryDirectory.appendingPathComponent("trommi-archived-\(UUID().uuidString)")
    Core.tools = FakeTools()
    HubClient.transportForTests = [FakeHub.self]
    FakeHub.shared = FakeHub.Hub()
  }
  override func tearDown() async throws { try? FileManager.default.removeItem(at: base) }

  func agentSays(_ room: Room, seq: Int, _ text: String) {
    FakeHub.shared.envelope(["sender": agent, "group": hex(FakeDevice.sessionGroup(room.roomIdHex, session)), "seq": seq, "time": 1000 + seq, "kind": KIND.TIMELINE_ITEM,
                             "tk": TIMELINE.CHAT, "ts": TIMELINE_SCOPE.SESSION, "tr": session, "files": [String]()], payload: ["content_type": "message", "text": text])
  }
  func texts(_ room: Room) -> [String] {
    let t = room.board.timelineOf("chat:session/\(session)")
    return t.items.keys.sorted().compactMap { t.items[$0]?.content?["text"].string }
  }
  func asked(_ path: String) -> Int { FakeHub.shared.lock.withLock { FakeHub.shared.posted.filter { $0.path == path }.count } }

  func testAnArchivedSessionIsLoadedWhenOpenedAndAgainAfterAReadBack() async throws {
    let room = try await Room.foundRoom(hubURL: hubURL, base: base).room
    defer { room.close() }
    FakeHub.shared.commit(group: room.roomId, ["session": session, "agent": agent])
    agentSays(room, seq: 1, "one"); agentSays(room, seq: 2, "two")
    // archived at the hub while this device was away
    let group = hex(FakeDevice.sessionGroup(room.roomIdHex, session))
    FakeHub.shared.lock.withLock { _ = FakeHub.shared.archivedGroups.insert(group) }

    _ = try await room.sync()
    XCTAssertEqual(texts(room), [], "the catch-up left the archived session's envelopes out")
    XCTAssertEqual(room.groups.values.first { $0.session != nil }?.archived, true, "held as archived from the hub's list")
    XCTAssertGreaterThan(room.cursor, 0)

    // opened: its envelopes, in one request over its group alone
    let before = asked("/v1/changes")
    _ = try? await room.loadOlder("chat:session/\(session)")   // (the fake hub serves no Chat pages: the session is loaded first)
    XCTAssertEqual(texts(room), ["one", "two"])
    XCTAssertEqual(asked("/v1/changes") - before, 1, "one request for the whole session")
    XCTAssertEqual(room.record.archivedOpened, [session])
    // once per run
    let again = try await room.loadArchived(session)
    XCTAssertEqual(again, 0)

    // a read back empties the board and reads the live sessions alone: the opened archived session comes again
    var report = SyncReport(), change = Change()
    room.board.reset()
    XCTAssertEqual(texts(room), [])
    await room.reloadArchived(&report, &change)
    XCTAssertEqual(texts(room), ["one", "two"])
  }
}
