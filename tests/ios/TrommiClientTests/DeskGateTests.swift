// DeskGateTests: what the hub's Desk holds back on this device (the client core's Desk through Board.setDeskListing, Room.deskFirst):
// no card the hub has settled stands on the Desk as a question, neither while a catch-up is behind nor after it.
import Foundation
import XCTest
@testable import TrommiClient

final class DeskGateTests: XCTestCase {
  private func card(_ b: Board, _ id: String, at n: Int, state: String = "open") {
    let rec = Rec(envelopeNumber: n, envelopeHash: "aa", senderDeviceId: "bb", senderRole: "agent", recipientDeviceId: nil, sentAt: 1, kind: 2, isHead: true, object: nil,
                  timelineKind: nil, timelineId: nil, sessionId: nil, content: nil, contentState: "ok", bind: nil, causal: Causal(senderDeviceId: "bb", senderSequence: 1, sentAt: 1, lamport: 1),
                  senderSequence: 1, epoch: 1)
    let c = Card(id, agent: "a", rec: rec)
    c.envelopeNumber = n; c.firstEnvelopeNumber = n; c.objectState = state
    b.cards[id] = c
  }
  private func shown(_ b: Board) -> Set<String> { Set(DeskModel(board: b).cards.filter { $0.status == "open" }.map(\.id)) }

  private func listing(_ b: Board, _ open: [String], at: Int) { b.setDeskListing(Dictionary(uniqueKeysWithValues: open.map { ($0, 0) }), at: at) }

  func testWhileBehindOnlyWhatTheDeskListsStands() {
    let b = Board()
    card(b, "old-answered-elsewhere", at: 10)
    card(b, "still-open", at: 11)
    b.project()
    XCTAssertEqual(shown(b), ["old-answered-elsewhere", "still-open"], "without a Desk read: as held")
    listing(b, ["still-open", "not-read-yet"], at: 50)
    b.project()
    XCTAssertEqual(shown(b), ["still-open"], "settled elsewhere: held back")
    XCTAssertEqual(b.stack, ["still-open"])
    card(b, "not-read-yet", at: 40)
    b.project()
    XCTAssertEqual(shown(b), ["still-open", "not-read-yet"], "read during the catch-up, open at the hub: shown at once")
  }

  func testAfterTheCatchUpWhatTheHubSettledStaysOff() {
    let b = Board()
    card(b, "ghost", at: 10)                  // the hub's Desk at 50 does not list it: settled there
    card(b, "open", at: 20)
    card(b, "newer", at: 60)                  // came after the Desk was read
    card(b, "closed", at: 30, state: "closed")
    listing(b, ["open"], at: 50)
    b.project()
    XCTAssertEqual(b.held, ["ghost"], "only what is open here, missing there, and has nothing newer")
    XCTAssertEqual(shown(b), ["open", "newer"])
    // something newer of it comes (reopened, revised): it stands again
    b.cards["ghost"]!.envelopeNumber = 70
    b.project()
    XCTAssertEqual(shown(b), ["open", "newer", "ghost"])
  }

  /** The Desk of a fresh sign-in comes from the hub's list (Room.deskPreview): a card's newest version stands alone there. */
  func testAPreviewTakesACardsNewestVersionAlone() {
    let agent = String(repeating: "b", count: 64), sid = String(repeating: "c", count: 32), id = String(repeating: "1", count: 32)
    func version3(_ b: Board) -> Bool {
      let s = b.sessionOf(sid); s.agentDeviceIds = [agent]; s.everAgentIds = [agent]; s.agentDeviceId = agent
      let rec = Rec(envelopeNumber: 9, envelopeHash: String(format: "%064x", 9), senderDeviceId: agent, senderRole: "agent", recipientDeviceId: nil, sentAt: 9, kind: KIND.OBJECT_VERSION,
                    isHead: true, object: ObjectHead(objectId: id, objectState: 1, urgency: 1, answeredAt: 0), timelineKind: nil, timelineId: nil, sessionId: sid,
                    content: ["object_type": "card", "object_version": 3, "previous_version_hash": .str(String(repeating: "7", count: 64)), "title": "Ship it?"], contentState: "ok", bind: nil,
                    causal: Causal(senderDeviceId: agent, senderSequence: 9, sentAt: 9, lamport: 9), senderSequence: 9, epoch: 1)
      var ch = Change()
      return b.apply(rec, change: &ch).applied
    }
    XCTAssertFalse(version3(Board()), "the room's own board takes versions in order only")
    let p = Board(); p.preview = true
    XCTAssertTrue(version3(p))
    XCTAssertEqual(p.cards[id]?.objectVersion, 3)
    XCTAssertEqual(p.cards[id]?.title, "Ship it?")
  }
}
