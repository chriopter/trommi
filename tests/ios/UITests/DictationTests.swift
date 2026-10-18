import XCTest

// Sealed dictation in the chat field, in the storeless demo (the core records nothing there and brings its own words):
// while recording, and with a long line of words, the field stays inside the screen and its stop and send buttons
// stay where a thumb reaches them. (A bug of 17 October 2026: the level bars widened the row past the screen.)
final class DictationTests: XCTestCase {
  private var app: XCUIApplication!
  override func setUpWithError() throws { continueAfterFailure = false }
  override func tearDownWithError() throws { app?.terminate(); app = nil }

  private func inside(_ element: XCUIElement, _ what: String) {
    let screen = app.windows.firstMatch.frame
    XCTAssertTrue(element.exists, "\(what) is there. " + app.debugDescription)
    XCTAssertTrue(screen.contains(element.frame), "\(what) stays on the screen: \(element.frame) in \(screen)")
  }

  func testTheFieldStaysOnTheScreenWhileRecordingAndWithLongWords() throws {
    app = XCUIApplication()
    app.launchEnvironment["TROMMI_SCREEN"] = "chat"
    app.launch()
    XCTAssertTrue(app.wait(for: .runningForeground, timeout: 15))
    let mic = app.buttons["composer-dictate"]
    XCTAssertTrue(mic.waitForExistence(timeout: 20), "an empty field offers the microphone. " + app.debugDescription)
    inside(mic, "the microphone")
    mic.tap()
    let stop = app.buttons["composer-dictate-stop"]
    XCTAssertTrue(stop.waitForExistence(timeout: 10))
    // (the bars fill up over some seconds: long enough for more than fit in the row)
    sleep(10)
    inside(stop, "the stop button while recording")
    let shot = XCTAttachment(screenshot: app.screenshot()); shot.name = "recording"; shot.lifetime = .keepAlways; add(shot)
    stop.tap()
    let send = app.buttons["composer-send"]
    XCTAssertTrue(send.waitForExistence(timeout: 10), "the words came: the field sends. " + app.debugDescription)
    inside(send, "the send button with the dictated words")
    let field = app.textViews["composer-field"].exists ? app.textViews["composer-field"] : app.textFields["composer-field"]
    field.tap()
    field.typeText(String(repeating: "Ein sehr langer diktierter Satz ohne Ende ", count: 12))
    inside(send, "the send button with a long line of words")
    inside(field, "the field with a long line of words")
    let long = XCTAttachment(screenshot: app.screenshot()); long.name = "long-words"; long.lifetime = .keepAlways; add(long)
  }

  /** Security: the facts of the sealed connection and its test (the demo's test passes at once, as a real one shows). */
  func testSecurityShowsTheEnclaveAndItsTest() throws {
    app = XCUIApplication()
    app.launchEnvironment["TROMMI_SCREEN"] = "settings--security"
    app.launch()
    XCTAssertTrue(app.wait(for: .runningForeground, timeout: 15))
    let test = app.buttons["dictation-test"]
    XCTAssertTrue(test.waitForExistence(timeout: 20), "the test of the sealed connection is there. " + app.debugDescription)
    XCTAssertTrue(app.staticTexts["Voxtral Small 24B"].exists)
    XCTAssertTrue(app.staticTexts["relays ciphertext only"].exists)
    test.tap()
    let passed = app.staticTexts.containing(NSPredicate(format: "label BEGINSWITH %@", "✓ sealed round trip")).firstMatch
    XCTAssertTrue(passed.waitForExistence(timeout: 10) || test.label.contains("✓ sealed round trip"), "the test's result is shown. " + app.debugDescription)
    let shot = XCTAttachment(screenshot: app.screenshot()); shot.name = "security-dictation"; shot.lifetime = .keepAlways; add(shot)
  }
}
