// Agents.swift: the concrete agents of a board (Settings · Agents), as the web app's ui.mjs agentsOf: the sessions (not
// archived, not removed, not on a person's own device) grouped by program and computer, as the connector reports them
// in its device register (platform, host, folder; the session's model where no platform is said). Under each agent
// its main sessions (a helper whose main is on the same agent is counted beside its main, not listed).
import Foundation

public struct ConcreteAgent: Identifiable {
  public struct Branch { public let agent: Agent; public let helpers: Int; public let folder: String }
  public var id: String { "\(program) · \(host)" }
  public let program: String
  /** The computer as the connector names it; "" when it named none. */
  public let host: String
  public let sessions: [Agent]
  public let tops: [Branch]
  /** The agent devices of all its sessions: what Remove takes out of the room. */
  public let devices: [String]
  public var online: Bool { sessions.contains { $0.online } }
  public var seen: UInt64 { sessions.map(\.seen).max() ?? 0 }
}

/** The program a session's agent device runs: what the connector says (platform), else what the model tells. */
public func programOf(platform: String?, model: String) -> String {
  switch platform {
  case "claude-code": return "Claude Code"
  case "codex": return "Codex"
  default:
    let m = model.lowercased()
    if m.hasPrefix("gpt") || m.hasPrefix("codex") || (m.hasPrefix("o") && m.dropFirst().first?.isNumber == true) { return "Codex" }
    return m.contains("claude") ? "Claude Code" : "Agent"
  }
}

/** The board's concrete agents, the most sessions first. me: this device (never removed with an agent). */
public func concreteAgents(_ agents: [Agent], members: [String: RoomMember], me: String? = nil) -> [ConcreteAgent] {
  var order: [String] = [], groups: [String: (program: String, host: String, list: [Agent])] = [:]
  for a in agents where !a.archived && !a.removed && !a.own {
    let d = a.agentDeviceId.flatMap { members[$0] }
    let program = programOf(platform: d?.platform, model: a.model), host = d?.host ?? ""
    let key = "\(program) · \(host)"
    if groups[key] == nil { groups[key] = (program, host, []); order.append(key) }
    groups[key]!.list.append(a)
  }
  return order.map { key -> ConcreteAgent in
    let g = groups[key]!, ids = Set(g.list.map(\.id))
    let tops = g.list.filter { $0.parent == nil || !ids.contains($0.parent!) }.map { a in
      ConcreteAgent.Branch(agent: a, helpers: g.list.filter { $0.parent == a.id }.count, folder: a.agentDeviceId.flatMap { members[$0]?.folder } ?? "")
    }
    var devices: [String] = []
    for a in g.list { if let d = a.agentDeviceId, d != me, members[d]?.deviceRole != "human", !devices.contains(d) { devices.append(d) } }
    return ConcreteAgent(program: g.program, host: g.host, sessions: g.list, tops: tops, devices: devices)
  }.sorted { ($0.sessions.count, $1.program, $1.host) > ($1.sessions.count, $0.program, $0.host) }
}
