// Demo.swift: the web demo's room as a board (the repository's demo/README.md): demo/data/fixture.json
// is the JS model (shared/README.md "The model") as JSON; this builds the same Board from it, with no
// room, no keys and no network. Every timestamp is shifted by (now - made_at), as demo.mjs does, so "5 min ago" stays
// five minutes ago. The app shows it in demo mode (Demo on the sign-in screen and in Settings, TROMMI_SCREEN=<id>).
import Foundation

public enum DemoFixture {
  /** The board of the fixture's bytes, its times moved to `now`. */
  public static func board(_ data: Data, now: UInt64 = nowMs()) throws -> Board {
    guard let raw = JV.parse(Array(data)), raw.object != nil else { throw TrommiError("bad-fixture", "the demo fixture is not a JSON object") }
    let made = raw["made_at"].double ?? Double(now)
    let f = shift(raw, by: Double(now) - made)
    return build(f)
  }

  /** Every time stamp (a number in ms under a key `*_at`, `at`, `until`, `since`) moved by `by` ms. */
  static func shift(_ v: JV, by: Double, key: String = "") -> JV {
    switch v {
    case .num(let d):
      let stamp = key == "at" || key == "until" || key == "since" || key.hasSuffix("_at") || key.hasSuffix("_since")
      return stamp && d > 1e12 ? .num((d + by).rounded()) : v
    case .arr(let a): return .arr(a.map { shift($0, by: by, key: key) })
    case .obj(let o): return .obj(Dictionary(uniqueKeysWithValues: o.map { ($0.key, shift($0.value, by: by, key: $0.key)) }))
    default: return v
    }
  }

  /**
   * The fixture as the client core's model reads a snapshot (client/src/model/snapshot.rs): every map as `[key,
   * value]` pairs, a timeline's items with it, the room registers as their raw entries; the core fills in what the
   * fixture leaves out, and the board is the copy of that model, as for a real room.
   */
  static func build(_ f: JV) -> Board {
    let b = Board()
    let room = f["room"]
    b.roomId = room["room_id"].string; b.hubURL = room["hub_url"].string ?? "mock:"; b.myDeviceId = room["my_device_id"].string; b.myRole = room["my_role"].string ?? "human"
    b.lastEnvelopeNumber = room["last_envelope_number"].int ?? 0
    b.connection = "live"
    func pairs(_ list: JV, _ id: (JV) -> String?) -> JV { .arr((list.array ?? []).compactMap { v in id(v).map { .arr([.str($0), v]) } }) }
    // a session is its agent's (the fixture names no group): known, as one this device holds the group of
    var cardsOf = [String: [(UInt64, String)]]()
    for c in f["cards"].array ?? [] {
      if let id = c["object_id"].string, let s = c["session_id"].string ?? c["agent_device_id"].string { cardsOf[s, default: []].append((clampedU64(c["created_at"].double), id)) }
    }
    let sessions: [JV] = (f["sessions"].array ?? []).compactMap { s in
      guard let dev = s["agent_device_id"].string else { return nil }
      let sid = s["session_id"].string ?? dev
      var o = s.object ?? [:]
      o["session_id"] = .str(sid); o["agent_device_ids"] = .arr([.str(dev)]); o["group_id"] = .str("demo"); o["session_key_epoch"] = 1
      o["card_ids"] = .arr((cardsOf[sid] ?? []).sorted { $0 < $1 }.map { .str($0.1) })
      o["timeline_key"] = .str("chat:session/\(sid)")
      o["last_activity_at"] = .num(Double((s["status_lines"].array ?? []).map { clampedU64($0["updated_at"].double) }.max() ?? 0))
      return .arr([.str(sid), .obj(o)])
    }
    let cards: [JV] = (f["cards"].array ?? []).compactMap { c in
      guard let id = c["object_id"].string, var o = c.object else { return nil }
      if o["session_id"] == nil || o["session_id"] == .null { o["session_id"] = c["agent_device_id"] }
      if o["timeline_key"] == nil { o["timeline_key"] = .str("chat:card/\(id)") }
      if o["content_state"] == nil { o["content_state"] = "ok" }
      if o["state_envelope_number"] == nil { o["state_envelope_number"] = c["envelope_number"] }
      return .arr([.str(id), .obj(o)])
    }
    let timelines: [JV] = (f["timelines"].object ?? [:]).map { key, items in
      let list = (items.array ?? []).filter { $0["envelope_number"].int != nil }
      let numbers = list.compactMap { $0["envelope_number"].int }
      let at = key.firstIndex(of: ":") ?? key.endIndex
      let id = at < key.endIndex ? String(key[key.index(after: at)...]) : ""
      return .arr([.str(key), .obj(["timeline_key": .str(key), "timeline_kind": .str(String(key[..<at])), "timeline_id": .str(id),
                                     "object_id": .str(String(id.split(separator: "/").last ?? "")), "item_count": .num(Double(numbers.count)),
                                     "newest_envelope_number": .num(Double(numbers.max() ?? 0)), "loaded_down_to": numbers.min().map { .num(Double($0)) } ?? .null,
                                     "has_more": false, "window_open": true, "items": .arr(list.map { .arr([$0["envelope_number"], $0]) })])])
    }
    var human = [JV]()
    let h = f["human"]
    for (prefix, map) in [("draft", "drafts"), ("snooze", "snoozes"), ("duck", "ducks"), ("desk", "desks"), ("session", "session_settings")] {
      for (id, v) in h[map].object ?? [:] { human.append(.arr([.str("\(prefix)/\(id)"), .obj(["value": v, "envelope_number": .null, "by_device_id": .null, "pending": false, "causal": .null])])) }
    }
    if !h["crown"].isNull { human.append(.arr(["crown", .obj(["value": h["crown"], "envelope_number": .null, "by_device_id": .null, "pending": false, "causal": .null])])) }
    let snap: JV = .obj(["room": room, "members": pairs(f["members"]) { $0["device_id"].string }, "sessions": .arr(sessions), "cards": .arr(cards),
                         "permissions": pairs(f["permissions"]) { $0["object_id"].string }, "notes": pairs(f["notes"]) { $0["object_id"].string },
                         "published": pairs(f["published"]) { $0["object_id"].string }, "timelines": .arr(timelines), "human": .arr(human)])
    _ = b.loadModel(String(decoding: snap.encoded(), as: UTF8.self))
    return b
  }
}
