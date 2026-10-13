// ReadBackDeskTests: a read back of the past (RoomPast.swift tendPast: keys of before came, the board is built anew
// from the hub's changes) never empties the Desk on screen, and never throws the cache away before it is through.
// The owner's report (12 October): the iPhone opened, every card was gone, the Desk said "Catching up… 0%" and
// "Questions land here.", and it caught up from scratch: the read back had reset the board the views show and
// wiped the cache at its start, so a presence or a timer drew the empty board, and an app ended meanwhile had no
// cache at its next launch.
import XCTest
@testable import TrommiClient

@MainActor
final class ReadBackDeskTests: XCTestCase {
  var base: URL!
  let hubURL = "http://127.0.0.1:9"
  let agent = String(repeating: "ab", count: 32), session = String(repeating: "cd", count: 16)
  let card = String(repeating: "a1", count: 16)

  override func setUp() async throws {
    base = FileManager.default.temporaryDirectory.appendingPathComponent("trommi-read-back-desk-\(UUID().uuidString)")
    Core.tools = FakeTools()
    HubClient.transportForTests = [FakeHub.self]
    FakeHub.shared = FakeHub.Hub()
  }
  override func tearDown() async throws { FakeHub.shared.offline = false; try? FileManager.default.removeItem(at: base) }

  private func shown(_ room: Room) -> Set<String> { Set(DeskModel(board: room.shownBoard).cards.filter { $0.status == "open" }.map(\.id)) }

  /** A room with one open question and one item passed over as written before this device came (read back once
   *  the keys of before come): synced, cached, the Desk lists the question. */
  private func roomWithAQuestion() async throws -> (Room, GroupId) {
    let room = try await Room.foundRoom(hubURL: hubURL, base: base).room
    let hub = FakeHub.shared
    hub.commit(group: room.roomId, ["session": session, "agent": agent])
    let group = FakeDevice.sessionGroup(room.roomIdHex, session)
    hub.envelope(["sender": agent, "group": hex(group), "seq": 1, "time": 1001, "kind": KIND.OBJECT_VERSION, "otype": OBJECT_TYPE.CARD, "ostate": CARD_STATE.OPEN, "oid": card, "files": [String]()],
                 payload: ["object_type": "card", "object_version": 1, "card_type": "decision", "title": "Still open?", "options": [["key": "yes", "label": "Yes"]]])
    hub.envelope(["sender": agent, "group": hex(group), "seq": 2, "time": 1002, "kind": KIND.TIMELINE_ITEM, "tk": TIMELINE.CHAT, "ts": TIMELINE_SCOPE.SESSION, "tr": session, "files": [String](), "before": true],
                 payload: ["content_type": "message", "text": "before this device came"])
    hub.lock.withLock { hub.deskCards = { [self] in [card] } }
    _ = try await room.sync()
    XCTAssertEqual(room.record.past?.passedOver, true)
    XCTAssertEqual(shown(room), [card])
    await room.saveCacheAndWait(snapshot: true)
    XCTAssertTrue(room.log.isEmpty, "what the cache holds is not kept in memory as well")
    return (room, group)
  }

  func testAReadBackNeverEmptiesTheDeskOnScreen() async throws {
    let (room, group) = try await roomWithAQuestion()
    var seen: [Set<String>] = []
    room.takeAhead = { [unowned room, self] _ in
      if room.readingBack { MainActor.assumeIsolated { seen.append(self.shown(room)) } }
      return nil
    }
    FakeHub.shared.message(group: group, ["keys": hex(group)])
    _ = try await room.sync()
    _ = try await room.sync()
    XCTAssertFalse(seen.isEmpty, "a read back ran")
    XCTAssertTrue(seen.allSatisfy { $0 == [card] }, "while reading back the Desk on screen keeps its question: \(seen)")
    XCTAssertNil(room.readBackView, "through: the views show the board again")

    XCTAssertEqual(shown(room), [card])
    room.close()
  }

  func testTheCacheSurvivesAnAppEndedDuringAReadBack() async throws {
    let (room, group) = try await roomWithAQuestion()
    let id = room.roomIdHex
    // the read back fails half way (the hub gone, the app ended)
    room.takeAhead = { [unowned room] _ in
      if room.readingBack { FakeHub.shared.offline = true }
      return nil
    }
    FakeHub.shared.message(group: group, ["keys": hex(group)])
    _ = try? await room.sync()
    _ = try? await room.sync()
    XCTAssertEqual(room.record.past?.readBack, true, "the read back is still to do")
    XCTAssertEqual(shown(room), [card], "the Desk on screen kept its question")
    await room.saveCacheAndWait(snapshot: true)
    room.close()

    // the next launch is warm: the cache of before is there, and its Desk is never empty
    FakeHub.shared.offline = false
    let again = try Room.open(base: base, roomId: id)
    let had = await again.restore()
    XCTAssertTrue(had, "the cache survived")
    XCTAssertGreaterThan(again.cursor, 0, "no catch-up from scratch")
    await again.waitForDesk()
    XCTAssertEqual(shown(again), [card], "a warm open shows the cached Desk at once")
    _ = try await again.sync()
    XCTAssertNotEqual(again.record.past?.readBack, true, "read back now")
    XCTAssertNil(again.readBackView)

    XCTAssertEqual(shown(again), [card])
    await again.saveCacheAndWait()
    again.close()

    // and the cache the read back wrote opens on the launch after
    let third = try Room.open(base: base, roomId: id)
    let had3 = await third.restore()
    XCTAssertTrue(had3)
    await third.waitForDesk()
    XCTAssertEqual(shown(third), [card])
    third.close()
  }
}
