// RegisterNameTests: the register names and the ids in their values between the wire and the model (Records.swift
// `registerName`, `registerValue`), against spec/register-names.json, which the web's codec test reads too: iOS writes
// what the web reads (base64url), reads what the web writes, and still reads the hex it wrote on the wire before.
import Foundation
import XCTest
@testable import TrommiClient

final class RegisterNameTests: XCTestCase {
  private func vectors() throws -> JV {
    var repo = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
    while repo.path != "/", !FileManager.default.fileExists(atPath: repo.appendingPathComponent("spec/register-names.json").path) { repo = repo.deletingLastPathComponent() }
    let data = try Data(contentsOf: repo.appendingPathComponent("spec/register-names.json"))
    return try XCTUnwrap(JV.parse(Bytes(data)))
  }

  func testNamesAreWrittenInBase64urlAndReadInBothForms() throws {
    let v = try vectors()
    let names = try XCTUnwrap(v["names"].array)
    XCTAssertGreaterThan(names.count, 10)
    for n in names {
      let wire = try XCTUnwrap(n["wire"].string), model = try XCTUnwrap(n["model"].string)
      XCTAssertEqual(Records.registerName(model, toWire: true), wire, "written: \(model)")
      XCTAssertEqual(Records.registerName(wire, toWire: false), model, "read: \(wire)")
      if let legacy = n["legacy_wire"].string { XCTAssertEqual(Records.registerName(legacy, toWire: false), model, "read as iOS wrote it: \(legacy)") }
    }
  }

  func testIdsInValuesAreWrittenInBase64urlAndReadInBothForms() throws {
    let v = try vectors()
    for x in try XCTUnwrap(v["values"].array) {
      let key = try XCTUnwrap(x["key"].string)
      XCTAssertEqual(Records.registerValue(key, x["model"], toWire: true), x["wire"], "written: \(key)")
      XCTAssertEqual(Records.registerValue(key, x["wire"], toWire: false), x["model"], "read: \(key)")
      if x.has("legacy_wire") { XCTAssertEqual(Records.registerValue(key, x["legacy_wire"], toWire: false), x["model"], "read as iOS wrote it: \(key)") }
    }
  }

  /** A card snoozed on the web, a draft, duck and session settings it wrote: on the board under the card's hex key. */
  func testRegistersTheWebWroteReachTheBoardUnderTheModelsKeys() throws {
    let card = Bytes((0..<16).map { UInt8((0xc1 + $0 * 7) & 255) }), sess = Bytes(repeating: 0x51, count: 16), desk = Bytes(repeating: 0xde, count: 16)
    let h = EnvelopeHeader(group: [], epoch: 0, sender: [], seq: 1, time: 0, kind: .register)
    func read(_ name: String, _ value: JV) -> JV { Records.fromWire(.obj(["name": .str(name), "value": value, "lamport": 3]), header: h, sessionId: nil)["values"] }
    XCTAssertTrue(read("snooze/" + b64u(card), .obj(["until": .null])).has("snooze/" + hex(card)))
    XCTAssertEqual(read("draft/" + b64u(card), .obj(["note": "maybe"]))["draft/" + hex(card)]["note"].string, "maybe")
    XCTAssertTrue(read("duck/" + b64u(card), .obj(["at": 1])).has("duck/" + hex(card)))
    XCTAssertEqual(read("session/" + b64u(sess), .obj(["name": "S", "desk": .str(b64u(desk))]))["session/" + hex(sess)]["desk"].string, hex(desk))
    // and what iOS wrote before, hex on the wire, is the same key
    XCTAssertTrue(read("snooze/" + hex(card), .obj(["until": .null])).has("snooze/" + hex(card)))
    XCTAssertEqual(read("session/" + hex(sess), .obj(["desk": .str(hex(desk))]))["session/" + hex(sess)]["desk"].string, hex(desk))
  }
}
