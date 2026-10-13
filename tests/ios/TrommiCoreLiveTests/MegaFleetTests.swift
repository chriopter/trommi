// Many iPhones of one person in one process: the iOS app's engine (`Room`, the real core), N times, as the human
// devices of the mega test's torture run (tests/client/mega/torture.rs). Runs only when TROMMI_MEGA_FLEET names a
// folder: commands come in files there (cmd-<n>.json → res-<n>.json, in order), exactly as for MegaObserverTests,
// each naming its device by `dev` (0…). Every device has a folder of its own (its store, its cache), its own core
// and its own live stream; they share the process, the network and the clock.
//
//   { op: "code", dev, hub, room_id, code }      the device signs in with the recovery code (a fresh install)
//   { op: "pair", dev, by, live }                the device joins by link, as "Pair a device" does: `by` makes the link
//                                                and confirms the six emoji; then it catches up (and follows the live
//                                                stream with `live`). { ms, device }
//   { op: "reinstall", dev, by, live }           the app deleted and installed again: its folder wiped, paired anew
//   { op: "kill", dev }                          the app killed: no save, nothing more of it
//   { op: "restart", dev, live }                 the app opened again from its folder (warm), caught up. { ms }
//   { op: "live", dev, on }                      the live stream on or off (off: an offline phase, nothing is read)
//   { op: "sync", dev? }                         catch up (every device that is on, at once, without dev). { ms }
//   { op: "snapshot", dev } | { op: "shown", dev } | { op: "stats", dev }
//   { op: "answer", dev, card, choices } | { op: "say", dev, session, text } | { op: "archive", dev, session }
//   { op: "allow_join", dev, allow } | { op: "invite_agent", dev } | { op: "confirm_invite", dev, invite_id, die }
//   { op: "quit" }
#if !os(iOS)
import Foundation
import XCTest
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif
@testable import TrommiClient
@testable import TrommiCoreLive

/// Bytes in and out by the bearer token a request carries (a device's tokens are noted as it gets them).
final class FleetNet: URLProtocol, @unchecked Sendable {
  static let lock = NSLock()
  nonisolated(unsafe) static var bytes: [String: (into: Int, out: Int, requests: Int)] = [:]
  static let plain: URLSession = {
    let cfg = URLSessionConfiguration.ephemeral
    cfg.httpCookieStorage = nil; cfg.urlCache = nil
    cfg.httpMaximumConnectionsPerHost = 64
    return URLSession(configuration: cfg)
  }()
  // (the live stream goes past: a data task here would hold it until its end)
  override class func canInit(with request: URLRequest) -> Bool { request.url?.path.contains("/stream") != true }
  override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
  private var forward: URLSessionDataTask?
  // (after stopLoading nothing more is told to the client: Linux Foundation traps on a task it already dropped,
  //  "Trying to access a behaviour for a task that in not in the registry")
  private let state = NSLock()
  private var stopped = false
  private var isStopped: Bool { state.withLock { stopped } }
  override func startLoading() {
    var req = request
    if req.httpBody == nil, let stream = req.httpBodyStream {
      stream.open(); var data = Data(); var buf = [UInt8](repeating: 0, count: 65536)
      while stream.hasBytesAvailable { let n = stream.read(&buf, maxLength: buf.count); if n <= 0 { break }; data.append(buf, count: n) }
      stream.close(); req.httpBodyStream = nil; req.httpBody = data
    }
    let who = req.value(forHTTPHeaderField: "authorization") ?? ""
    let out = req.httpBody?.count ?? 0
    FleetNet.lock.withLock { var b = FleetNet.bytes[who] ?? (0, 0, 0); b.out += out; b.requests += 1; FleetNet.bytes[who] = b }
    // (a data task made here, on the session's own queue, would wait for that queue: made on another one)
    let sent = req
    DispatchQueue.global().async { [self] in
      let task = FleetNet.plain.dataTask(with: sent) { [self] data, response, error in
        if isStopped { return }
        if let error { client?.urlProtocol(self, didFailWithError: error); return }
        if let response { client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed) }
        if let data {
          client?.urlProtocol(self, didLoad: data)
          FleetNet.lock.withLock { var b = FleetNet.bytes[who] ?? (0, 0, 0); b.into += data.count; FleetNet.bytes[who] = b }
        }
        client?.urlProtocolDidFinishLoading(self)
      }
      let go = state.withLock { () -> Bool in
        if stopped { return false }
        forward = task
        return true
      }
      if go { task.resume() }
    }
  }
  override func stopLoading() { state.withLock { stopped = true; forward?.cancel() } }
}

@MainActor
final class MegaFleetTests: XCTestCase {
  final class Dev {
    var room: Room?
    var base: URL
    var live: Task<Void, Never>?
    var tokens = Set<String>()
    init(base: URL) { self.base = base }
  }
  private var devs: [Int: Dev] = [:]
  private var root: URL!
  private var hubURL = ""

  private func dev(_ cmd: [String: Any]) throws -> Dev {
    let i = cmd["dev"] as? Int ?? 0
    if let d = devs[i] { return d }
    let d = Dev(base: root.appendingPathComponent("dev-\(i)"))
    try FileManager.default.createDirectory(at: d.base, withIntermediateDirectories: true)
    devs[i] = d
    return d
  }
  private func room(_ cmd: [String: Any]) throws -> Room {
    guard let r = try dev(cmd).room else { throw TrommiError("test", "device \(cmd["dev"] ?? 0) has no room") }
    return r
  }
  private func note(_ d: Dev) async {
    if ProcessInfo.processInfo.environment["TROMMI_FLEET_NOTE"] == "0" { return }
    if let t = try? await d.room?.hub.accessToken() { d.tokens.insert("Bearer \(t)") }
  }
  private func settle(_ r: Room) async { for _ in 0..<3 { _ = try? await r.sync() } }
  private func goLive(_ d: Dev, _ on: Bool) {
    d.live?.cancel(); d.live = nil
    guard on, let r = d.room else { return }
    d.live = Task { await r.runLive() }
  }
  private func ms(_ t: UInt64) -> UInt64 { nowMs() - t }

  private func snapshot(_ r: Room) -> [String: Any] {
    let b = r.board
    var cards = [String: Any](), perms = [String: Any](), sessions = [String: Any]()
    for c in b.cards.values {
      var a: Any = NSNull()
      if let ans = c.answer { a = ["action": ans.answerAction, "choices": ans.choices.sorted()] }
      cards[c.objectId] = ["state": c.objectState, "title": c.contentState == "ok" ? c.title as Any : NSNull(), "version": c.objectVersion,
                           "closed_how": c.closedHow as Any? ?? NSNull(), "in_revision": c.inRevision?.by as Any? ?? NSNull(), "answer": a,
                           "held": b.heldBack(c.objectId, open: c.objectState == "open"), "session": c.sessionId as Any? ?? NSNull()]
    }
    for p in b.permissions.values {
      perms[p.objectId] = ["state": p.permissionState, "tool": p.toolName.isEmpty ? NSNull() : p.toolName as Any,
                           "held": b.heldBack(p.objectId, open: p.permissionState == "pending"), "session": p.sessionId as Any? ?? NSNull()]
    }
    for s in b.sessions.values {
      var name: Any = NSNull()
      // (an archived session's content, its name too, loads only when it is opened: no name to compare)
      if !s.settings["archived"].truthy, case .obj(let o) = s.profile, let n = o["agent_name"]?.string { name = n }
      sessions[s.sessionId] = ["agent_name": name, "parent": s.parentSessionId as Any? ?? NSNull()]
    }
    return ["desk": b.stack, "permissions_open": b.openPermissionIds, "cards": cards, "permissions": perms, "sessions": sessions,
            "snoozed": b.human.snoozes.keys.sorted(), "snooze_values": b.human.snoozes.mapValues { $0.any },
            "debug": ["alerts": b.alerts.map { "\($0.code): \($0.message)" }]]
  }

  /** The session list as the sidebar shows it (DeskModel.agents, not archived, in order). */
  private func shown(_ r: Room) -> [[String: Any]] {
    let agents = DeskModel(board: r.board).agents
    let byId = Dictionary(agents.map { ($0.id, $0.sessionId) }, uniquingKeysWith: { a, _ in a })
    return agents.filter { !$0.archived }.map { a in
      ["session": a.sessionId, "name": a.name, "online": a.online, "agent": a.agentDeviceId as Any? ?? NSNull(), "parent": a.parent.flatMap { byId[$0] ?? $0 } as Any? ?? NSNull()]
    }
  }

  /** Pairs device `d` by device `by` (the link, the six emoji, "They match"), as "Pair a device" does. */
  private func pair(_ d: Dev, by: Room, live: Bool) async throws -> [String: Any] {
    let t = nowMs()
    var p = try await by.createPairing(app: "https://app.example")
    let link = p.link, base = d.base
    let joining = Task { () throws -> Room in try await Room.join(link: link, base: base, pollMs: 100) { _ in } }
    let until = nowMs() + 120_000
    while try await !by.checkPairing(&p) {
      if nowMs() > until { joining.cancel(); throw TrommiError("test", "no Request to the pairing") }
      try await Task.sleep(nanoseconds: 100_000_000)
    }
    try await by.confirmPairing(p, matches: true)
    let r = try await joining.value
    let joined = ms(t)
    d.room = r
    await settle(r)
    goLive(d, live)
    await note(d)
    return ["ms": ms(t), "joined_ms": joined, "device": r.board.myDeviceId]
  }

  private func run(_ cmd: [String: Any]) async throws -> Any {
    let op = cmd["op"] as? String ?? ""
    switch op {
    case "code":
      let d = try dev(cmd)
      let t = nowMs()
      let code = try XCTUnwrap(cmd["code"] as? String).fleetHex()
      hubURL = cmd["hub"] as! String
      let r = try await Room.joinWithRecoveryCode(hubURL: hubURL, roomId: cmd["room_id"] as! String, code: code, base: d.base)
      d.room = r
      await settle(r)
      goLive(d, cmd["live"] as? Bool ?? true)
      await note(d)
      return ["ms": ms(t), "device": r.board.myDeviceId]
    case "pair":
      let d = try dev(cmd)
      let by = try room(["dev": cmd["by"] as? Int ?? 0])
      return try await pair(d, by: by, live: cmd["live"] as? Bool ?? true)
    case "reinstall":
      let d = try dev(cmd)
      d.live?.cancel(); d.live = nil
      if let r = d.room { r.close() }
      d.room = nil
      try? FileManager.default.removeItem(at: d.base)
      try FileManager.default.createDirectory(at: d.base, withIntermediateDirectories: true)
      let by = try room(["dev": cmd["by"] as? Int ?? 0])
      return try await pair(d, by: by, live: cmd["live"] as? Bool ?? true)
    case "kill":
      let d = try dev(cmd)
      d.live?.cancel(); d.live = nil
      d.room?.close()
      d.room = nil
      return [:]
    case "restart":
      let d = try dev(cmd)
      d.live?.cancel(); d.live = nil
      if let r = d.room { await r.shutdown() }
      let t = nowMs()
      guard let id = Store.rooms(base: d.base).first else { throw TrommiError("test", "no room stored") }
      let r = try Room.open(base: d.base, roomId: id)
      _ = await r.restore()
      let warm = ms(t)
      d.room = r
      await settle(r)
      goLive(d, cmd["live"] as? Bool ?? true)
      await note(d)
      return ["ms": ms(t), "warm_ms": warm]
    case "live":
      let d = try dev(cmd)
      goLive(d, cmd["on"] as? Bool ?? true)
      return [:]
    case "sync":
      let t = nowMs()
      if cmd["dev"] != nil { let d = try dev(cmd); if let r = d.room { await settle(r); await note(d) }; return ["ms": ms(t)] }
      await withTaskGroup(of: Void.self) { g in
        for d in devs.values { if let r = d.room { g.addTask { @MainActor in await self.settle(r); await self.note(d) } } }
      }
      return ["ms": ms(t)]
    case "snapshot": return snapshot(try room(cmd))
    case "shown": return shown(try room(cmd))
    case "stats":
      let d = try dev(cmd)
      let (into, out, requests) = FleetNet.lock.withLock { () -> (Int, Int, Int) in
        var a = (0, 0, 0)
        for t in d.tokens { if let b = FleetNet.bytes[t] { a.0 += b.into; a.1 += b.out; a.2 += b.requests } }
        return a
      }
      return ["bytes_in": into, "bytes_out": out, "requests": requests, "cards": d.room?.board.cards.count ?? 0, "desk": d.room?.board.stack.count ?? 0]
    case "answer": let r = try room(cmd); try await r.answer(cardId: cmd["card"] as! String, choices: cmd["choices"] as? [String] ?? []); return [:]
    case "say": let r = try room(cmd); try await r.sendMessage(sessionId: cmd["session"] as? String, text: cmd["text"] as? String ?? ""); return [:]
    case "archive": let r = try room(cmd); try await r.editSession(cmd["session"] as! String, ["archived": .bool(true)]); return [:]
    case "allow_join":
      let r = try room(cmd)
      let until = nowMs() + UInt64(cmd["timeout_ms"] as? Int ?? 60_000)
      while nowMs() < until {
        _ = try? await r.sync()
        let folder = cmd["folder"] as? String
        if let a = SelfJoinPlan.asks(r.board, answered: [], holds: { _ in true }, pending: []).first(where: { a in folder.map { f in a.ask["folder"].string.map { $0 == f || $0.hasSuffix("/" + f) } ?? false } ?? true }) {
          try await r.answerVouch(a.nonce, allow: cmd["allow"] as? Bool ?? true)
          return ["nonce": a.nonce]
        }
        try await Task.sleep(nanoseconds: 200_000_000)
      }
      throw TrommiError("test", "no self-join ask")
    case "invite_agent":
      let r = try room(cmd)
      let inv = try await r.createAgentInvite(app: "https://app.example", label: cmd["label"] as? String)
      invites[inv.pairing.inviteId] = inv
      return ["invite_id": inv.pairing.inviteId, "link": inv.pairing.link]
    case "confirm_invite":
      let d = try dev(cmd)
      let r = try room(cmd)
      guard var inv = invites[cmd["invite_id"] as? String ?? ""] else { throw TrommiError("test", "no such invite") }
      let until = nowMs() + 60_000
      while try await !r.checkPairing(&inv.pairing) {
        if nowMs() > until { throw TrommiError("test", "no Request to the invite") }
        try await Task.sleep(nanoseconds: 100_000_000)
      }
      let agent = inv.pairing.newcomerId ?? ""
      if cmd["die"] as? Bool == true {
        // the app is killed right after "They match": the Commit goes out, the app is gone before it founds anything
        let confirming = Task { @MainActor in try? await r.confirmAgent(inv, matches: true) }
        let end = nowMs() + 20_000
        while nowMs() < end && r.board.members[agent] == nil {
          _ = try? await r.sync()
          try await Task.sleep(nanoseconds: 50_000_000)
        }
        d.live?.cancel(); d.live = nil
        r.close(); d.room = nil
        confirming.cancel()
        return ["agent": agent, "died": true]
      }
      let sid = try await r.confirmAgent(inv, matches: true)
      return ["agent": agent, "session": sid]
    case "quit":
      for d in devs.values { d.live?.cancel(); if let r = d.room { await r.shutdown() } }
      return [:]
    default: throw TrommiError("test", "unknown op \(op)")
    }
  }
  private var invites: [String: Room.AgentInvite] = [:]

  func testFleet() async throws {
    guard let path = ProcessInfo.processInfo.environment["TROMMI_MEGA_FLEET"], !path.isEmpty else { throw XCTSkip("TROMMI_MEGA_FLEET names no folder") }
    Core.tools = LiveCore()
    HubClient.transportForTests = [FleetNet.self]
    defer { HubClient.transportForTests = [] }
    let dir = URL(fileURLWithPath: path)
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    root = dir.appendingPathComponent("devices")
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    try "\(ProcessInfo.processInfo.processIdentifier)".write(to: dir.appendingPathComponent("ready"), atomically: true, encoding: .utf8)
    // Commands run one after the other, except those marked `"async": true`, which start at once and answer when done:
    // many devices act at the same time.
    var n = 0
    while true {
      let file = dir.appendingPathComponent("cmd-\(n).json")
      while !FileManager.default.fileExists(atPath: file.path) { try await Task.sleep(nanoseconds: 20_000_000) }
      let cmd = try JSONSerialization.jsonObject(with: Data(contentsOf: file)) as? [String: Any] ?? [:]
      let k = n
      let work = { @MainActor () async in
        var res: [String: Any]
        do { res = ["ok": true, "result": try await self.run(cmd)] } catch {
          let code = (error as? TrommiError)?.code ?? (error as? HubError).map { "\($0)" } ?? ""
          res = ["ok": false, "error": code, "message": "\(error)"]
        }
        let tmp = dir.appendingPathComponent("res-\(k).json.tmp")
        try? JSONSerialization.data(withJSONObject: res, options: [.sortedKeys]).write(to: tmp)
        try? FileManager.default.moveItem(at: tmp, to: dir.appendingPathComponent("res-\(k).json"))
      }
      if cmd["async"] as? Bool == true { Task { @MainActor in await work() } } else { await work() }
      if cmd["op"] as? String == "quit" { return }
      n += 1
    }
  }
}

private extension String {
  func fleetHex() throws -> Bytes {
    var out = Bytes(); var i = startIndex
    while i < endIndex { let j = index(i, offsetBy: 2); guard let b = UInt8(self[i..<j], radix: 16) else { throw TrommiError("test", "bad hex") }; out.append(b); i = j }
    return out
  }
}
#endif
