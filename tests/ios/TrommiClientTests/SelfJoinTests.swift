// SelfJoinTests: self-join (spec/v1.md 12.4) on the human's side, the part that decides (SelfJoin.swift
// SelfJoinPlan, as the web's client.ts answerAsks and vouchConfirm): which asks of the agents get a link, which are
// refused at once, and when a Request is let in without the emoji.
import Foundation
import XCTest
@testable import TrommiClient

final class SelfJoinTests: XCTestCase {
  let h = String(repeating: "1", count: 64), agent = String(repeating: "b", count: 64)
  let s1 = String(repeating: "c", count: 32), s2 = String(repeating: "d", count: 32)
  var n = 1

  private func set(_ b: Board, _ values: JV, by sender: String, role: String, session: String? = nil) {
    n += 1
    var ch = Change()
    b.apply(OldRec(envelopeNumber: n, envelopeHash: String(format: "%064x", n), senderDeviceId: sender, senderRole: role, recipientDeviceId: nil, sentAt: UInt64(n), kind: KIND.STATUS, isHead: true,
                object: nil, timelineKind: nil, timelineId: nil, sessionId: session, content: ["values": values], contentState: "ok", bind: nil,
                causal: Causal(senderDeviceId: sender, senderSequence: UInt64(n), sentAt: UInt64(n), lamport: n), senderSequence: UInt64(n), epoch: 1), change: &ch)
  }
  private func room() -> Board {
    let b = Board()
    b.myDeviceId = h
    var g = Change(); b.applySession(sessionId: s1, agentIds: [agent], epoch: 1, parentSessionId: nil, archived: false, change: &g)
    return b
  }
  private let ask: JV = ["name": "second", "folder": "~/project", "account": "a1b2c3d4e5f60718", "account_label": "claude .claude", "vouched_by": "project", "request_hash": .null]

  func testAnAskOfTheAgentGetsALinkOnce() {
    let b = room()
    set(b, [.init("join_ask/a1"): ask], by: agent, role: "agent", session: s1)
    let asks = SelfJoinPlan.asks(b, answered: [])
    XCTAssertEqual(asks.map(\.nonce), ["a1"])
    XCTAssertEqual(asks.first?.sessionId, s1)
    XCTAssertNil(asks.first?.refused)
    XCTAssertEqual(SelfJoinPlan.asks(b, answered: ["a1"]), [], "answered by this device already")
    // answered by another device of the person: nothing more to do
    set(b, [.init("join_link/a1"): ["link": "https://app.example/join#v1.x", "invite_id": "00"]], by: h, role: "human", session: s1)
    XCTAssertEqual(SelfJoinPlan.asks(b, answered: []), [])
  }

  func testAnArchivedSessionAndAnAskWithoutAccountGetNoLink() {
    let b = room()
    set(b, [.init("join_ask/a2"): ["name": "x", "account": ""]], by: agent, role: "agent", session: s1)
    XCTAssertEqual(SelfJoinPlan.asks(b, answered: []).first?.refused, "bad-ask")
    set(b, [.init("session/\(s1)"): ["archived": true]], by: h, role: "human")
    XCTAssertEqual(SelfJoinPlan.asks(b, answered: []), [])
  }

  func testPastEightLiveSessionsOfTheAccountTheAskIsRefused() {
    let b = room()
    for i in 0..<8 {
      let sid = String(format: "%032x", i + 100), dev = String(format: "%064x", i + 100)
      var g = Change(); b.applySession(sessionId: sid, agentIds: [dev], epoch: 1, parentSessionId: nil, archived: false, change: &g)
      set(b, [.init("session/\(sid)"): ["vouched": ["by": .str(s1), "account": "a1b2c3d4e5f60718"]]], by: h, role: "human")
    }
    set(b, [.init("join_ask/a3"): ask], by: agent, role: "agent", session: s1)
    XCTAssertEqual(SelfJoinPlan.asks(b, answered: []).first?.refused, "cap")
    // another account is not counted against it
    set(b, [.init("join_ask/a4"): ["name": "y", "account": "ffffffffffffffff"]], by: agent, role: "agent", session: s1)
    XCTAssertNil(SelfJoinPlan.asks(b, answered: []).first { $0.nonce == "a4" }?.refused)
  }

  func testOpenLinksAndAsksAtOnceCountAgainstTheCap() {
    let b = room()
    for i in 0..<10 { set(b, [.init("join_ask/c\(i)"): ask], by: agent, role: "agent", session: s1) }
    let asks = SelfJoinPlan.asks(b, answered: [])
    XCTAssertEqual(asks.filter { $0.refused == nil }.count, 8, "ten asks at once: eight links")
    XCTAssertEqual(SelfJoinPlan.asks(b, answered: [], pending: Array(repeating: "a1b2c3d4e5f60718", count: 8)).filter { $0.refused == nil }.count, 0, "eight links still open")
  }

  func testAnAgentThatNamesAnotherAccountEachTimeIsBoundByTheRoomCap() {
    let b = room()
    for i in 0..<30 { set(b, [.init("join_ask/\(String(format: "%02x", i))"): ["name": "x", "account": .str(String(format: "%016x", i))]], by: agent, role: "agent", session: s1) }
    let asks = SelfJoinPlan.asks(b, answered: [])
    XCTAssertEqual(asks.filter { $0.refused == nil }.count, 8, "no more links open at once")
    XCTAssertEqual(asks.filter { $0.refused == "busy" }.count, 30 - 8)
    // sixteen live sessions that joined so, of sixteen accounts, and eight more open: the room is full
    for i in 0..<16 {
      let sid = String(format: "%032x", i + 200), dev = String(format: "%064x", i + 200)
      var g = Change(); b.applySession(sessionId: sid, agentIds: [dev], epoch: 1, parentSessionId: nil, archived: false, change: &g)
      set(b, [.init("session/\(sid)"): ["vouched": ["by": .str(s1), "account": .str(String(format: "%016x", i + 100))]]], by: h, role: "human")
    }
    XCTAssertEqual(SelfJoinPlan.asks(b, answered: [], pending: (0..<8).map { String(format: "%016x", $0 + 300) }).filter { $0.refused == "cap" }.count, 30)
  }

  func testTheRequestIsLetInOnlyWhenTheAskNamesIt() {
    let b = room()
    set(b, [.init("join_ask/a5"): ask], by: agent, role: "agent", session: s1)
    XCTAssertNil(SelfJoinPlan.verdict(b, session: s1, nonce: "a5", requestHash: "abc"), "not named yet: wait")
    let named: JV = ["name": "second", "account": "a1b2c3d4e5f60718", "request_hash": "abc"]
    set(b, [.init("join_ask/a5"): named], by: agent, role: "agent", session: s1)
    XCTAssertEqual(SelfJoinPlan.verdict(b, session: s1, nonce: "a5", requestHash: "abc"), true)
    XCTAssertEqual(SelfJoinPlan.verdict(b, session: s1, nonce: "a5", requestHash: "abd"), false, "another Request: burned")
  }
}
