// Usage.swift: an agent's account and usage (TrommiClient/Usage.swift reads them): one mark of two rings beside a
// session's More button, and the plan, the e-mail and one row per window on the agent's card in Settings · Agents.
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
/** One meter's ring: a faint track and, from the top clockwise, the part used (urgent from 80 %). */
private struct MeterRing: View {
  let used: Int
  let width: CGFloat
  var body: some View {
    ZStack {
      Circle().stroke(Ink.fg.opacity(0.12), lineWidth: width)
      Circle().trim(from: 0, to: CGFloat(used) / 100)
        .stroke(hot(used) ? Ink.urgHigh : Ink.accent, style: StrokeStyle(lineWidth: width, lineCap: .round))
        .rotationEffect(.degrees(-90))
    }
  }
}
/** Beside a session's More button (session.mjs sessionWho, ui.mjs usageMark): one mark of two rings, one inside the
 *  other (as the watch's activity rings): the week outside, the 5-hour window inside; the whole in words for VoiceOver. */
struct UsageMark: View {
  let acc: UsageAccount
  var size: CGFloat = 30
  var body: some View {
    let five = acc.meters.first { $0.id == "five_hour" }, week = acc.meters.first { $0.id == "weekly" }
    let w = size * 0.115
    if !acc.meters.isEmpty {
      ZStack {
        if let week { MeterRing(used: week.used, width: w).padding(w / 2) }
        if let five { MeterRing(used: five.used, width: w).padding(week == nil ? w / 2 : w * 2.05) }
      }
      .frame(width: size, height: size)
      .accessibilityElement(children: .ignore)
      .accessibilityLabel(acc.meters.map { "\($0.long): \($0.used) percent used" }.joined(separator: ", "))
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
