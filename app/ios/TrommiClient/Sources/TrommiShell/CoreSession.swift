// The native AppCore executor. Every core call is serialized by this actor. DeviceApply is a barrier across
// answers: incoming events may be handled, but no effect behind a write starts before its durable result.
import Foundation

public struct NativeNetworkHooks: Sendable {
  public var cached: @Sendable (NativePacket) async -> NativePacket?
  public var before: @Sendable (String, String) async -> String?
  public var after: @Sendable (String, String, Int, Int) async -> Void
  public init(cached: @escaping @Sendable (NativePacket) async -> NativePacket? = { _ in nil }, before: @escaping @Sendable (String, String) async -> String? = { _, _ in nil }, after: @escaping @Sendable (String, String, Int, Int) async -> Void = { _, _, _, _ in }) { self.cached = cached; self.before = before; self.after = after }
}

public actor CoreSession {
  public typealias Output = @Sendable (NativePacket) async -> Void
  private let core = AppCore()
  private let stores: ShellStore?
  private let output: Output
  private let networkHooks: NativeNetworkHooks
  private var queue: [NativePacket] = []
  private var draining = false
  private var closed = false
  private var tasks: [Int: Task<Void, Never>] = [:]
  private var transfers: [Int: NativeHTTP] = [:]
  private var timer: Task<Void, Never>?
  public init(directory: URL?, network: NativeNetworkHooks = NativeNetworkHooks(), output: @escaping Output) throws { stores = try directory.map { try ShellStore(directory: $0) }; self.output = output; networkHooks = network }
  public func send(_ packet: NativePacket) async {
    guard !closed else { return }
    do {
      let json = try Spent.time("app.encode") { String(decoding: try JSONSerialization.data(withJSONObject: packet.json), as: UTF8.self) }
      let answer = try Spent.time("app.handle") { try core.handle(nowMs: UInt64(Date().timeIntervalSince1970 * 1000), event: AppPacket(json: json, blobs: packet.blobs)) }
      let decoded = try Spent.time("app.effects.decode") { try JSONSerialization.jsonObject(with: Data(answer.json.utf8)) }
      guard let effects = decoded as? [[String: Any]] else { throw TrommiError("bad-format") }
      queue.append(contentsOf: effects.map { NativePacket($0, blobs: answer.blobs) })
      await drain()
    } catch {
      // Do not include a binding error's JSON or values: inputs can hold credentials.
      await output(NativePacket(["type": "log", "level": "error", "text": "The app core could not handle an event."]))
    }
  }
  public func viewJSON() throws -> String { try core.viewJson() }
  public func close() { closed = true; queue = []; stores?.close(); timer?.cancel(); for task in tasks.values { task.cancel() }; for transfer in transfers.values { transfer.cancel() }; core.close() }
  private func reply(_ type: String, id: Int, result: NativePacket) async {
    await send(NativePacket(["type": type, "id": id, "result": result.json], blobs: result.blobs))
  }
  private func failure(_ error: Error) -> NativePacket {
    let kind = (error as? StoreError) == .conflict ? "conflict" : "failed"
    return NativePacket(["err": ["kind": kind, "detail": "Native I/O failed."]])
  }
  private func drain() async {
    guard !draining else { return }; draining = true
    defer { draining = false }
    while !queue.isEmpty {
      let packet = queue.removeFirst(); let e = packet.json; let id = e["id"] as? Int ?? 0
      switch e["type"] as? String {
      case "store":
        let op = e["op"] as? [String: Any] ?? [:]
        do {
          guard let stores else { throw TrommiError("storage") }
          let answer = try Spent.time("store." + (op["op"] as? String ?? "unknown")) { try stores.run(NativePacket(op, blobs: packet.blobs)) }
          await reply("store", id: id, result: NativePacket(["ok": answer.json], blobs: answer.blobs))
        } catch {
          if op["op"] as? String == "device_apply" { queue.removeAll() }
          await reply("store", id: id, result: failure(error))
        }
      case "secret":
        do { guard let stores else { throw TrommiError("storage") }; await reply("secret", id: id, result: try stores.secret(NativePacket(e["op"] as? [String: Any] ?? [:], blobs: packet.blobs))) }
        catch { await reply("secret", id: id, result: failure(error)) }
      case "http", "open_stream":
        let stream = e["type"] as? String == "open_stream"
        let request = e["request"] as? [String: Any] ?? [:]
        tasks[id] = Task { await self.beginNetwork(id, request: NativePacket(request, blobs: packet.blobs), stream: stream) }
      case "cancel": transfers.removeValue(forKey: id)?.cancel(); tasks.removeValue(forKey: id)?.cancel()
      case "timer":
        timer?.cancel(); timer = nil
        if let at = e["at_ms"] as? Double {
          let delay = max(0, at - Date().timeIntervalSince1970 * 1000)
          timer = Task { try? await Task.sleep(nanoseconds: UInt64(delay * 1_000_000)); if !Task.isCancelled { await self.send(NativePacket(["type": "wake"])) } }
        }
      default: await output(packet)
      }
    }
  }
  private func beginNetwork(_ id: Int, request: NativePacket, stream: Bool) async {
    let method = request.json["method"] as? String ?? "GET", url = request.json["url"] as? String ?? ""
    let refused = await networkHooks.before(method, url)
    if Task.isCancelled { await networkHooks.after(method, url, 0, 0); return }
    if let kind = refused {
      await transportFailed(id, stream: stream, kind: kind)
      await networkHooks.after(method, url, 0, 0); tasks[id] = nil; return
    }
    if !stream, let cached = await networkHooks.cached(request) {
      let size = cached.blobs.reduce(0) { $0 + $1.count }
      if size > (request.json["max_answer"] as? Int ?? 65536) { await transportFailed(id, stream: false, kind: "too_large") }
      else { await reply("http", id: id, result: NativePacket(["ok": cached.json], blobs: cached.blobs)) }
      await networkHooks.after(method, url, cached.json["status"] as? Int ?? 0, size); tasks[id] = nil; return
    }
    let transfer = NativeHTTP(limit: request.json["max_answer"] as? Int ?? 65536, stream: stream)
    do {
      let parts = try transfer.start(request); transfers[id] = transfer
      await network(id, parts: parts, stream: stream, method: method, url: url)
    } catch { await transportFailed(id, stream: stream, kind: "failed"); await networkHooks.after(method, url, 0, 0) }
  }
  private func transportFailed(_ id: Int, stream: Bool, kind: String) async {
    let error = ["kind": kind, "detail": ""]
    if stream { await send(NativePacket(["type": "stream", "id": id, "item": ["item": "failed", "error": error]])) }
    else { await reply("http", id: id, result: NativePacket(["err": error])) }
  }
  private func network(_ id: Int, parts: AsyncStream<NativeHTTP.Part>, stream: Bool, method: String, url: String) async {
    var status = 0; var headers: [[String: String]] = []; var body = Data(); var received = 0; var terminal = false
    for await part in parts {
      if Task.isCancelled { break }
      switch part {
      case .opened(let code, let h):
        status = code; headers = h
        if stream { await send(NativePacket(["type": "stream", "id": id, "item": ["item": "opened", "status": code, "headers": h]])) }
      case .chunk(let data):
        received += data.count
        if stream { await send(NativePacket(["type": "stream", "id": id, "item": ["item": "chunk", "bytes": 0]], blobs: [data])) } else { body.append(data) }
        transfers[id]?.resume()
      case .ended:
        terminal = true
        if stream { await send(NativePacket(["type": "stream", "id": id, "item": ["item": "ended"]])) }
        else { await reply("http", id: id, result: NativePacket(["ok": ["status": status, "headers": headers, "body": 0]], blobs: [body])) }
      case .failed(let kind):
        terminal = true
        let error = ["kind": kind, "detail": ""]
        if stream { await send(NativePacket(["type": "stream", "id": id, "item": ["item": "failed", "error": error]])) }
        else { await reply("http", id: id, result: NativePacket(["err": error])) }
      }
    }
    if !terminal && !Task.isCancelled { await transportFailed(id, stream: stream, kind: "failed") }
    await networkHooks.after(method, url, status, received)
    transfers[id] = nil; tasks[id] = nil
  }
}
