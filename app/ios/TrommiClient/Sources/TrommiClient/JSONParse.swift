// JSONParse.swift: JSON text (UTF-8) into a JV in one pass over the bytes. The client core hands the app its model as
// JSON (BoardMirror.swift `take(patch:)`, a warm open's whole model in one text): Foundation's JSONDecoder went through
// Codable, tried each kind of value in turn and threw for each miss, and took most of a warm open's time. This reads
// RFC 8259 JSON strictly: what is not JSON is nil; numbers are Doubles, as before; of a key given twice the first stands, as before.
import Foundation

enum JSONParse {
  /// Deeper nesting than this is refused (no input may exhaust the stack).
  static let maxDepth = 512

  static func parse(_ bytes: Bytes) -> JV? {
    bytes.withUnsafeBufferPointer { buf -> JV? in
      var p = Parser(b: buf, i: 0)
      p.space()
      guard let v = p.value(depth: 0) else { return nil }
      p.space()
      return p.i == buf.count ? v : Optional<JV>.none
    }
  }

  private struct Parser {
    let b: UnsafeBufferPointer<UInt8>
    var i: Int

    mutating func space() {
      while i < b.count, b[i] == 0x20 || b[i] == 0x0A || b[i] == 0x0D || b[i] == 0x09 { i += 1 }
    }
    mutating func literal(_ word: StaticString, _ v: JV) -> JV? {
      let n = word.utf8CodeUnitCount
      guard i + n <= b.count else { return nil }
      let w = word.utf8Start
      for k in 0..<n where b[i + k] != w[k] { return nil }
      i += n
      return v
    }
    mutating func value(depth: Int) -> JV? {
      guard i < b.count, depth <= JSONParse.maxDepth else { return nil }
      switch b[i] {
      case UInt8(ascii: "{"): return object(depth: depth)
      case UInt8(ascii: "["): return array(depth: depth)
      case UInt8(ascii: "\""): return string().map(JV.str)
      case UInt8(ascii: "t"): return literal("true", .bool(true))
      case UInt8(ascii: "f"): return literal("false", .bool(false))
      case UInt8(ascii: "n"): return literal("null", .null)
      default: return number()
      }
    }
    mutating func object(depth: Int) -> JV? {
      i += 1
      var o = [String: JV]()
      space()
      if i < b.count, b[i] == UInt8(ascii: "}") { i += 1; return .obj(o) }
      while true {
        space()
        guard i < b.count, b[i] == UInt8(ascii: "\""), let k = string() else { return nil }
        space()
        guard i < b.count, b[i] == UInt8(ascii: ":") else { return nil }
        i += 1
        space()
        guard let v = value(depth: depth + 1) else { return nil }
        if o[k] == nil { o[k] = v }
        space()
        guard i < b.count else { return nil }
        if b[i] == UInt8(ascii: ",") { i += 1; continue }
        if b[i] == UInt8(ascii: "}") { i += 1; return .obj(o) }
        return nil
      }
    }
    mutating func array(depth: Int) -> JV? {
      i += 1
      var a = [JV]()
      space()
      if i < b.count, b[i] == UInt8(ascii: "]") { i += 1; return .arr(a) }
      while true {
        space()
        guard let v = value(depth: depth + 1) else { return nil }
        a.append(v)
        space()
        guard i < b.count else { return nil }
        if b[i] == UInt8(ascii: ",") { i += 1; continue }
        if b[i] == UInt8(ascii: "]") { i += 1; return .arr(a) }
        return nil
      }
    }
    mutating func hex4() -> UInt32? {
      guard i + 4 <= b.count else { return nil }
      var v: UInt32 = 0
      for k in 0..<4 {
        let c = b[i + k]
        let d: UInt32
        switch c {
        case UInt8(ascii: "0")...UInt8(ascii: "9"): d = UInt32(c - UInt8(ascii: "0"))
        case UInt8(ascii: "a")...UInt8(ascii: "f"): d = UInt32(c - UInt8(ascii: "a") + 10)
        case UInt8(ascii: "A")...UInt8(ascii: "F"): d = UInt32(c - UInt8(ascii: "A") + 10)
        default: return nil
        }
        v = v << 4 | d
      }
      i += 4
      return v
    }
    mutating func string() -> String? {
      i += 1
      let start = i
      // the common case: no escape, valid UTF-8 taken as it is
      while i < b.count {
        let c = b[i]
        if c == UInt8(ascii: "\"") {
          let s = String(bytes: UnsafeBufferPointer(rebasing: b[start..<i]), encoding: .utf8)
          i += 1
          return s
        }
        if c == UInt8(ascii: "\\") { break }
        if c < 0x20 { return nil }
        i += 1
      }
      var out = [UInt8](UnsafeBufferPointer(rebasing: b[start..<i]))
      while i < b.count {
        let c = b[i]
        if c == UInt8(ascii: "\"") {
          i += 1
          return String(bytes: out, encoding: .utf8)
        }
        if c < 0x20 { return nil }
        if c != UInt8(ascii: "\\") { out.append(c); i += 1; continue }
        i += 1
        guard i < b.count else { return nil }
        let e = b[i]
        i += 1
        switch e {
        case UInt8(ascii: "\""): out.append(0x22)
        case UInt8(ascii: "\\"): out.append(0x5C)
        case UInt8(ascii: "/"): out.append(0x2F)
        case UInt8(ascii: "b"): out.append(0x08)
        case UInt8(ascii: "f"): out.append(0x0C)
        case UInt8(ascii: "n"): out.append(0x0A)
        case UInt8(ascii: "r"): out.append(0x0D)
        case UInt8(ascii: "t"): out.append(0x09)
        case UInt8(ascii: "u"):
          guard var cp = hex4() else { return nil }
          if (0xD800...0xDBFF).contains(cp) {
            // a high surrogate takes its low one; alone it is no character
            guard i + 2 <= b.count, b[i] == UInt8(ascii: "\\"), b[i + 1] == UInt8(ascii: "u") else { return nil }
            i += 2
            guard let lo = hex4(), (0xDC00...0xDFFF).contains(lo) else { return nil }
            cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00)
          } else if (0xDC00...0xDFFF).contains(cp) {
            return nil
          }
          guard let scalar = Unicode.Scalar(cp) else { return nil }
          out.append(contentsOf: Array(String(Character(scalar)).utf8))
        default: return nil
        }
      }
      return nil
    }
    mutating func number() -> JV? {
      let start = i
      if i < b.count, b[i] == UInt8(ascii: "-") { i += 1 }
      guard i < b.count else { return nil }
      if b[i] == UInt8(ascii: "0") { i += 1 }
      else if (UInt8(ascii: "1")...UInt8(ascii: "9")).contains(b[i]) { while i < b.count, (UInt8(ascii: "0")...UInt8(ascii: "9")).contains(b[i]) { i += 1 } }
      else { return nil }
      if i < b.count, b[i] == UInt8(ascii: ".") {
        i += 1
        let f = i
        while i < b.count, (UInt8(ascii: "0")...UInt8(ascii: "9")).contains(b[i]) { i += 1 }
        if i == f { return nil }
      }
      if i < b.count, b[i] == UInt8(ascii: "e") || b[i] == UInt8(ascii: "E") {
        i += 1
        if i < b.count, b[i] == UInt8(ascii: "+") || b[i] == UInt8(ascii: "-") { i += 1 }
        let f = i
        while i < b.count, (UInt8(ascii: "0")...UInt8(ascii: "9")).contains(b[i]) { i += 1 }
        if i == f { return nil }
      }
      // (a whole number of up to 15 digits is read here; anything else by the standard library, exactly)
      var whole = true
      var v: Int64 = 0
      var neg = false
      var k = start
      if b[k] == UInt8(ascii: "-") { neg = true; k += 1 }
      if i - k > 15 { whole = false }
      while whole, k < i {
        let c = b[k]
        guard (UInt8(ascii: "0")...UInt8(ascii: "9")).contains(c) else { whole = false; break }
        v = v * 10 + Int64(c - UInt8(ascii: "0"))
        k += 1
      }
      if whole { return .num(neg ? -Double(v) : Double(v)) }
      guard let s = String(bytes: UnsafeBufferPointer(rebasing: b[start..<i]), encoding: .utf8), let d = Double(s), d.isFinite else { return nil }
      return .num(d)
    }
  }
}
