// TrafficTests: what this device sent and received (Traffic.swift), and the account's usage as the hub names it.
import Foundation
import XCTest
@testable import TrommiClient

final class TrafficTests: XCTestCase {
  private var suite: String = ""
  override func setUp() { suite = "trommi-traffic-\(UUID().uuidString)"; Traffic.store = UserDefaults(suiteName: suite)!; Traffic.forget() }
  override func tearDown() { UserDefaults().removePersistentDomain(forName: suite); Traffic.store = .standard; Traffic.forget() }

  func testCountsAddUpAreKeptAndReset() {
    let start = Traffic.totals
    XCTAssertEqual(start.down, 0); XCTAssertEqual(start.up, 0)
    Traffic.add(down: 1000, up: 20)
    Traffic.add(down: 500)
    Traffic.forget()   // as after a relaunch: read back from the settings
    XCTAssertEqual(Traffic.totals.down, 1500)
    XCTAssertEqual(Traffic.totals.up, 20)
    XCTAssertEqual(Traffic.totals.since.timeIntervalSince1970, start.since.timeIntervalSince1970, accuracy: 0.001)
    let now = Date(timeIntervalSince1970: 1_800_000_000)
    Traffic.reset(now: now)
    Traffic.forget()
    XCTAssertEqual(Traffic.totals.down, 0); XCTAssertEqual(Traffic.totals.up, 0)
    XCTAssertEqual(Traffic.totals.since, now)
  }

  func testTheHubsAnswerIsRead() {
    let u = AccountUsage(["rooms": 1, "groups": 41, "live_groups": 38, "devices": 9, "commits": 120, "messages": 4, "envelopes": 7215,
                          "cards": 570, "open_cards": 31, "files": 214, "file_bytes": 182_400_000, "log_bytes": 1_000_000, "envelope_bytes": 31_500_000,
                          "total_bytes": 214_900_000])
    XCTAssertEqual(u.envelopes, 7215); XCTAssertEqual(u.openCards, 31); XCTAssertEqual(u.liveGroups, 38)
    XCTAssertEqual(u.totalBytes, 214_900_000); XCTAssertEqual(u.fileBytes, 182_400_000)
    XCTAssertEqual(AccountUsage([:]), AccountUsage())
  }
}
