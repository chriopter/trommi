// ChainGapTests: an envelope whose sender's chain misses what stands before it (`gap`: written before this device came
// into the group, or passed over) is not dropped: the chain is fetched from where this device holds it, taken in
// order, and the envelope after it too (9.0.6), as the web engine's `fillGap` does. Found by the mega test of the
// clients (tests/client/mega.rs): an iPhone that a Welcome let into a helper session missed its cards for good.
import XCTest
@testable import TrommiClient

@MainActor
final class ChainGapTests: XCTestCase {
  var base: URL!
  let hubURL = "http://127.0.0.1:9"
  let agent = String(repeating: "ab", count: 32), session = String(repeating: "cd", count: 16)

  override func setUp() async throws {
    base = FileManager.default.temporaryDirectory.appendingPathComponent("trommi-chain-gap-\(UUID().uuidString)")
    Core.tools = FakeTools()
    HubClient.transportForTests = [FakeHub.self]
    FakeHub.shared = FakeHub.Hub()
  }
  override func tearDown() async throws { try? FileManager.default.removeItem(at: base) }

  private func says(_ group: String, seq: Int, _ text: String) -> [String: Any] {
    ["sender": agent, "group": group, "seq": seq, "time": 1000 + seq, "kind": KIND.TIMELINE_ITEM, "tk": TIMELINE.CHAT, "ts": TIMELINE_SCOPE.SESSION, "tr": session, "files": [String]()]
  }

  func testAGapInASendersChainIsFilledFromTheHub() async throws {
    let room = try await Room.foundRoom(hubURL: hubURL, base: base).room
    let hub = FakeHub.shared
    hub.commit(group: room.roomId, ["session": session, "agent": agent])
    let group = hex(FakeDevice.sessionGroup(room.roomIdHex, session))
    // the agent's first two lines are in the hub's order before this device holds the group's past (the hub serves
    // them on the sender's chain); the third comes in the order
    var chain = [[String: Any]]()
    for seq in 1...2 {
      let bytes = j(["h": says(group, seq: seq, "line \(seq)"), "p": hex(j(["content_type": "message", "text": "line \(seq)"]))])
      chain.append(["seq": seq, "change": seq, "envelope": b64u(bytes)])
    }
    hub.lock.withLock { hub.chains["\(group)/\(agent)"] = chain }
    hub.envelope(says(group, seq: 3, "line 3"), payload: ["content_type": "message", "text": "line 3"])
    _ = try await room.sync()
    let cut = try await room.onCore { try $0.cutOf(group: try unhex(group), device: try unhex(self.agent)) }
    XCTAssertEqual(cut.seq, 3, "the chain was filled and the envelope after the gap taken")
    XCTAssertFalse(room.board.alerts.contains { $0.code == "gap" }, "no gap is said: it was filled")
    room.close()
  }
}
