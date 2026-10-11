// ChatFollowTests: the "N new" pill of a conversation (ChatFollow.swift, SessionScreen).
import XCTest
@testable import TrommiClient

final class ChatFollowTests: XCTestCase {
  func testAtTheEndNewMessagesAreFollowed() {
    var f = ChatFollow()
    XCTAssertTrue(f.arrived(3, mine: false))
    XCTAssertEqual(f.unseen, 0)
  }

  func testAwayFromTheEndTheyAreCounted() {
    var f = ChatFollow()
    f.scrolled(toEnd: 900)
    XCTAssertFalse(f.arrived(3, mine: false))
    XCTAssertFalse(f.arrived(4, mine: false))
    XCTAssertEqual(f.unseen, 7)
  }

  /** His case on build 59: "7 new" stayed after he scrolled all the way down. */
  func testReachingTheEndClearsTheCount() {
    var f = ChatFollow()
    f.scrolled(toEnd: 900)
    _ = f.arrived(7, mine: false)
    f.scrolled(toEnd: 0)
    XCTAssertEqual(f.unseen, 0)
    XCTAssertTrue(f.atBottom)
    XCTAssertTrue(f.arrived(1, mine: false), "at the end it follows again")
  }

  func testNearTheEndCountsAsTheEnd() {
    var f = ChatFollow()
    f.scrolled(toEnd: 900)
    _ = f.arrived(2, mine: false)
    f.scrolled(toEnd: 60)
    XCTAssertEqual(f.unseen, 0)
  }

  /** The end seen without a scroll that ends there (the keyboard, a taller composer, the status rows growing). */
  func testTheEndOnScreenClearsTheCount() {
    var f = ChatFollow()
    f.left()
    _ = f.arrived(7, mine: false)
    f.reachedEnd()
    XCTAssertEqual(f.unseen, 0)
    XCTAssertTrue(f.atBottom)
  }

  func testSeeingTheNewestMessageClearsTheCount() {
    var f = ChatFollow()
    f.left()
    _ = f.arrived(7, mine: false)
    f.sawNewest()
    XCTAssertEqual(f.unseen, 0)
    XCTAssertFalse(f.atBottom, "the newest seen is not yet the end: it does not follow by itself")
  }

  func testHisOwnMessageTakesHimToTheEnd() {
    var f = ChatFollow()
    f.left()
    _ = f.arrived(2, mine: false)
    XCTAssertTrue(f.arrived(1, mine: true))
    XCTAssertEqual(f.unseen, 0)
  }
}
