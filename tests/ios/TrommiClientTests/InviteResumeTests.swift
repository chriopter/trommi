// InviteResumeTests: "Pair a device" (or a reinstall, which pairs anew) in a room that has sessions. After "They match"
// the newcomer is added to every live session group (5.2.7), one KeyPackage each; but the newcomer puts its
// KeyPackages up only once it is in the room, so the first claim may find none (the hub's 404 "no KeyPackage for a
// device"). That is no failure of the pairing: the step stays with the core, and the upkeep takes it on at the next
// sync, also after the app was closed and opened again (the web's engine.ts `invited`).
import XCTest
@testable import TrommiClient

@MainActor
final class InviteResumeTests: XCTestCase {
  var base: URL!
  let agent = String(repeating: "ab", count: 32), session = String(repeating: "cd", count: 16)
  let newcomer = Bytes(repeating: 0x5a, count: 32), invite = Bytes(repeating: 0x17, count: 16)

  override func setUp() async throws {
    base = FileManager.default.temporaryDirectory.appendingPathComponent("trommi-invite-\(UUID().uuidString)")
    Core.tools = FakeTools()
    HubClient.transportForTests = [FakeHub.self]
    FakeHub.shared = FakeHub.Hub()
  }
  override func tearDown() async throws { try? FileManager.default.removeItem(at: base) }

  /** A room of this device with one live session, and an invite of it that still has to add the newcomer there. */
  private func roomWithOpenAdd() async throws -> (Room, FakeDevice, GroupId) {
    let room = try await Room.foundRoom(hubURL: "http://127.0.0.1:9", base: base).room
    FakeHub.shared.commit(group: room.roomId, ["session": session, "agent": agent])
    _ = try await room.sync()
    let device = try XCTUnwrap(room.device as? FakeDevice)
    let group = FakeDevice.sessionGroup(room.roomIdHex, session)
    try device.inviteAddsForTest(invite: invite, group: group, device: newcomer)
    return (room, device, group)
  }
  private func until(_ what: String, _ ok: () -> Bool) async throws {
    for _ in 0..<250 where !ok() { try await Task.sleep(nanoseconds: 20_000_000) }
    XCTAssertTrue(ok(), what)
  }

  /** The newcomer has no KeyPackage at the hub yet: "They match" ends without an error; once it has, the next sync
   *  adds it to the session. */
  func testAMissingKeyPackageWaitsAndTheNextSyncAddsTheDevice() async throws {
    let (room, device, group) = try await roomWithOpenAdd()
    var thrown: Error?
    do { try await room.finishInvite(invite) } catch { thrown = error }
    XCTAssertNil(thrown, "no KeyPackage yet is no failure of the pairing")
    XCTAssertTrue(device.added.isEmpty)
    XCTAssertEqual(try device.inviteSteps().count, 1, "the step stays with the core")
    // the newcomer joined the room and put its KeyPackages up
    FakeHub.shared.lock.withLock { _ = FakeHub.shared.keyPackages.insert(hex(newcomer)) }
    _ = try await room.sync()
    try await until("the upkeep adds the newcomer") { device.added.contains { $0.device == newcomer && $0.group == group } }
    try await until("no step left") { (try? device.inviteSteps().isEmpty) == true }
    XCTAssertEqual(device.added.count, 1, "added once")
    room.close()
  }

  /** The app was closed before the step could be done: the next launch takes it on. */
  func testAnOpenStepIsTakenOnAfterALaunch() async throws {
    let (room, _, group) = try await roomWithOpenAdd()
    try? await room.finishInvite(invite)
    let id = room.roomIdHex
    room.close()
    FakeHub.shared.lock.withLock { _ = FakeHub.shared.keyPackages.insert(hex(newcomer)) }
    let again = try Room.open(base: base, roomId: id)
    let device = try XCTUnwrap(again.device as? FakeDevice)
    XCTAssertEqual(try device.inviteSteps().count, 1, "kept with the device's state")
    _ = try await again.sync()
    try await until("the step is taken on after the launch") { device.added.contains { $0.device == newcomer && $0.group == group } }
    again.close()
  }

  /** While the hub holds none, the upkeep keeps looking, without a new sync: the device is added once it has one. */
  func testTheUpkeepLooksAgainUntilTheKeyPackageCame() async throws {
    let (room, device, _) = try await roomWithOpenAdd()
    _ = try await room.sync()
    try await Task.sleep(nanoseconds: 300_000_000)
    XCTAssertTrue(device.added.isEmpty)
    FakeHub.shared.lock.withLock { _ = FakeHub.shared.keyPackages.insert(hex(newcomer)) }
    try await until("added by a later round") { device.added.contains { $0.device == newcomer } }
    room.close()
  }

  /** An agent the room enrols that sits in no session (its inviter went away after "They match") gets its session
   *  from this device, once it is connected and has waited; an agent with a session is left alone (engine.ts seatAgents). */
  func testAnAgentWithoutASessionGetsOneFromThisDevice() async throws {
    let room = try await Room.foundRoom(hubURL: "http://127.0.0.1:9", base: base).room
    let device = try XCTUnwrap(room.device as? FakeDevice)
    let lone = Bytes(repeating: 0x42, count: 32)
    FakeHub.shared.commit(group: room.roomId, ["session": session, "agent": agent])
    FakeHub.shared.lock.withLock { _ = FakeHub.shared.keyPackages.insert(hex(lone)); _ = FakeHub.shared.keyPackages.insert(agent) }
    device.roomAgentsForTest = [lone, try unhex(agent)]
    room.seatGraceMs = 0
    var ch = Change()
    room.board.applyMembers([(id: hex(lone), role: "agent", active: true, added: 1, removed: nil)], change: &ch)
    room.board.members[hex(lone)]?.isOnline = true
    _ = try await room.sync()
    try await until("its session is founded") { device.founded.contains(lone) }
    try await Task.sleep(nanoseconds: 300_000_000)
    XCTAssertEqual(device.founded.map(hex), [hex(lone)], "once, and none for the agent that has its session")
    room.close()
  }
}
