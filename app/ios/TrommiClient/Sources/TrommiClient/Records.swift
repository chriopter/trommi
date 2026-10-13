// Records.swift: the Scribble Board's bodies between the views' form and the wire's (board items, the snapshot file).
// Every other body and every register is written by the client core (core/client/src/codec.rs, `bodyToWire`,
// `registerToWire`) and read by it (inside `ClientModel`). Here, for the board only:
//   attachment_id        a body's `file_id` (base64url there, lower-case hex in the model), likewise poster_attachment_id
//   stroke_ids, board units  a board item's `shape_ids` and whole 1/16 units (`boardItem`)
import Foundation

enum Records {




  /**
   * Byte strings in a body are base64url on the wire (spec/v1.md section 2) and lower-case hex in the model, which
   * names things by hex everywhere (timelines, cards, devices): file_id ↔ attachment_id, and every `…object_id`,
   * `…object_ids` and `previous_version_hash`. A value that is not the id it should be is left as it is.
   */
  static func renameFiles(_ v: JV, toWire: Bool, depth: Int) -> JV {
    if depth > 32 { return v }
    func id(_ s: String, _ lengths: [Int]) -> JV? {
      guard let bytes = toWire ? (try? unhex(s)) : (try? unb64u(s)), lengths.contains(bytes.count) else { return nil }
      return .str(toWire ? b64u(bytes) : hex(bytes))
    }
    switch v {
    case .arr(let a): return .arr(a.map { renameFiles($0, toWire: toWire, depth: depth + 1) })
    case .obj(let o):
      var out = [String: JV]()
      for (k, x) in o {
        if let name = toWire ? Self.toWire[k] : Self.fromWire[k], let s = x.string { out[name] = id(s, [16]) ?? x }
        else if k.hasSuffix("object_id"), let s = x.string { out[k] = id(s, [16]) ?? x }
        else if k.hasSuffix("object_ids"), let list = x.array { out[k] = .arr(list.map { $0.string.flatMap { id($0, [16]) } ?? $0 }) }
        else if k == "previous_version_hash", let s = x.string { out[k] = id(s, [32]) ?? x }
        else { out[k] = renameFiles(x, toWire: toWire, depth: depth + 1) }
      }
      return .obj(out)
    default: return v
    }
  }
  private static let fromWire = ["file_id": "attachment_id", "poster_file_id": "poster_attachment_id"]
  private static let toWire = ["attachment_id": "file_id", "poster_attachment_id": "poster_file_id"]
  /** The file ids a body names, for the signed header: a body's attachments, and the pictures of a board item. */
  static func fileIds(_ wire: JV) -> [FileId] {
    var out = [FileId]()
    let refs = (wire["attachments"].array ?? []) + (wire["strokes"].array ?? []).map { $0["attachment"] }
    for a in refs { for k in ["file_id", "poster_file_id"] { if let id = a[k].string, let b = try? unb64u(id), !out.contains(b) { out.append(b) } } }
    return out
  }

  // ---- the Scribble Board's bodies (spec/v1.md 10.5) ---------------------------------------------------------
  //
  // The views and `CanvasState` (Canvas.swift) keep the model they had: positions and lengths in board units with
  // fractions, shape ids "<sender in hex>/<number>/<index>", `stroke_ids`, a picture's size and type beside its
  // attachment. On the wire (10.5) every position and length is a whole number of 1/16 unit, an id names its
  // sender in base64url, the list is `shape_ids`, a stroke carries no transform, and a picture is `rect` and
  // `attachment` alone. These two functions are the one place where the two meet.

  private static let lengths: Set<String> = ["width", "size", "wrap"]
  /** A length or position in the other form: board units ↔ whole 1/16 units (kept within 32 bits). */
  private static func q(_ v: JV, toWire: Bool) -> JV {
    guard let d = v.double, d.isFinite else { return v }
    return .num(toWire ? max(-2_147_483_648, min(2_147_483_647, InkWire.jsRound(d * InkWire.Q))) : d / InkWire.Q)
  }
  /** A shape id in the other form: its sender is hex in the model, base64url on the wire. */
  static func shapeId(_ id: String, toWire: Bool) -> String {
    let parts = id.split(separator: "/", omittingEmptySubsequences: false)
    guard parts.count == 3, let sender = toWire ? (try? unhex(String(parts[0]))) : (try? unb64u(String(parts[0]))), sender.count == 32 else { return id }
    return "\(toWire ? b64u(sender) : hex(sender))/\(parts[1])/\(parts[2])"
  }
  /** One shape of a `strokes` item or of a snapshot. To the wire: a transform is applied to the points and left out. */
  static func boardShape(_ entry: JV, toWire: Bool) -> JV {
    guard var e = entry.object, let tool = e["tool"]?.string else { return entry }
    if toWire, InkWire.STROKE_TOOLS.contains(tool), let m = e["transform"]?.array?.compactMap({ $0.double }), let ink = InkWire.unpack(e["points"]?.string) {
      let baked = InkWire.bake(ink, m)
      e["points"] = .str(InkWire.pack(baked.ink))
      if let w = e["width"]?.double { e["width"] = .num(w * baked.scale) }
    }
    for k in lengths { if let v = e[k] { e[k] = q(v, toWire: toWire) } }
    for k in ["at", "rect"] { if let a = e[k]?.array { e[k] = .arr(a.map { q($0, toWire: toWire) }) } }
    if toWire {
      if let w = e["width"]?.double { e["width"] = .num(max(1, min(16_000, w))) }
      if let z = e["z"]?.double { e["z"] = .num(InkWire.jsRound(z)) }
      // What the model keeps beside a picture's attachment is the attachment's own on the wire.
      for k in ["transform", "continues", "nw", "nh", "mime", "name", "by"] { e.removeValue(forKey: k) }
      if tool == "image", var a = e["attachment"]?.object {
        if a["file_name"] == nil { a["file_name"] = entry["name"].string.map { .str($0) } ?? "picture" }
        if a["media_type"] == nil { a["media_type"] = entry["mime"].string.map { .str($0) } ?? "image/png" }
        if a["width"] == nil, let w = entry["nw"].int, w > 0 { a["width"] = .n(w) }
        if a["height"] == nil, let h = entry["nh"].int, h > 0 { a["height"] = .n(h) }
        e["attachment"] = .obj(a)
      }
    } else if tool == "image" {
      let a = e["attachment"] ?? .null
      if let w = a["width"].double { e["nw"] = .num(w) }
      if let h = a["height"].double { e["nh"] = .num(h) }
      if let m = a["media_type"].string { e["mime"] = .str(m) }
      if let n = a["file_name"].string { e["name"] = .str(n) }
    }
    return .obj(e)
  }
  /** A board item's body between the model and the wire. A body that is no board item is returned as it is. */
  static func boardItem(_ content: JV, toWire: Bool) -> JV {
    guard var c = content.object else { return content }
    switch c["content_type"]?.string {
    case "strokes":
      // (a piece that continues a stroke is not a shape of 10.5: it is not written)
      let list = (c["strokes"]?.array ?? []).filter { !toWire || $0["continues"].isNull }
      c["strokes"] = .arr(list.map { boardShape($0, toWire: toWire) })
    case "erase", "send_away", "move":
      let (from, to) = toWire ? ("stroke_ids", "shape_ids") : ("shape_ids", "stroke_ids")
      if let ids = c.removeValue(forKey: from)?.array { c[to] = .arr(ids.map { $0.string.map { .str(shapeId($0, toWire: toWire)) } ?? $0 }) }
      if let off = c["offset"]?.array { c["offset"] = .arr(off.map { q($0, toWire: toWire) }) }
    default: break
    }
    return .obj(c)
  }
  /**
   * A snapshot file of 10.8 (`{ v: 3, shapes, frontier, gone, moved }`, decompressed) as `CanvasState.load` takes
   * it: the shapes in the model's form with `id` and `by`, the frontier and the ids by hex. nil for a `v` this
   * version does not read (above 3: needs a newer Trommi) or a file that is no such object.
   */
  static func boardSnapshot(_ file: JV) -> JV? {
    guard file.object != nil, let v = file["v"].int, v == 3 else { return nil }
    let shapes: [JV] = (file["shapes"].array ?? []).compactMap { e in
      guard let id = e["id"].string.map({ shapeId($0, toWire: false) }) else { return nil }
      return boardShape(renameFiles(e, toWire: false, depth: 0), toWire: false).with("id", .str(id)).with("by", .str(String(id.prefix { $0 != "/" })))
    }
    var frontier = [String: JV]()
    for (writer, head) in file["frontier"].object ?? [:] {
      guard let w = try? unb64u(writer), w.count == 32, let seq = head[0].int else { continue }
      frontier[hex(w)] = .arr([.n(seq), (head[1].string.flatMap { try? unb64u($0) }).map { .str(hex($0)) } ?? .null])
    }
    let gone = (file["gone"].array ?? []).compactMap { $0.string.map { JV.str(shapeId($0, toWire: false)) } }
    let moved: [JV] = (file["moved"].array ?? []).compactMap { m in
      guard let id = m[0].string else { return nil }
      return .arr([.str(shapeId(id, toWire: false)), q(m[1], toWire: false), q(m[2], toWire: false)])
    }
    return .obj(["v": 3, "shapes": .arr(shapes), "frontier": .obj(frontier), "gone": .arr(gone), "moved": .arr(moved)])
  }


}
