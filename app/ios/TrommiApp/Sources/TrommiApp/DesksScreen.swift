// DesksScreen.swift: Settings → Desks, as the web's /settings/desks (desks.mjs): a card per desk with its Scribble
// Board small and what it holds, counted by the client core (`deskContents`), and a bin that deletes the desk with
// everything on it. The confirm names exactly what goes and is held to delete; while it runs the card shows how far
// it is. A delete broken off is finished on the next start (BoardModel.resumeDeskDeletes).
import SwiftUI
import TrommiClient

struct DesksPage: View {
  @EnvironmentObject var model: BoardModel
  @Environment(\.colorScheme) private var scheme
  @State private var boards: [String: CanvasState] = [:]
  @State private var asking: DeskDesc?
  var body: some View {
    let _ = model.version
    let desks = model.desk?.desks ?? []
    let counts = Dictionary(uniqueKeysWithValues: desks.map { d in (d.id, model.board?.deskContents(d.id, canvas: boards[d.id]) ?? DeskCounts()) })
    let sessions = counts.values.reduce(0) { $0 + $1.sessions }, helpers = counts.values.reduce(0) { $0 + $1.helpers }
    SettingsPage(title: "Desks") {
      Text("\(desks.count) \(desks.count == 1 ? "desk" : "desks") · \(sessions) \(sessions == 1 ? "session" : "sessions"), \(helpers) \(helpers == 1 ? "helper" : "helpers")")
        .font(Face.text(15)).foregroundStyle(Ink.muted)
      ForEach(desks) { d in card(d, counts[d.id] ?? DeskCounts(), last: desks.count < 2) }
      Text("Each desk with its Scribble Board small. The bin deletes a desk with everything on it; you see what goes before.")
        .font(Face.text(13)).foregroundStyle(Ink.muted).fixedSize(horizontal: false, vertical: true)
    }
    .sheet(item: $asking) { d in DeskDeleteSheet(desk: d, counts: counts[d.id] ?? DeskCounts()).environmentObject(model) }
    .task(id: desks.map(\.id)) { await load(desks) }
  }

  private func card(_ d: DeskDesc, _ c: DeskCounts, last: Bool) -> some View {
    let going = model.deskDeleting[d.id]
    return VStack(alignment: .leading, spacing: 0) {
      BoardThumb(shapes: boards[d.id].map { Array($0.shapes.values) } ?? [], dark: scheme == .dark)
        .frame(height: 120).frame(maxWidth: .infinity)
        .background(scheme == .dark ? Color(hex: 0x111715) : Color(hex: 0xfffdf6))
      Rectangle().fill(Ink.line).frame(height: 1)
      HStack(spacing: 10) {
        Sketch("desk").frame(width: 24, height: 24)
        Text(d.name).font(Face.display(17, .bold)).lineLimit(1)
        Spacer(minLength: 4)
        Button { asking = d } label: { Sketch("bin", color: last ? Ink.faint : Ink.muted).frame(width: 20, height: 20).frame(width: 40, height: 40) }
          .buttonStyle(.plain).disabled(last || going != nil || model.demo)
          .accessibilityLabel("Delete \(d.name)")
      }
      .padding(.leading, 16).padding(.trailing, 8).padding(.top, 8)
      LazyVGrid(columns: Array(repeating: GridItem(.flexible(), alignment: .leading), count: 3), alignment: .leading, spacing: 12) {
        number(c.sessions, "sessions"); number(c.helpers, "helpers"); number(c.openCards, "open cards")
        number(c.doneCards, "done"); number(c.board, "on the board"); number(c.files, "files")
      }
      .padding(.horizontal, 16).padding(.top, 8).padding(.bottom, going == nil ? 16 : 8)
      .opacity(going == nil ? 1 : 0.45)
      if let g = going {
        VStack(alignment: .leading, spacing: 6) {
          ProgressView(value: g).tint(Ink.deny)
          Text("Deleting…").font(Face.text(12)).foregroundStyle(Ink.muted)
        }
        .padding(.horizontal, 16).padding(.bottom, 16)
      }
    }
    .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(Ink.surface))
    .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(Ink.fg.opacity(0.7), lineWidth: 1.5))
    .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
  }
  private func number(_ k: Int?, _ word: String) -> some View {
    VStack(alignment: .leading, spacing: 2) {
      Text(k.map(String.init) ?? "…").font(Face.text(22, .bold)).monospacedDigit().foregroundStyle(Ink.fg)
      Text(word).font(Face.text(12)).foregroundStyle(Ink.muted)
    }
    .accessibilityElement(children: .combine)
  }
  /** Every desk's board, read once (the demo's from its fixture). */
  private func load(_ desks: [DeskDesc]) async {
    for d in desks where boards[d.id] == nil {
      if model.demo, let b = model.desk?.board { boards[d.id] = (try? Room.demoCanvas(b, deskBoard(d.id))) ?? CanvasState(); continue }
      guard let room = model.room, let st = try? await room.loadCanvas(deskBoard(d.id)), !Task.isCancelled else { continue }
      boards[d.id] = st
    }
  }
}

/** The confirm: every number the core counts that is not nothing, then Hold to Delete; while it runs, how far it is. */
struct DeskDeleteSheet: View {
  @EnvironmentObject var model: BoardModel
  @Environment(\.dismiss) private var dismiss
  let desk: DeskDesc
  let counts: DeskCounts
  @State private var holding = false
  var body: some View {
    let going = model.deskDeleting[desk.id]
    VStack(alignment: .leading, spacing: 14) {
      Text("Delete \(desk.name)?").font(Face.display(22, .bold))
      if let g = going {
        ProgressView(value: g).tint(Ink.deny)
        Text("Deleting… \(Int((g * 100).rounded()))%").font(Face.text(14)).foregroundStyle(Ink.muted)
      } else {
        let rows = lines
        Text(rows.isEmpty ? "Nothing stands on this desk: only its name goes." : "This goes for good, on every device:").font(Face.text(16, .medium))
        if !rows.isEmpty {
          VStack(spacing: 0) {
            ForEach(Array(rows.enumerated()), id: \.offset) { i, r in
              if i > 0 { Rectangle().fill(Ink.line).frame(height: 1) }
              HStack(alignment: .firstTextBaseline, spacing: 12) {
                Text("\(r.n)").font(Face.text(16, .bold)).monospacedDigit().frame(minWidth: 34, alignment: .trailing)
                (Text(r.word).font(Face.text(15)) + Text(r.rest.isEmpty ? "" : "  \(r.rest)").font(Face.text(13)).foregroundColor(Ink.muted))
                Spacer(minLength: 0)
              }
              .padding(.horizontal, 14).padding(.vertical, 9)
            }
          }
          .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Ink.lineStrong))
        }
        Text("The server drops the encrypted data within 30 days. Your agents' computers keep nothing of it they can still open. This cannot be undone.")
          .font(Face.text(13)).foregroundStyle(Ink.muted).fixedSize(horizontal: false, vertical: true)
        HStack(spacing: 10) {
          Button { dismiss() } label: { Text("Cancel").font(Face.text(16, .semibold)).frame(maxWidth: .infinity, minHeight: 46) }
            .buttonStyle(.plain).overlay(Capsule().strokeBorder(Ink.lineStrong, lineWidth: 1.5))
          holdButton
        }
      }
    }
    .padding(20)
    .presentationDetents([.medium, .large])
    .onChange(of: model.deskDeleting[desk.id] == nil) { gone in if gone && model.deskGone.contains(desk.id) { dismiss() } }
  }
  /** Held 1.6 s: the delete runs. A shorter press does nothing. */
  private var holdButton: some View {
    ZStack(alignment: .leading) {
      Capsule().strokeBorder(Ink.deny, lineWidth: 1.5)
      GeometryReader { g in Capsule().fill(Ink.deny).frame(width: holding ? g.size.width : 0).animation(holding ? .linear(duration: 1.6) : .easeOut(duration: 0.15), value: holding) }
      Text("Hold to Delete").font(Face.text(16, .semibold)).foregroundStyle(holding ? Ink.surface : Ink.deny).frame(maxWidth: .infinity)
    }
    .frame(maxWidth: .infinity, minHeight: 46).clipShape(Capsule())
    .onLongPressGesture(minimumDuration: 1.6, pressing: { holding = $0 }) { holding = false; model.deleteDesk(desk.id, name: desk.name) }
    .accessibilityElement(children: .ignore).accessibilityLabel("Hold to Delete").accessibilityAddTraits(.isButton)
    .accessibilityAction { model.deleteDesk(desk.id, name: desk.name) }
  }
  private var lines: [(n: Int, word: String, rest: String)] {
    let c = counts
    func w(_ n: Int, _ one: String, _ many: String) -> String { n == 1 ? one : many }
    let all: [(Int, String, String)] = [
      (c.sessions, w(c.sessions, "session", "sessions"), "archived"),
      (c.helpers, w(c.helpers, "helper", "helpers"), "archived"),
      (c.archived, w(c.archived, "archived session", "archived sessions"), ""),
      (c.agents, w(c.agents, "agent removed from your account", "agents removed from your account"), "their keys stop working"),
      (c.openCards, w(c.openCards, "open card", "open cards"), "shredded"),
      (c.doneCards, w(c.doneCards, "done card", "done cards"), ""),
      (c.board ?? 0, w(c.board ?? 0, "thing on the Scribble Board", "things on the Scribble Board"), ""),
      (c.files, w(c.files, "file", "files"), "deleted at once"),
      (c.goals, w(c.goals, "goal", "goals"), ""),
    ]
    return all.filter { $0.0 > 0 }.map { (n: $0.0, word: $0.1, rest: $0.2) }
  }
}

extension BoardModel {
  /** Deletes desk `id` with everything on it (Room.deleteDesk), its progress in `deskDeleting`. */
  func deleteDesk(_ id: String, name: String) {
    guard deskDeleting[id] == nil, let room = acting(), !demo else { return }
    deskDeleting[id] = 0
    Task { @MainActor in
      do {
        try await room.deleteDesk(id) { [weak self] done, total in self?.deskDeleting[id] = total > 0 ? Double(done) / Double(total) : 0 }
        deskGone.insert(id)
        deskDeleting[id] = nil
        say("Deleted", name)
      } catch {
        deskDeleting[id] = nil
        fail("Not deleted yet", error)
      }
    }
  }
  /** On start: every desk a delete was begun on (by any device) is finished. */
  func resumeDeskDeletes() {
    guard !demo, let room = room else { return }
    for id in room.board.desksDeleting() where deskDeleting[id] == nil {
      let name = desk?.desks.first { $0.id == id }?.name ?? "A desk"
      say("Finishing a delete", name)
      deleteDesk(id, name: name)
    }
  }
}
