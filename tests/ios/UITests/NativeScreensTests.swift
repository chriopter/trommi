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
  /** A sign-in whose app is closed right after the Desk shows (its sessions not yet joined): the app opens on the
   *  Desk again, says the sign-in was cut off and offers Log in; logging in finishes it and the line goes. Opt-in, as
   *  the test below: a private account file and an isolated hub. */
  func testACutOffSignInOpensOnTheDeskAndAsksToLogInAgain() throws {
    guard let path = ProcessInfo.processInfo.environment["TROMMI_SIGNIN_ACCOUNT_FILE"],
          !path.isEmpty, !path.hasPrefix("$(") else { throw XCTSkip("Set TROMMI_SIGNIN_ACCOUNT_FILE for the isolated real-account gate.") }
    let account = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: URL(fileURLWithPath: path))) as? [String: String])
    app = XCUIApplication()
    app.launchEnvironment["TROMMI_TEST_HUB"] = "http://127.0.0.1:18874"
    app.launch()
    let login = app.buttons["way-login"]
    XCTAssertTrue(login.waitForExistence(timeout: 20), app.debugDescription)
    login.tap()
    let email = app.textFields["account-email"]
    XCTAssertTrue(email.waitForExistence(timeout: 10))
    email.tap(); email.typeText(try XCTUnwrap(account["email"]))
    let password = app.secureTextFields["account-password"]
    password.tap(); password.typeText(try XCTUnwrap(account["password"]))
    app.buttons["account-submit"].tap()
    let row = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "desk-card-")).firstMatch
    XCTAssertTrue(row.waitForExistence(timeout: 90))
    app.terminate(); app.launch()
    let resume = app.buttons["resume-login"]
    XCTAssertTrue(row.waitForExistence(timeout: 30), "a cut-off sign-in opens on the Desk, it does not sign out. " + app.debugDescription)
    XCTAssertTrue(resume.waitForExistence(timeout: 60), "the cut-off sign-in is said, with Log in. " + app.debugDescription)
    capture("resume-line")
    resume.tap()
    let again = app.textFields["account-email"]
    XCTAssertTrue(again.waitForExistence(timeout: 10))
    again.tap(); again.typeText(try XCTUnwrap(account["email"]))
    let secret = app.secureTextFields["account-password"]
    secret.tap(); secret.typeText(try XCTUnwrap(account["password"]))
    app.buttons["account-submit"].tap()
    XCTAssertTrue(resume.waitForNonExistence(timeout: 90), "logging in finishes the sign-in and the line goes. " + app.debugDescription)
    XCTAssertTrue(row.waitForExistence(timeout: 10))
    capture("resume-done")
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
    XCTAssertTrue(app.tabBars.buttons["Chats"].exists, "The signed-in iPhone must keep its Desk · Chats · Search tab bar.")
    XCTAssertTrue(app.tabBars.buttons["Desk"].exists)
    XCTAssertTrue(app.buttons["note-open"].exists)
    capture("real-signin-desk")
    row.tap()
    XCTAssertTrue(app.staticTexts["card-title"].waitForExistence(timeout: 15))
    capture("real-signin-card")
    app.buttons["back"].tap()
    XCTAssertTrue(app.tabBars.buttons["Chats"].waitForExistence(timeout: 10))
    app.tabBars.buttons["Chats"].tap()
    let chat = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "chat-session-")).firstMatch
    XCTAssertTrue(chat.waitForExistence(timeout: 10))
    chat.tap()
    XCTAssertTrue(app.staticTexts["talk-message"].firstMatch.waitForExistence(timeout: 15))
    capture("real-signin-chat")
    app.buttons["back"].tap()
    XCTAssertTrue(app.tabBars.buttons["Desk"].waitForExistence(timeout: 10))
    app.tabBars.buttons["Desk"].tap()
    app.buttons["note-open"].tap()
    XCTAssertTrue(app.descendants(matching: .any)["note-text"].waitForExistence(timeout: 10))
    capture("real-signin-note")
    app.descendants(matching: .any)["note-close"].tap()
    XCTAssertTrue(row.waitForExistence(timeout: 10))
    app.terminate(); app.launch()
    XCTAssertTrue(row.waitForExistence(timeout: 30), "The signed-in Desk must survive a restart.")
    // signing out: the start screen says so
    app.buttons["place-pill"].tap()
    app.buttons["Settings"].firstMatch.tap()
    let signOut = app.buttons["Sign out…"].firstMatch
    for _ in 0..<6 where !signOut.isHittable { app.swipeUp() }
    signOut.tap()
    app.buttons["Sign out"].firstMatch.tap()
    let notice = app.staticTexts["account-notice"]
    XCTAssertTrue(notice.waitForExistence(timeout: 30), app.debugDescription)
    XCTAssertEqual(notice.label, "Logged out.")
    capture("real-signin-logged-out")
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
    // (the top row: round back, the title in its own pill in the middle, round ⋯)
    XCTAssertTrue(app.buttons["back"].exists && app.buttons["session-title"].exists && app.buttons["session-more"].exists, app.debugDescription)
    XCTAssertLessThan(abs(app.buttons["session-title"].frame.midX - app.windows.firstMatch.frame.midX), 30, "the title sits in the middle")
    capture("chat")
  }
  /** The chat's "+" offers the Photo Library (lost once in the switch to the app core: it opened Files only), and a
   *  photo picked there goes out with the message. */
  func testChatPlusAttachesAPhoto() {
    launch("chat")
    let plus = app.buttons["attach"].firstMatch
    XCTAssertTrue(plus.waitForExistence(timeout: 15), app.debugDescription)
    plus.tap()
    let photos = app.buttons["attach-photos"]
    XCTAssertTrue(photos.waitForExistence(timeout: 5), "the + offers the Photo Library. " + app.debugDescription)
    XCTAssertTrue(app.buttons["attach-file"].exists, "the + offers Choose File.")
    capture("chat-plus-menu")
    photos.tap()
    let photo = app.images.matching(NSPredicate(format: "identifier BEGINSWITH %@ OR label BEGINSWITH %@", "PXGGridLayout", "Photo")).firstMatch
    guard photo.waitForExistence(timeout: 15) else { capture("chat-photo-picker"); return }
    capture("chat-photo-picker")
    photo.tap()
    let add = app.buttons["Add"]
    if add.waitForExistence(timeout: 3) { add.tap() }
    let chip = app.buttons["Remove"].firstMatch
    XCTAssertTrue(chip.waitForExistence(timeout: 15), "the picked photo lies in the composer. " + app.debugDescription)
    capture("chat-photo-attached")
    app.buttons["Send"].firstMatch.tap()
    XCTAssertTrue(chip.waitForNonExistence(timeout: 10), "sent: the composer is empty again. " + app.debugDescription)
    XCTAssertFalse(app.staticTexts["That line cannot be sent."].waitForExistence(timeout: 2), "the line with the photo went out.")
    capture("chat-photo-sent")
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
  // ---- what came back after the move to the app core: the duck for all, Blitz, the Chat list, the session's
  // switch and menu, Settings' pages, the Artifacts filter, the note that saves itself ----
  func testDeskDuckForAllAndBlitz() {
    launch("desk")
    let cards = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "desk-card-"))
    XCTAssertTrue(cards.firstMatch.waitForExistence(timeout: 15))
    let duck = app.buttons["desk-duck"], blitz = app.buttons["desk-blitz"]
    XCTAssertTrue(duck.exists && blitz.exists, "the duck for all and Blitz stand at the Desk's top right. " + app.debugDescription)
    capture("desk-ways")
    blitz.tap()
    XCTAssertTrue(app.staticTexts["card-title"].waitForExistence(timeout: 10), "Blitz opens the first open card")
    app.buttons["back"].tap()
    XCTAssertTrue(duck.waitForExistence(timeout: 10))
    duck.tap()
    let yes = app.alerts.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", "Yes")).firstMatch
    XCTAssertTrue(yes.waitForExistence(timeout: 5), app.debugDescription)
    capture("desk-duck-ask")
    yes.tap()
    let toast = app.descendants(matching: .any).matching(NSPredicate(format: "label BEGINSWITH %@", "Left to the agents")).firstMatch
    XCTAssertTrue(toast.waitForExistence(timeout: 10), "one toast with its Undo. " + app.debugDescription)
    XCTAssertFalse(duck.exists, "nothing left to duck")
    capture("desk-duck-done")
  }
  func testChatListIsCalm() {
    launch("chats")
    let teaser = app.staticTexts.matching(NSPredicate(format: "identifier BEGINSWITH %@", "chat-teaser-")).firstMatch
    XCTAssertTrue(teaser.waitForExistence(timeout: 15), app.debugDescription)
    XCTAssertFalse(teaser.label.trimmingCharacters(in: .whitespaces).isEmpty, "a row says what was said last")
    // (calm, as a messenger's: a session's helpers are said, not drawn; no dots, what is new or waits is said in the
    // row's label; every team's chats in one list, each team only a title over them, no team switch)
    let row = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label CONTAINS %@", "chat-session-", "helper")).firstMatch
    XCTAssertTrue(row.exists, "a row says its helpers. " + app.debugDescription)
    XCTAssertFalse(app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "chat-helpers-")).firstMatch.exists, "no helper dots")
    let dot = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "chat-unread-")).firstMatch
    XCTAssertFalse(dot.exists, "no dot. " + app.debugDescription)
    XCTAssertFalse(app.buttons["place-pill"].exists, "no team switch over the chats. " + app.debugDescription)
    let titles = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "chat-team-"))
    XCTAssertGreaterThanOrEqual(titles.count, 2, "every team's chats, each under its title. " + app.debugDescription)
    capture("chats")
    // (a team's title folds its chats to one row of their drawings; a tap on that row opens them again)
    let before = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "chat-session-")).count
    titles.element(boundBy: 1).tap()
    let folded = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "chat-folded-")).firstMatch
    XCTAssertTrue(folded.waitForExistence(timeout: 5), "folded. " + app.debugDescription)
    XCTAssertLessThan(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "chat-session-")).count, before)
    capture("chats-folded")
    folded.tap()
    XCTAssertTrue(folded.waitForNonExistence(timeout: 5), "open again. " + app.debugDescription)
    // (Search: the same list under the words; the core keeps the chats that have them)
    app.tabBars.buttons["Search"].tap()
    let field = app.searchFields.firstMatch
    XCTAssertTrue(field.waitForExistence(timeout: 10), app.debugDescription)
    field.tap(); field.typeText("Payments")
    let found = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "chat-session-"))
    expectation(for: NSPredicate(format: "count == 1"), evaluatedWith: found)
    waitForExpectations(timeout: 10)
    XCTAssertTrue(found.firstMatch.label.hasPrefix("Payments"), found.firstMatch.label)
    capture("chats-search")
    field.typeText("zzz")
    XCTAssertTrue(app.staticTexts["chats-none"].waitForExistence(timeout: 10), app.debugDescription)
  }
  func testAgentBubblesAndOneWorkingLine() {
    launch("chat--payments-service")
    XCTAssertTrue(app.staticTexts["talk-message"].firstMatch.waitForExistence(timeout: 15))
    // (an iPhone: one small line above the field, the newest status line and "and N more"; a tap opens them all)
    let more = app.buttons["status-line"]
    XCTAssertTrue(more.waitForExistence(timeout: 5), app.debugDescription)
    XCTAssertFalse(app.descendants(matching: .any)["session-helpers"].exists, "no helper list on an iPhone")
    capture("chat-bubbles")
    more.tap()
    capture("chat-status-more")
  }
  func testChatListAndChatInDark() {
    app = XCUIApplication()
    app.launchArguments += ["-trommi-theme", "dark"]
    app.launchEnvironment["TROMMI_SCREEN"] = "chats"
    app.launch()
    XCTAssertTrue(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "chat-session-")).firstMatch.waitForExistence(timeout: 15))
    Thread.sleep(forTimeInterval: 1)
    capture("chats-dark")
    app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "chat-session-")).firstMatch.tap()
    XCTAssertTrue(app.buttons["session-title"].waitForExistence(timeout: 10))
    Thread.sleep(forTimeInterval: 1)
    capture("chat-dark")
  }
  func testSessionQuestionsOnlyAndItsMenu() {
    launch("chat")
    XCTAssertTrue(app.staticTexts["talk-message"].firstMatch.waitForExistence(timeout: 15))
    // (the chip shows only while questions wait, with how many)
    let chip = app.buttons["session-mode-questions"]
    XCTAssertTrue(chip.waitForExistence(timeout: 5), app.debugDescription)
    let count = chip.label.components(separatedBy: CharacterSet.decimalDigits.inverted).compactMap { Int($0) }.first ?? 0
    XCTAssertGreaterThan(count, 0, chip.label)
    chip.tap()
    XCTAssertTrue(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "session-question-")).firstMatch.waitForExistence(timeout: 5), "every question it asked. " + app.debugDescription)
    XCTAssertTrue(app.buttons["session-mode-all"].exists, "the chip leads back to all messages")
    // (what waits is listed as the Desk's cards, as many as the chip counted)
    XCTAssertEqual(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "desk-card-")).count, count, "the open questions listed are the ones counted. " + app.debugDescription)
    capture("session-questions")
    app.buttons["session-more"].tap()
    XCTAssertTrue(app.buttons["Rename…"].waitForExistence(timeout: 5))
    XCTAssertTrue(app.buttons["Change Icon…"].exists)
    XCTAssertTrue(app.buttons["Move to Team…"].exists)
    capture("session-menu")
    app.buttons["Rename…"].tap()
    // (an alert's text field does not carry its SwiftUI identifier: it is the alert's one field)
    let field = app.alerts.textFields.firstMatch
    XCTAssertTrue(field.waitForExistence(timeout: 5), app.debugDescription)
    let name = field.value as? String ?? ""
    XCTAssertFalse(name.isEmpty, "the field starts with the session's name")
    field.typeText(" 2")
    app.alerts.buttons["Rename"].tap()
    XCTAssertTrue(app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", name + " 2")).firstMatch.waitForExistence(timeout: 5), "renamed. " + app.debugDescription)
  }
  func testSettingsHasItsPages() {
    launch("settings")
    let devices = app.buttons["settings-row-devices"]
    XCTAssertTrue(devices.waitForExistence(timeout: 15))
    XCTAssertTrue(app.buttons["settings-row-agents"].exists && app.buttons["settings-row-account"].exists)
    capture("settings-list")
    devices.tap()
    XCTAssertTrue(app.descendants(matching: .any)["settings-devices"].waitForExistence(timeout: 5))
    capture("settings-devices")
    app.buttons["back"].tap()
    app.buttons["settings-row-account"].tap()
    XCTAssertTrue(app.buttons["Change password"].waitForExistence(timeout: 5))
  }
  func testArtifactsFilter() {
    launch("artifacts")
    let pages = app.buttons["artifacts-kind-pages"]
    XCTAssertTrue(pages.waitForExistence(timeout: 15), app.debugDescription)
    capture("artifacts-all")
    pages.tap()
    expectation(for: NSPredicate(format: "selected == true"), evaluatedWith: pages)
    waitForExpectations(timeout: 5)
    capture("artifacts-pages")
    app.buttons["artifacts-kind-media"].tap()
    capture("artifacts-media")
  }
  /** The team's knowledge (demo room): the core's entries, the filter Pinned, the editor of a new entry. */
  func testKnowledgePage() {
    launch("knowledge")
    let pinned = app.buttons["knowledge-filter-pinned"]
    XCTAssertTrue(pinned.waitForExistence(timeout: 15), app.debugDescription)
    capture("knowledge-all")
    pinned.tap()
    expectation(for: NSPredicate(format: "selected == true"), evaluatedWith: pinned)
    waitForExpectations(timeout: 5)
    capture("knowledge-pinned")
    app.buttons["knowledge-add"].tap()
    XCTAssertTrue(app.textFields.firstMatch.waitForExistence(timeout: 5))
    capture("knowledge-add")
  }
  func testTheNoteSavesItself() {
    launch("desk")
    XCTAssertTrue(app.buttons["note-open"].waitForExistence(timeout: 15))
    app.buttons["note-open"].tap()
    let field = app.descendants(matching: .any)["note-text"]
    XCTAssertTrue(field.waitForExistence(timeout: 5))
    // (a tap on the paper just below the words starts writing, as one on the words does)
    field.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 1)).withOffset(CGVector(dx: 0, dy: 24)).tap()
    XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 5), "the paper below the words is the field's. " + app.debugDescription)
    field.typeText("Buy oat milk")
    XCTAssertFalse(app.buttons["Keep in Note"].exists, "no button to keep it: it keeps itself")
    Thread.sleep(forTimeInterval: 1.2)
    app.descendants(matching: .any)["note-close"].tap()
    XCTAssertTrue(app.buttons["note-open"].waitForExistence(timeout: 5))
    app.buttons["note-open"].tap()
    XCTAssertTrue(field.waitForExistence(timeout: 5))
    XCTAssertTrue((field.value as? String ?? "").contains("Buy oat milk"), "kept while typing, there again when it opens: \(String(describing: field.value))")
    capture("note-kept")
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
