// HubRetryTests: a GET that meets 502 to 504 (or no answer) is tried again after a short wait, as the web client's hub
// does (`get_retry`): it changes nothing. A write is sent once. Found by the mega test of the clients
// (tests/client/mega.rs): one hiccup of the hub failed an iPhone's whole sign-in with the code, while the web went on.
import XCTest
@testable import TrommiClient

final class HubRetryTests: XCTestCase {
  override func setUp() async throws {
    HubClient.transportForTests = [FakeHub.self]
    FakeHub.shared = FakeHub.Hub()
    HubClient.getRetryMs = [10, 10]
  }
  override func tearDown() async throws { HubClient.getRetryMs = [300, 1000] }

  func testAGetIsAskedAgainAfterAHiccup() async throws {
    let hub = try HubClient(hubURL: "http://127.0.0.1:9")
    FakeHub.shared.lock.withLock { FakeHub.shared.failTimes["GET /v1/welcomes"] = 2 }
    let list = try await hub.request("GET", "/welcomes", auth: false)
    XCTAssertNotNil(list, "answered on the third try")
    FakeHub.shared.lock.withLock { FakeHub.shared.failTimes["GET /v1/welcomes"] = 3 }
    do { _ = try await hub.request("GET", "/welcomes", auth: false); XCTFail("three hiccups in a row are said") } catch let e as HubError { XCTAssertEqual(e.status, 502) }
  }

  func testAWriteIsSentOnce() async throws {
    let hub = try HubClient(hubURL: "http://127.0.0.1:9")
    FakeHub.shared.lock.withLock { FakeHub.shared.failTimes["POST /v1/requests"] = 1 }
    do { _ = try await hub.request("POST", "/requests", body: ["kind": "handover"], auth: false); XCTFail("refused") } catch let e as HubError { XCTAssertEqual(e.status, 502) }
    XCTAssertEqual(FakeHub.shared.lock.withLock { FakeHub.shared.failTimes["POST /v1/requests"] }, 0)
  }
}
