import XCTest
@testable import TrommiClient

final class RecordStoreTests: XCTestCase {
  func rec(_ n: Int, text: String = "hello") -> Rec {
    Rec(envelopeNumber: n, json: .obj(["received": .obj(["change": .num(Double(n)), "payload": .str("{\"text\":\"\(text)\"}"), "l": .arr([.null, .str("Ä")]), "k": .num(-3)])]), opened: true)
  }
  func testRoundTrip() throws {
    let r = rec(42)
    let back = try XCTUnwrap(Rec.decode(Rec.encode(r)))
    XCTAssertEqual(back.envelopeNumber, 42)
    XCTAssertEqual(back.json, r.json)
    XCTAssertTrue(back.opened)
  }
  func testAppendReadLastWinsAndTornTail() throws {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent("rs-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: dir) }
    let rs = RecordStore(dir: dir, key: systemRandom(32))
    try rs.append((1...1200).map { rec($0) })
    try rs.append([rec(7, text: "again")])
    XCTAssertLessThan(rs.lastAppended, 1024)
    let ix = try XCTUnwrap(rs.index())
    XCTAssertEqual(ix.entries.count, 1200)
    let all = try XCTUnwrap(rs.decode(ix, 0..<ix.entries.count))
    XCTAssertEqual(all.map { $0.envelopeNumber }, Array(1...1200))
    XCTAssertEqual(all[6].json["received"]["payload"].string, "{\"text\":\"again\"}")
    // a torn write at the end: cut off, the rest kept
    let seg = try XCTUnwrap(FileManager.default.contentsOfDirectory(at: rs.recordsDir, includingPropertiesForKeys: nil).sorted { $0.path < $1.path }.last)
    let h = try FileHandle(forWritingTo: seg); try h.seekToEnd(); try h.write(contentsOf: Data([0, 0, 1, 0, 1, 2])); try h.close()
    XCTAssertEqual(try XCTUnwrap(rs.index()).entries.count, 1200)
    // a record opened with another key: refused
    let other = RecordStore(dir: dir, key: systemRandom(32))
    XCTAssertNil(other.decode(try XCTUnwrap(other.index()), 0..<1))
  }
}
