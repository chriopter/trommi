import SwiftUI
import TrommiCoreRust

/**
 * The page Wakeups of the team in view (`ViewModel.wakeups`, the core's WakeupsView): when the team's agents start
 * again on their own, what they will do then, how often, who set it, and how the last run went. Cancel and Other time
 * write the person's word (`wakeup_cancel`, `wakeup_move`); the agent's connector applies it. The core decides every
 * word and the order; this only draws.
 */
struct WakeupsScreen: View {
  @EnvironmentObject var shell: AppShell
  @State private var cancelling: CoreView.WakeupRowView?
  @State private var moving: CoreView.WakeupRowView?
  @State private var when = Date()
  var body: some View {
    ScrollView {
      LazyVStack(alignment: .leading, spacing: 12) {
        if let w = shell.view.wakeups {
          VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 6) {
              PenMark("sketch:desk", color: Ink.muted).frame(width: 16, height: 16)
              Text(w.team.en).font(Face.text(14, .semibold)).foregroundStyle(Ink.muted)
            }
            .padding(.top, 10)
            HStack(alignment: .firstTextBaseline, spacing: 8) {
              PageHeading(text: w.title.en)
              Text("(\(w.count))").font(Face.display(26, .bold)).foregroundStyle(Ink.muted).accessibilityLabel(w.summary.en)
            }
            Text(w.lead.en).font(Face.text(15)).foregroundStyle(Ink.muted)
          }
          if !w.rows.isEmpty {
            VStack(spacing: 0) {
              ForEach(Array(w.rows.enumerated()), id: \.element.id) { i, row in
                if i > 0 { Divider().overlay(Ink.line) }
                wakeup(row, w.words)
              }
            }
            .background(RoundedRectangle(cornerRadius: 18, style: .continuous).fill(Ink.surface))
            .overlay(RoundedRectangle(cornerRadius: 18, style: .continuous).stroke(Ink.line, lineWidth: 1))
          }
          if let none = w.none { Text(none.en).font(Face.text(15)).foregroundStyle(Ink.muted).padding(.vertical, 20).frame(maxWidth: .infinity).multilineTextAlignment(.center).accessibilityIdentifier("wakeups-none") }
        } else {
          Loading()
        }
      }
      .padding(.horizontal, 16).padding(.bottom, 120)
      .frame(maxWidth: 760).frame(maxWidth: .infinity)
    }
    .background(Ink.bg)
    .pushedPills(center: { EmptyView() }, trailing: { EmptyView() })
    .confirmationDialog(shell.view.wakeups?.words.cancelAsk.en ?? "", isPresented: Binding(get: { cancelling != nil }, set: { if !$0 { cancelling = nil } }), titleVisibility: .visible) {
      if let row = cancelling { Button(shell.view.wakeups?.words.cancel.en ?? "Cancel", role: .destructive) { shell.act(["do": "wakeup_cancel", "session_id": row.sessionId, "wakeup_id": row.id]); cancelling = nil }.accessibilityIdentifier("wakeup-cancel-confirm") }
    }
    .sheet(item: $moving) { row in
      NavigationStack {
        DatePicker(shell.view.wakeups?.words.moveTo.en ?? "Other time", selection: $when, in: Date().addingTimeInterval(60)...)
          .datePickerStyle(.graphical).padding(16)
          .toolbar {
            ToolbarItem(placement: .cancellationAction) { Button(shell.view.wakeups?.words.back.en ?? "Back") { moving = nil } }
            ToolbarItem(placement: .confirmationAction) {
              Button(shell.view.wakeups?.words.save.en ?? "Save") {
                shell.act(["do": "wakeup_move", "session_id": row.sessionId, "wakeup_id": row.id, "at_ms": UInt64(when.timeIntervalSince1970 * 1000)])
                moving = nil
              }
            }
          }
      }
      .presentationDetents([.medium, .large])
    }
  }
  /** One wakeup: who set it, when it runs next and how often, its note, how the last run went; Other time and Cancel
   *  (none while his word waits for the agent's computer). */
  private func wakeup(_ row: CoreView.WakeupRowView, _ words: CoreView.WakeupWordsView) -> some View {
    HStack(alignment: .top, spacing: 12) {
      AgentMark(mark: row.mark, size: 28, hue: Pen.hueFor(id: row.sessionId, mark: row.mark)).frame(width: 32, height: 32)
      VStack(alignment: .leading, spacing: 4) {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
          Text(row.next.en).font(Face.text(16, .semibold)).foregroundStyle(Ink.fg)
          Text(row.howOften.en).font(Face.text(11, .semibold)).foregroundStyle(row.recurring ? Ink.accent : Ink.muted)
            .padding(.horizontal, 7).padding(.vertical, 1).background(Capsule().fill(row.recurring ? Ink.accentSoft : Ink.sunken))
        }
        Text(row.note).font(Face.text(14)).foregroundStyle(Ink.fg).multilineTextAlignment(.leading)
        Text([row.who.en, row.helper, row.last?.en].compactMap { $0 }.joined(separator: " · ")).font(Face.text(12)).foregroundStyle(Ink.faint)
        if let pending = row.pending { Text(pending.en).font(Face.text(12, .semibold)).foregroundStyle(Ink.accent) }
        if row.pending == nil {
          HStack(spacing: 8) {
            Button(words.moveTo.en) { when = Date(timeIntervalSince1970: Double(row.atMs) / 1000); moving = row }
              .font(Face.text(13, .semibold)).foregroundStyle(Ink.fg)
              .padding(.horizontal, 12).padding(.vertical, 6).background(Capsule().stroke(Ink.lineStrong, lineWidth: 1))
              .accessibilityIdentifier("wakeup-move")
            Button(words.cancel.en) { cancelling = row }
              .font(Face.text(13, .semibold)).foregroundStyle(Ink.deny)
              .padding(.horizontal, 12).padding(.vertical, 6).background(Capsule().stroke(Ink.lineStrong, lineWidth: 1))
              .accessibilityIdentifier("wakeup-cancel")
          }
          .buttonStyle(.plain).padding(.top, 4)
        }
      }
      .frame(maxWidth: .infinity, alignment: .leading)
    }
    .padding(14)
    .accessibilityIdentifier("wakeup-row")
  }
}

extension CoreView.WakeupRowView: Identifiable {}
