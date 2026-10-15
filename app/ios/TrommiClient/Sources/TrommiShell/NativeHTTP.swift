import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif

// A transport only: no authentication, cookies, redirect, JSON interpretation or retry. Stream bytes stay in
// delegate order; a regular response is bounded before it enters the buffer.
final class NativeHTTP: NSObject, URLSessionDataDelegate, @unchecked Sendable {
  enum Part { case opened(Int, [[String: String]]), chunk(Data), ended, failed(String) }
  private let callbacks: OperationQueue = { let q = OperationQueue(); q.maxConcurrentOperationCount = 1; return q }()
  private var continuation: AsyncStream<Part>.Continuation?
  private var session: URLSession?
  private var task: URLSessionDataTask?
  private var received = 0
  private var status = 0
  private let limit: Int
  private let stream: Bool
  private var finished = false
  private var deadline: DispatchWorkItem?
  private let delivery = NSCondition()
  private var waiting = false
  private var cancelled = false
  private var timedOut = false
  private var timeoutActive = false
  init(limit: Int, stream: Bool) { self.limit = limit; self.stream = stream }
  func start(_ packet: NativePacket) throws -> AsyncStream<Part> {
    let req = packet.json
    guard let text = req["url"] as? String, let url = URL(string: text), ["https", "http"].contains(url.scheme) else { throw TrommiError("bad-format") }
    var request = URLRequest(url: url)
    request.httpMethod = req["method"] as? String ?? "GET"
    // URLSession's timeout is an independent idle timer (and on Linux may be rounded to whole seconds), not
    // the core's deadline. Keep its transport timers beyond any practical stream lifetime and enforce the
    // precise core deadline ourselves; it is cleared as soon as a successful stream's headers arrive.
    request.timeoutInterval = TimeInterval(Int32.max)
    let seconds = Double(req["timeout_ms"] as? Int ?? 30000) / 1000
    for h in req["headers"] as? [[String: String]] ?? [] { if let n = h["name"], let v = h["value"] { request.addValue(v, forHTTPHeaderField: n) } }
    if let body = req["body"], !(body is NSNull) { request.httpBody = try packet.bytes(body) }
    let config = URLSessionConfiguration.ephemeral
    config.httpCookieStorage = nil; config.httpShouldSetCookies = false; config.urlCache = nil; config.urlCredentialStorage = nil
    config.timeoutIntervalForRequest = TimeInterval(Int32.max); config.timeoutIntervalForResource = TimeInterval(Int32.max)
    config.requestCachePolicy = .reloadIgnoringLocalCacheData
    let parts = AsyncStream<Part>(bufferingPolicy: .bufferingOldest(8)) {
      continuation = $0
      $0.onTermination = { [weak self] why in if case .cancelled = why { self?.cancel() } }
    }
    session = URLSession(configuration: config, delegate: self, delegateQueue: callbacks)
    task = session!.dataTask(with: request)
    delivery.lock(); timeoutActive = true; delivery.unlock()
    let expiry = DispatchWorkItem { [weak self] in
      guard let self else { return }
      self.delivery.lock()
      guard self.timeoutActive else { self.delivery.unlock(); return }
      self.timeoutActive = false; self.timedOut = true; self.waiting = false
      self.delivery.broadcast(); self.delivery.unlock()
      self.callbacks.addOperation { [weak self] in
        guard let self, !self.finished else { return }
        let task = self.task; self.finish(.failed("timeout")); task?.cancel()
      }
    }
    deadline = expiry
    DispatchQueue.global(qos: .utility).asyncAfter(deadline: .now() + max(0, seconds), execute: expiry)
    task!.resume()
    return parts
  }
  func cancel() {
    delivery.lock(); cancelled = true; waiting = false; delivery.broadcast(); delivery.unlock()
    callbacks.addOperation { [weak self] in self?.task?.cancel() }
  }
  // Acknowledge a delivered chunk; never suspend/resume URLSession's task. In FoundationNetworking that can
  // restart the HTTP transfer, replaying its first bytes. Blocking the serial delegate until consumption instead
  // provides bounded backpressure without replay and can always be interrupted by cancellation or the deadline.
  func resume() { delivery.lock(); waiting = false; delivery.broadcast(); delivery.unlock() }
  private func finish(_ part: Part) {
    guard !finished else { return }; finished = true
    deadline?.cancel(); deadline = nil
    delivery.lock(); timeoutActive = false; waiting = false; delivery.broadcast(); delivery.unlock()
    continuation?.yield(part); continuation?.finish(); continuation = nil
    session?.finishTasksAndInvalidate(); session = nil; task = nil
  }
  func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void) { completionHandler(nil) }
  func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive response: URLResponse, completionHandler: @escaping (URLSession.ResponseDisposition) -> Void) {
    guard !finished else { completionHandler(.cancel); return }
    guard let response = response as? HTTPURLResponse else { finish(.failed("failed")); completionHandler(.cancel); return }
    status = response.statusCode
    delivery.lock(); let expired = timedOut
    if stream && status == 200 { timeoutActive = false }
    delivery.unlock()
    if expired { finish(.failed("timeout")); completionHandler(.cancel); return }
    if stream && status == 200 { deadline?.cancel(); deadline = nil }
    if (!stream || status != 200), response.expectedContentLength > Int64(limit) { finish(.failed("too_large")); completionHandler(.cancel); return }
    let headers = response.allHeaderFields.map { ["name": String(describing: $0.key), "value": String(describing: $0.value)] }
    continuation?.yield(.opened(status, headers)); completionHandler(.allow)
  }
  func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive data: Data) {
    guard !finished else { return }
    if !stream || status != 200 {
      guard data.count <= limit - received else { finish(.failed("too_large")); dataTask.cancel(); return }
      received += data.count
    }
    delivery.lock()
    guard !cancelled && !timedOut else { delivery.unlock(); return }
    waiting = true; delivery.unlock()
    let result = continuation?.yield(.chunk(data))
    delivery.lock()
    if let result, case .enqueued = result {
      while waiting && !cancelled && !timedOut { delivery.wait() }
      delivery.unlock()
    } else {
      waiting = false; delivery.unlock(); finish(.failed("too_large")); dataTask.cancel()
    }
  }
  func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
    delivery.lock(); let expired = timedOut, stopped = cancelled; delivery.unlock()
    if expired { finish(.failed("timeout")); return }
    if stopped { finish(.failed("cancelled")); return }
    if let error = error as? URLError {
      finish(.failed(error.code == .timedOut ? "timeout" : error.code == .cancelled ? "cancelled" : "offline"))
    } else if error != nil { finish(.failed("failed")) } else { finish(.ended) }
  }
}
