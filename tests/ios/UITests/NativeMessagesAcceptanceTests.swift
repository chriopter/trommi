import XCTest

final class NativeMessagesAcceptanceTests: XCTestCase {
  private let app = XCUIApplication()
  override func setUpWithError() throws { continueAfterFailure = false }
  private func launch() {
    app.launchArguments = ["-trommi-theme", "dark"]
    app.launchEnvironment = ["TROMMI_SCREEN": "chats"]
    app.launch()
    XCTAssertTrue(app.buttons["chat-folder-main"].waitForExistence(timeout: 20), app.debugDescription)
  }
  private func controls() {
    let search = app.searchFields.firstMatch
    let plus = app.buttons["side-plus"]
    let bolt = app.buttons["chats-blitz-all"]
    XCTAssertTrue(search.waitForExistence(timeout: 5), app.debugDescription)
    XCTAssertTrue(plus.exists, app.debugDescription)
    XCTAssertTrue(bolt.exists, app.debugDescription)
    XCTAssertTrue(plus.isHittable && bolt.isHittable)
    XCTAssertGreaterThan(search.frame.midY, app.frame.height * 0.75)
    XCTAssertLessThan(abs(search.frame.midY - plus.frame.midY), 45)
    XCTAssertLessThan(abs(search.frame.midY - bolt.frame.midY), 45)
    XCTAssertGreaterThan(plus.frame.midX, search.frame.midX)
    XCTAssertGreaterThan(plus.frame.midX, bolt.frame.midX)
    XCTAssertEqual(app.tabBars.count, 0)
  }
  /// Opt-in isolated core-backed fixtures; no production account, Hub or LLM.
  func testHermesAutomaticWorkAndIdleWithManualTask() throws {
    guard let prefix = ProcessInfo.processInfo.environment["TROMMI_SPEED_FIXTURE"], !prefix.isEmpty else {
      throw XCTSkip("Set TROMMI_SPEED_FIXTURE to the isolated activity fixture prefix")
    }
    for state in ["work", "idle-manual", "final"] {
      app.launchArguments = ["-trommi-theme", "dark"]
      app.launchEnvironment = ["TROMMI_SCREEN": "chat", "TROMMI_FIXTURE": prefix + "-" + state + ".json"]
      app.launch()
      XCTAssertTrue(app.descendants(matching: .any)["composer-field"].waitForExistence(timeout: 20), app.debugDescription)
      let face = app.buttons["working-face"]
      if state == "work" {
        XCTAssertTrue(face.waitForExistence(timeout: 5), "Automatic work needs no manual status. " + app.debugDescription)
      } else {
        XCTAssertFalse(face.exists, "A manual task must not recreate Hermes typing. " + app.debugDescription)
      }
      if state == "final" {
        XCTAssertTrue(app.staticTexts["The isolated answer is finished."].waitForExistence(timeout: 5), app.debugDescription)
      }
      let shot = XCTAttachment(screenshot: app.screenshot())
      shot.name = "hermes-" + state; shot.lifetime = .keepAlways; add(shot)
      app.terminate()
    }
  }
  func testBottomControlsAtRootAndInsideTeam() {
    launch(); controls()
    app.buttons["chat-folder-main"].tap()
    controls()
    XCTAssertFalse(app.buttons["chat-folder-platform"].exists)
  }
}
