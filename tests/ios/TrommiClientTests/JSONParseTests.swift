// JSONParseTests: the one-pass JSON reader (JSONParse.swift) reads what Foundation's JSONDecoder read, and refuses
// what is no JSON; and it is fast enough for a warm open's whole model.
import Foundation
import XCTest
@testable import TrommiClient

final class JSONParseTests: XCTestCase {
  private func old(_ s: String) -> JV? { try? JSONDecoder().decode(JV.self, from: Data(s.utf8)) }

  func testItReadsWhatTheDecoderRead() {
    let cases = [
      #"{"a":1,"b":[true,false,null],"c":{"d":"e"},"f":-0.5e3,"g":12345678901234567890,"h":1.5,"i":-7}"#,
      #"  [ "x\"y\\z\/\b\f\n\r\t", "\u00e9\u20ac\ud83d\ude00", "ünïcödé 🙂" ]  "#,
      #"{"a":1,"a":2}"#, "0", "-0", "\"\"", "[]", "{}", "1E+2", "3e-2", "9007199254740993",
    ]
    for c in cases { XCTAssertEqual(JV.parse(Bytes(c.utf8)), old(c), c) }
  }

  func testItRefusesWhatIsNoJSON() {
    let bad = ["", "{", "[1,]", "{\"a\":}", "01", "1.", "-", "tru", "\"\u{01}\"", "\"\\ud800\"", "\"\\x\"", "[1] 2", "nul", "{'a':1}", "1e400"]
    for c in bad { XCTAssertNil(JV.parse(Bytes(c.utf8)), c) }
    XCTAssertNil(JV.parse(Array(repeating: UInt8(ascii: "["), count: 10_000)), "too deep")
    XCTAssertNil(JV.parse([0x22, 0xFF, 0x22]), "not UTF-8")
  }

  func testAWholeModelIsReadQuickly() {
    var cards = [JV]()
    for i in 0..<1000 {
      cards.append(.arr([.str(String(format: "%032x", i)), .obj(["title": .str("Question \(i) with some words"), "body": .str(String(repeating: "text é ", count: 40)),
                                                            "options": .arr([.obj(["key": "a", "label": "A"]), .obj(["key": "b", "label": "B"])]), "envelope_number": .num(Double(i)), "versions": .arr([])])]))
    }
    let bytes = JV.obj(["cards": .arr(cards)]).encoded()
    let t = Date()
    XCTAssertNotNil(JV.parse(bytes))
    let fast = Date().timeIntervalSince(t)
    let t2 = Date()
    XCTAssertNotNil(try? JSONDecoder().decode(JV.self, from: Data(bytes)))
    let slow = Date().timeIntervalSince(t2)
    XCTAssertLessThan(fast, slow / 2, "\(Int(fast * 1000)) ms vs \(Int(slow * 1000)) ms for \(bytes.count) bytes")
  }
}
