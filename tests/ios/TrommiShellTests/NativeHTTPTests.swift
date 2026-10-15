import XCTest
import Foundation
@testable import TrommiClient

final class NativeHTTPTests: XCTestCase {
  func testResponseLimitsAndRedirectsAreEnforcedByTheTransport() async throws {
    let process = Process(), pipe = Pipe()
    process.executableURL = URL(fileURLWithPath: "/usr/bin/python3")
    process.arguments = ["-u", "-c", #"""
import signal
signal.pthread_sigmask(signal.SIG_SETMASK, [])
signal.signal(signal.SIGTERM, signal.SIG_DFL)
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
class Handler(BaseHTTPRequestHandler):
 def log_message(self, *args): pass
 def do_GET(self):
  self.send_response(302 if self.path == '/redirect' else 200)
  if self.path == '/declared': self.send_header('Content-Length', '1000')
  if self.path == '/redirect': self.send_header('Location', '/unexpected')
  self.end_headers()
  try: self.wfile.write(b'123456789' if self.path in ('/declared','/body') else b'ok')
  except (BrokenPipeError, ConnectionResetError): pass
server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
print(server.server_port, flush=True)
server.serve_forever()
"""#]
    process.standardOutput = pipe; process.standardError = FileHandle.nullDevice
    try process.run(); defer { process.terminate(); process.waitUntilExit() }
    guard let value = String(data: pipe.fileHandleForReading.availableData, encoding: .utf8), let port = Int(value.trimmingCharacters(in: .whitespacesAndNewlines)) else { return XCTFail("the local test peer did not start") }
    for path in ["declared", "body", "redirect", "okay"] {
      let transfer = NativeHTTP(limit: 8, stream: false)
      let parts = try transfer.start(NativePacket(["url": "http://127.0.0.1:\(port)/\(path)", "method": "GET", "timeout_ms": 2000]))
      var status = 0, bytes = Data(), failure: String?, ended = false
      for await part in parts {
        switch part {
        case .opened(let code, _): status = code
        case .chunk(let data): bytes.append(data); transfer.resume()
        case .ended: ended = true
        case .failed(let why): failure = why
        }
      }
      if path == "declared" || path == "body" { XCTAssertEqual(failure, "too_large", path); XCTAssertLessThanOrEqual(bytes.count, 8, path) }
      else { XCTAssertNil(failure); XCTAssertTrue(ended); XCTAssertEqual(status, path == "redirect" ? 302 : 200); XCTAssertEqual(bytes, Data("ok".utf8)) }
    }
  }
}
