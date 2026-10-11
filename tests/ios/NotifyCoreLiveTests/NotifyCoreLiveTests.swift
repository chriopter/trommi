// NotifyCoreLive on the real Rust library, on Linux: what the Notification Service Extension links.
// The manifest's line for this target:
//   .testTarget(name: "NotifyCoreLiveTests", dependencies: ["NotifyCoreLive"]),
import Foundation
import XCTest
import PushNotify
import NotifyCoreLive

final class NotifyCoreLiveTests: XCTestCase {
  let core = NotifyCoreLive()

  func testTheLibraryIsLinked() {
    XCTAssertFalse(core.version.isEmpty)
  }

  /// What is not a notification sealed under this phone's key opens nothing, and says why with the specification's code.
  func testOpenPushRefusesWhatDoesNotOpen() {
    let key = [UInt8](repeating: 7, count: 32)
    let garbage = (0..<80).map { UInt8(truncatingIfNeeded: $0 &* 37) }
    XCTAssertThrowsError(try core.openPush(key: key, sealed: garbage)) { XCTAssertEqual(($0 as? NotifyCoreError)?.code, "decrypt-failed") }
    XCTAssertThrowsError(try core.openPush(key: key, sealed: [])) { XCTAssertNotNil($0 as? NotifyCoreError) }
    XCTAssertThrowsError(try core.openPush(key: [1, 2, 3], sealed: garbage)) { XCTAssertEqual(($0 as? NotifyCoreError)?.code, "bad-format") }
  }

  /// A sealed push that opens: not testable here yet. The binding seals no push (the hub does), and spec/vectors
  /// carries none with its key.
  func testOpenPushOpensASealedPush() throws {
    throw XCTSkip("no sealed APNs push to open: the binding seals none and spec/vectors carries none")
  }

  /// A Live Activity block that does not open, or has another length, gives no counts.
  func testOpenLiveRefusesWhatDoesNotOpen() {
    let key = [UInt8](repeating: 7, count: 32)
    let garbage = (0..<44).map { UInt8(truncatingIfNeeded: $0 &* 37) }
    XCTAssertThrowsError(try core.openLive(key: key, sealed: garbage)) { XCTAssertEqual(($0 as? NotifyCoreError)?.code, "decrypt-failed") }
    XCTAssertThrowsError(try core.openLive(key: key, sealed: Array(garbage.dropLast()))) { XCTAssertEqual(($0 as? NotifyCoreError)?.code, "bad-format") }
    // the widget, which shows the marks then
    var hub = LiveCounts(working: 1, waiting: 1)
    hub.s = Data(garbage).base64EncodedString().replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
    XCTAssertEqual(hub.opened(key: key, core: core), hub)
  }
}
