import XCTest

// The iPhone's Chats as folders: the root lists the teams only, one row each; a team's row pushes a new screen with
// that team's chats; a chat opens its page as today; the system's back goes the same way home. Bundled demo
// (demo/data/fixture.json): team `main` is "Fernly", team `platform` is "Platform". No account, no network.
final class IOSProposalTests: XCTestCase {
  private var app: XCUIApplication!
  override func setUpWithError() throws { continueAfterFailure = false }
  override func tearDownWithError() throws { app?.terminate() }
  private func launch(_ screen: String) {
    app = XCUIApplication()
    app.launchEnvironment["TROMMI_SCREEN"] = screen
    app.launchEnvironment["TROMMI_THEME"] = "light"
    app.launch()
  }
  private func capture(_ name: String) {
    let image = XCTAttachment(screenshot: app.screenshot())
    image.name = name; image.lifetime = .keepAlways; add(image)
  }
  private func any(_ prefix: String) -> XCUIElementQuery {
    app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", prefix))
  }
  private func back() {
    let custom = app.buttons["back"]
    let button = custom.exists ? custom : app.navigationBars.buttons.element(boundBy: 0)
    XCTAssertTrue(button.waitForExistence(timeout: 5), app.debugDescription)
    button.tap()
  }

  func testTeamFolderPushesItsChatsAndBackComesHome() {
    launch("chats")
    // root: the teams as folders, no chat rows and no tab bar
    let fernly = app.descendants(matching: .any)["chat-folder-main"]
    XCTAssertTrue(fernly.waitForExistence(timeout: 20), "root: expected the Fernly folder row\n" + app.debugDescription)
    XCTAssertTrue(app.descendants(matching: .any)["chat-folder-platform"].exists, app.debugDescription)
    XCTAssertEqual(any("chat-session-").count, 0, "root: no chat rows\n" + app.debugDescription)
    XCTAssertEqual(app.tabBars.count, 0)
    capture("folder-root")

    // the folder pushes a new screen: Fernly's chats only
    fernly.tap()
    let chats = any("chat-session-")
    XCTAssertTrue(chats.firstMatch.waitForExistence(timeout: 10), "team: expected Fernly's chats\n" + app.debugDescription)
    XCTAssertFalse(fernly.exists, "team: the folder list is behind, not unfolded in place")
    XCTAssertTrue(app.navigationBars.staticTexts["Fernly"].waitForExistence(timeout: 10), app.debugDescription)
    let rows = chats.count
    capture("team-chats")

    // a chat opens its page as today, with its pinned cards
    chats.firstMatch.tap()
    XCTAssertTrue(app.descendants(matching: .any)["pinned-cards"].waitForExistence(timeout: 10), app.debugDescription)
    XCTAssertEqual(app.tabBars.count, 0)
    capture("chat-card")

    // back: Fernly's chats again, then the folders
    back()
    XCTAssertTrue(chats.firstMatch.waitForExistence(timeout: 10), app.debugDescription)
    XCTAssertEqual(chats.count, rows)
    back()
    XCTAssertTrue(fernly.waitForExistence(timeout: 10), app.debugDescription)
    XCTAssertEqual(any("chat-session-").count, 0, "root again: no chat rows\n" + app.debugDescription)
  }

  func testMovingChatToNoTeamShowsItAtRootAndItStillOpens() {
    launch("chats")
    let platform = app.buttons["chat-folder-platform"]
    XCTAssertTrue(platform.waitForExistence(timeout: 20), app.debugDescription)
    platform.tap()
    let chat = any("chat-session-").firstMatch
    XCTAssertTrue(chat.waitForExistence(timeout: 10), app.debugDescription)
    let identifier = chat.identifier
    chat.tap()
    XCTAssertTrue(app.buttons["session-title"].waitForExistence(timeout: 10), app.debugDescription)
    app.buttons["session-title"].tap()
    let move = app.buttons["Move to Team…"]
    XCTAssertTrue(move.waitForExistence(timeout: 5), app.debugDescription)
    move.tap()
    let root = app.buttons["No team"]
    XCTAssertTrue(root.waitForExistence(timeout: 5), "A chat must be movable outside every Team\n" + app.debugDescription)
    capture("move-no-team-menu")
    root.tap()
    back()
    XCTAssertFalse(app.buttons[identifier].exists, "The chat must leave Platform")
    back()
    let rootChat = app.buttons[identifier]
    XCTAssertTrue(rootChat.waitForExistence(timeout: 10), "The chat must stand alongside the folders\n" + app.debugDescription)
    XCTAssertTrue(platform.exists)
    XCTAssertTrue(app.buttons["chat-folder-main"].exists)
    XCTAssertEqual(app.tabBars.count, 0)
    capture("root-chat-folders")
    rootChat.tap()
    XCTAssertTrue(app.buttons["session-title"].waitForExistence(timeout: 10), app.debugDescription)
    back()
    XCTAssertTrue(rootChat.waitForExistence(timeout: 10), app.debugDescription)
  }

  func testDeskAndBatchUtilityRemainReachable() {
    launch("chats")
    XCTAssertTrue(app.buttons["side-avatar"].waitForExistence(timeout: 20))
    app.buttons["side-avatar"].tap()
    app.buttons["Desk"].tap()
    XCTAssertTrue(app.descendants(matching: .any)["desk-content"].waitForExistence(timeout: 10), app.debugDescription)
    app.buttons["side-plus"].tap()
    let duck = app.buttons["IDGAD"]
    XCTAssertTrue(duck.waitForExistence(timeout: 5), app.debugDescription)
    duck.tap()
    XCTAssertTrue(app.alerts.firstMatch.waitForExistence(timeout: 5), app.debugDescription)
    app.alerts.buttons["Cancel"].tap()
    back()
    XCTAssertTrue(app.buttons["chat-folder-main"].waitForExistence(timeout: 10))
    XCTAssertTrue(app.buttons["chats-blitz-all"].waitForExistence(timeout: 10), app.debugDescription)
    XCTAssertEqual(app.tabBars.count, 0)
  }

  func testSearchDoesNotExposeAccordionTeams() {
    launch("chats")
    XCTAssertTrue(app.buttons["side-search"].waitForExistence(timeout: 20))
    app.buttons["side-search"].tap()
    XCTAssertTrue(app.descendants(matching: .any)["chat-search"].waitForExistence(timeout: 10))
    XCTAssertEqual(any("chat-team-").count, 0)
    let field = app.searchFields.firstMatch
    XCTAssertTrue(field.waitForExistence(timeout: 5))
    field.tap(); field.typeText("Fernly")
    XCTAssertTrue(any("chat-session-").firstMatch.waitForExistence(timeout: 10))
    XCTAssertEqual(any("chat-team-").count, 0)
    any("chat-session-").firstMatch.tap()
    XCTAssertTrue(app.descendants(matching: .any)["pinned-cards"].waitForExistence(timeout: 10))
    back()
    XCTAssertTrue(app.buttons["chat-folder-main"].waitForExistence(timeout: 10))
  }

  /** Open cards of the demo's team `main` (Fernly), from demo/data/fixture.json. */
  private static let fernly: Set<String> = [
    "Which hero for the landing page?", "What should the sign-up button say?", "Send a weekly summary e-mail on Sundays?",
    "Launch on Monday at 09:00, or on Tuesday?", "Ask for notifications on step 3, or after the first watering?",
    "Skip the flaky Safari checkout test until after the launch?", "Social card for the launch: photo or illustration?",
    "Rename the colour “clay” to “due” in the tokens?", "Which icon style for the tab bar?", "Small or medium widget first?",
    "Which screenshots go first in the App Store?", "Hold 1.2 for the widget crash fix in build 419?",
    "Dark mode: keep the plant photos at full colour?", "Done: onboarding in three steps is on staging",
    "Landing page: 1.4 s on a mid-range phone over 4G", "Which colour palette for the launch?",
    "Which drawing welcomes new people?", "Which headline for the launch?",
  ]

  func testGlobalBlitzFromPlatformWalksFernlyAndComesBack() {
    launch("chats")
    let platform = app.descendants(matching: .any)["chat-folder-platform"]
    XCTAssertTrue(platform.waitForExistence(timeout: 20), app.debugDescription)
    platform.tap()
    let page = app.descendants(matching: .any)["team-chats-platform"]
    XCTAssertTrue(page.waitForExistence(timeout: 10), app.debugDescription)
    XCTAssertTrue(app.navigationBars.staticTexts["Platform"].waitForExistence(timeout: 10), app.debugDescription)

    // the bolt at the top right walks every team's cards, not only Platform's
    let bolt = app.buttons["chats-blitz-all"]
    XCTAssertTrue(bolt.waitForExistence(timeout: 10), app.debugDescription)
    XCTAssertTrue(bolt.isEnabled)
    XCTAssertGreaterThan(bolt.frame.midX, app.frame.midX, "the bolt stands at the top right")
    capture("platform-chats")
    bolt.tap()
    let title = app.staticTexts["card-title"]
    XCTAssertTrue(title.waitForExistence(timeout: 15), app.debugDescription)
    XCTAssertEqual(app.tabBars.count, 0)
    var seen = [title.label]
    capture("global-flash-start")
    // (on at least once, until a card of Fernly)
    while seen.count < 2 || !Self.fernly.contains(seen.last!) {
      XCTAssertLessThan(seen.count, 30, "no Fernly card on every team's Blitz: \(seen)")
      app.buttons["card-menu"].tap()
      let next = app.buttons["card-menu-next"]
      XCTAssertTrue(next.waitForExistence(timeout: 10), "walked: \(seen)\n" + app.debugDescription)
      next.tap()
      let moved = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label != %@", seen.last!), object: title)
      XCTAssertEqual(XCTWaiter().wait(for: [moved], timeout: 10), .completed, "Next did not move on from \(seen.last!)")
      seen.append(title.label)
    }
    XCTAssertTrue(Self.fernly.contains(seen.last!), "\(seen)")
    XCTAssertEqual(app.tabBars.count, 0)
    capture("global-flash")

    // back: Platform's chats, not the folders
    back()
    XCTAssertTrue(page.waitForExistence(timeout: 10), app.debugDescription)
    XCTAssertTrue(app.navigationBars.staticTexts["Platform"].waitForExistence(timeout: 10), app.debugDescription)
    XCTAssertTrue(any("chat-session-").firstMatch.exists, app.debugDescription)
    XCTAssertFalse(platform.exists, "back from the Blitz is the team's page, not the folders")
    XCTAssertEqual(app.tabBars.count, 0)
  }
}
