// RoomArchived.swift: only active sessions are loaded (spec/hub-api.md point 46). A catch-up and the Desk read with
// `active=1`: an archived session's envelopes are left out (its Commits still come, so the core's group state stays
// whole). The session is loaded when its page is opened, in one pass over its group alone (`GET /v1/changes?group=`),
// each envelope taken as the next of its sender's chain (as a gap's links are). A read back reads the live sessions
// alone too: the archived sessions this device opened are loaded again after it. The web client does the same
// (app/web/core/client.ts `loadArchived`).
import Foundation

extension Room {
  /** The sessions the hub holds as archived that this device still holds as live (archived while it was away): held
   *  as archived here too, so that they are loaded only when one is opened. One small request; a failure is upkeep. */
  func learnArchived(_ change: inout Change) async {
    guard let listed = try? await noted({ try await hub.groups(archived: true) }) else { return }
    var any = false
    for g in listed where g["live"] as? Bool == false {
      guard let id = (g["group_id"] as? String).flatMap({ try? unb64u($0) }),
            let held = groups.values.first(where: { $0.group == id && $0.session != nil }), !held.archived else { continue }
      if (try? await onCore({ try $0.archive(group: id) })) != nil { any = true }
    }
    if any { try? await refreshGroups(&change) }
  }

  /**
   * An archived session, loaded when it is opened: its envelopes, which no catch-up brought, through the core and
   * onto the board. Once per run; a live session has nothing to load. Returns how many envelopes were taken.
   */
  @discardableResult public func loadArchived(_ sessionId: String) async throws -> Int {
    guard let g = groups.values.first(where: { $0.session.map { hex($0.session) } == sessionId }), g.archived,
          !archivedLoaded.contains(sessionId) else { return 0 }
    archivedLoaded.insert(sessionId)
    var report = SyncReport(), change = Change(), taken = 0
    do {
      var after: UInt64 = 0
      for _ in 0..<1_000 {
        let page = try await noted { try await hub.changes(after: after, group: g.group) }
        for j in page["items"] as? [JSON] ?? [] where j["kind"] as? String == "envelope" {
          guard let n = Wire.uint(j["change"]), let bytes = (j["envelope"] as? String).flatMap({ try? unb64u($0) }) else { continue }
          let void = j["void_code"] as? String
          let e = try? await onCore { device -> ReceivedEnvelope in
            var e = try device.receiveEnvelope(bytes, change: n, ordered: true, voidCode: void, nowMs: nowMs())
            // (one this device holds already, taken while the session was live: read again for the board)
            if e.outcome == .refused, e.code == "replay" { e = try device.receiveEnvelope(bytes, change: n, ordered: false, voidCode: void, nowMs: nowMs()) }
            return e
          }
          if let e, e.outcome != .refused { apply(e, report: &report, &change); taken += 1 }
        }
        guard page["more"] as? Bool == true, let next = Wire.uint(page["change"]), next > after else { break }
        after = next
      }
    } catch { archivedLoaded.remove(sessionId); throw error }
    var opened = (record.archivedOpened ?? []).filter { $0 != sessionId }
    opened.append(sessionId)
    record.archivedOpened = Array(opened.suffix(20))
    try? store.save(record)
    change.sessions.insert(sessionId)
    board.project(&change)
    emit(change)
    return taken
  }

  /** After a read back: the archived sessions this device opened, loaded again. */
  func reloadArchived(_ report: inout SyncReport, _ change: inout Change) async {
    for sid in record.archivedOpened ?? [] {
      archivedLoaded.remove(sid)
      _ = try? await loadArchived(sid)
    }
  }
}
