// CodeJoinPastTests: a catch-up that read past a session's envelopes before the sign-in's joins with the recovery code
// went through (the device was not in that group yet: passed over, `group-behind`) reads them back once the joins are
// through. Found by the mega test of the clients (tests/client/mega.rs): an iPhone signed in with the code never
// showed a helper session's name that its agent had written before.
import XCTest
@testable import TrommiClient

@MainActor
final class CodeJoinPastTests: XCTestCase {
  var base: URL!
  let hubURL = "http://127.0.0.1:9"
  let agent = String(repeating: "ab", count: 32), session = String(repeating: "cd", count: 16)

  override func setUp() async throws {
    base = FileManager.default.temporaryDirectory.appendingPathComponent("trommi-code-join-past-\(UUID().uuidString)")
    Core.tools = FakeTools()
    HubClient.transportForTests = [FakeHub.self]
    FakeHub.shared = FakeHub.Hub()
    FakeTools.codeJoinsSessions = [(session, agent)]
  }
  override func tearDown() async throws {
    FakeTools.codeJoinsSessions = nil
    try? FileManager.default.removeItem(at: base)
  }

  func testWhatWasPassedOverBeforeTheCodeJoinsIsReadBack() async throws {
    let room = try await Room.foundRoom(hubURL: hubURL, base: base).room
    let group = FakeDevice.sessionGroup(room.roomIdHex, session)
    FakeHub.shared.envelope(["sender": agent, "group": hex(group), "seq": 1, "time": 1001, "kind": KIND.TIMELINE_ITEM, "tk": TIMELINE.CHAT, "ts": TIMELINE_SCOPE.SESSION, "tr": session,
                             "files": [String](), "passed": true], payload: ["content_type": "message", "text": "before the join"])
    let first = try await room.sync()
    XCTAssertEqual(first.beforeJoining, 1, "not in the session's group yet: passed over")
    XCTAssertEqual(texts(room), [])

    room.record.codeJoin = true
    await room.finishCodeJoin(code: [1, 2, 3])
    XCTAssertNil(room.record.codeJoin, "the join finished")
    _ = try await room.sync()
    XCTAssertEqual(texts(room), ["before the join"], "read back once the session's group is held")
    XCTAssertNotEqual(room.record.past?.passedOver, true, "nothing is left to do")
    room.close()
  }

  private func texts(_ room: Room) -> [String] { room.board.timelines.values.flatMap { $0.items.values.compactMap { $0.content?["text"].string } } }
}
