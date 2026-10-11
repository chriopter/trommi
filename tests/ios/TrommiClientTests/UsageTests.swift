// UsageTests: an agent's account and usage as its connector names them (Usage.swift, as the web's ui.mjs usageMeters):
// the plan tag, the meters the account has, a window that started again left out, the times of a reset, Codex's credits,
// and the demo room's agents carrying theirs in their main sessions.
import Foundation
import XCTest
@testable import TrommiClient

final class UsageTests: XCTestCase {
  let now: UInt64 = 1_791_707_400_000   // Sun 11 Oct 2026 08:30 UTC
  func json(_ s: String) -> JV { try! JSONDecoder().decode(JV.self, from: Data(s.utf8)) }

  func testClaudeCodeHasTheFiveHourWindowAndTheWeek() throws {
    let a = try XCTUnwrap(UsageAccount(json(#"{"kind":"claude","plan":"max","email":"ada@example.org","five_hour":{"used":62.4,"resets_at":\#(now + 7_800_000)},"weekly":{"used":84,"resets_at":\#(now + 271_200_000)}}"#), now: now))
    XCTAssertEqual(a.planTag, "Claude Max")
    XCTAssertEqual(a.email, "ada@example.org")
    XCTAssertEqual(a.meters.map(\.label), ["5 h", "week"])
    XCTAssertEqual(a.meters.map(\.used), [62, 84])
    XCTAssertNil(a.creditsText)
  }

  func testCodexHasTheWeekItsCreditsAndResets() throws {
    let a = try XCTUnwrap(UsageAccount(json(#"{"kind":"codex","plan":"prolite","email":"ada@work.example","weekly":{"used":41,"resets_at":\#(now + 1000)},"credits":2400,"full_resets":1}"#), now: now))
    XCTAssertEqual(a.planTag, "ChatGPT Pro Lite")
    XCTAssertEqual(a.meters.map(\.id), ["weekly"])
    XCTAssertEqual(a.creditsText, "2,400 credits")
    XCTAssertEqual(a.fullResetsText, "1 full reset")
  }

  func testAWindowThatStartedAgainIsLeftOutAndPercentsStayInRange() throws {
    let a = try XCTUnwrap(UsageAccount(json(#"{"kind":"claude","plan":"pro","five_hour":{"used":90,"resets_at":\#(now - 1)},"weekly":{"used":130}}"#), now: now))
    XCTAssertEqual(a.meters.map(\.id), ["weekly"])
    XCTAssertEqual(a.meters.first?.used, 100)
    XCTAssertNil(a.meters.first?.resetsAt)
    XCTAssertNil(UsageAccount(json(#"{"kind":"claude"}"#), now: now), "nothing to show without a plan or an e-mail")
    XCTAssertNil(UsageAccount(.null, now: now))
  }

  func testAResetWithinADayIsAClockTimeElseADay() {
    let utc = TimeZone(identifier: "UTC")!
    XCTAssertEqual(usageReset(now + 5 * 3_600_000 + 13 * 60_000, now: now, timeZone: utc), "13:43")
    XCTAssertEqual(usageReset(now + 3 * 86_400_000, now: now, timeZone: utc), "Wed 14 Oct")
  }

  func testTheDemoRoomsAgentsCarryTheirAccountInTheirMainSession() throws {
    var repo = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
    while repo.path != "/", !FileManager.default.fileExists(atPath: repo.appendingPathComponent("demo/data/fixture.json").path) { repo = repo.deletingLastPathComponent() }
    let b = try DemoFixture.board(try Data(contentsOf: repo.appendingPathComponent("demo/data/fixture.json")), now: nowMs())
    let helper = try XCTUnwrap(b.sessions.values.first { $0.profile["agent_name"].string == "Landing Page" })
    XCTAssertNil(helper.profile["account"].object, "the connector writes it into its main session only")
    let main = try XCTUnwrap(b.sessions.values.first { $0.profile["agent_name"].string == "Fernly Website" })
    let acc = try XCTUnwrap(usageOf(b, agentDeviceId: main.agentDeviceId, now: nowMs()))
    XCTAssertEqual(acc.planTag, "Claude Max")
    XCTAssertEqual(acc.meters.map(\.used), [62, 84])
    let codex = try XCTUnwrap(b.sessions.values.first { $0.profile["agent_name"].string == "Infrastructure" })
    XCTAssertEqual(usageOf(b, agentDeviceId: codex.agentDeviceId, now: nowMs())?.meters.map(\.label), ["week"])
  }
}
