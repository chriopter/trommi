// File-command observers for tests/client/mega. They execute the shipped native shell; all model and account
// behavior comes from AppCore. Opt-in only: the ordinary shell gate runs no fleet or load test.
import Foundation
import XCTest
@testable import TrommiClient
#if canImport(Glibc)
import Glibc
#else
import Darwin
#endif

private func now() -> Double { Date().timeIntervalSince1970 * 1000 }
private func object<T: Encodable>(_ value: T) -> [String: Any] { (try? JSONSerialization.jsonObject(with: JSONEncoder().encode(value))) as? [String: Any] ?? [:] }
private func dictionary(_ value: Any?) -> [String: Any] { value as? [String: Any] ?? [:] }
private func array(_ value: Any?) -> [[String: Any]] { value as? [[String: Any]] ?? [] }
private func text(_ value: Any?) -> String? { dictionary(value)["en"] as? String }
private let null = NSNull()

private actor ObserverState {
  var view = CoreView.ViewModel()
  var flying = 0
  var lastNet = now()
  var requests: [String: Int] = [:]
  var bytes = 0
  var faults: [String: Any] = [:]
  var rng: UInt64 = 7
  var presence: [[Any]] = []
  var seen: [String: Bool] = [:]
  var dying: CoreSession?
  var didDie = false
  var tracing = false
  var traceStart = 0.0
  var samples: [[Any]] = []
  func beginTrace() { tracing = true; traceStart = Double(Spent.now) / 1e6; samples = []; Spent.traceStart() }
  func counts() -> [String: Any] { ["desk": view.desk.stack.count, "cards": view.desk.stack.count + view.desk.done.count + view.desk.later.count, "sessions": view.sidebar.sessions.count, "online": view.sidebar.sessions.filter(\.online).count, "reading": view.phase != .live] }
  func trace() -> [String: Any] {
    tracing = false
    let spans: [[Any]] = Spent.traceTake().map { ["step", $0.0, Double($0.1) / 1e6, Double($0.2) / 1e6] }
    return ["platform": "ios", "t0": traceStart, "spans": spans, "samples": samples, "events": [], "final": counts(), "requests": requests]
  }
  func arm(_ core: CoreSession?) { dying = core; didDie = false }
  func died() -> Bool { didDie }
  func take(_ packet: NativePacket) throws {
    if packet.json["type"] as? String == "render" {
      try view.apply(JSONDecoder().decode(CoreView.ViewDelta.self, from: JSONSerialization.data(withJSONObject: packet.json["delta"]!)))
      if tracing { samples.append([Double(Spent.now) / 1e6, counts()]) }
      for row in view.sidebar.sessions {
        if let before = seen[row.id], before != row.online { presence.append([now(), row.id, row.online]) }
        seen[row.id] = row.online
      }
    }
  }
  func model() -> CoreView.ViewModel { view }
  func json() -> [String: Any] { object(view) }
  func before(_ method: String, _ address: String) async -> String? {
    guard let url = URL(string: address), !url.path.hasSuffix("/stream") else { return nil }
    let path = url.path.replacingOccurrences(of: "/[A-Za-z0-9_-]{20,}", with: "/:id", options: .regularExpression)
    requests["\(method) \(path)", default: 0] += 1; flying += 1; lastNet = now()
    rng = (rng &* 1103515245 &+ 12345) & 0x7fffffff
    let delay = faults["delay_ms"] as? Double ?? 0
    if delay > 0 { try? await Task.sleep(nanoseconds: UInt64(delay * 1_000_000)) }
    if faults["changes_down"] as? Bool == true && url.path.hasSuffix("/changes") { return "offline" }
    return Double(rng) / Double(0x7fffffff) < (faults["fail"] as? Double ?? 0) ? "offline" : nil
  }
  func after(_ method: String, _ address: String, _ status: Int, _ count: Int) async {
    guard URL(string: address)?.path.hasSuffix("/stream") != true else { return }
    flying = max(0, flying - 1); bytes += count; lastNet = now()
    if method == "POST", address.hasSuffix("/commits"), (200..<300).contains(status), let core = dying { dying = nil; didDie = true; await core.close() }
  }
  func setFaults(_ value: [String: Any]) { faults = value; if let seed = value["seed"] as? NSNumber { rng = seed.uint64Value } }
  func quiet() -> Bool { flying == 0 && now() - lastNet >= 150 }
  func log(_ reset: Bool) -> [[Any]] { let out = presence; if reset { presence = [] }; return out }
  func stats(_ reset: Bool) -> [String: Any] {
    var usage = rusage()
    #if canImport(Glibc)
    getrusage(Int32(RUSAGE_SELF.rawValue), &usage); let rss = Int(usage.ru_maxrss) * 1024
    #else
    getrusage(RUSAGE_SELF, &usage); let rss = Int(usage.ru_maxrss)
    #endif
    let out: [String: Any] = ["rss": rss, "requests": requests, "bytes": bytes, "flying": flying, "quiet_ms": now() - lastNet, "desk": view.desk.stack.count, "cards": view.desk.stack.count + view.desk.done.count + view.desk.later.count, "phase": view.phase.rawValue]
    if reset { requests = [:]; bytes = 0 }; return out
  }
}

private final class NativeObserver {
  let directory: URL
  let state = ObserverState()
  var core: CoreSession?
  var hub = ""
  init(_ directory: URL) { self.directory = directory }
  func open() async throws {
    let state = self.state
    core = try CoreSession(directory: directory, network: NativeNetworkHooks(before: { method, url in await state.before(method, url) }, after: { method, url, status, count in await state.after(method, url, status, count) })) { packet in try? await state.take(packet) }
    await send(["type": "start", "config": ["platform": "ios", "version": "test", "seed": 7, "hub": hub]])
  }
  func send(_ value: [String: Any]) async { await core?.send(NativePacket(value)) }
  func act(_ value: [String: Any]) async { await send(["type": "action", "action": value]) }
  func account(_ value: [String: Any]) async { await act(["do": "account", "action": value]) }
  func focus(_ screen: [String: Any]) async { await send(["type": "focus", "focus": ["screen": screen, "visible": true, "desk": "all"]]) }
  func wait(_ label: String, timeout: Double = 180_000, _ predicate: (CoreView.ViewModel) -> Bool) async throws {
    let until = now() + timeout
    while !predicate(await state.model()) {
      if now() >= until { throw TrommiError("timeout", label) }
      try await Task.sleep(nanoseconds: 50_000_000)
    }
  }
  func quiet() async throws {
    let until = now() + 60_000
    while !(await state.quiet()) {
      if now() >= until { throw TrommiError("timeout", "native I/O did not settle") }
      try await Task.sleep(nanoseconds: 50_000_000)
    }
  }
  func settle() async throws { await send(["type": "foreground"]); try await wait("live", timeout: 180_000) { $0.phase == .live }; try await quiet() }
  func snapshot(_ given: [String]) async throws -> [String: Any] {
    let initial = await state.model(); let desk = initial.desk
    var ids = Set(desk.stack + desk.offYourMind + desk.later + desk.done + given + (desk.starred?.rows.map(\.id) ?? []) + (desk.running?.rows.map(\.id) ?? []))
    for row in initial.sidebar.sessions {
      await focus(["screen": "session", "session_id": row.id]); try await quiet()
      for question in (await state.model()).sessions[row.id]?.questions ?? [] { ids.insert(question.id) }
    }
    var cards: [String: Any] = [:], permissions: [String: Any] = [:], snooze: [String: Any] = [:]
    for id in ids.sorted() {
      await focus(["screen": "card", "card_id": id]); try await quiet()
      guard let card = (await state.model()).cards[id] else { continue }
      let held = card.status == "catching"
      if card.kind == "request" {
        let choice = card.options.first(where: \.chosen)?.key
        let status: Any = held ? null : card.state == "open" ? "pending" : choice == "allow" ? "allowed" : choice == "deny" ? "denied" : card.answer.summary == "Answered in the terminal" ? "withdrawn" : "expired"
        permissions[id] = ["state": status, "tool": card.title.hasPrefix("Approval: ") ? String(card.title.dropFirst(10)) as Any : null, "held": held, "session": card.sessionId as Any? ?? null]
      } else {
        var answer: Any = null
        if card.state != "open" && !held {
          if card.status == "shredded" { answer = ["action": "shred", "choices": []] as [String: Any] }
          else if card.kind == "info" { if card.answer.back == "reopen" { answer = ["action": "read", "choices": []] as [String: Any] } }
          else { let choices = card.options.filter(\.chosen).map(\.key).sorted(); if !choices.isEmpty || card.answer.label?.key.hasPrefix("card.said.duck") == true { answer = ["action": "answer", "choices": choices] as [String: Any] } }
        }
        let how = ["decided": "answered", "shredded": "shredded", "done": "closed"][card.status]
        cards[id] = ["state": card.state, "title": card.answer.mode == "loading" ? null : card.title as Any, "version": card.history, "closed_how": card.state == "open" ? null : how as Any? ?? null, "in_revision": card.answer.mode == "revising" ? true as Any : null, "answer": answer, "held": held, "session": card.sessionId as Any? ?? null]
        if let until = card.snoozedUntil { snooze[id] = until }
      }
    }
    await focus(["screen": "desk"])
    let view = await state.model(); var sessions: [String: Any] = [:]
    for row in view.sidebar.sessions { sessions[row.id] = ["agent_name": row.name.en, "parent": row.parent as Any? ?? null] }
    return ["desk": view.desk.stack, "permissions_open": view.desk.rows.filter { $0.kind == "request" }.map(\.id), "cards": cards, "permissions": permissions, "sessions": sessions, "snoozed": view.desk.later.sorted(), "snooze_values": snooze, "debug": ["phase": view.phase.rawValue, "toasts": view.toasts.map { $0.text.en }]]
  }
  func run(_ cmd: [String: Any]) async throws -> Any {
    let op = cmd["op"] as? String ?? ""
    switch op {
    case "signin", "code":
      if cmd["trace"] as? Bool == true { await state.beginTrace() }
      hub = cmd["hub"] as? String ?? ""; try await open()
      guard let email = cmd["email"] as? String, let password = cmd["password"] as? String else { throw TrommiError("bad-format", "the native observer signs in with the test account") }
      for attempt in 0..<6 {
        await account(["step": "sign_in", "email": email, "password": password, "hub": hub])
        try await wait("sign-in") { $0.phase == .live || $0.phase == .opening || $0.account.problem != nil }
        if (await state.model()).account.problem == nil { break }
        if attempt == 5 { throw TrommiError("sign-in", "the test account could not sign in") }
        try await Task.sleep(nanoseconds: 2_100_000_000)
      }
      try await settle(); return ["device": (await state.model()).settings.devices.first(where: \.thisDevice)?.id as Any? ?? null, "not_joined": 0]
    case "sync": try await settle(); return [:] as [String: Any]
    case "restart": await core?.close(); try await open(); try await settle(); return [:] as [String: Any]
    case "kill": await core?.close(); core = nil; return [:] as [String: Any]
    case "live": await send(["type": cmd["on"] as? Bool == false ? "background" : "foreground"]); return [:] as [String: Any]
    case "faults": await state.setFaults(cmd); return [:] as [String: Any]
    case "snapshot": return try await snapshot(cmd["ids"] as? [String] ?? [])
    case "stats", "unsettled": return await state.stats(cmd["reset"] as? Bool == true)
    case "shown": return (await state.model()).sidebar.sessions.map { ["session": $0.id, "name": $0.name.en, "online": $0.online, "agent": null, "parent": $0.parent as Any? ?? null] as [String: Any] }
    case "presence_log": return await state.log(cmd["reset"] as? Bool == true)
    case "answer": await act(["do": "answer", "card_id": cmd["card"] ?? "", "choices": cmd["choices"] ?? []])
    case "take_back": await act(["do": "decide_again", "card_id": cmd["card"] ?? ""])
    case "verdict": await act(["do": "verdict", "request_id": cmd["request"] ?? "", "allow": cmd["allow"] as? Bool == true])
    case "snooze": if let until = cmd["until"], !(until is NSNull) { await act(["do": "snooze", "card_id": cmd["card"] ?? "", "until_ms": until]) } else { await act(["do": "unsnooze", "card_id": cmd["card"] ?? ""]) }
    case "explain", "hand_back": await act(["do": "hand_back", "card_id": cmd["card"] ?? "", "note": op == "explain" ? "What??" : "back to you", "rework": op == "hand_back"])
    case "say": await act(["do": "write", "session_id": cmd["session"] ?? "", "text": cmd["text"] ?? ""])
    case "archive": await act(["do": "archive_session", "session_id": cmd["session"] ?? "", "on": true])
    case "invite_agent":
      await account(["step": "invite_agent", "label": cmd["label"] ?? ""])
      try await wait("invite") { $0.account.link != nil && $0.account.inviteId != nil }
      let v = await state.model(); return ["invite_id": v.account.inviteId!, "link": v.account.link!]
    case "confirm_invite":
      let id = cmd["invite_id"] as? String ?? ""
      try await wait("invite Request") { $0.account.step == "compare_emoji" && $0.account.inviteId == id }
      if cmd["die"] as? Bool == true { await state.arm(core) }
      await account(["step": "confirm_emoji", "invite_id": id, "matches": true])
      if cmd["die"] as? Bool == true {
        let until = now() + 30_000
        while !(await state.died()) { if now() >= until { throw TrommiError("timeout", "the confirmation Commit was not posted") }; try await Task.sleep(nanoseconds: 30_000_000) }
        core = nil; return ["agent": null, "died": true]
      }
      try await wait("invite complete") { $0.account.step != "compare_emoji" }; try await settle(); return ["agent": null]
    case "allow_join":
      let folder = cmd["folder"] as? String
      try await wait("self-join ask") { $0.account.asks.contains { !$0.joining && (folder == nil || $0.folder == folder || $0.folder.hasSuffix("/" + (folder ?? ""))) } }
      let ask = (await state.model()).account.asks.first { !$0.joining && (folder == nil || $0.folder == folder || $0.folder.hasSuffix("/" + (folder ?? ""))) }!
      await account(["step": "answer_ask", "nonce": ask.nonce, "allow": cmd["allow"] as? Bool != false]); try await settle(); return ["nonce": ask.nonce]
    case "trace":
      if let linger = cmd["linger_ms"] as? Double, linger > 0 { try await Task.sleep(nanoseconds: UInt64(linger * 1_000_000)) }
      return await state.trace()
    case "quit": await core?.close(); return [:] as [String: Any]
    default: throw TrommiError("bad-format", "unknown observer command")
    }
    try await quiet(); return [:] as [String: Any]
  }
}

private func commands(_ variable: String, fleet: Bool) async throws {
  guard let folder = ProcessInfo.processInfo.environment[variable] else { throw XCTSkip("opt-in native observer") }
  let directory = URL(fileURLWithPath: folder); try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
  var devices: [Int: NativeObserver] = [:]
  try Data(String(ProcessInfo.processInfo.processIdentifier).utf8).write(to: directory.appendingPathComponent("ready"), options: .atomic)
  for n in 0...Int.max {
    let input = directory.appendingPathComponent("cmd-\(n).json")
    while !FileManager.default.fileExists(atPath: input.path) { try await Task.sleep(nanoseconds: 30_000_000) }
    let cmd = try JSONSerialization.jsonObject(with: Data(contentsOf: input)) as! [String: Any]
    let dev = cmd["dev"] as? Int ?? 0; let op = cmd["op"] as? String ?? ""
    if devices[dev] == nil { devices[dev] = NativeObserver(directory.appendingPathComponent("device-\(dev)")) }
    let observer = devices[dev]!; var response: [String: Any]
    do {
      var result: Any
      if fleet && (op == "pair" || op == "reinstall") {
        guard let by = devices[cmd["by"] as? Int ?? 0] else { throw TrommiError("not-found", "pairing device") }
        if op == "reinstall" { await observer.core?.close(); try? FileManager.default.removeItem(at: observer.directory) }
        observer.hub = by.hub; try await observer.open()
        await by.account(["step": "invite_device"]); try await by.wait("device invite") { $0.account.link != nil && $0.account.inviteId != nil }
        let invite = await by.state.model(); await observer.account(["step": "join", "link": invite.account.link!])
        try await by.wait("new device Request") { $0.account.step == "compare_emoji" }
        await by.account(["step": "confirm_emoji", "invite_id": invite.account.inviteId!, "matches": true]); try await observer.settle()
        result = ["device": (await observer.state.model()).settings.devices.first(where: \.thisDevice)?.id as Any? ?? null]
      } else if fleet && op == "sync" && cmd["dev"] == nil {
        for device in devices.values where device.core != nil { try await device.settle() }; result = [:] as [String: Any]
      } else { result = try await observer.run(cmd) }
      response = ["ok": true, "result": result]
    } catch { response = ["ok": false, "error": (error as? TrommiError)?.code ?? "failed", "message": String(describing: error)] }
    try JSONSerialization.data(withJSONObject: response, options: [.sortedKeys]).write(to: directory.appendingPathComponent("res-\(n).json"), options: .atomic)
    if op == "quit" { for device in devices.values { await device.core?.close() }; return }
  }
}
final class MegaObserverTests: XCTestCase { func testObserve() async throws { try await commands("TROMMI_MEGA_OBSERVER", fleet: false) } }
final class MegaFleetTests: XCTestCase { func testFleet() async throws { try await commands("TROMMI_MEGA_FLEET", fleet: true) } }
