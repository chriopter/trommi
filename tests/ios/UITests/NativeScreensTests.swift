import XCTest

// Real UI automation of the same storeless room used by web and native screenshots.
// Every launch explicitly selects demo mode: no device store, credentials or hub.
final class NativeScreensTests: XCTestCase {
  private var app: XCUIApplication!
  override func setUpWithError() throws { continueAfterFailure = false }
  override func tearDownWithError() throws { app?.terminate(); app = nil }
  private func launch(_ screen: String) {
    app = XCUIApplication()
    app.launchEnvironment["TROMMI_SCREEN"] = screen
    app.launch()
    XCTAssertTrue(app.wait(for: .runningForeground, timeout: 15))
  }
  private func capture(_ name: String) {
    let shot = XCTAttachment(screenshot: app.screenshot())
    shot.name = name; shot.lifetime = .keepAlways; add(shot)
  }
  func testDeskOpensARealCard() {
    launch("desk")
    let row = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "desk-card-")).firstMatch
    XCTAssertTrue(row.waitForExistence(timeout: 15))
    XCTAssertFalse(row.label.isEmpty)
    capture("desk")
    row.tap()
    let title = app.staticTexts["card-title"]
    XCTAssertTrue(title.waitForExistence(timeout: 10))
    XCTAssertFalse(title.label.isEmpty)
  }
  func testCardContainsTextAndLoadedPictures() {
    launch("card--two-pictures")
    let title = app.staticTexts["card-title"]
    XCTAssertTrue(title.waitForExistence(timeout: 15))
    XCTAssertFalse(title.label.isEmpty)
    let picture = app.images["native-picture"].firstMatch
    XCTAssertTrue(picture.waitForExistence(timeout: 10))
    capture("card--two-pictures")
  }
  func testChatContainsAnActualMessage() {
    launch("chat")
    let message = app.staticTexts["talk-message"].firstMatch
    XCTAssertTrue(message.waitForExistence(timeout: 15))
    XCTAssertFalse(message.label.isEmpty)
    capture("chat")
  }
  func testScribbleBoardControlsRespond() {
    launch("scribble-board")
    let erase = app.switches["Erase"]
    XCTAssertTrue(erase.waitForExistence(timeout: 15))
    let previous = erase.value as? String
    erase.tap()
    let changed = NSPredicate(format: "value != %@", previous ?? "")
    expectation(for: changed, evaluatedWith: erase)
    waitForExpectations(timeout: 3)
    XCTAssertTrue(app.steppers["Width"].exists)
    capture("scribble-board")
  }
}
