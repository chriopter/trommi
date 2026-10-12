// The iOS app's engine (`Room`, the real core) as one human device of the mega test (tests/client/mega.rs). Runs only
// when TROMMI_MEGA_OBSERVER names a folder: it takes commands from files there and answers each in a file beside it,
// exactly as tests/client/mega/web-observer.mjs does (the commands are listed there).
//
//   TROMMI_MEGA_OBSERVER=<folder> swift test --filter MegaObserverTests
#if !os(iOS)
import Foundation
import XCTest
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif
@testable import TrommiClient
@testable import TrommiCoreLive

/// The hub misbehaving for this device: some requests fail with 502, some wait, the changes route can be down.
final class MegaFaults: URLProtocol, @unchecked Sendable {
  nonisolated(unsafe) static var fail = 0.0
  nonisolated(unsafe) static var delayMs = 0
  nonisolated(unsafe) static var changesDown = false
  nonisolated(unsafe) static var rng: UInt64 = 1
  static let lock = NSLock()
  static func rand() -> Double { lock.withLock { rng = (rng &* 1103515245 &+ 12345) & 0x7fffffff; return Double(rng) / Double(0x7fffffff) } }
  /// The real network, for what passes.
  static let plain: URLSession = {
    let cfg = URLSessionConfiguration.ephemeral
    cfg.httpCookieStorage = nil; cfg.urlCache = nil
    return URLSession(configuration: cfg)
  }()
  override class func canInit(with request: URLRequest) -> Bool { true }
  override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
  private var forward: URLSessionDataTask?
  private func answer(_ status: Int, _ body: String) {
    let r = HTTPURLResponse(url: request.url!, statusCode: status, httpVersion: "HTTP/1.1", headerFields: ["content-type": "application/json", "retry-after": "1"])!
    client?.urlProtocol(self, didReceive: r, cacheStoragePolicy: .notAllowed)
    client?.urlProtocol(self, didLoad: Data(body.utf8))
    client?.urlProtocolDidFinishLoading(self)
  }
  /// Requests by route ("GET /v1/welcomes", …), for the load rules.
  nonisolated(unsafe) static var asked: [String: Int] = [:]
  override func startLoading() {
    let path = request.url?.path ?? ""
    let route = "\(request.httpMethod ?? "GET") " + path.replacingOccurrences(of: #"/[A-Za-z0-9_-]{20,}"#, with: "/:id", options: .regularExpression)
    MegaFaults.lock.withLock { MegaFaults.asked[route, default: 0] += 1 }
    if MegaFaults.changesDown && path.contains("/v1/changes") { return answer(503, #"{"error":"overloaded","message":"mega: changes down"}"#) }
    if MegaFaults.fail > 0 && !path.contains("/stream") && MegaFaults.rand() < MegaFaults.fail { return answer(502, #"{"error":"unavailable","message":"mega: injected"}"#) }
    var req = request
    // (a body given as a stream reaches a URLProtocol without httpBody)
    if req.httpBody == nil, let stream = req.httpBodyStream {
      stream.open(); var data = Data(); var buf = [UInt8](repeating: 0, count: 65536)
      while stream.hasBytesAvailable { let n = stream.read(&buf, maxLength: buf.count); if n <= 0 { break }; data.append(buf, count: n) }
      stream.close(); req.httpBodyStream = nil; req.httpBody = data
    }
    let wait = MegaFaults.delayMs > 0 ? Double(Int(MegaFaults.rand() * Double(MegaFaults.delayMs))) / 1000 : 0
    DispatchQueue.global().asyncAfter(deadline: .now() + wait) { [self] in
      forward = MegaFaults.plain.dataTask(with: req) { [self] data, response, error in
        if let error { client?.urlProtocol(self, didFailWithError: error); return }
        if let response { client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed) }
        if let data { client?.urlProtocol(self, didLoad: data) }
        client?.urlProtocolDidFinishLoading(self)
      }
      forward?.resume()
    }
  }
  override func stopLoading() { forward?.cancel() }
}

@MainActor
final class MegaObserverTests: XCTestCase {
  private var room: Room?
  private var base: URL!

  private func settle() async {
    guard let room else { return }
    for _ in 0..<3 { _ = try? await room.sync() }
  }

  private func snapshot() -> [String: Any] {
    let b = room!.board
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
      if case .obj(let o) = s.profile, let n = o["agent_name"]?.string { name = n }
      sessions[s.sessionId] = ["agent_name": name, "parent": s.parentSessionId as Any? ?? NSNull()]
    }
    return ["desk": b.stack, "permissions_open": b.openPermissionIds, "cards": cards, "permissions": perms, "sessions": sessions,
            "debug": ["alerts": b.alerts.map { "\($0.code): \($0.message)" }, "groups": Array(room!.sessionGroupIds).sorted(),
                      "past": room!.record.past.map { ["toLearn": $0.toLearn, "passedOver": $0.passedOver, "closed": $0.closed, "readBack": $0.readBack] as [String: Any] } as Any? ?? NSNull(),
                      "requests": MegaFaults.lock.withLock { MegaFaults.asked.filter { $0.key.contains("requests") || $0.key.contains("welcomes") } }],
            "snoozed": b.human.snoozes.keys.sorted(), "snooze_values": b.human.snoozes.mapValues { $0.any }]
  }

  private func run(_ cmd: [String: Any]) async throws -> Any {
    let op = cmd["op"] as? String ?? ""
    switch op {
    case "signin":
      let code = try XCTUnwrap(cmd["code"] as? String).hexBytes()
      room = try await Room.joinWithRecoveryCode(hubURL: cmd["hub"] as! String, roomId: cmd["room_id"] as! String, code: code, base: base)
      await settle()
      return ["device": room!.board.myDeviceId]
    case "sync": await settle(); return [:]
    case "restart":
      let id = room!.roomIdHex
      await room!.saveCacheAndWait(snapshot: true)
      await room!.shutdown()
      room = try Room.open(base: base, roomId: id)
      _ = await room!.restore()
      return [:]
    case "faults":
      if let f = cmd["fail"] as? Double { MegaFaults.fail = f }
      if let d = cmd["delay_ms"] as? Int { MegaFaults.delayMs = d }
      if let c = cmd["changes_down"] as? Bool { MegaFaults.changesDown = c }
      if let s = cmd["seed"] as? Int { MegaFaults.lock.withLock { MegaFaults.rng = UInt64(s) } }
      return [:]
    case "answer": try await room!.answer(cardId: cmd["card"] as! String, choices: cmd["choices"] as? [String] ?? []); await settle(); return [:]
    case "take_back": try await room!.decideAgain(cardId: cmd["card"] as! String); await settle(); return [:]
    case "verdict": try await room!.verdict(requestId: cmd["request"] as! String, allow: cmd["allow"] as? Bool ?? false); await settle(); return [:]
    case "snooze":
      let until = (cmd["until"] as? NSNumber).map { $0.uint64Value }
      try await room!.snooze(cardId: cmd["card"] as! String, until: until); await settle(); return [:]
    case "explain": try await room!.sendMessage(cardId: cmd["card"] as? String, text: "What??", fields: ["explain": .bool(true)]); await settle(); return [:]
    case "hand_back": try await room!.sendMessage(cardId: cmd["card"] as? String, text: "back to you", fields: ["hand_back": .bool(true)]); await settle(); return [:]
    case "snapshot": return snapshot()
    case "stats":
      var rss = 0.0
      if let status = try? String(contentsOfFile: "/proc/self/status", encoding: .utf8), let line = status.split(separator: "\n").first(where: { $0.hasPrefix("VmRSS:") }) {
        rss = (Double(line.split(separator: " ").dropFirst().first ?? "0") ?? 0) * 1024
      }
      let requests = MegaFaults.lock.withLock { () -> [String: Int] in let a = MegaFaults.asked; if cmd["reset"] as? Bool == true { MegaFaults.asked = [:] }; return a }
      return ["rss": rss, "desk": room?.board.stack.count ?? 0, "cards": room?.board.cards.count ?? 0, "requests": requests]
    case "quit": await room?.shutdown(); return [:]
    default: throw TrommiError("test", "unknown op \(op)")
    }
  }

  func testObserve() async throws {
    guard let path = ProcessInfo.processInfo.environment["TROMMI_MEGA_OBSERVER"], !path.isEmpty else { throw XCTSkip("TROMMI_MEGA_OBSERVER names no folder") }
    Core.tools = LiveCore()
    HubClient.transportForTests = [MegaFaults.self]
    defer { HubClient.transportForTests = [] }
    let dir = URL(fileURLWithPath: path)
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    base = dir.appendingPathComponent("device")
    try FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
    try "\(ProcessInfo.processInfo.processIdentifier)".write(to: dir.appendingPathComponent("ready"), atomically: true, encoding: .utf8)
    var n = 0
    while true {
      let file = dir.appendingPathComponent("cmd-\(n).json")
      while !FileManager.default.fileExists(atPath: file.path) { try await Task.sleep(nanoseconds: 30_000_000) }
      let cmd = try JSONSerialization.jsonObject(with: Data(contentsOf: file)) as? [String: Any] ?? [:]
      var res: [String: Any]
      do { res = ["ok": true, "result": try await run(cmd)] } catch {
        let code = (error as? TrommiError)?.code ?? (error as? HubError).map { "\($0)" } ?? ""
        res = ["ok": false, "error": code, "message": "\(error)"]
      }
      let tmp = dir.appendingPathComponent("res-\(n).json.tmp")
      try JSONSerialization.data(withJSONObject: res, options: [.sortedKeys]).write(to: tmp)
      try FileManager.default.moveItem(at: tmp, to: dir.appendingPathComponent("res-\(n).json"))
      if cmd["op"] as? String == "quit" { return }
      n += 1
    }
  }
}

private extension String {
  func hexBytes() throws -> Bytes {
    var out = Bytes(); var i = startIndex
    while i < endIndex { let j = index(i, offsetBy: 2); guard let b = UInt8(self[i..<j], radix: 16) else { throw TrommiError("test", "bad hex") }; out.append(b); i = j }
    return out
  }
}
#endif
