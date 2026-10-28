import XCTest

// The iPhone's native look ("Grouped"), the production one: the root as two inset groups (the chats on no team, the
// teams) under the large title, a team's group page (a face per chat, its questions, then what is to read), one row of
// controls at the bottom (the system's search; the bolt over every team and the + in one group), no tab bar, the
// person's picture as a small button at the top right; a team pushes, the system's back comes home; a chat keeps its
// pinned card, composer, Markdown and Copy. Bundled demo (demo/data/fixture.json): team `main` is "Fernly", team
// `platform` is "Platform". No account, no network.
final class NativeDesignCaptureTests: XCTestCase {
  private var app: XCUIApplication!
  override func setUpWithError() throws { continueAfterFailure = false }
  override func tearDownWithError() throws { app?.terminate(); app = nil }
  private func launch(dark: Bool = false) {
    app = XCUIApplication()
    app.launchArguments = ["-trommi-theme", dark ? "dark" : "light"]
    app.launchEnvironment = ["TROMMI_SCREEN": "chats"]
    app.launch()
    XCTAssertTrue(app.buttons["chat-folder-platform"].waitForExistence(timeout: 20), app.debugDescription)
  }
  private func capture(_ name: String) {
    let attachment = XCTAttachment(screenshot: app.screenshot())
    attachment.name = name; attachment.lifetime = .keepAlways; add(attachment)
  }
  private func any(_ prefix: String) -> XCUIElementQuery {
    app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", prefix))
  }
  private func back() {
    let button = app.navigationBars.buttons.element(boundBy: 0)
    XCTAssertTrue(button.waitForExistence(timeout: 5), app.debugDescription)
    button.tap()
  }
  /** One row at the bottom: the search field leading, the bolt and the + trailing, the + last; none of them in the
   *  top bar, no tab bar. */
  private func assertBottomControls(_ page: String, file: StaticString = #filePath, line: UInt = #line) {
    let search = app.searchFields.firstMatch
    let bolt = app.buttons["chats-blitz-all"]
    let plus = app.buttons["side-plus"]
    XCTAssertTrue(search.waitForExistence(timeout: 10), "\(page): the system's search field\n" + app.debugDescription, file: file, line: line)
    XCTAssertTrue(bolt.waitForExistence(timeout: 5), "\(page): the bolt\n" + app.debugDescription, file: file, line: line)
    XCTAssertTrue(plus.waitForExistence(timeout: 5), "\(page): the +\n" + app.debugDescription, file: file, line: line)
    let screen = app.windows.firstMatch.frame
    for (name, e) in [("search", search), ("bolt", bolt), ("+", plus)] {
      XCTAssertGreaterThan(e.frame.midY, screen.height * 0.8, "\(page): \(name) stands at the bottom", file: file, line: line)
      XCTAssertLessThan(abs(e.frame.midY - search.frame.midY), 12, "\(page): \(name) on the search's row", file: file, line: line)
    }
    XCTAssertLessThan(search.frame.midX, bolt.frame.midX, "\(page): search leading, the bolt after it", file: file, line: line)
    XCTAssertLessThan(bolt.frame.midX, plus.frame.midX, "\(page): the + last", file: file, line: line)
    XCTAssertGreaterThan(bolt.frame.midX, screen.midX, "\(page): the bolt trailing", file: file, line: line)
    XCTAssertFalse(app.navigationBars.buttons["chats-blitz-all"].exists, "\(page): no bolt at the top right", file: file, line: line)
    XCTAssertFalse(app.navigationBars.buttons["side-plus"].exists, "\(page): no + at the top right", file: file, line: line)
    XCTAssertFalse(app.buttons["side-search"].exists, "\(page): no search button at the top", file: file, line: line)
    XCTAssertEqual(app.tabBars.count, 0, "\(page): no tab bar", file: file, line: line)
  }
  private func assertLargeTitle(_ title: String, file: StaticString = #filePath, line: UInt = #line) {
    let text = app.navigationBars.staticTexts[title]
    XCTAssertTrue(text.waitForExistence(timeout: 10), app.debugDescription, file: file, line: line)
    XCTAssertLessThan(text.frame.midX, app.windows.firstMatch.frame.midX, "the large title stands at the left", file: file, line: line)
  }
  /** On a team's page: the head of its first chat's block, tapped (it opens the chat); the chat's row id. */
  private func openFirstChatOfTeam() -> String {
    XCTAssertTrue(app.descendants(matching: .any)["desk-content"].waitForExistence(timeout: 10), app.debugDescription)
    let head = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "group-block-")).firstMatch
    XCTAssertTrue(head.waitForExistence(timeout: 10), app.debugDescription)
    let id = "chat-session-" + head.identifier.dropFirst("group-block-".count)
    head.tap()
    return id
  }
  /** Moves the first chat of Platform out of every team (the session menu's "No team") and comes home; its id. */
  private func moveFirstPlatformChatToRoot() -> String {
    app.buttons["chat-folder-platform"].tap()
    let id = openFirstChatOfTeam()
    let title = app.buttons["session-title"]
    XCTAssertTrue(title.waitForExistence(timeout: 10), app.debugDescription)
    title.tap()
    XCTAssertTrue(app.buttons["Move to Team…"].waitForExistence(timeout: 5), app.debugDescription)
    app.buttons["Move to Team…"].tap()
    XCTAssertTrue(app.buttons["No team"].waitForExistence(timeout: 5), app.debugDescription)
    app.buttons["No team"].tap()
    back(); back()
    XCTAssertTrue(app.buttons["chat-folder-platform"].waitForExistence(timeout: 10), app.debugDescription)
    return id
  }

  func testRootHasSearchBoltAndPlusOnOneBottomRow() {
    launch()
    assertLargeTitle("Chats")
    XCTAssertTrue(app.buttons["side-avatar"].exists, "the person's picture and its menu, a small button in the bar\n" + app.debugDescription)
    XCTAssertGreaterThan(app.buttons["side-avatar"].frame.midX, app.windows.firstMatch.frame.midX, "the picture stands at the top right")
    assertBottomControls("root")
    capture("A-root-controls")
  }

  func testTeamPageHasTheSameBottomRowAndSystemBack() {
    launch()
    app.buttons["chat-folder-platform"].tap()
    XCTAssertTrue(app.descendants(matching: .any)["desk-content"].waitForExistence(timeout: 10), app.debugDescription)
    XCTAssertFalse(app.buttons["chat-folder-platform"].exists, "the team pushed a page of its own")
    assertLargeTitle("Platform")
    assertBottomControls("team")
    capture("A-team-controls")
    back()
    XCTAssertTrue(app.buttons["chat-folder-platform"].waitForExistence(timeout: 10), app.debugDescription)
    assertBottomControls("root again")
  }

  func testChatsOnNoTeamStandBeforeTheFolders() {
    launch()
    let id = moveFirstPlatformChatToRoot()
    let chat = app.buttons[id]
    XCTAssertTrue(chat.waitForExistence(timeout: 10), "the chat on no team stands at the root\n" + app.debugDescription)
    for folder in any("chat-folder-").allElementsBoundByIndex {
      XCTAssertLessThan(chat.frame.minY, folder.frame.minY, "chats on no team first, then the folders: \(folder.identifier)")
    }
    assertBottomControls("root with a chat")
    capture("A-root-chat-folders")
  }

  func testBottomSearchFindsThroughTheCoreOnRootAndTeam() {
    launch()
    let field = app.searchFields.firstMatch
    XCTAssertTrue(field.waitForExistence(timeout: 10), app.debugDescription)
    field.tap(); field.typeText("Payments")
    let found = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label BEGINSWITH %@", "chat-session-", "Payments")).firstMatch
    XCTAssertTrue(found.waitForExistence(timeout: 10), "root: the core's chats under the words\n" + app.debugDescription)
    XCTAssertEqual(app.tabBars.count, 0)
    capture("A-root-search")
    field.typeText("zzz")
    XCTAssertTrue(app.staticTexts["chats-none"].waitForExistence(timeout: 10), app.debugDescription)
    let cancel = app.buttons["Cancel"]
    if cancel.exists { cancel.tap() } else { field.buttons.firstMatch.tap() }
    XCTAssertTrue(app.buttons["chat-folder-platform"].waitForExistence(timeout: 10), app.debugDescription)

    app.buttons["chat-folder-main"].tap()
    XCTAssertTrue(app.descendants(matching: .any)["desk-content"].waitForExistence(timeout: 10), app.debugDescription)
    let teamField = app.searchFields.firstMatch
    XCTAssertTrue(teamField.waitForExistence(timeout: 10), app.debugDescription)
    teamField.tap(); teamField.typeText("Payments")
    XCTAssertTrue(found.waitForExistence(timeout: 10), "team: the same search, every team's chats\n" + app.debugDescription)
    found.tap()
    XCTAssertTrue(app.buttons["session-title"].waitForExistence(timeout: 10), app.debugDescription)
  }

  func testBottomPlusOpensItsMenuAndTheNote() {
    launch()
    let plus = app.buttons["side-plus"]
    XCTAssertTrue(plus.waitForExistence(timeout: 10), app.debugDescription)
    plus.tap()
    XCTAssertTrue(app.buttons["add-agent"].waitForExistence(timeout: 5), app.debugDescription)
    XCTAssertTrue(app.buttons["add-team"].exists, app.debugDescription)
    capture("A-plus-menu")
    app.buttons["note-open"].tap()
    XCTAssertTrue(app.descendants(matching: .any)["note-text"].waitForExistence(timeout: 10), app.debugDescription)
    app.descendants(matching: .any)["note-close"].tap()
    XCTAssertTrue(app.buttons["chat-folder-platform"].waitForExistence(timeout: 10), app.debugDescription)
  }

  func testBottomBoltWalksEveryTeamAndComesBackToTheTeam() {
    launch()
    app.buttons["chat-folder-platform"].tap()
    let page = app.descendants(matching: .any)["desk-content"]
    XCTAssertTrue(page.waitForExistence(timeout: 10), app.debugDescription)
    let bolt = app.buttons["chats-blitz-all"]
    XCTAssertTrue(bolt.waitForExistence(timeout: 10), app.debugDescription)
    XCTAssertTrue(bolt.isEnabled, "the demo has open cards")
    bolt.tap()
    XCTAssertTrue(app.staticTexts["card-title"].waitForExistence(timeout: 15), app.debugDescription)
    XCTAssertEqual(app.tabBars.count, 0)
    capture("A-bolt-card")
    let custom = app.buttons["back"]
    if custom.exists { custom.tap() } else { back() }
    XCTAssertTrue(page.waitForExistence(timeout: 10), "back from the Blitz is the team's page\n" + app.debugDescription)
    assertBottomControls("team after the Blitz")
  }

  func testChatKeepsPinnedCardComposerAndMessageActions() {
    launch(dark: true)
    app.buttons["chat-folder-main"].tap()
    XCTAssertTrue(app.descendants(matching: .any)["desk-content"].waitForExistence(timeout: 10), app.debugDescription)
    capture("A-team-dark")
    _ = openFirstChatOfTeam()
    XCTAssertTrue(app.buttons["session-title"].waitForExistence(timeout: 10), app.debugDescription)
    XCTAssertLessThan(abs(app.buttons["session-title"].frame.midX - app.windows.firstMatch.frame.midX), 30, "the title sits in the middle")
    XCTAssertTrue(app.descendants(matching: .any)["pinned-cards"].waitForExistence(timeout: 10), "the chat's decision card\n" + app.debugDescription)
    XCTAssertTrue(app.buttons["attach"].firstMatch.exists, "the composer with its +\n" + app.debugDescription)
    XCTAssertFalse(app.buttons["chats-blitz-all"].exists, "the list's controls stay with the list")
    XCTAssertEqual(app.tabBars.count, 0)
    let messages = app.descendants(matching: .any).matching(identifier: "talk-message")
    let visible = messages.allElementsBoundByIndex.first { $0.isHittable }
    XCTAssertNotNil(visible, "a visible message for the real long-press action\n" + app.debugDescription)
    let message = visible ?? messages.firstMatch
    capture("A-chat-dark")
    message.press(forDuration: 1.0)
    XCTAssertTrue(app.buttons["Copy"].waitForExistence(timeout: 5), "a message keeps Copy\n" + app.debugDescription)
    app.buttons["Copy"].tap()
    back()
    XCTAssertTrue(app.descendants(matching: .any)["desk-content"].waitForExistence(timeout: 10), app.debugDescription)
    assertBottomControls("team after the chat")
    back()
    XCTAssertTrue(app.buttons["chat-folder-main"].waitForExistence(timeout: 10), app.debugDescription)
  }
}
