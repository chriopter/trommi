import XCTest

final class NativeChatRecencyTests: XCTestCase {
  private let app = XCUIApplication()
  override func setUpWithError() throws { continueAfterFailure = false }
  override func tearDownWithError() throws { app.terminate() }
  private var chats: XCUIElementQuery { app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH 'chat-session-'")) }
  private func back() {
    let custom = app.buttons["back"]
    (custom.exists ? custom : app.navigationBars.buttons.element(boundBy: 0)).tap()
  }
  private func capture(_ name: String) {
    let image = XCTAttachment(screenshot: app.screenshot())
    image.name = name; image.lifetime = .keepAlways; add(image)
  }
  private func sendToLastChat(_ text: String) -> String {
    XCTAssertGreaterThanOrEqual(chats.count, 2, app.debugDescription)
    let chat = chats.element(boundBy: chats.count - 1)
    let id = chat.identifier
    chat.tap()
    let view = app.textViews["composer-field"]
    let field = view.exists ? view : app.textFields["composer-field"]
    XCTAssertTrue(field.waitForExistence(timeout: 10), app.debugDescription)
    field.tap(); field.typeText(text)
    let send = app.buttons["composer-send"]
    XCTAssertTrue(send.isEnabled, app.debugDescription)
    send.tap()
    back()
    XCTAssertTrue(chats.firstMatch.waitForExistence(timeout: 10), app.debugDescription)
    XCTAssertEqual(chats.firstMatch.identifier, id, "a genuine sent message must bump this chat above the others")
    return id
  }
  private func moveFirstPlatformChatToRoot() {
    app.buttons["chat-folder-platform"].tap()
    XCTAssertTrue(chats.firstMatch.waitForExistence(timeout: 10), app.debugDescription)
    chats.firstMatch.tap()
    app.buttons["session-title"].tap()
    app.buttons["Move to Team…"].tap()
    app.buttons["No team"].tap()
    back(); back()
    XCTAssertTrue(app.buttons["chat-folder-platform"].waitForExistence(timeout: 10), app.debugDescription)
  }
  func testSentMessageBumpsTeamAndRootWithFreshCore() {
    app.launchArguments = ["-trommi-theme", "dark"]
    app.launchEnvironment = ["TROMMI_SCREEN": "chats"]
    app.launch()
    XCTAssertTrue(app.buttons["chat-folder-main"].waitForExistence(timeout: 20), app.debugDescription)
    app.buttons["chat-folder-main"].tap()
    XCTAssertTrue(chats.firstMatch.waitForExistence(timeout: 10), app.debugDescription)
    _ = sendToLastChat("Local newest-message ordering proof in a Team.")
    capture("A-recency-team")
    back()
    moveFirstPlatformChatToRoot()
    moveFirstPlatformChatToRoot()
    _ = sendToLastChat("Local newest-message ordering proof at Root.")
    XCTAssertTrue(app.buttons["chat-folder-main"].exists)
    XCTAssertTrue(app.buttons["chat-folder-platform"].exists)
    XCTAssertLessThan(chats.firstMatch.frame.minY, app.buttons["chat-folder-main"].frame.minY)
    capture("A-recency-root")
  }
}
