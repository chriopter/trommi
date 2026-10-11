// AgentsTests: the concrete agents of the demo room (Agents.swift, as the web's ui.mjs agentsOf): its sessions grouped
// by program and computer as their device registers say, the main sessions under each, the helpers counted.
import Foundation
import XCTest
@testable import TrommiClient

final class AgentsTests: XCTestCase {
  func testTheProgramIsWhatTheConnectorSaysElseTheModel() {
    XCTAssertEqual(programOf(platform: "claude-code", model: ""), "Claude Code")
    XCTAssertEqual(programOf(platform: "codex", model: "claude-opus"), "Codex")
    XCTAssertEqual(programOf(platform: nil, model: "gpt-6.1-sol"), "Codex")
    XCTAssertEqual(programOf(platform: nil, model: "claude-opus-5-5"), "Claude Code")
    XCTAssertEqual(programOf(platform: nil, model: ""), "Agent")
  }

  func testTheDemoRoomsAgentsAreProgramsOnComputers() throws {
    var repo = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
    while repo.path != "/", !FileManager.default.fileExists(atPath: repo.appendingPathComponent("demo/data/fixture.json").path) { repo = repo.deletingLastPathComponent() }
    let b = try DemoFixture.board(try Data(contentsOf: repo.appendingPathComponent("demo/data/fixture.json")), now: nowMs())
    let d = DeskModel(board: b)
    let agents = concreteAgents(d.agents, members: b.members, me: b.myDeviceId)
    XCTAssertEqual(agents.map(\.id), ["Claude Code · MacBook Pro", "Claude Code · Mac mini", "Codex · MacBook Pro", "Claude Code · build-01", "Codex · build-01"])
    XCTAssertEqual(agents.reduce(0) { $0 + $1.sessions.count }, d.agents.filter { !$0.archived && !$0.removed && !$0.own }.count, "every session under one agent")
    let first = agents[0]
    XCTAssertEqual(Set(first.tops.map { $0.agent.id }), ["fernly-web", "fernly-ios"])
    XCTAssertEqual(first.tops.map(\.helpers).reduce(0, +), first.sessions.count - first.tops.count, "the others are helpers, counted beside their main")
    XCTAssertEqual(first.tops.first { $0.agent.id == "fernly-web" }?.folder, "~/code/fernly-web")
    XCTAssertEqual(first.devices.count, Set(first.sessions.compactMap(\.agentDeviceId)).count)
  }
}
