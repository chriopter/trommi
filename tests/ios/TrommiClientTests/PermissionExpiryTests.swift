// PermissionExpiryTests: when a permission request runs out, the same rule as the web's model.ts `project()`: past its
// bound deadline, or (a request whose body was not read here) 62 minutes after it was sent, a pending request becomes
// `expired`, and leaves the open requests and the Desk.
import Foundation
import XCTest
@testable import TrommiClient

final class PermissionExpiryTests: XCTestCase {
  let agent = String(repeating: "b", count: 64), human = String(repeating: "e", count: 64), sid = String(repeating: "c", count: 32)
  let t0: UInt64 = 1_800_000_000_000

  private func board() -> Board {
    let b = Board()
    let s = b.sessionOf(sid); s.agentDeviceIds = [agent]; s.everAgentIds = [agent]; s.agentDeviceId = agent
    return b
  }
  private func rec(_ n: Int, kind: Int, sender: String, role: String, to: String? = nil, object: ObjectHead?, content: JV?, bind: DecodedBind?, hash: String? = nil) -> Rec {
    Rec(envelopeNumber: n, envelopeHash: hash ?? String(format: "%064x", n), senderDeviceId: sender, senderRole: role, recipientDeviceId: to, sentAt: t0, kind: kind,
        isHead: true, object: object, timelineKind: nil, timelineId: nil, sessionId: sid, content: content, contentState: content == nil ? "header" : "ok", bind: bind,
        causal: Causal(senderDeviceId: sender, senderSequence: UInt64(n), sentAt: t0, lamport: n), senderSequence: UInt64(n), epoch: 1)
  }
  /** A request of the agent; `expires` nil: only its header was read (no body, no bind). */
  @discardableResult private func request(_ b: Board, _ id: String, at n: Int, expires: UInt64?) -> Bool {
    var ch = Change()
    let r = rec(n, kind: KIND.PERMISSION_REQUEST, sender: agent, role: "agent", object: ObjectHead(objectId: id, objectState: 1, urgency: 1, answeredAt: 0),
                content: expires == nil ? nil : ["tool_name": "Bash", "description": "ls", "input_preview": "ls"],
                bind: expires.map { .permissionRequest(requestId: id, expiresAt: $0) }, hash: String(format: "%064x", Int(id.prefix(4), radix: 16) ?? 0))
    return b.apply(r, change: &ch).applied
  }
  private func state(_ b: Board, _ id: String) -> String? { b.permissions[id]?.permissionState }
  private func deskStatus(_ b: Board, _ id: String, now: UInt64) -> String? {
    let d = DeskModel(board: b); d.update(now: now)
    return d.cards.first { $0.id == id }?.status
  }

  func testARequestPastItsDeadlineIsExpired() {
    let b = board(), id = String(repeating: "1", count: 32)
    XCTAssertTrue(request(b, id, at: 5, expires: t0 + 1_500))
    b.project(now: t0 + 1_500)
    XCTAssertEqual(state(b, id), "pending", "at the deadline: still open (web: now > ends)")
    XCTAssertEqual(b.openPermissionIds, [id])
    b.project(now: t0 + 1_501)
    XCTAssertEqual(state(b, id), "expired", "the word the web shows too")
    XCTAssertEqual(b.openPermissionIds, [])
    XCTAssertEqual(deskStatus(b, id, now: t0 + 1_501), "done")
  }

  func testAnUnreadRequestExpiresSixtyTwoMinutesAfterItWasSent() {
    let b = board(), id = String(repeating: "2", count: 32)
    XCTAssertTrue(request(b, id, at: 5, expires: nil))
    b.project(now: t0 + 62 * 60_000)
    XCTAssertEqual(state(b, id), "pending")
    XCTAssertEqual(b.openPermissionIds, [id], "no deadline known: open, as on the web")
    XCTAssertEqual(deskStatus(b, id, now: t0 + 62 * 60_000), "open")
    b.project(now: t0 + 62 * 60_000 + 1)
    XCTAssertEqual(state(b, id), "expired")
    XCTAssertEqual(b.openPermissionIds, [])
  }

  func testTheBodyReadLaterBringsTheDeadline() {
    let b = board(), id = String(repeating: "3", count: 32)
    XCTAssertTrue(request(b, id, at: 5, expires: nil))
    XCTAssertTrue(request(b, id, at: 5, expires: t0 + 1_000), "the same version again, with its body")
    XCTAssertEqual(b.permissions[id]?.toolName, "Bash")
    b.project(now: t0 + 1_001)
    XCTAssertEqual(state(b, id), "expired", "its own deadline, not the 62 minutes")
  }

  func testAVerdictReadAfterTheDeadlineStillCounts() {
    let b = board(), id = String(repeating: "4", count: 32)
    request(b, id, at: 5, expires: t0 + 1_000)
    b.project(now: t0 + 5_000)          // a catch-up long after: the request runs out before its verdict is read
    XCTAssertEqual(state(b, id), "expired")
    let hash = b.permissions[id]!.versionHash
    var ch = Change()
    let v = rec(6, kind: KIND.VERDICT, sender: human, role: "human", to: agent, object: nil, content: [:],
                bind: .verdict(requestId: id, requestHash: hash, expiresAt: t0 + 1_000, allow: true))
    XCTAssertNil(b.apply(v, change: &ch).refused)
    XCTAssertEqual(state(b, id), "allowed", "as the web model takes it")
  }

  func testOneHeldBackAsSettledAtTheHubIsLeftAlone() {
    let b = board(), id = String(repeating: "5", count: 32)
    request(b, id, at: 5, expires: t0 + 1_000)
    b.deskGate = []
    b.project(now: t0 + 5_000)
    XCTAssertEqual(state(b, id), "pending", "held back (web: catching_up) keeps its state")
    XCTAssertEqual(b.openPermissionIds, [])
  }
}
