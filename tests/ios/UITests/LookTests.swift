import XCTest
#if canImport(UIKit)
import UIKit
#endif

// The app's look, held against reference pictures (Reference/*.png), so that a change of the design cannot slip in
// unseen: the signed-out start screen, light and dark, on a fresh install. The pictures are of an iPhone 17 Pro
// (iOS 27); on another screen size the comparison is skipped and says so. The status bar (its clock) is left out.
// A design change that is meant updates the reference: run the test, take the attachment "<name>-now" and put it in
// Reference/ under the same name.
final class LookTests: XCTestCase {
  private var app: XCUIApplication!
  override func setUpWithError() throws { continueAfterFailure = false }
  override func tearDownWithError() throws { app?.terminate(); app = nil }

  func testSignInStartLooksAsBefore() throws { try look("signin-start-light", theme: "light") }
  func testSignInStartLooksAsBeforeInDark() throws { try look("signin-start-dark", theme: "dark") }

  private func look(_ name: String, theme: String) throws {
    app = XCUIApplication()
    // (the app's own theme setting, as Settings → Theme keeps it: light or dark whatever the Simulator's is)
    app.launchArguments += ["-trommi-theme", theme]
    // (a hub nobody answers: the screen is the same, and nothing leaves the Simulator)
    app.launchEnvironment["TROMMI_TEST_HUB"] = "http://127.0.0.1:9"
    app.launch()
    XCTAssertTrue(app.buttons["way-login"].waitForExistence(timeout: 20), "A fresh install opens on the start screen. " + app.debugDescription)
    // (the screen settles: fonts, the pen's line)
    Thread.sleep(forTimeInterval: 1.5)
    let shot = app.screenshot()
    let now = XCTAttachment(screenshot: shot); now.name = name + "-now"; now.lifetime = .keepAlways; add(now)
    guard let url = Bundle(for: LookTests.self).url(forResource: name, withExtension: "png"), let data = try? Data(contentsOf: url) else {
      return XCTFail("No reference picture \(name).png in tests/ios/UITests/Reference")
    }
    let diff = try XCTUnwrap(Self.difference(shot.pngRepresentation, data))
    if let skip = diff.skip { throw XCTSkip(skip) }
    XCTAssertLessThan(diff.share, 0.004, "\(name): \(String(format: "%.2f", diff.share * 100)) % of the screen differs from the reference picture")
  }

  /** The share of pixels (below the status bar) whose colour differs by more than a little; or why it cannot be said. */
  static func difference(_ a: Data, _ b: Data) -> (share: Double, skip: String?)? {
    #if canImport(UIKit)
    guard let x = UIImage(data: a)?.cgImage, let y = UIImage(data: b)?.cgImage else { return nil }
    guard x.width == y.width, x.height == y.height else { return (0, "The reference is \(y.width)x\(y.height), this screen \(x.width)x\(x.height): run on an iPhone 17 Pro") }
    let w = x.width, h = x.height
    func pixels(_ i: CGImage) -> [UInt8] {
      var p = [UInt8](repeating: 0, count: w * h * 4)
      let ctx = CGContext(data: &p, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
      ctx.draw(i, in: CGRect(x: 0, y: 0, width: w, height: h))
      return p
    }
    let p = pixels(x), q = pixels(y)
    let top = h * 6 / 100   // the status bar
    var off = 0, all = 0
    for row in top..<h {
      for col in 0..<w {
        let k = (row * w + col) * 4
        let d = max(abs(Int(p[k]) - Int(q[k])), abs(Int(p[k + 1]) - Int(q[k + 1])), abs(Int(p[k + 2]) - Int(q[k + 2])))
        if d > 24 { off += 1 }
        all += 1
      }
    }
    return (Double(off) / Double(max(1, all)), nil)
    #else
    return nil
    #endif
  }
}
