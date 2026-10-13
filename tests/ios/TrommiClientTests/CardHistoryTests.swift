// CardHistoryTests: a card's whole history under it (client/src/model/types.rs `history_of`, Desk.swift eventsOf,
// CardScreen.swift RevisionRow). Every revision stays, with when and what changed: a newer one never replaces those
// before it. The history comes from the client core whole, also on a lean card (no version body read for it).
import Foundation
import XCTest
@testable import TrommiClient

final class CardHistoryTests: XCTestCase {
  private let agent = String(repeating: "b", count: 64), sid = String(repeating: "c", count: 32), id = String(repeating: "4", count: 32)

  /** Version n of the card, revised with a note from version 2 on. */
  private func version(_ b: Board, _ n: Int) {
    var content: [String: JV] = ["object_type": "card", "card_type": "info", "object_version": .num(Double(n)), "title": n == 6 ? "Done: the history" : "Working on: the history", "body": .str("Step \(n - 1) built")]
    if n > 1 { content["previous_version_hash"] = .str(String(format: "%064x", 10 * (n - 1))); content["change_note"] = .str("step \(n - 1) built") }
    let rec = OldRec(envelopeNumber: 10 * n, envelopeHash: String(format: "%064x", 10 * n), senderDeviceId: agent, senderRole: "agent", recipientDeviceId: nil, sentAt: UInt64(1000 * n), kind: KIND.OBJECT_VERSION,
                     isHead: true, object: ObjectHead(objectId: id, objectState: 1, urgency: 1, answeredAt: 0), timelineKind: nil, timelineId: nil, sessionId: sid,
                     content: .obj(content), contentState: "ok", bind: nil,
                     causal: Causal(senderDeviceId: agent, senderSequence: UInt64(n), sentAt: UInt64(1000 * n), lamport: n), senderSequence: UInt64(n), epoch: 1)
    var ch = Change()
    XCTAssertTrue(b.apply(rec, change: &ch).applied)
  }

  func testACardRevisedFiveTimesShowsFiveEntries() throws {
    let b = Board()
    for n in 1...6 { version(b, n) }
    b.project()
    let c = try XCTUnwrap(b.cards[id])
    XCTAssertEqual(c.history.map(\.objectVersion), [1, 2, 3, 4, 5, 6], "the client core keeps every revision")
    let d = DeskModel(board: b)
    d.update(now: 10_000)
    XCTAssertEqual(d.byCard[id]?.title, "Done: the history", "the newest version stands on top")
    let revised = d.messagesOfCard(id).filter { $0.from == "event" && $0.kind == "revised" }
    XCTAssertEqual(revised.count, 5, "a card revised five times shows five entries")
    XCTAssertEqual(revised.map { $0.version ?? 0 }, [2, 3, 4, 5, 6])
    XCTAssertEqual(revised.map(\.text), (1...5).map { "step \($0) built" }, "each says what changed")
    XCTAssertEqual(d.byCard[id]?.versions.map(\.objectVersion), [1, 2, 3, 4, 5], "every version before the newest opens under the one after it")
  }
}
