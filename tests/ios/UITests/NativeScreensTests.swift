import XCTest

// Native UI automation: isolated real-account sign-in and explicit storeless demo scenes.
// Private account and full-size timing gates run only when their inputs are supplied.
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
  func testFreshAccountSignInRendersDesk() throws {
    guard let path = ProcessInfo.processInfo.environment["TROMMI_SIGNIN_ACCOUNT_FILE"],
          !path.isEmpty, !path.hasPrefix("$(") else { throw XCTSkip("Set TROMMI_SIGNIN_ACCOUNT_FILE for the isolated real-account gate.") }
    let account = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: URL(fileURLWithPath: path))) as? [String: String])
    app = XCUIApplication()
    app.launchEnvironment["TROMMI_TEST_HUB"] = "http://127.0.0.1:18874"
    app.launch()
    let login = app.buttons["way-login"]
    XCTAssertTrue(login.waitForExistence(timeout: 20), app.debugDescription)
    capture("real-signin-start")
    login.tap()
    let email = app.textFields["account-email"]
    XCTAssertTrue(email.waitForExistence(timeout: 10), app.debugDescription)
    email.tap(); email.typeText(try XCTUnwrap(account["email"]))
    let password = app.secureTextFields["account-password"]
    password.tap(); password.typeText(try XCTUnwrap(account["password"]))
    app.buttons["account-submit"].tap()
    let row = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "desk-card-")).firstMatch
    XCTAssertTrue(row.waitForExistence(timeout: 90), "Real sign-in must render the seeded Desk, not an empty page. " + app.debugDescription)
    XCTAssertFalse(row.label.isEmpty)
    XCTAssertTrue(app.buttons["tab-chat"].exists, "The signed-in iPhone must keep its Chat · Desk · Note navigation.")
    XCTAssertTrue(app.buttons["tab-desk"].exists)
    XCTAssertTrue(app.buttons["tab-note"].exists)
    capture("real-signin-desk")
    row.tap()
    XCTAssertTrue(app.staticTexts["card-title"].waitForExistence(timeout: 15))
    capture("real-signin-card")
    app.buttons["back"].tap()
    XCTAssertTrue(app.buttons["tab-chat"].waitForExistence(timeout: 10))
    app.buttons["tab-chat"].tap()
    let chat = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "chat-session-")).firstMatch
    XCTAssertTrue(chat.waitForExistence(timeout: 10))
    chat.tap()
    XCTAssertTrue(app.staticTexts["talk-message"].firstMatch.waitForExistence(timeout: 15))
    capture("real-signin-chat")
    app.buttons["back"].tap()
    XCTAssertTrue(app.buttons["tab-note"].waitForExistence(timeout: 10))
    app.buttons["tab-note"].tap()
    XCTAssertTrue(app.buttons["Keep in Note"].waitForExistence(timeout: 10))
    capture("real-signin-note")
    app.buttons["tab-desk"].tap()
    XCTAssertTrue(row.waitForExistence(timeout: 10))
    app.terminate(); app.launch()
    XCTAssertTrue(row.waitForExistence(timeout: 30), "The signed-in Desk must survive a restart.")
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
    capture("scribble-before-toggle")
    erase.coordinate(withNormalizedOffset: CGVector(dx: 0.9, dy: 0.5)).tap()
    let changed = NSPredicate(format: "value != %@", previous ?? "")
    expectation(for: changed, evaluatedWith: erase)
    waitForExpectations(timeout: 3)
    XCTAssertTrue(app.steppers["Width"].exists)
    capture("scribble-board")
  }
  func testDevicesShowTheCoresLastSeenText() {
    launch("settings--devices")
    let seen = app.staticTexts.matching(NSPredicate(format: "label CONTAINS %@", "last seen 2026-10-08")).firstMatch
    for _ in 0..<8 {
      if seen.exists { break }
      app.swipeUp()
    }
    XCTAssertTrue(seen.waitForExistence(timeout: 5), "Offline devices show the core's last-contact text. " + app.debugDescription)
    capture("devices-last-seen")
  }
  func testComputerApprovalShowsItsPermissionAndExplicitCommand() {
    launch("settings")
    let approve = app.buttons["Approve a computer"]
    XCTAssertTrue(approve.waitForExistence(timeout: 15))
    approve.tap()
    let permission = app.staticTexts.matching(NSPredicate(format: "label CONTAINS %@", "without another tap")).firstMatch
    XCTAssertTrue(permission.waitForExistence(timeout: 10), "Computer approval must explain the authority it grants. " + app.debugDescription)
    let command = app.staticTexts.matching(NSPredicate(format: "label CONTAINS %@", "computer=true")).firstMatch
    XCTAssertTrue(command.exists, "The copied instructions must explicitly request computer approval.")
    capture("computer-approval-permission")
  }
  func testFullSizeDeskLaunchAndMemory() throws {
    let env = ProcessInfo.processInfo.environment
    guard let fixture = env["TROMMI_SPEED_FIXTURE"], let rows = env["TROMMI_SPEED_ROWS"], !fixture.isEmpty, Int(rows) != nil else {
      throw XCTSkip("Opt-in made-up full-size room; no account or hub")
    }
    let target = XCUIApplication()
    target.launchEnvironment["TROMMI_SCREEN"] = "desk"
    target.launchEnvironment["TROMMI_FIXTURE"] = fixture
    target.launchEnvironment["TROMMI_SPEED_TRACE"] = "1"
    let options = XCTMeasureOptions(); options.iterationCount = 3
    measure(metrics: [XCTApplicationLaunchMetric(waitUntilResponsive: true)], options: options) {
      target.launch()
      let desk = target.scrollViews["desk-content"]
      XCTAssertTrue(desk.waitForExistence(timeout: 15))
      expectation(for: NSPredicate(format: "value BEGINSWITH %@", "{"), evaluatedWith: desk)
      waitForExpectations(timeout: 15)
      guard let text = desk.value as? String, let data = text.data(using: .utf8), let report = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { XCTFail("Missing native startup metrics"); return }
      XCTAssertEqual((report["rows"] as? NSNumber)?.intValue, Int(rows))
      let note = XCTAttachment(string: text); note.name = "native-desk-startup"; note.lifetime = .keepAlways; add(note)
      print("NATIVE_DESK " + text)
      XCTAssertTrue(target.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "desk-card-")).firstMatch.exists)
      target.terminate()
    }
  }

}
