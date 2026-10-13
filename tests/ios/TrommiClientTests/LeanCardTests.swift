// LeanCardTests.swift: the client core hands the app its cards lean (client/src/model/patch.rs `LeanCard`): lists and
// objects, the versions' bodies and the answers before the one in force are read from the core when first asked.
import XCTest
@testable import TrommiClient

final class LeanCardTests: XCTestCase {
  private func demo() throws -> Board {
    var repo = URL(fileURLWithPath: #filePath)
    while repo.path != "/", !FileManager.default.fileExists(atPath: repo.appendingPathComponent("demo/data/fixture.json").path) { repo = repo.deletingLastPathComponent() }
    return try DemoFixture.board(try Data(contentsOf: repo.appendingPathComponent("demo/data/fixture.json")), now: 1_800_000_000_000)
  }

  func testWhatALeanCardLeftOutIsReadWhenAsked() throws {
    let b = try demo()
    let withOptions = b.cards.values.filter { $0.lazyFields.contains("options") }
    XCTAssertFalse(withOptions.isEmpty, "the demo has cards with options, left out of the patch")
    let d = DeskModel(board: b)
    d.update(now: 1_800_000_000_000)
    // (the Desk is made without reading them)
    XCTAssertTrue(withOptions.allSatisfy { $0.lean }, "making the Desk read no card whole")
    let c = withOptions[0]
    let whole = try XCTUnwrap(JV.parse(Bytes(try b.engine.cardJson(objectId: c.objectId).utf8)))
    XCTAssertEqual(d.byCard[c.objectId]?.options.map { $0.key }, (whole["options"].array ?? []).map { Option($0).key })
    XCTAssertFalse(c.lean, "read whole once asked")
    XCTAssertEqual(c.versions.map { $0.content?["title"] }, (whole["versions"].array ?? []).map { $0["content"]["title"] })
    XCTAssertEqual(c.answers.count, whole["answers"].array?.count ?? 0)
  }

  func testAVersionsBodyIsReadWhenAsked() throws {
    let b = try demo()
    let revised = try XCTUnwrap(b.cards.values.first { $0.versions.count > 1 && $0.lean })
    let whole = try XCTUnwrap(JV.parse(Bytes(try b.engine.cardJson(objectId: revised.objectId).utf8)))
    XCTAssertEqual(revised.versions[0].content?["title"], whole["versions"][0]["content"]["title"])
    XCTAssertNotNil(revised.versions[0].content?["title"].string)
  }
}
