// What the app puts into the Live Activity (PushNotify LiveState): the most urgent card first, cut to the island's
// lengths, two answers only for a two-option decision, and nothing of a card while the phone cannot read the room.
// The hub's pushes (two counts) still decode, with no card.
import XCTest
@testable import PushNotify

final class LiveStateTests: XCTestCase {
  let two = [LiveOption(key: "ship", label: "Ship it"), LiveOption(key: "wait", label: "Wait")]

  func cand(_ id: String, urgency: String = "normal", kind: String = "decision", created: UInt64 = 1_000, title: String? = nil,
            options: [LiveOption]? = nil, multiple: Bool = false) -> LiveCandidate {
    LiveCandidate(id: id, title: title ?? "Card \(id)", agent: "iOS", kind: kind, urgency: urgency, created: created, options: options ?? two, multiple: multiple)
  }

  func testMostUrgentFirstThenLongestWaiting() {
    XCTAssertNil(LiveState.pick([]))
    // the older of two normal cards
    XCTAssertEqual(LiveState.pick([cand("b", created: 2_000), cand("a", created: 1_000)])?.id, "a")
    // a knock before an older normal card
    XCTAssertEqual(LiveState.pick([cand("old", created: 1), cand("knock", urgency: "high", created: 9_000)])?.id, "knock")
    // blocking before a knock; a permission request ranks with blocking, the older of the two wins
    let list = [cand("knock", urgency: "high", created: 1), cand("block", urgency: "critical", created: 5_000),
                cand("perm", urgency: "normal", kind: "permission", created: 4_000)]
    XCTAssertEqual(LiveState.pick(list)?.id, "perm")
    // low ranks with normal; equal times: the id decides, so the pick does not flicker
    XCTAssertEqual(LiveState.pick([cand("y", urgency: "low"), cand("x")])?.id, "x")
  }

  func testCutToTheIsland() {
    let long = String(repeating: "word ", count: 40)
    let c = LiveState.card(cand("a", title: long + "\n\nmore"))
    XCTAssertLessThanOrEqual(c.title.count, LiveState.titleMax)
    XCTAssertTrue(c.title.hasSuffix("…"))
    XCTAssertFalse(c.title.contains("\n"))
    XCTAssertEqual(LiveState.clip("  Ship   the\nsign-in ", 90), "Ship the sign-in")
    XCTAssertEqual(LiveState.clip("abcdefghij", 5), "abcd…")
    let named = LiveState.card(LiveCandidate(id: "n", title: "t", agent: String(repeating: "N", count: 60), kind: "decision", urgency: "normal",
                                             created: 1, options: [LiveOption(key: "a", label: String(repeating: "L", count: 40)), LiveOption(key: "b", label: "No")]))
    XCTAssertLessThanOrEqual(named.agent.count, LiveState.agentMax)
    XCTAssertLessThanOrEqual(named.options[0].label.count, LiveState.labelMax)
    XCTAssertEqual(named.options.map(\.key), ["a", "b"])
    XCTAssertEqual(LiveState.card(cand("t", created: 1_500)).since, Date(timeIntervalSince1970: 1.5))
  }

  func testTwoAnswersOnlyForATwoOptionDecision() {
    XCTAssertEqual(LiveState.card(cand("a")).options.count, 2)
    XCTAssertTrue(LiveState.card(cand("three", options: two + [LiveOption(key: "x", label: "X")])).options.isEmpty)
    XCTAssertTrue(LiveState.card(cand("one", options: [two[0]])).options.isEmpty)
    XCTAssertTrue(LiveState.card(cand("multi", multiple: true)).options.isEmpty)
    // a permission request opens in the app: allowing a tool is not done from the lock screen
    XCTAssertTrue(LiveState.card(cand("perm", kind: "permission")).options.isEmpty)
  }

  func testKnockAndCritical() {
    XCTAssertFalse(LiveState.card(cand("n")).knocks)
    XCTAssertTrue(LiveState.card(cand("h", urgency: "high")).knocks)
    XCTAssertFalse(LiveState.card(cand("h", urgency: "high")).critical)
    XCTAssertTrue(LiveState.card(cand("c", urgency: "critical")).critical)
    XCTAssertTrue(LiveState.card(cand("p", kind: "permission")).critical)
  }

  func testStateCountsAndPick() {
    let s = LiveState.state(working: 2, waiting: [cand("a", created: 5), cand("b", urgency: "high", created: 9)], readable: true)
    XCTAssertEqual(s.working, 2)
    XCTAssertEqual(s.waiting, 2)
    XCTAssertEqual(s.exact, true)
    XCTAssertEqual(s.card?.id, "b")
    XCTAssertNil(s.done)
    XCTAssertNil(LiveState.state(working: 1, waiting: [], readable: true).card)
  }

  /** Locked out or without keys: the counts alone, and not a word of a card in what goes to ActivityKit. */
  func testNothingOfACardWhenNotReadable() throws {
    let secret = "Rotate the production key tonight"
    let s = LiveState.state(working: 1, waiting: [cand("a", urgency: "critical", title: secret)], readable: false)
    XCTAssertNil(s.card)
    XCTAssertEqual(s.waiting, 1)
    let json = String(decoding: try JSONEncoder().encode(s), as: UTF8.self)
    XCTAssertFalse(json.contains("Rotate"))
    XCTAssertFalse(json.contains("iOS"))
    XCTAssertFalse(json.contains("\"card\""))
  }

  /** The hub's content-state: two marks, nothing else; the card's keys are absent and decode as nil. */
  func testHubPushDecodesWithoutACard() throws {
    let s = try JSONDecoder().decode(LiveCounts.self, from: Data(#"{"working":1,"waiting":1}"#.utf8))
    XCTAssertEqual(s, LiveCounts(working: 1, waiting: 1))
    XCTAssertNil(s.card)
    XCTAssertNil(s.exact)
    XCTAssertEqual(s.line, "Agents at work · Questions waiting")
    let full = LiveState.state(working: 1, waiting: [cand("a")], readable: true)
    XCTAssertEqual(try JSONDecoder().decode(LiveCounts.self, from: JSONEncoder().encode(full)), full)
  }
}
