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
  init(limit: Int, stream: Bool) { self.limit = limit; self.stream = stream }
  func start(_ packet: NativePacket) throws -> AsyncStream<Part> {
    let req = packet.json
    guard let text = req["url"] as? String, let url = URL(string: text), ["https", "http"].contains(url.scheme) else { throw TrommiError("bad-format") }
    var request = URLRequest(url: url)
    request.httpMethod = req["method"] as? String ?? "GET"
    request.timeoutInterval = Double(req["timeout_ms"] as? Int ?? 30000) / 1000
    for h in req["headers"] as? [[String: String]] ?? [] { if let n = h["name"], let v = h["value"] { request.addValue(v, forHTTPHeaderField: n) } }
    if let body = req["body"], !(body is NSNull) { request.httpBody = try packet.bytes(body) }
    let config = URLSessionConfiguration.ephemeral
    config.httpCookieStorage = nil; config.httpShouldSetCookies = false; config.urlCache = nil
    config.requestCachePolicy = .reloadIgnoringLocalCacheData
    let parts = AsyncStream<Part>(bufferingPolicy: .bufferingOldest(8)) { continuation = $0 }
    session = URLSession(configuration: config, delegate: self, delegateQueue: callbacks)
    task = session!.dataTask(with: request); task!.resume()
    return parts
  }
  func cancel() { callbacks.addOperation { [weak self] in self?.task?.cancel() } }
  func resume() {
    callbacks.addOperation { [weak self] in
      guard let self, !self.finished, let task = self.task, task.state == .suspended else { return }
      task.resume()
    }
  }
  private func finish(_ part: Part) {
    guard !finished else { return }; finished = true
    continuation?.yield(part); continuation?.finish(); continuation = nil
    session?.finishTasksAndInvalidate(); session = nil; task = nil
  }
  func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void) { completionHandler(nil) }
  func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive response: URLResponse, completionHandler: @escaping (URLSession.ResponseDisposition) -> Void) {
    guard let response = response as? HTTPURLResponse else { finish(.failed("failed")); completionHandler(.cancel); return }
    status = response.statusCode
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
    dataTask.suspend()
    if let result = continuation?.yield(.chunk(data)), case .dropped = result { finish(.failed("too_large")); dataTask.cancel() }
  }
  func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
    if let error = error as? URLError {
      finish(.failed(error.code == .timedOut ? "timeout" : error.code == .cancelled ? "cancelled" : "offline"))
    } else if error != nil { finish(.failed("failed")) } else { finish(.ended) }
  }
}
