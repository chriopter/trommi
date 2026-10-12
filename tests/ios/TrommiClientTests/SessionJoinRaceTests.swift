// SessionJoinRaceTests: a device that joins a session group after it read the hub's Desk (a sign-in's joins with the
// code, a Welcome of another human device) reads the Desk again, which lists that session's cards only now; and a
// sign-in whose session another human device added it to first (the Welcome won the race) asks for that session's
// keys at once, as the web client does (room.ts `joinSessions`, client.ts `deskSoon` on `groups`).
import XCTest
@testable import TrommiClient

@MainActor
final class SessionJoinRaceTests: XCTestCase {
  var base: URL!
  let hubURL = "http://127.0.0.1:9"
  let agent = String(repeating: "ab", count: 32), session = String(repeating: "cd", count: 16), card = String(repeating: "5", count: 32)

  override func setUp() async throws {
    base = FileManager.default.temporaryDirectory.appendingPathComponent("trommi-join-race-\(UUID().uuidString)")
    Core.tools = FakeTools()
    HubClient.transportForTests = [FakeHub.self]
    FakeHub.shared = FakeHub.Hub()
  }
  override func tearDown() async throws { try? FileManager.default.removeItem(at: base) }

  private func founded() async throws -> Room { try await Room.foundRoom(hubURL: hubURL, base: base).room }
  /** Another human device added this one to the session: its Welcome waits at the hub. The hub's Desk lists the
   *  session's open card once this device is in its group (after the Welcome was read). */
  private func welcomeWaits(_ room: Room) {
    let hub = FakeHub.shared
    hub.lock.withLock {
      hub.welcomes = [["group_id": b64u(FakeDevice.sessionGroup(room.roomIdHex, session)), "welcome": b64u(j(["session": session, "agent": agent, "by": String(repeating: "ef", count: 32)]))]]
      hub.deskCards = { [card] in hub.welcomesRead > 0 ? [card] : [] }
    }
  }
  private func handovers() -> [String] {
    FakeHub.shared.lock.withLock { FakeHub.shared.posted.filter { $0.path == "/v1/requests" && $0.body["kind"] as? String == "handover" }.compactMap { $0.body["group"] as? String } }
  }

  func testTheDeskIsReadAgainAfterASessionGroupWasJoined() async throws {
    let room = try await founded()
    welcomeWaits(room)
    _ = try await room.sync()
    XCTAssertEqual(room.sessionGroupIds, [hex(FakeDevice.sessionGroup(room.roomIdHex, session))])
    XCTAssertEqual(room.deskSeen?.open, [card], "the Desk read after the join lists the session's card: it is not held back as settled")
    XCTAssertEqual(FakeHub.shared.lock.withLock { FakeHub.shared.deskReads }, 2)
    room.close()
  }

  func testAWelcomeOnTheStreamReadsTheDeskAgain() async throws {
    let room = try await founded()
    FakeHub.shared.lock.withLock { FakeHub.shared.deskCards = { [] } }
    _ = try await room.sync()
    welcomeWaits(room)
    FakeHub.shared.lock.withLock { FakeHub.shared.welcomesRead = 0 }
    await room.handleStreamEvent("welcome", .obj([:]))
    XCTAssertEqual(room.deskSeen?.open, [card])
    room.close()
  }

  func testASignInWhoseSessionAWelcomeWonAsksForItsKeysAtOnce() async throws {
    let room = try await founded()
    room.record.codeJoin = true            // the code's joins of a sign-in are not through
    welcomeWaits(room)
    _ = try await room.sync()
    XCTAssertEqual(handovers(), [b64u(FakeDevice.sessionGroup(room.roomIdHex, session))], "asked for at once, not after the grace")
    room.close()
  }

  func testAWelcomeOutsideASignInWaitsForTheGrace() async throws {
    let room = try await founded()
    welcomeWaits(room)
    _ = try await room.sync()
    XCTAssertEqual(handovers(), [], "the core's grace decides (7.1.1)")
    room.close()
  }
}
