// RecordsForTests.swift: the register names and values in the wire's form, as the tests write the records they
// hand the board (OldRec.swift). The app writes them through the client core (`registerToWire`).
import Foundation
@testable import TrommiClient

extension Records {
  /**
   * The register names whose tail is one id, and its length in bytes: a card, session or desk id (16), a device id or
   * an envelope hash (32). A name for a new kind of per-card or per-session register goes into this list, and that is
   * all it needs. On the wire the id is base64url (spec/v1.md section 2, 9.3.3), as the web writes it; in the model
   * it is lower-case hex. A tail that is no such id (`desk/main`) stays as it is.
   */
  static let idNames: [(prefix: String, bytes: Int)] = [
    ("draft/", 16), ("snooze/", 16), ("duck/", 16), ("star/", 16), ("desk/", 16), ("session/", 16),
    ("read/session/", 16), ("read/card/", 16), ("read/desk/", 16), ("device/", 32), ("alert/", 32),
  ]
  /**
   * A register's name between the wire and the model. Written: always base64url. Read: base64url, and also the hex
   * that iOS wrote on the wire before 2026-10-12, which is already the model's form and stays as it is, so both name
   * the same key. (Where both forms of one name hold a value, the one that came last in the hub's order shows.)
   */
  static func registerName(_ name: String, toWire: Bool) -> String {
    for (prefix, n) in idNames where name.hasPrefix(prefix) {
      let id = String(name.dropFirst(prefix.count))
      guard let bytes = toWire ? (try? unhex(id)) : (try? unb64u(id)), bytes.count == n else { return name }
      return prefix + (toWire ? b64u(bytes) : hex(bytes))
    }
    return name
  }
  /**
   * The ids inside a register's value, as the web maps them (app/web/core/codec.ts `registerValue`): the crown's
   * session and agent device, a desk's crown, a session's desk, the Goals' desk. Base64url on the wire, hex in the
   * model; a value in the other form (an older iOS wrote hex) or no id at all (`main`) stays as it is.
   */
  static func registerValue(_ key: String, _ v: JV, toWire: Bool) -> JV {
    guard let o = v.object else { return v }
    func id(_ x: JV?, _ n: Int) -> JV? {
      guard let s = x?.string else { return x }
      guard let bytes = toWire ? (try? unhex(s)) : (try? unb64u(s)), bytes.count == n else { return x }
      return .str(toWire ? b64u(bytes) : hex(bytes))
    }
    func crown(_ c: JV) -> JV {
      guard var co = c.object else { return c }
      if co["session_id"] != nil { co["session_id"] = id(co["session_id"], 16) }
      if co["agent_device_id"] != nil { co["agent_device_id"] = id(co["agent_device_id"], 32) }
      return .obj(co)
    }
    var out = o
    if key == "crown" { return crown(v) }
    if key.hasPrefix("desk/"), let c = o["crown"] { out["crown"] = crown(c) }
    else if key.hasPrefix("session/"), o["desk"] != nil { out["desk"] = id(o["desk"], 16) }
    else if key == "goals" || key.hasPrefix("goals/"), o["desk_id"] != nil { out["desk_id"] = id(o["desk_id"], 16) }
    else if key.hasPrefix("alert/"), o["sender_device_id"] != nil { out["sender_device_id"] = id(o["sender_device_id"], 32) }
    else { return v }
    return .obj(out)
  }

}
