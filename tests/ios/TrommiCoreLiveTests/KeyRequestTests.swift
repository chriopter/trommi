// Key requests (spec/v1.md 7.1.1) with the engine (`Room`), the real core and `PocketHub` behind the hub's routes: a
// phone let in by link did not take its key handover before the hub swept it (application messages are kept 30
// days). It asks the human devices for the keys; the engine of the other human device answers from the hub's list
// with a key handover in the room group, and the phone reads what was written before it came.
import Foundation
import XCTest
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif
@testable import TrommiClient
@testable import TrommiCoreLive

@MainActor
final class KeyRequestTests: XCTestCase {
  let tools = LiveCore()
  var hub = PocketHub()
  private var transport: [AnyClass] = []

  override func setUp() async throws {
    Core.tools = tools
    transport = HubClient.transportForTests
    hub = PocketHub()
    PocketRoutes.install(hub)
  }
  override func tearDown() async throws { HubClient.transportForTests = transport }

  private func newDevice() throws -> LiveDevice {
    try XCTUnwrap(try tools.createDevice(store: DeviceStore(directory: try scratchFolder(self), key: systemRandom(32))) as? LiveDevice)
  }

  /// Every stored envelope again, in the hub's order and then each for its body, as the apps read back once keys
  /// came. How many did not open for lack of a key.
  private func readAgain(_ device: LiveDevice) throws -> Int {
    var noKey = 0
    for ordered in [true, false] {
      for stored in hub.envelopes {
        let got = try device.receiveEnvelope(stored.bytes, change: stored.change, ordered: ordered, voidCode: nil, nowMs: nowMs())
        if !ordered && got.code == "no-key" { noKey += 1 }
      }
    }
    return noKey
  }

  func testAPhoneThatMissedItsHandoverAsksAndTheEngineAnswers() async throws {
    // A founds the room and writes a desk; its device is the engine's.
    let store = try Store.new(base: try scratchFolder(self))
    let state = try store.openState(create: true)
    let device = try XCTUnwrap(try tools.createDevice(store: state) as? LiveDevice)
    let room = try device.foundRoom(recoveryCode: try tools.generateRecoveryCode(), nowMs: nowMs())
    try hub.post(device)
    let desk = utf8(#"{"created_at":1,"name":"Work"}"#)
    _ = try device.seal(.register(group: room, name: "desk/work", value: desk), files: [], nowMs: nowMs())
    try hub.post(device)

    // The phone comes by link and is handed the keys; it is away until the hub swept the handover.
    let phone = try newDevice()
    let invite = try exchangeInvite(from: device, to: phone, tools: tools)
    _ = try device.inviteConfirm(invite: invite.invite, numbers: invite.inviterShows, requestHash: invite.requestHash, matches: true, nowMs: nowMs())
    try hub.post(device)
    _ = try phone.joinInvited(try XCTUnwrap(hub.welcomes.last), nowMs: nowMs())
    _ = try device.inviteHandover(invite: invite.invite)
    try hub.post(device)
    hub.log.removeAll { if case .message = $0.kind { return true } else { return false } }

    let record = RoomRecord(hubURL: PocketRoutes.url, roomId: hex(room), myDeviceId: hex(device.id), role: "human", deviceRegisterSent: false, past: nil)
    try store.save(record)
    let a = try Room(store: store, record: record, deviceStore: state, device: device)
    addTeardownBlock { await a.shutdown() }

    try hub.deliver(to: phone)
    let served = try hub.served(room)
    _ = try phone.learnHistory(group: room, founding: served.founding, commits: served.commits)
    XCTAssertGreaterThan(try readAgain(phone), 0, "the desk does not open without its key")
    XCTAssertNil(try phone.register(group: room, name: "desk/work"))

    // Not at once (the handover behind an Add comes within seconds); five minutes later, once.
    XCTAssertEqual(try phone.keyRequests(nowMs: nowMs()), [])
    let later = nowMs() + 5 * 60_000
    XCTAssertEqual(try phone.keyRequests(nowMs: later), [room])
    XCTAssertEqual(try phone.keyRequests(nowMs: later + 1000), [])
    PocketRoutes.wishes.append(["id": 1, "kind": "handover", "device": b64u(phone.id), "group_id": b64u(room), "at": nowMs()])

    // The engine reads the hub's list at its sync and answers with a handover in the room group.
    let before = hub.log.count
    _ = try await a.sync()
    for _ in 0..<200 where hub.log.count == before { try await Task.sleep(nanoseconds: 20_000_000) }
    XCTAssertGreaterThan(hub.log.count, before, "a key handover was posted")
    // (the wish is answered once, however often the engine syncs)
    let answered = hub.log.count
    _ = try await a.sync()
    try await Task.sleep(nanoseconds: 200_000_000)
    XCTAssertEqual(hub.log.count, answered)

    try hub.deliver(to: phone)
    XCTAssertEqual(try readAgain(phone), 0, "everything opens")
    XCTAssertEqual(try phone.register(group: room, name: "desk/work"), desk)
  }
}
