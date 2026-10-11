// Usage.swift: an agent's account and usage (TrommiClient/Usage.swift reads them): two thin bars under a session's
// name, and the plan, the e-mail and one row per window on the agent's card in Settings · Agents.
// The track is the ink at 12 %, the fill the accent; from 80 % on, fill and percent turn urgent.
import SwiftUI
import TrommiClient

/** The account an agent's session runs on: its process's, else (a helper that is a device of its own) its main's. */
@MainActor func usageOf(_ model: BoardModel, _ a: Agent) -> UsageAccount? {
  guard let b = model.board else { return nil }
  return TrommiClient.usageOf(b, agentDeviceId: a.agentDeviceId, now: nowMs())
    ?? a.parent.flatMap { model.agent($0) }.flatMap { TrommiClient.usageOf(b, agentDeviceId: $0.agentDeviceId, now: nowMs()) }
}
private func hot(_ used: Int) -> Bool { used >= 80 }

struct UsageBar: View {
  let used: Int
  var height: CGFloat = 5
  var body: some View {
    GeometryReader { g in
      ZStack(alignment: .leading) {
        Capsule().fill(Ink.fg.opacity(0.12))
        Capsule().fill(hot(used) ? Ink.urgHigh : Ink.accent).frame(width: g.size.width * CGFloat(used) / 100)
      }
    }.frame(height: height)
  }
}
/** Compact, under a session's name pill: each meter its word, a thin bar, its percent, in a glass pill. */
struct UsageStrip: View {
  let acc: UsageAccount
  var body: some View {
    if !acc.meters.isEmpty {
      HStack(spacing: 12) {
        ForEach(acc.meters) { m in
          HStack(spacing: 5) {
            Text(m.label).font(Face.text(12)).foregroundStyle(Ink.muted)
            UsageBar(used: m.used).frame(width: 46)
            Text("\(m.used) %").font(Face.text(12, .bold)).foregroundStyle(hot(m.used) ? Ink.urgHigh : Ink.fg)
          }
          .accessibilityElement(children: .ignore)
          .accessibilityLabel("\(m.long): \(m.used) percent used")
        }
      }
      .padding(.horizontal, 12).padding(.vertical, 6).glass(Capsule(), interactive: false)
    }
  }
}
/** Fuller, on an agent's card in Settings · Agents: the plan and the e-mail, one row per window, Codex's credits. */
struct UsageCard: View {
  let acc: UsageAccount
  var body: some View {
    let now = nowMs()
    VStack(alignment: .leading, spacing: 9) {
      HStack(spacing: 8) {
        if !acc.plan.isEmpty {
          Text(acc.planTag).font(Face.text(12, .bold)).padding(.horizontal, 7).padding(.vertical, 3)
            .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Ink.fg, lineWidth: 1.3))
        }
        if !acc.email.isEmpty { Text(acc.email).font(Face.text(14)).foregroundStyle(Ink.muted).lineLimit(1).truncationMode(.middle) }
      }
      ForEach(acc.meters) { m in
        VStack(alignment: .leading, spacing: 4) {
          HStack {
            Text(m.long).font(Face.text(14)).foregroundStyle(Ink.muted)
            Spacer()
            Text("\(m.used) %").font(Face.text(14, .bold)).foregroundStyle(hot(m.used) ? Ink.urgHigh : Ink.fg)
          }
          UsageBar(used: m.used, height: 7)
          if let at = m.resetsAt { Text("resets \(usageReset(at, now: now))").font(Face.text(12)).foregroundStyle(Ink.muted) }
        }
      }
      let facts = [acc.creditsText, acc.fullResetsText].compactMap { $0 }
      if !facts.isEmpty {
        HStack(spacing: 8) {
          ForEach(facts, id: \.self) { f in
            Text(f).font(Face.text(12, .semibold)).padding(.horizontal, 7).padding(.vertical, 3).background(RoundedRectangle(cornerRadius: 6).fill(Ink.fg.opacity(0.07)))
          }
        }
      }
    }
    .padding(.horizontal, 16).padding(.vertical, 12).frame(maxWidth: .infinity, alignment: .leading)
  }
}
