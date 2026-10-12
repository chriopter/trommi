// PassedOverReadBackTests: an envelope written in an epoch before this device came into a session group is passed
// over (`group-behind`); when the keys of before come by a handover, the passed over changes are read back and it
// opens, as the web engine rescans on every handover that brought keys. Found by the mega test of the clients
// (tests/client/mega.rs): an iPhone signed in with the code, whose session a Welcome won, never showed that helper
// session's name.
import XCTest
@testable import TrommiClient

@MainActor
final class PassedOverReadBackTests: XCTestCase {
  var base: URL!
  let hubURL = "http://127.0.0.1:9"
  let agent = String(repeating: "ab", count: 32), session = String(repeating: "cd", count: 16)

  override func setUp() async throws {
    base = FileManager.default.temporaryDirectory.appendingPathComponent("trommi-passed-over-\(UUID().uuidString)")
    Core.tools = FakeTools()
    HubClient.transportForTests = [FakeHub.self]
    FakeHub.shared = FakeHub.Hub()
  }
  override func tearDown() async throws { try? FileManager.default.removeItem(at: base) }

  func testWhatWasPassedOverOpensWhenTheKeysOfBeforeCome() async throws {
    let room = try await Room.foundRoom(hubURL: hubURL, base: base).room
    let hub = FakeHub.shared
    hub.commit(group: room.roomId, ["session": session, "agent": agent])
    let group = FakeDevice.sessionGroup(room.roomIdHex, session)
    hub.envelope(["sender": agent, "group": hex(group), "seq": 1, "time": 1001, "kind": KIND.TIMELINE_ITEM, "tk": TIMELINE.CHAT, "ts": TIMELINE_SCOPE.SESSION, "tr": session, "files": [String](), "before": true],
                 payload: ["content_type": "message", "text": "before this device came"])
    let first = try await room.sync()
    XCTAssertEqual(first.beforeJoining, 1, "written before this device came: passed over")
    XCTAssertEqual(room.record.past?.passedOver, true)
    XCTAssertEqual(texts(room), [])

    hub.message(group: group, ["keys": hex(group)])
    _ = try await room.sync()
    _ = try await room.sync()
    XCTAssertEqual(texts(room), ["before this device came"], "read back once the keys came")
    XCTAssertNotEqual(room.record.past?.passedOver, true, "nothing is left to do")
    room.close()
  }

  private func texts(_ room: Room) -> [String] { room.board.timelines.values.flatMap { $0.items.values.compactMap { $0.content?["text"].string } } }
}
