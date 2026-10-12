// WarmDeskTests: a launch from the cache shows only what the hub's Desk lists as open (the owner's rule: the Desk is
// the hub's open list). The first paint waits a moment for the hub's Desk (Room.holdDesk, waitForDesk); past it the
// cached Desk stands as the list kept from the last launch filters it, never an empty one (the owner's rule of 12
// October: only a first sign-in may show an empty Desk); and a card this device knew as settled only from the hub's
// list never comes back on a later launch, online or not (the list kept beside the cache, RoomCache.swift DeskKept).
import XCTest
@testable import TrommiClient

@MainActor
final class WarmDeskTests: XCTestCase {
  var base: URL!
  let hubURL = "http://127.0.0.1:9"
  let agent = String(repeating: "ab", count: 32), session = String(repeating: "cd", count: 16)
  let kept = String(repeating: "a1", count: 16), closedA = String(repeating: "a2", count: 16), status = String(repeating: "a3", count: 16)

  override func setUp() async throws {
    base = FileManager.default.temporaryDirectory.appendingPathComponent("trommi-warm-desk-\(UUID().uuidString)")
    Core.tools = FakeTools()
    HubClient.transportForTests = [FakeHub.self]
    FakeHub.shared = FakeHub.Hub()
  }
  override func tearDown() async throws { try? FileManager.default.removeItem(at: base) }

  private func asks(_ room: Room, seq: Int, card: String, _ title: String, type: String = "decision") {
    FakeHub.shared.envelope(["sender": agent, "group": hex(FakeDevice.sessionGroup(room.roomIdHex, session)), "seq": seq, "time": 1000 + seq, "kind": KIND.OBJECT_VERSION,
                             "otype": OBJECT_TYPE.CARD, "ostate": CARD_STATE.OPEN, "oid": card, "files": [String]()],
                            payload: ["object_type": "card", "object_version": 1, "card_type": type, "title": title, "options": type == "info" ? [[String: String]]() : [["key": "yes", "label": "Yes"]]])
  }
  private func shown(_ room: Room) -> Set<String> { Set(DeskModel(board: room.board).cards.filter { $0.status == "open" }.map(\.id)) }
  private func deskLists(_ ids: [String]) { FakeHub.shared.lock.withLock { FakeHub.shared.deskCards = { ids } } }

  /** Three cards seen open, then (while the device is away) a question and an old status info settled elsewhere:
   *  the hub's Desk lists only the one left. The closings never reach this device (as one it cannot read). */
  private func away() async throws -> String {
    let room = try await Room.foundRoom(hubURL: hubURL, base: base).room
    FakeHub.shared.commit(group: room.roomId, ["session": session, "agent": agent])
    asks(room, seq: 1, card: kept, "Still open?")
    asks(room, seq: 2, card: closedA, "Answered elsewhere?")
    asks(room, seq: 3, card: status, "Status 11:05: all rooms still working", type: "info")
    deskLists([kept, closedA, status])
    _ = try await room.sync()
    XCTAssertEqual(shown(room), [kept, closedA, status])
    await room.saveCacheAndWait()
    room.close()
    deskLists([kept])
    return room.roomIdHex
  }

  func testTheFirstPaintWaitsForTheHubsDeskAndShowsOnlyWhatItLists() async throws {
    let id = try await away()
    let room = try Room.open(base: base, roomId: id)
    let had = await room.restore()
    XCTAssertTrue(had)
    XCTAssertTrue(room.deskHolding, "the app draws nothing of the Desk yet")
    XCTAssertEqual(shown(room), [kept, closedA, status], "the cached Desk as the list kept from the last launch has it")
    _ = try await room.sync()
    XCTAssertFalse(room.deskHolding)
    XCTAssertFalse(room.board.deskWaiting)
    XCTAssertEqual(shown(room), [kept], "only what the hub's Desk lists")
    XCTAssertEqual(room.board.stack, [kept])
    await room.saveCacheAndWait()
    room.close()
  }

  func testPastTheWaitTheCachedDeskStandsUntilTheDeskAnswers() async throws {
    let id = try await away()
    let room = try Room.open(base: base, roomId: id)
    _ = await room.restore()
    let t = Date()
    await room.waitForDesk()
    XCTAssertGreaterThanOrEqual(Date().timeIntervalSince(t), 0.25, "the first paint waited a moment for the Desk")
    XCTAssertFalse(room.deskHolding, "drawn now, with a quiet \"Catching up…\"")
    XCTAssertFalse(room.board.deskWaiting)
    XCTAssertEqual(shown(room), [kept, closedA, status], "never an empty Desk on a warm open")
    XCTAssertTrue(room.behind)
    _ = try await room.sync()
    XCTAssertEqual(shown(room), [kept])
    room.close()
  }

  func testACardKnownAsSettledOnlyFromTheHubsListNeverComesBackOffline() async throws {
    let id = try await away()
    var room = try Room.open(base: base, roomId: id)
    _ = await room.restore()
    _ = try await room.sync()          // the Desk lists the one left: the other two are held back as settled at the hub
    XCTAssertEqual(shown(room), [kept])
    XCTAssertEqual(room.board.cards[status]?.objectState, "open", "its closing never reached this device")
    await room.saveCacheAndWait()
    room.close()
    // the next launch, without the hub: the list kept from the last decides
    FakeHub.shared.offline = true
    room = try Room.open(base: base, roomId: id)
    _ = await room.restore()
    XCTAssertTrue(room.board.deskWaiting, "nothing open stands until the hub's Desk answers or the wait ends")
    _ = try? await room.sync()
    XCTAssertFalse(room.deskHolding)
    XCTAssertFalse(room.board.deskWaiting)
    XCTAssertEqual(shown(room), [kept], "the old question and the old status info stay off the Desk")
    room.close()
  }

  func testWhileWaitingNothingOpenStandsAndTheFingerprintSeesTheStack() {
    let b = Board()
    let rec = Rec(envelopeNumber: 5, envelopeHash: "aa", senderDeviceId: "bb", senderRole: "agent", recipientDeviceId: nil, sentAt: 1, kind: 2, isHead: true, object: nil,
                  timelineKind: nil, timelineId: nil, sessionId: nil, content: nil, contentState: "ok", bind: nil, causal: Causal(senderDeviceId: "bb", senderSequence: 1, sentAt: 1, lamport: 1),
                  senderSequence: 1, epoch: 1)
    let c = Card("x", agent: "a", rec: rec); c.envelopeNumber = 5; c.objectState = "open"
    b.cards["x"] = c
    b.project()
    let before = b.fingerprint
    b.deskWaiting = true
    b.project()
    XCTAssertTrue(b.heldBack("x", open: true))
    XCTAssertEqual(b.stack, [])
    XCTAssertNotEqual(b.fingerprint, before, "the app draws again when what stands changes")
    b.deskWaiting = false
    b.project()
    XCTAssertEqual(b.stack, ["x"])
  }
}
