// SpeedTests: what the main thread pays per change of a big board (the model side of the iOS speed harness). Opt-in:
//
//   node tests/tools/mega-fixture.mjs /tmp/mega.json          # 380 sessions, 520 cards, a chat of 5,000 items
//   TROMMI_SPEED_FIXTURE=/tmp/mega.json swift test --filter SpeedTests
//
// Prints one table row per step (median of RUNS, in ms). The steps are what BoardModel.changed and the open screens
// do after every batch: the Desk's model again, the Desk in view, the open chat's messages, the chat list's rows.
// The tests below it (always run, on the demo room) hold that the caches behind those numbers change nothing.
import Foundation
import XCTest
@testable import TrommiClient

final class SpeedTests: XCTestCase {
  static let RUNS = 15

  private func board() throws -> Board {
    guard let path = ProcessInfo.processInfo.environment["TROMMI_SPEED_FIXTURE"] else { throw XCTSkip("TROMMI_SPEED_FIXTURE not set") }
    return try DemoFixture.board(try Data(contentsOf: URL(fileURLWithPath: path)))
  }
  private func ms(_ op: () -> Void) -> Double {
    var t = [Double]()
    for _ in 0..<Self.RUNS {
      let t0 = DispatchTime.now().uptimeNanoseconds
      op()
      t.append(Double(DispatchTime.now().uptimeNanoseconds - t0) / 1e6)
    }
    return t.sorted()[t.count / 2]
  }
  private func row(_ what: String, _ v: Double) { print("SPEED | " + what.padding(toLength: 56, withPad: " ", startingAt: 0) + " | " + String(format: "%8.2f ms", v)) }

  /** The session with the longest chat (the fixture's 5,000). */
  private func bigSession(_ b: Board) -> String {
    b.timelines.values.filter { $0.timelineId.hasPrefix("session/") }.max { $0.items.count < $1.items.count }!.objectId
  }
  /** One new chat message from an agent of `session`, as the live stream brings it. */
  private func message(_ b: Board, session: String, n: Int) -> Rec {
    let agent = b.sessions[session]!.agentDeviceId!
    return Rec(envelopeNumber: n, envelopeHash: String(format: "%064x", n), senderDeviceId: agent, senderRole: "agent", recipientDeviceId: nil, sentAt: nowMs(),
               kind: KIND.TIMELINE_ITEM, isHead: false, object: nil, timelineKind: "chat", timelineId: "session/\(session)", sessionId: session,
               content: ["text": .str("one more line \(n)"), "attachments": .arr([])], contentState: "ok", bind: nil,
               causal: Causal(senderDeviceId: agent, senderSequence: UInt64(n), sentAt: nowMs(), lamport: 1), senderSequence: UInt64(n), epoch: 1)
  }

  func testTheMainThreadPerChange() throws {
    let path = ProcessInfo.processInfo.environment["TROMMI_SPEED_FIXTURE"] ?? ""
    if path.isEmpty { throw XCTSkip("TROMMI_SPEED_FIXTURE not set") }
    let bytes = try Data(contentsOf: URL(fileURLWithPath: path))
    row("load: fixture to board (\(bytes.count / 1024) KB)", ms { _ = try? DemoFixture.board(bytes) })
    let b = try board()
    let big = bigSession(b)
    let quiet = b.sessions.keys.sorted().first { $0 != big }!
    let d = DeskModel(board: b)
    let bigAgent = d.agents.first { $0.sessionId == big }!.id
    print("SPEED | board: \(b.sessions.count) sessions, \(b.cards.count) cards, \(b.timelines[timelineKeyOf("chat", "session/\(big)")]!.items.count) items in the big chat, \(d.view(desk: ALL_DESKS).units.count) units on All desks")

    row("Board.fingerprint", ms { _ = b.fingerprint })
    row("DeskModel.update (all cards, sessions)", ms { d.update() })
    row("view(desk: nil) after update", ms { d.update(); _ = d.view(desk: nil) } - ms { d.update() })
    row("view(desk: All desks) after update", ms { d.update(); _ = d.view(desk: ALL_DESKS) } - ms { d.update() })
    row("messagesOf(5,000-item chat) after update", ms { d.update(); _ = d.messagesOf(agent: bigAgent) } - ms { d.update() })
    let rows = d.view(desk: ALL_DESKS).units.prefix(12).map { $0.id }
    row("chat list: 12 visible rows' teasers after update", ms { d.update(); for a in rows { _ = ChatTeaser.of(d.messagesOf(agent: a)) } } - ms { d.update() })
    let units = d.view(desk: ALL_DESKS).units
    // what SessionScreen's body does on every run: also on a scroll's start and end (its @State), not only per change
    d.update(); _ = d.messagesOf(agent: bigAgent)
    row("SessionScreen body work, 5,000 items (per run)", ms {
      let all = d.messagesOf(agent: bigAgent)
      _ = d.askAt(agent: bigAgent)
      _ = d.hasOlder(agent: bigAgent)
      _ = Array(all.enumerated())
    })
    row("chat list: newestWord for every unit (unread dots)", ms { for u in units { _ = d.newestWord(sessionId: u.agent.deviceId) } })

    // the whole cycle of one live message elsewhere while the big chat is open: what BoardModel.changed and the
    // open SessionScreen pay per batch on the main thread
    var n = 900_000_000
    let cycle = ms {
      n += 1
      var ch = Change()
      b.apply(message(b, session: quiet, n: n), change: &ch)
      _ = b.fingerprint
      d.update()
      _ = d.view(desk: ALL_DESKS)
      let all = d.messagesOf(agent: bigAgent)
      _ = Array(all.enumerated())
      _ = d.askAt(agent: bigAgent)
      _ = d.hasOlder(agent: bigAgent)
      for a in rows { _ = ChatTeaser.of(d.messagesOf(agent: a)) }
    }
    row("ONE LIVE MESSAGE elsewhere, big chat open (cycle)", cycle)
    let cycleHere = ms {
      n += 1
      var ch = Change()
      b.apply(message(b, session: big, n: n), change: &ch)
      _ = b.fingerprint
      d.update()
      _ = d.view(desk: ALL_DESKS)
      _ = d.messagesOf(agent: bigAgent)
      _ = d.hasOlder(agent: bigAgent)
      for a in rows { _ = ChatTeaser.of(d.messagesOf(agent: a)) }
    }
    row("ONE LIVE MESSAGE into the open big chat (cycle)", cycleHere)
    row("BoardCodec.encode (the cache)", ms { _ = BoardCodec.encode(b) })
    let enc = BoardCodec.encode(b)
    row("BoardCodec.decode (the cache, \(enc.count / 1024) KB)", ms { let x = Board(); enc.withUnsafeBufferPointer { try? BoardCodec.decode($0, into: x) } })
  }

  // ---- the caches change nothing ----

  private func demo() throws -> Board {
    var repo = URL(fileURLWithPath: #filePath)
    while repo.path != "/", !FileManager.default.fileExists(atPath: repo.appendingPathComponent("demo/data/fixture.json").path) { repo = repo.deletingLastPathComponent() }
    return try DemoFixture.board(try Data(contentsOf: repo.appendingPathComponent("demo/data/fixture.json")), now: 1_800_000_000_000)
  }
  /** What a conversation shows, row by row (Message is not Equatable). */
  private func rows(_ m: [Message]) -> [String] {
    m.map { "\($0.id)|\($0.seq)|\($0.from)|\($0.kind ?? "")|\($0.text)|\($0.ts)|\($0.attachments)|\($0.published ?? "")|\($0.itemState)|\($0.terminal ?? "")|\($0.work != nil)|\($0.pending)" }
  }
  /** Every conversation of a model kept across updates equals the one a fresh model builds. */
  private func assertSame(_ kept: DeskModel, _ b: Board, _ what: String, file: StaticString = #filePath, line: UInt = #line) {
    kept.update(now: 1_800_000_000_000)
    let fresh = DeskModel(board: b)
    fresh.update(now: 1_800_000_000_000)
    for a in fresh.agents {
      XCTAssertEqual(rows(kept.messagesOf(agent: a.id)), rows(fresh.messagesOf(agent: a.id)), "\(what): the chat of \(a.name)", file: file, line: line)
    }
    for c in fresh.cards { XCTAssertEqual(rows(kept.messagesOfCard(c.id)), rows(fresh.messagesOfCard(c.id)), "\(what): the talk of card \(c.number)", file: file, line: line) }
  }

  func testAKeptChatFollowsEveryChange() throws {
    let b = try demo()
    let d = DeskModel(board: b)
    for a in d.agents { _ = d.messagesOf(agent: a.id) }
    for c in d.cards { _ = d.messagesOfCard(c.id) }
    assertSame(d, b, "nothing changed")
    // a new message into a chat
    let s = b.timelines.values.filter { $0.timelineId.hasPrefix("session/") }.max { $0.items.count < $1.items.count }!.objectId
    var ch = Change()
    b.apply(message(b, session: s, n: 990_000_001), change: &ch)
    XCTAssertTrue(ch.timelines.count + ch.sessions.count > 0, "the message was taken")
    assertSame(d, b, "a new message")
    // a body changed in place (a header filled, an item edited where the timeline lies)
    let t = b.timelines[timelineKeyOf("chat", "session/\(s)")]!
    let n = t.numbers.first!
    t.items[n]!.content = .obj(["text": .str("changed in place"), "attachments": .arr([])])
    assertSame(d, b, "an item changed in place")
    XCTAssertTrue(rows(d.messagesOf(agent: d.agents.first { $0.sessionId == s }!.id)).contains { $0.contains("changed in place") })
    // an own echo on its way
    t.echoes["local-1"] = TimelineItem(envelopeNumber: nil, localId: "local-1", pending: true, envelopeHash: nil, senderDeviceId: b.myDeviceId ?? "", senderSequence: nil,
                                       recipientDeviceId: nil, sentAt: 1_800_000_000_001, itemState: "loaded", contentType: "message", content: .obj(["text": .str("on its way")]))
    assertSame(d, b, "an echo")
    // an artifact a message names is closed: its message goes
    if let (pid, _) = b.published.first(where: { p in b.timelines.values.contains { t in t.items.values.contains { $0.content?["published_object_id"].string == p.key } } }) {
      b.published[pid]!.objectState = "closed"
      assertSame(d, b, "an artifact closed")
    }
    // a device becomes human (who said it changes)
    if let agent = b.members.values.first(where: { $0.deviceRole == "agent" }) {
      agent.deviceRole = "human"
      assertSame(d, b, "a device's role")
    }
    // the board read again from scratch: new timelines, revisions from 0
    let again = try demo()
    b.adopt(again)
    assertSame(d, b, "the board adopted")
  }

  func testTheDeskUnitsAreTheSame() throws {
    let b = try demo()
    let d = DeskModel(board: b)
    for desk in [nil, ALL_DESKS] + d.desks.map({ Optional($0.id) }) {
      let v = d.view(desk: desk, now: 1_800_000_000_000)
      for u in v.units {
        // (the summary as it was computed before the per-agent indices: over every agent, every card, every line)
        let fresh = v.fresh.filter { $0.agent == u.id }
        XCTAssertEqual(u.open, fresh.count)
        XCTAssertEqual(u.stuck, fresh.contains { $0.isKnock })
        XCTAssertEqual(u.running, u.agent.online && d.tasks.contains { $0.agent == u.id && $0.state == "working" })
        let waiting = d.cards.filter { $0.agent == u.id && $0.status == "open" && $0.withAgent == nil }
        let blocked = d.blockedOf(u.agent, now: 1_800_000_000_000)
        if !u.agent.archived && u.agent.online && linkOf(u.agent, now: 1_800_000_000_000)?.state != "cut" {
          let want = waiting.contains { $0.kind == "permission" } ? "permission" : waiting.contains { $0.urgency == "critical" } ? "blocking" : nil
          XCTAssertEqual(blocked?.0, want, "\(u.agent.name)")
        }
        XCTAssertEqual(u.blocked?.why, blocked?.0)
      }
    }
  }
}
