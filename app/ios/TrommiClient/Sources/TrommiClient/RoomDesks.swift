// RoomDesks.swift: deleting a desk with everything on it (client/src/desk_delete.rs), as the web's client.ts
// `deskDeleteStep` and desks.mjs `deleteDesk`. The client core plans each step from the board and what this device read
// of the desk's Scribble Board; this device does the step and asks again, until the desk is gone:
// mark the desk (`deleting`, so that any human device finishes a delete broken off), shred its open cards, remove the
// agents that sit on no other desk, archive its sessions, erase its board, delete its files at the hub, write an empty
// snapshot, take the desk's register away. This app writes no board snapshots (the web does, RoomActions.swift
// loadCanvas reads them): the erased board's items stay sealed at the hub until a snapshot covers them, or are dropped
// with the rest after 30 days.
import Foundation

/** What a desk holds, counted by the client core (`desk_contents`): the numbers Settings → Desks shows and the
 *  confirm names. `board`, `pictures`, `notes`: nil while the desk's board is not read. */
public struct DeskCounts: Equatable {
  public var sessions = 0, helpers = 0, archived = 0, openCards = 0, doneCards = 0, agents = 0, files = 0, artifacts = 0, goals = 0
  public var board: Int?, pictures: Int?, notes: Int?
  public init() {}
  init(_ j: JV) {
    func n(_ k: String) -> Int { j[k].double.map { Int($0) } ?? 0 }
    sessions = n("sessions"); helpers = n("helpers"); archived = n("archived"); openCards = n("open_cards"); doneCards = n("done_cards")
    agents = n("agents"); files = n("files"); artifacts = n("artifacts"); goals = n("goals")
    board = j["board"].double.map { Int($0) }; pictures = j["pictures"].double.map { Int($0) }; notes = j["notes"].double.map { Int($0) }
  }
}

extension Board {
  /** The desks a delete was begun on, by any device: finished on start (`resumeDeskDeletes` in the app). */
  public func desksDeleting() -> [String] {
    guard myRole == "human", let t = try? engine.desksDeleting(), let j = JV.parse(Bytes(t.utf8)) else { return [] }
    return j.array?.compactMap(\.string) ?? []
  }
  /** What desk `id` holds, counted; with its board when it is read (`canvas`). */
  public func deskContents(_ id: String, canvas: CanvasState?) -> DeskCounts {
    let b = canvas.map { String(decoding: Room.boardKnown($0, snapshotEmpty: false).encoded(), as: UTF8.self) } ?? ""
    guard let t = try? engine.deskContents(deskId: id, board: b), let j = JV.parse(Bytes(t.utf8)) else { return DeskCounts() }
    return DeskCounts(j)
  }
}

extension Room {
  /** The desk's board as the core asks for it: its shapes, its pictures' files, its notes. */
  nonisolated static func boardKnown(_ st: CanvasState, snapshotEmpty: Bool) -> JV {
    let shapes = st.shapes.values
    let files = Set(shapes.filter { $0.tool == "image" }.compactMap { $0.attachment?["attachment_id"].string }.filter { $0.count == 32 })
    return .obj(["shapes": .arr(shapes.map { .str($0.id) }), "files": .arr(files.sorted().map { .str($0) }),
                 "notes": .n(UInt64(shapes.filter { $0.tool == "sticky" || $0.tool == "text" }.count)), "snapshot_empty": .bool(snapshotEmpty)])
  }

  /**
   * Deletes desk `id` with everything on it, or finishes a delete begun before (on any device). `progress` hears how
   * far it is (done of total) after every step. Throws when a step fails or never shows done; the next start goes on.
   */
  public func deleteDesk(_ id: String, progress: @escaping @MainActor (Int, Int) -> Void = { _, _ in }) async throws {
    var known: [String: JV] = ["files_gone": .arr([])]
    var gone = [String](), canvas: CanvasState? = nil, began = false, last = "", same = 0
    let timeline = deskBoard(id)
    for _ in 0..<400 {
      known["files_gone"] = .arr(gone.map { .str($0) })
      let text = try board.engine.deskDeletePlan(deskId: id, known: String(decoding: JV.obj(known).encoded(), as: UTF8.self), by: deviceIdHex, nowMs: nowMs())
      guard let p = JV.parse(Bytes(text.utf8)), let step = p["step"].string else { throw TrommiError("bad-format", "the delete's plan does not read") }
      let done = Int(p["done"].double ?? 0), total = Int(p["total"].double ?? 1)
      progress(done, total)
      if step == "done" || (step == "refused" && p["code"].string == "no-desk" && began) { progress(total, total); return }
      if step == "refused" { throw TrommiError(p["code"].string ?? "refused", p["code"].string == "last-desk" ? "Your only desk stays." : "No such desk.") }
      began = true
      let ids = { (k: String) in p[k].array?.compactMap(\.string) ?? [] }
      switch step {
      case "mark":
        guard let reg = p["register"].string else { throw TrommiError("bad-format", "no register") }
        try await setRegisters([reg: p["value"]])
      case "shred":
        for c in ids("object_ids") {
          do { try await shred(cardId: c) } catch { if board.cards[c]?.objectState == "open" { throw error } }
        }
      case "remove_devices":
        try await removeDevices(ids("device_ids"))
      case "archive":
        for s in p["sessions"].array ?? [] {
          guard let key = s["key"].string else { continue }
          try await editSession(key, ["archived": true])
          // (the hub takes nothing more for the group, and neither does this device: as the web's client.ts archive)
          if let g = s["group_id"].string, let gid = try? unhex(g) {
            _ = try? await noted { try await hub.archiveGroup(gid) }
            _ = try? await onCore { try $0.archive(group: gid) }
          }
        }
      case "load_board":
        canvas = try await loadCanvas(timeline)
      case "erase_board":
        let all = ids("shape_ids")
        for at in stride(from: 0, to: all.count, by: 2000) {
          try await sendCanvas(timeline, .obj(["content_type": "erase", "stroke_ids": .arr(all[at..<min(all.count, at + 2000)].map { .str($0) })]))
        }
        canvas = try await loadCanvas(timeline)
      case "delete_files":
        for f in ids("file_ids") {
          do { try await hub.deleteFile(try unhex(f)) } catch let e as TrommiError where e.code == "not-found" {}
          gone.append(f)
        }
      case "write_snapshot":
        // (this app writes no snapshot: the board is erased; its items go when a snapshot covers them, or in 30 days)
        canvas = canvas ?? (try await loadCanvas(timeline))
      case "remove_desk":
        guard let reg = p["register"].string else { throw TrommiError("bad-format", "no register") }
        try await setRegisters([reg: .null])
      default:
        throw TrommiError("needs-update", "a step of this delete is new to this app: \(step)")
      }
      if let c = canvas { known["board"] = Room.boardKnown(c, snapshotEmpty: step == "write_snapshot" || known["board"]?["snapshot_empty"].bool == true) }
      // (a step the board does not show done yet: the hub's answer is on its way; one that never does is given up)
      let key = "\(step):\(done)"
      same = key == last ? same + 1 : 0
      last = key
      if same >= 3 { try? await Task.sleep(nanoseconds: UInt64(600_000_000 * same)) }
      if same >= 8 { throw TrommiError("stuck", "the step \(step) does not finish") }
    }
    throw TrommiError("stuck", "the delete did not end")
  }
}
