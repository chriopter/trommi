// BlockerTests: the red hand and the Desk are one rule, the client core's (core/client/src/status.rs): a blocking question
// raises its session's hand exactly while it stands on the Desk. Put off (Later), both go; a newer version of it brings
// both back ("Back from snooze"). The same test runs on the web (tests/web/model/stack-echoes.test.mjs).
import Foundation
import XCTest
@testable import TrommiClient

final class BlockerTests: XCTestCase {
  let human = String(repeating: "a", count: 64), agent = String(repeating: "b", count: 64)
  let sid = String(repeating: "c", count: 32), card = String(repeating: "1", count: 32)

  private func rec(_ n: Int, kind: Int, from: String, session: String?, object: ObjectHead? = nil, content: JV?) -> OldRec {
    OldRec(envelopeNumber: n, envelopeHash: String(format: "%064x", n), senderDeviceId: from, senderRole: from == human ? "human" : "agent", recipientDeviceId: nil,
           sentAt: UInt64(1_000_000 + n), kind: kind, isHead: kind != KIND.TIMELINE_ITEM, object: object, sessionId: session, content: content, contentState: "ok",
           causal: Causal(senderDeviceId: from, senderSequence: UInt64(n), sentAt: UInt64(n), lamport: n), senderSequence: UInt64(n), epoch: 1)
  }
  private func question(_ b: Board, _ n: Int, version: Int) {
    var ch = Change()
    b.apply(rec(n, kind: KIND.OBJECT_VERSION, from: agent, session: sid, object: ObjectHead(objectId: card, objectState: 1, urgency: 3, answeredAt: 0),
                content: ["object_type": "card", "object_version": .num(Double(version)), "title": "Merge now?", "options": [["key": "y", "label": "Yes"]]]), change: &ch)
  }
  private func unit(_ b: Board) -> DeskUnit? { b.project(); return DeskModel(board: b).view(desk: nil).units.first }

  func testTheRedHandStandsExactlyWhileTheQuestionStandsOnTheDesk() throws {
    let b = Board()
    b.myDeviceId = human
    var ch = Change()
    b.applyMembers([(id: human, role: "human", active: true, added: 0, removed: nil), (id: agent, role: "agent", active: true, added: 1, removed: nil)], change: &ch)
    b.applySession(sessionId: sid, agentIds: [agent], epoch: 1, parentSessionId: nil, archived: false, change: &ch)
    b.applyPresence(.obj(["device_id": .str(agent), "is_online": true]))
    question(b, 2, version: 1)
    XCTAssertEqual(unit(b)?.blocked?.why, "blocking")
    XCTAssertEqual(DeskModel(board: b).view(desk: nil).fresh.map { $0.id }, [card])
    // put off until tomorrow: off the Desk, and no hand
    b.apply(rec(3, kind: KIND.STATUS, from: human, session: nil, content: ["values": ["snooze/\(card)": ["until": .num(Double(nowMs() + 86_400_000)), "at": .num(Double(nowMs()))]]]), change: &ch)
    XCTAssertNil(unit(b)?.blocked, "put off: no red hand")
    let v = DeskModel(board: b).view(desk: nil)
    XCTAssertTrue(v.fresh.isEmpty)
    XCTAssertEqual(v.snoozed.map { $0.id }, [card])
    // the agent revises it: a newer version wakes it, and the hand comes back with it
    question(b, 4, version: 2)
    XCTAssertEqual(unit(b)?.blocked?.why, "blocking")
    let back = DeskModel(board: b).view(desk: nil)
    XCTAssertEqual(back.fresh.map { $0.id }, [card])
    XCTAssertNotNil(back.fresh.first?.unsnoozed, "Back from snooze")
  }
}
