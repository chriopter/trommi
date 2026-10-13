// SessionListTests: the session list the iPhone shows is the web's (app.mjs BoardState.agents, tests/web/views/
// sessions.test.mjs): one entry per folder (a connector paired again in its folder replaces its older session, which
// goes to the archive with its helpers), a helper its agent closed goes to the archive although its profile names no
// parent (the group does), and the order is the order of the session ids, a place given going first.
import Foundation
import XCTest
@testable import TrommiClient

final class SessionListTests: XCTestCase {
  let human = String(repeating: "e", count: 64)
  let d0 = String(repeating: "d0", count: 32), d1 = String(repeating: "d1", count: 32)
  let s0 = String(repeating: "50", count: 16), s1 = String(repeating: "51", count: 16), h0 = String(repeating: "60", count: 16), h1 = String(repeating: "61", count: 16)

  private var n = 0
  private func rec(from: String, session: String?, values: [String: JV]) -> OldRec {
    n += 1
    return OldRec(envelopeNumber: n, envelopeHash: String(format: "%064x", n), senderDeviceId: from, senderRole: from == human ? "human" : "agent", recipientDeviceId: nil, sentAt: UInt64(1_000_000 + n),
               kind: KIND.STATUS, isHead: true, object: nil, timelineKind: nil, timelineId: nil, sessionId: session, content: .obj(["values": .obj(values)]), contentState: "ok",
               bind: nil, causal: Causal(senderDeviceId: from, senderSequence: UInt64(n), sentAt: UInt64(n), lamport: n), senderSequence: UInt64(n), epoch: 1)
  }
  private func folder(_ f: String, host: String = "desktop") -> JV { .obj(["platform": "claude-code", "folder": .str(f), "host": .str(host)]) }
  /** The owner's room: the first pairing of a folder (d0, session s0) and the second one of the same folder (d1, s1). */
  private func pairedTwice(helperOfOld: Bool = false) -> Board {
    let b = Board()
    b.myDeviceId = human
    var ch = Change()
    b.applyMembers([(id: human, role: "human", active: true, added: 0, removed: nil), (id: d0, role: "agent", active: true, added: 3, removed: nil),
                    (id: d1, role: "agent", active: true, added: 5, removed: nil)], change: &ch)
    b.applySession(sessionId: s0, agentIds: [d0], epoch: 1, parentSessionId: nil, archived: false, change: &ch)
    b.applySession(sessionId: s1, agentIds: [d1], epoch: 1, parentSessionId: nil, archived: false, change: &ch)
    if helperOfOld { b.applySession(sessionId: h0, agentIds: [d0], epoch: 1, parentSessionId: s0, archived: false, change: &ch) }
    b.apply(rec(from: d0, session: s0, values: ["device/\(d0)": folder("~/git/trommi-hub")]), change: &ch)
    b.apply(rec(from: d1, session: s1, values: ["device/\(d1)": folder("~/git/trommi-hub")]), change: &ch)
    return b
  }
  private func agents(_ b: Board) -> [Agent] { b.project(); return DeskModel(board: b).agents }
  private func shown(_ b: Board) -> [String] { agents(b).filter { !$0.archived }.map { $0.sessionId ?? "" } }

  func testAConnectorPairedAgainInItsFolderIsOneEntry() {
    let b = pairedTwice(helperOfOld: true)
    XCTAssertEqual(shown(b), [s1], "the newest pairing of the folder; the old one and its helper are archived")
    let old = agents(b).first { $0.sessionId == s0 }
    XCTAssertEqual(old?.archived, true)
    XCTAssertEqual(old?.replaced, true)
    XCTAssertEqual(agents(b).first { $0.sessionId == h0 }?.archived, true, "its helper goes with it")
  }

  func testTwoFoldersAreTwoEntries() {
    let b = pairedTwice()
    var ch = Change()
    b.apply(rec(from: d0, session: s0, values: ["device/\(d0)": folder("~/git/other")]), change: &ch)
    XCTAssertEqual(shown(b), [s0, s1])
  }

  func testTheOldSessionStaysWhenFetchedBack() {
    let b = pairedTwice()
    var ch = Change()
    b.apply(rec(from: human, session: nil, values: ["session/\(s0)": .obj(["archived": .bool(false)])]), change: &ch)
    XCTAssertEqual(shown(b), [s0, s1])
  }

  func testAHelperItsAgentClosedIsArchivedByItsGroupsParent() {
    let b = pairedTwice()
    var ch = Change()
    b.applySession(sessionId: h1, agentIds: [d1], epoch: 1, parentSessionId: s1, archived: false, change: &ch)
    // (the connector's profile of a helper names no parent: the group does, 5.2.5)
    b.apply(rec(from: d1, session: h1, values: ["profile": .obj(["agent_name": "H1", "closed_at": .num(5)])]), change: &ch)
    let h = agents(b).first { $0.sessionId == h1 }
    XCTAssertEqual(h?.archived, true, "closed by its agent: in the archive, as on the web")
    XCTAssertEqual(h?.main, false)
    XCTAssertEqual(agents(b).first { $0.sessionId == s1 }?.main, true)
  }

  func testTheOrderIsTheSessionIdsWithAPlaceGivenFirst() {
    let b = pairedTwice()
    var ch = Change()
    b.apply(rec(from: d0, session: s0, values: ["device/\(d0)": folder("~/git/other")]), change: &ch)
    let s2 = String(repeating: "52", count: 16), d2 = String(repeating: "d2", count: 32)
    b.applyMembers([(id: d2, role: "agent", active: true, added: 6, removed: nil)], change: &ch)
    b.applySession(sessionId: s2, agentIds: [d2], epoch: 1, parentSessionId: nil, archived: false, change: &ch)
    XCTAssertEqual(shown(b), [s0, s1, s2])
    b.apply(rec(from: human, session: nil, values: ["session/\(s2)": .obj(["position": .num(0)])]), change: &ch)
    XCTAssertEqual(shown(b), [s0, s2, s1], "a place given ties with the first of the list and stands after it, as the web's stable sort")
  }
}
