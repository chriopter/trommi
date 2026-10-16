// Talk.swift: the Chat page's list, a card's own page, a session's conversation and the rows of a talk, drawn as the
// app drew them before the shared app core (ChatList.swift, CardScreen.swift, SessionScreen.swift, Rich.swift of that
// time): calm rows with the session's drawing in a round tinted field, the floating glass pills at the top, the
// message bubbles, the big answer tiles and the glass composer at the bottom. Drawing only: what shows and what it
// says come from the core's view model.
import SwiftUI
import TrommiClient
import UniformTypeIdentifiers
#if canImport(UIKit)
import UIKit
#endif

// ---- small drawing helpers of this file ----------------------------------------------------------------------------

/** "14:05": a time beside a message (the shell writes the time). */
func talkClock(_ ms: UInt64) -> String { let f = DateFormatter(); f.dateFormat = "HH:mm"; return f.string(from: Date(timeIntervalSince1970: Double(ms) / 1000)) }
/** "5 minutes ago": a time under a title. */
func talkAgo(_ ms: UInt64) -> String {
  guard ms > 0 else { return "" }
  let f = RelativeDateTimeFormatter(); f.locale = Locale(identifier: "en_GB"); f.unitsStyle = .full
  return f.localizedString(for: Date(timeIntervalSince1970: Double(ms) / 1000), relativeTo: Date())
}
/** "now", "4m", "3h", "4 Oct": a time beside a small chip. */
func talkAgoShort(_ ms: UInt64) -> String {
  let min = Int((Date().timeIntervalSince1970 * 1000 - Double(ms)) / 60000)
  if min < 1 { return "now" }
  if min < 60 { return "\(min)m" }
  if min < 1440 { return "\(min / 60)h" }
  let f = DateFormatter(); f.locale = Locale(identifier: "en_GB"); f.dateFormat = "d MMM"
  return f.string(from: Date(timeIntervalSince1970: Double(ms) / 1000))
}
/** A card's urgency (0 low … 3 critical) as the inks name it. */
func cardUrgency(_ u: UInt8) -> String { u >= 3 ? "critical" : u == 2 ? "high" : u == 0 ? "low" : "normal" }
/** Who speaks in a talk: the session's drawing, its name and colour. */
struct TalkWho { var mark: String; var name: String; var hue: Int }

// ---- the Chat list ---------------------------------------------------------------------------------------------------

/** The Chat page's list, calm as Messages': one row per session, its drawing in a round tinted field (the crown and a
 *  green dot while it works as small badges), its name and under it its small line; at the right only what needs him. */
struct ChatsScreen: View {
  @EnvironmentObject var shell: AppShell
  var body: some View {
    List {
      ForEach(shell.view.sidebar.sessions, id: \.id) { row in
        if let heading = row.heading {
          HStack(spacing: 8) {
            Text(heading.text.en).font(Face.text(13, .semibold, relativeTo: .footnote)).kerning(0.3).foregroundStyle(Ink.muted).lineLimit(1)
            if heading.kind == "away" { DashedRule().frame(height: 1) }
            Spacer(minLength: 0)
          }
          .padding(.top, 14).padding(.bottom, 4)
          .accessibilityElement(children: .combine).accessibilityAddTraits(.isHeader)
          .listRowInsets(EdgeInsets(top: 0, leading: 20, bottom: 0, trailing: 20))
          .listRowBackground(Color.clear).listRowSeparator(.hidden)
        }
        ChatRow(row: row)
      }
    }
    .listStyle(.plain)
    .scrollContentBackground(.hidden)
    .environment(\.defaultMinListRowHeight, 1)
    .contentMargins(.bottom, 100, for: .scrollContent)
    .background(Ink.bg)
    .topPills(leading: { PlacePill() }, trailing: { EmptyView() })
    .refreshable { shell.act(["do": "retry"]) }
  }
}

/** One session of the Chat list; a helper (depth 1) is a compact row on its main's quiet tone. */
struct ChatRow: View {
  @EnvironmentObject var shell: AppShell
  let row: CoreView.SessionRowView
  var body: some View {
    let small = row.depth > 0
    let side: CGFloat = small ? 36 : 50
    let hue = Pen.hueFor(id: row.slug.isEmpty ? row.id : row.slug, mark: row.mark)
    let working = row.folded?.working ?? row.working
    let open = row.folded?.open ?? row.open
    let stopped = (row.folded?.hand ?? row.hand) != nil
    Button { shell.focus(.session(sessionId: row.id)) } label: {
      HStack(spacing: 12) {
        AgentMark(mark: row.mark, size: side * 0.58, hue: hue)
          .frame(width: side, height: side)
          .background(Circle().fill(Tone.color(hue: hue, .wash)))
          .opacity(row.online ? 1 : 0.6)
          .overlay(alignment: .topLeading) { if row.crowned { PenMark("crown").frame(width: side * 0.4, height: side * 0.29).rotationEffect(.degrees(-14)).offset(x: -3, y: -4) } }
          .overlay(alignment: .bottomTrailing) { if working { Circle().fill(Ink.stDone).frame(width: 11, height: 11).overlay(Circle().stroke(Ink.bg, lineWidth: 2)).offset(x: -1, y: -1) } }
          .accessibilityHidden(true)
        VStack(alignment: .leading, spacing: 2) {
          Text(row.name.en).font(Face.text(small ? 16 : 17, .semibold)).foregroundStyle(row.online ? Ink.fg : Ink.muted).lineLimit(1)
          if let cap = row.cap {
            Text(cap.word.en).font(Face.text(15, relativeTo: .subheadline)).foregroundStyle(cap.kind == "unheard" || cap.kind == "cut" ? Ink.urgHigh : Ink.muted).lineLimit(1)
          }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        if stopped { PenMark("hand", color: Ink.surface, blocked: true).frame(width: 26, height: 26) }
        else if open > 0 {
          Text("\(open)").font(Face.text(13, .bold, relativeTo: .footnote)).foregroundStyle(Ink.bg).monospacedDigit()
            .padding(.horizontal, 6).frame(minWidth: 22, minHeight: 22).background(Capsule().fill(Ink.fg))
        }
      }
      .padding(.horizontal, small ? 8 : 0)
      .frame(minHeight: small ? 52 : 76)
      .background { if small { RoundedRectangle(cornerRadius: 14, style: .continuous).fill(Tone.color(hue: hue, .wash).opacity(0.55)) } }
      .opacity(row.hushed ? 0.6 : 1)
      .contentShape(Rectangle())
    }
    .buttonStyle(.plain)
    .accessibilityLabel([row.name.en, row.says?.en ?? row.cap?.word.en ?? ""].filter { !$0.isEmpty }.joined(separator: ". "))
    .accessibilityIdentifier("chat-session-" + row.id)
    .listRowInsets(EdgeInsets(top: 2, leading: small ? 12 : 20, bottom: 2, trailing: small ? 12 : 20))
    .listRowBackground(Color.clear)
    .listRowSeparatorTint(Ink.line)
    .listRowSeparator(small ? .hidden : .visible, edges: .bottom)
    .listRowSeparator(.hidden, edges: .top)
    .alignmentGuide(.listRowSeparatorLeading) { _ in side + 12 }
  }
}

// ---- a card's page ---------------------------------------------------------------------------------------------------

struct CardScreen: View {
  @EnvironmentObject var shell: AppShell
  let card: CoreView.CardView
  @State private var note = ""
  @State private var choices: Set<String> = []
  @State private var attachments: [String] = []
  /** Answered on this page just now: the page then offers the way on (`card.after`). */
  @State private var answeredHere = false
  private var urgency: String { cardUrgency(card.urgency) }
  private var who: TalkWho? { card.from.map { TalkWho(mark: $0.mark, name: $0.name, hue: Pen.hueFor(id: $0.sessionId, mark: $0.mark)) } }
  var body: some View {
    ScrollView {
      VStack(alignment: .leading, spacing: 18) {
        lead
        if !card.pictures.isEmpty {
          VStack(spacing: 10) {
            ForEach(card.pictures, id: \.fileId) { picture in
              NativePicture(id: picture.fileId).frame(maxWidth: .infinity, maxHeight: 360)
                .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
            }
          }
        }
        FileRows(ids: card.files)
        answers
        if card.answer.mode == "options" && !card.sections.isEmpty { sections }
        if let n = card.note { LinkNoteBox(note: n) }
        if answeredHere, let after = card.after { afterStep(after) }
        thread
      }
      .padding(.horizontal, 18).padding(.top, 8).padding(.bottom, 24)
      .frame(maxWidth: 760).frame(maxWidth: .infinity)
    }
    .scrollDismissesKeyboard(.interactively)
    .background(Ink.bg.ignoresSafeArea())
    .bottomBar {
      if card.kind != "request" {
        TalkComposer(placeholder: card.ask.en, text: $note, attachments: $attachments) {
          shell.act(["do": "write", "session_id": card.sessionId ?? "", "card_id": card.id, "text": note, "attachments": attachments]); note = ""; attachments = []
        }
      }
    }
    .pushedPills(extraTrailing: 54) { EmptyView() } trailing: {
      if card.kind != "request" {
        Button { shell.act(["do": "star", "card_id": card.id, "on": !card.starred]) } label: { StarMark(on: card.starred).frame(width: 22, height: 22).pillCircle() }
          .buttonStyle(.plain).accessibilityLabel(card.starSays.en).accessibilityAddTraits(card.starred ? .isSelected : [])
      }
      Menu {
        if let id = card.sessionId { Button { shell.focus(.session(sessionId: id)) } label: { Label("Open Session", systemImage: "bubble.left.and.bubble.right") } }
        Button { copyText(card.title + (card.text.isEmpty ? "" : "\n\n" + card.text)) } label: { Label("Copy", systemImage: "doc.on.doc") }
        if card.status == "open" && card.kind != "request" {
          Button { shell.act(["do": "snooze", "card_id": card.id]) } label: { Label("Later", systemImage: "zzz") }
          Button(role: .destructive) { shell.act(["do": "shred", "card_id": card.id, "note": note]) } label: { Label("Shred", systemImage: "trash") }
        }
      } label: { Image(systemName: "ellipsis").font(.system(size: 18, weight: .semibold)).foregroundStyle(Ink.fg).pillCircle() }
      .accessibilityLabel("More")
    }
    .onAppear { note = card.draft; choices = Set(card.options.filter(\.chosen).map(\.key)) }
    .onChange(of: card.status) { was, now in answeredHere = was == "open" && now != "open" }
    .onChange(of: note) { _, value in shell.act(["do": "draft", "place": card.id, "text": value]) }
  }

  // ---- the head: knock, work, who, title, when, the text ----

  @ViewBuilder private var lead: some View {
    VStack(alignment: .leading, spacing: 10) {
      if let knock = card.knock {
        HStack(spacing: 6) { Sketch("knock", color: Ink.surface).frame(width: 15, height: 15); Text(knock.en.uppercased()).font(Face.text(11, .bold)).kerning(1) }
          .foregroundStyle(Ink.surface).padding(.horizontal, 8).padding(.vertical, 4).background(RoundedRectangle(cornerRadius: 6).fill(Ink.urgency(urgency)))
      }
      if let work = card.work { CardWorkStrip(work: work) }
      if let from = card.from {
        Button { shell.focus(.session(sessionId: from.sessionId)) } label: {
          HStack(spacing: 6) {
            AgentMark(from: from, size: 20)
            ForEach(Array(from.path.dropLast().enumerated()), id: \.offset) { _, p in
              Text(p).font(Face.text(14, .medium)).foregroundStyle(Ink.muted).lineLimit(1)
              Text("›").font(Face.text(14)).foregroundStyle(Ink.faint)
            }
            Text(from.path.last ?? from.name).font(Face.text(14, .bold)).foregroundStyle(Ink.fg).lineLimit(1)
          }
          .frame(minHeight: 44, alignment: .leading).contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel("From \(from.path.joined(separator: ", "))")
      }
      Text(card.title).font(Face.display(30, .heavy)).foregroundStyle(Ink.fg).fixedSize(horizontal: false, vertical: true).textSelection(.enabled)
        .accessibilityIdentifier("card-title")
      HStack(spacing: 6) {
        Text(card.nr.en).font(Face.text(14, .medium)).foregroundStyle(Ink.muted)
        if card.askedMs > 0 { Text("·").foregroundStyle(Ink.faint); Text(talkAgo(card.askedMs)).font(Face.text(14)).foregroundStyle(Ink.muted) }
        if let place = card.place { Text("·").foregroundStyle(Ink.faint); Text(place.says.en).font(Face.text(14)).foregroundStyle(Ink.muted) }
        ForEach(Array(card.notes.enumerated()), id: \.offset) { _, n in Text("· \(n.en)").font(Face.text(14)).foregroundStyle(Ink.muted) }
      }
      .lineLimit(1)
      if let into = card.mergedInto {
        Button { shell.focus(.card(cardId: into.id)) } label: { Text("Nr. \(into.number): \(into.title)").font(Face.text(15, .medium)).foregroundStyle(Ink.accent).multilineTextAlignment(.leading) }.buttonStyle(.plain)
      }
      if !card.mergedFrom.isEmpty {
        VStack(alignment: .leading, spacing: 4) {
          ForEach(card.mergedFrom, id: \.id) { o in
            Button { shell.focus(.card(cardId: o.id)) } label: { Text("Nr. \(o.number) · \(o.title)").font(Face.text(14)).foregroundStyle(Ink.accent).lineLimit(1) }.buttonStyle(.plain)
          }
        }
      }
      if !card.text.isEmpty { Markdown(text: card.text) }
      if !card.html.isEmpty { InlineHTML(html: card.html).frame(minHeight: 240).clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous)) }
    }
  }

  // ---- the answers ----

  @ViewBuilder private var answers: some View {
    let a = card.answer
    VStack(alignment: .leading, spacing: 10) {
      switch a.mode {
      case "options":
        ForEach(card.options, id: \.key) { o in option(o) }
        if card.multiple {
          Button { answer(Array(choices)) } label: { Text("Send the answer").font(Face.text(17, .semibold)).frame(maxWidth: .infinity, minHeight: 54) }
            .buttonStyle(TileStyle(lead: true, urgency: urgency)).disabled(choices.isEmpty)
        }
        if card.kind == "decision" {
          HStack(spacing: 10) { Rectangle().fill(Ink.lineStrong).frame(height: 1); Text("OR").font(Face.text(13, .semibold)).kerning(1.4).foregroundStyle(Ink.muted); Rectangle().fill(Ink.lineStrong).frame(height: 1) }
            .padding(.vertical, 4)
          Button { shell.act(["do": "answer", "card_id": card.id, "choices": card.options.filter(\.recommended).map(\.key), "trusted": true, "note": note]) } label: { EmptyView() }
            .buttonStyle(SideWay(kind: .duck) { on in
              HStack(spacing: 14) {
                PenMark(doc: PenStore.doc("sketch:duck"), inks: PenInks(stroke: Ink.fg, duck: true, water: on ? Ink.duckInk : Ink.fg)).frame(width: 52, height: 44)
                Text("I don’t give a duck").font(Face.text(18, .semibold)); Spacer()
              }.padding(.horizontal, 16)
            })
            .accessibilityLabel("I don’t give a duck")
          otherWays
        }
      case "read":
        Button { shell.act(["do": "read", "card_id": card.id]) } label: {
          HStack(spacing: 10) { Sketch("tick", color: Ink.accentFg).frame(width: 22, height: 22); Text("Got it").font(Face.text(18, .semibold)) }.frame(maxWidth: .infinity, minHeight: 60)
        }.buttonStyle(TileStyle(lead: true, urgency: urgency))
        otherWays
      case "request":
        HStack(spacing: 8) {
          Button { verdict(false) } label: { Text("Deny").font(Face.text(17, .semibold)).frame(maxWidth: .infinity, minHeight: 58) }.buttonStyle(TileStyle(lead: false, urgency: urgency))
          Button { verdict(true) } label: { Text("Allow").font(Face.text(17, .semibold)).frame(maxWidth: .infinity, minHeight: 58) }.buttonStyle(TileStyle(lead: true, urgency: urgency))
        }
      case "said":
        stood(a.label?.en ?? "", a.detail?.en ?? "")
        ForEach(Array(a.optionNotes.enumerated()), id: \.offset) { _, n in still(n.label, n.note.en) }
        saidFoot(a)
        if let settled = a.settled { still("✓ Settled by your answer", settled.en) }
        if !a.summary.isEmpty { still("Done by the agent", a.summary) }
        if a.finished {
          Button { shell.act(["do": "hand_back", "card_id": card.id]) } label: { PenMark("sketch:what", color: Ink.fg).frame(width: 70, height: 24).frame(maxWidth: .infinity, minHeight: 56) }
            .buttonStyle(TileStyle(lead: false)).accessibilityLabel("What??")
        }
      default:
        if let label = a.label { still(label.en, a.detail?.en ?? "") }
        else if let says = a.says { Text(says.en).font(Face.text(15)).foregroundStyle(Ink.muted) }
        if a.did != nil || !a.back.isEmpty { saidFoot(a) }
      }
    }
  }
  private var otherWays: some View {
    HStack(spacing: 10) {
      Button { shell.act(["do": "hand_back", "card_id": card.id, "note": note]) } label: { EmptyView() }
        .buttonStyle(SideWay(kind: .what) { on in PenMark("sketch:what", color: on ? .white : Ink.fg, width: 2.2).frame(width: 90, height: 32) })
        .accessibilityLabel("What??")
      Button { shell.act(["do": "hand_back", "card_id": card.id, "note": note, "rework": true]) } label: { EmptyView() }
        .buttonStyle(SideWay(kind: .reverse) { on in Sketch("reverse", color: on ? .white : Ink.fg).frame(width: 30, height: 30) })
        .accessibilityLabel("Reverse: back to the agent for rework, with the comments")
    }
  }
  @ViewBuilder private func option(_ o: CoreView.OptionView) -> some View {
    let on = choices.contains(o.key)
    Button {
      if card.multiple { if on { choices.remove(o.key) } else { choices.insert(o.key) } }
      else { answer([o.key]) }
    } label: {
      HStack(alignment: .top, spacing: 12) {
        if card.multiple { PenMark(on ? "desk:BOX_TICK" : "desk:BOX", color: on ? Ink.accentFg : Ink.fg).frame(width: 26, height: 26) }
        VStack(alignment: .leading, spacing: 4) {
          Text(o.label).font(Face.display(19, .bold)).foregroundStyle(on ? Ink.accentFg : Ink.fg).multilineTextAlignment(.leading)
            .background(alignment: .bottomTrailing) { if o.recommended && !on { PenUnderline().stroke(Ink.urgHigh.opacity(0.75), style: StrokeStyle(lineWidth: 2.4, lineCap: .round)).frame(width: 60, height: 7).offset(y: 6) } }
          if !o.detail.isEmpty { Text(o.detail).font(Face.text(15)).foregroundStyle(on ? Ink.accentFg.opacity(0.82) : Ink.muted).multilineTextAlignment(.leading) }
          if o.settles { HStack(spacing: 4) { Sketch("tick", color: on ? Ink.accentFg : Ink.accent).frame(width: 13, height: 13); Text("settles it").font(Face.text(12, .medium)).foregroundStyle(on ? Ink.accentFg : Ink.accent) } }
          if !o.note.isEmpty { Text(o.note).font(Face.text(14)).italic().foregroundStyle(on ? Ink.accentFg.opacity(0.82) : Ink.muted) }
        }
        Spacer(minLength: 4)
      }
      .padding(16).frame(maxWidth: .infinity, alignment: .leading)
      .contentShape(Rectangle())
    }
    .buttonStyle(TalkPressStyle())
    .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(on ? Ink.accent : Ink.surface))
    .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
    .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(on ? Color.clear : o.recommended ? Ink.fg : (who.map { Tone.color(hue: $0.hue, .edge) } ?? Ink.lineStrong), lineWidth: o.recommended ? 2 : 1))
    .accessibilityLabel(o.tip.map { "\(o.label), \($0.en)" } ?? o.label)
  }
  /** The options' own paragraphs, after the answers: one block per option, open. */
  private var sections: some View {
    VStack(alignment: .leading, spacing: 14) {
      Rectangle().fill(Ink.line).frame(height: 1)
      ForEach(Array(card.sections.enumerated()), id: \.offset) { _, s in
        VStack(alignment: .leading, spacing: 4) {
          Text("\(Text(s.label).font(Face.display(17, .bold)))\(Text(s.recommended ? "  recommended" : "").font(Face.text(13)).foregroundColor(Ink.accent))")
          if !s.text.isEmpty { Markdown(text: s.text, size: 15) }
          if !s.html.isEmpty { InlineHTML(html: s.html).frame(minHeight: 160) }
        }
      }
    }
  }
  private func still(_ label: String, _ detail: String = "") -> some View {
    VStack(alignment: .leading, spacing: 4) {
      Text(label).font(Face.display(19, .bold)).foregroundStyle(Ink.fg)
      if !detail.isEmpty { Text(detail).font(Face.text(15)).foregroundStyle(Ink.muted) }
    }
    .padding(16).frame(maxWidth: .infinity, alignment: .leading)
    .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(Ink.surface))
    .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(Ink.lineStrong, lineWidth: 1))
  }
  /** The answer that stands: the tile solid in the accent, the pen's tick at its left. */
  private func stood(_ label: String, _ detail: String) -> some View {
    HStack(alignment: .center, spacing: 12) {
      Sketch("tick", color: Ink.accentFg, width: 2.4).frame(width: 24, height: 24).accessibilityHidden(true)
      VStack(alignment: .leading, spacing: 4) {
        Text(label).font(Face.display(19, .bold)).foregroundStyle(Ink.accentFg).multilineTextAlignment(.leading)
        if !detail.isEmpty { Text(detail).font(Face.text(15)).foregroundStyle(Ink.accentFg.opacity(0.82)).multilineTextAlignment(.leading) }
      }
      Spacer(minLength: 0)
    }
    .padding(16).frame(maxWidth: .infinity, alignment: .leading)
    .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(Ink.accent))
    .accessibilityElement(children: .combine)
  }
  /** The line under what was said: what he did and when, and the way back as a quiet underlined link. */
  private func saidFoot(_ a: CoreView.AnswerView) -> some View {
    HStack(alignment: .center, spacing: 6) {
      Text("\(Text(a.did?.en ?? ""))\(Text(a.atMs.map { "  ·  \(talkAgo($0))" } ?? "").foregroundColor(Ink.muted.opacity(0.75)))")
        .font(Face.text(13, .medium)).foregroundStyle(Ink.muted)
      Spacer(minLength: 8)
      if a.back == "reopen" || a.back == "takeback" {
        Button { shell.act(["do": a.back == "reopen" ? "decide_again" : "take_back", "card_id": card.id]) } label: {
          HStack(spacing: 5) {
            ReturnArrow().stroke(Ink.muted, style: StrokeStyle(lineWidth: 2, lineCap: .round, lineJoin: .round)).frame(width: 15, height: 15)
            Text("Take back").font(Face.text(13, .semibold)).underline().foregroundStyle(Ink.muted)
          }
          .fixedSize().frame(minWidth: 44, minHeight: 44).contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(a.back == "reopen" ? "Take back your answer" : "Take back the card")
      }
    }
    .padding(.horizontal, 4).padding(.top, -6)
  }
  /** Just answered here: the next open question (or that there is none) and Close. */
  private func afterStep(_ after: CoreView.AfterView) -> some View {
    VStack(spacing: 8) {
      if let n = after.next {
        Button { if !shell.path.isEmpty { shell.path.removeLast() }; shell.focus(.card(cardId: n.id)) } label: {
          HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 2) { Text(after.says.en).font(Face.text(17, .bold)); Text(n.title).font(Face.text(13)).opacity(0.7).lineLimit(1) }
            Spacer(minLength: 8)
            Image(systemName: "arrow.right").font(.system(size: 17, weight: .semibold))
          }
          .foregroundStyle(Ink.bg).padding(.horizontal, 16).padding(.vertical, 12)
          .frame(maxWidth: .infinity, alignment: .leading)
          .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Ink.fg))
          .contentShape(Rectangle())
        }.buttonStyle(.plain)
      } else {
        HStack(spacing: 8) { Sketch("tick", color: Ink.accent).frame(width: 18, height: 18); Text(after.says.en).font(Face.text(15, .semibold)).foregroundStyle(Ink.muted) }
          .frame(maxWidth: .infinity, alignment: .leading)
      }
      Button("Close") { if !shell.path.isEmpty { shell.path.removeLast() } }
        .font(Face.text(15, .semibold)).foregroundStyle(Ink.muted).underline().frame(minHeight: 44)
    }
    .padding(.top, 6)
  }

  // ---- the card's own talk ----

  @ViewBuilder private var thread: some View {
    if !card.lead.isEmpty || !card.talk.isEmpty || !card.versions.isEmpty || card.earlier != nil {
      VStack(alignment: .leading, spacing: 10) {
        EndDivider(title: "The talk")
        ForEach(Array(card.lead.enumerated()), id: \.offset) { _, l in
          VStack(alignment: .leading, spacing: 6) {
            Text(l.label.en.uppercased()).font(Face.text(11, .bold)).kerning(1).foregroundStyle(Ink.muted)
            if !l.text.isEmpty { Markdown(text: l.text, size: 15) }
            if !l.html.isEmpty { InlineHTML(html: l.html).frame(minHeight: 200) }
            ForEach(l.links, id: \.url) { link in
              if let url = URL(string: link.url) { Link(destination: url) { Text("\(Text(link.label).foregroundColor(Ink.fg))  \(Text(link.short).foregroundColor(Ink.accent))").font(Face.text(14)) } }
            }
          }
          .padding(12).frame(maxWidth: .infinity, alignment: .leading)
          .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Ink.surface2))
        }
        if let earlier = card.earlier {
          Button(earlier.says.en) { shell.act(["do": "show_more", "of": ["list": "comments", "card": card.id]]) }.font(Face.text(14, .semibold)).foregroundStyle(Ink.accent)
        }
        ForEach(card.talk, id: \.id) { row in TalkRow(row: row, who: who, inCard: true) }
        ForEach(card.versions, id: \.n) { v in RevisionRow(version: v) }
      }
    }
  }
  private func answer(_ keys: [String]) { shell.act(["do": "answer", "card_id": card.id, "choices": keys, "note": note, "attachments": attachments]) }
  private func verdict(_ allow: Bool) { shell.act(["do": "verdict", "request_id": card.id, "allow": allow]) }
}

/** The strip above a card's title while its session works on it: the drawing in its working ring, who, what. */
struct CardWorkStrip: View {
  let work: CoreView.CardWorkView
  var body: some View {
    let hue = Pen.hueFor(id: work.sessionId, mark: work.mark)
    HStack(spacing: 10) {
      ZStack {
        WorkingRing(working: true, color: Tone.color(hue: hue, .mid)).frame(width: 32, height: 32)
        AgentMark(mark: work.mark, size: 20, hue: hue)
      }
      VStack(alignment: .leading, spacing: 1) {
        Text(work.says.en).font(Face.text(15, .semibold)).foregroundStyle(Ink.fg).lineLimit(1)
        Text(work.atMs > 0 ? "\(work.words.en) · \(talkAgo(work.atMs))" : work.words.en).font(Face.text(14)).foregroundStyle(Ink.muted).lineLimit(1)
      }
    }
    .padding(12).frame(maxWidth: .infinity, alignment: .leading)
    .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(Tone.color(hue: hue, .wash)))
  }
}

/** One version that stood before, folded to read as it was. */
struct RevisionRow: View {
  let version: CoreView.VersionView
  var body: some View {
    VStack(alignment: .leading, spacing: 6) {
      HStack(alignment: .firstTextBaseline, spacing: 8) {
        Image(systemName: "pencil.circle").font(.system(size: 13, weight: .medium)).foregroundStyle(Ink.muted).frame(width: 18)
        Text("Version \(version.n)").font(Face.text(14, .semibold)).foregroundStyle(Ink.fg)
        Spacer(minLength: 4)
        if version.atMs > 0 { Text(talkAgo(version.atMs)).font(Face.text(11)).foregroundStyle(Ink.faint) }
      }
      if !version.note.isEmpty { Text(version.note).font(Face.text(15)).foregroundStyle(Ink.fg).padding(.leading, 26) }
      DisclosureGroup("See version \(version.n)") {
        VStack(alignment: .leading, spacing: 6) {
          Text(version.title).font(Face.display(17, .bold))
          if !version.text.isEmpty { Markdown(text: version.text, size: 15, color: Ink.muted) }
          ForEach(version.options, id: \.key) { o in Text("· \(o.label)").font(Face.text(15)).foregroundStyle(Ink.muted) }
        }
        .padding(12).frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 12).strokeBorder(Ink.lineStrong, style: StrokeStyle(lineWidth: 1, dash: [4, 3])))
        .padding(.top, 6)
      }
      .font(Face.text(14, .medium)).tint(Ink.muted).padding(.leading, 26)
    }
    .padding(.vertical, 4)
  }
}

/** The pen's return arrow of the Take back link, in its 24-unit box. */
struct ReturnArrow: Shape {
  func path(in r: CGRect) -> Path {
    let k = min(r.width, r.height) / 24
    func p(_ x: CGFloat, _ y: CGFloat) -> CGPoint { CGPoint(x: r.minX + x * k, y: r.minY + y * k) }
    var path = Path()
    path.move(to: p(9.8, 5.6)); path.addQuadCurve(to: p(4.3, 11.2), control: p(6.8, 8.4)); path.addQuadCurve(to: p(10, 16.6), control: p(7.1, 13.8))
    path.move(to: p(4.9, 11.1)); path.addQuadCurve(to: p(14.9, 11.2), control: p(10.4, 10.4))
    path.addQuadCurve(to: p(19.5, 16.4), control: p(19.8, 12.5)); path.addQuadCurve(to: p(15, 19.7), control: p(19, 19.5))
    return path
  }
}
/** The finger on an option: a little darker and a hair smaller, flat. */
struct TalkPressStyle: ButtonStyle {
  func makeBody(configuration: Configuration) -> some View {
    configuration.label
      .background(Ink.fg.opacity(configuration.isPressed ? 0.07 : 0))
      .scaleEffect(configuration.isPressed ? 0.97 : 1).animation(.easeOut(duration: 0.1), value: configuration.isPressed)
  }
}

/** The three side answers (the duck, What??, the reverse card): calm at rest, held they take their colour. */
struct SideWay<L: View>: ButtonStyle {
  enum Kind { case duck, what, reverse }
  let kind: Kind
  @ViewBuilder let face: (Bool) -> L
  func makeBody(configuration: Configuration) -> some View { SideWayBody(kind: kind, on: configuration.isPressed, face: face) }
}
private struct SideWayBody<L: View>: View {
  let kind: SideWay<L>.Kind
  let on: Bool
  let face: (Bool) -> L
  @Environment(\.colorScheme) private var scheme
  var body: some View {
    let shape = RoundedRectangle(cornerRadius: 12, style: .continuous)
    face(on)
      .foregroundStyle(on ? (kind == .duck ? Ink.duckInk : .white) : Ink.fg)
      .frame(maxWidth: .infinity, minHeight: 64)
      .background {
        ZStack {
          shape.fill(Ink.surface)
          shape.fill(Ink.accent.opacity(0.09))
          if on {
            switch kind {
            case .duck: shape.fill(Ink.duckPale)
            case .what: shape.fill(Color(hex: 0x7a5bc7))
            case .reverse:
              AngularGradient(stops: [.init(color: Color(hex: 0xd9483b), location: 0), .init(color: Color(hex: 0xd9483b), location: 0.25),
                                      .init(color: Color(hex: 0xf3c52f), location: 0.25), .init(color: Color(hex: 0xf3c52f), location: 0.5),
                                      .init(color: Color(hex: 0x3f9f58), location: 0.5), .init(color: Color(hex: 0x3f9f58), location: 0.75),
                                      .init(color: Color(hex: 0x2f70c4), location: 0.75), .init(color: Color(hex: 0x2f70c4), location: 1)],
                              center: UnitPoint(x: 0.53, y: 0.47), angle: .degrees(-83)).clipShape(shape)
            }
          }
        }
      }
      .overlay(shape.strokeBorder(on ? (kind == .duck ? Ink.duckInk : kind == .what ? Color(hex: 0x7a5bc7).opacity(0.7) : Color(hex: 0x14161a)) : (scheme == .dark ? Ink.lineStrong : .clear), lineWidth: on ? 1.5 : 1))
      .contentShape(shape)
      .scaleEffect(on ? 0.97 : 1)
      .animation(.easeOut(duration: 0.12), value: on)
      .sensoryFeedback(.impact(weight: .light), trigger: on) { _, now in now }
  }
}

/** Above a field or under the answers: whether the session hears, and what became of his words. */
struct LinkNoteBox: View {
  let note: CoreView.LinkNoteView
  var body: some View {
    let tone: Color = note.tone == "cut" || note.tone == "asleep" || note.tone == "unheard" ? Ink.urgHigh : note.tone == "heard" ? Ink.accent : Ink.muted
    let sign = note.tone == "unheard" || note.tone == "sent" ? "letter" : note.tone == "heard" ? "tick" : note.tone == "cut" || note.tone == "gone" ? "ear-off" : "ear"
    HStack(alignment: .top, spacing: 10) {
      Sketch(sign, color: tone).frame(width: 20, height: 20)
      VStack(alignment: .leading, spacing: 4) {
        if let r = note.receipt { Text(r.en).font(Face.text(15, .medium)).foregroundStyle(Ink.fg) }
        if let l = note.line { Text(l.en).font(Face.text(14)).foregroundStyle(Ink.muted) }
        if let fix = note.fix {
          Text(fix.say.en).font(Face.text(13)).foregroundStyle(Ink.muted)
          if !fix.code.isEmpty { TalkCodeChip(text: fix.code) }
        }
      }
      Spacer(minLength: 0)
    }
    .padding(12).frame(maxWidth: .infinity, alignment: .leading)
    .background(RoundedRectangle(cornerRadius: 12).fill(Ink.surface.opacity(0.7)))
    .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Ink.lineStrong))
  }
}
struct TalkCodeChip: View {
  let text: String
  @State private var copied = false
  var body: some View {
    Button { copyText(text); copied = true } label: {
      Text(copied ? "Copied" : text).font(Face.mono(13, .medium)).foregroundStyle(Ink.fg).padding(.horizontal, 8).padding(.vertical, 4)
        .background(RoundedRectangle(cornerRadius: 6).fill(Ink.sunken))
    }.buttonStyle(.plain)
  }
}

// ---- the composer ----------------------------------------------------------------------------------------------------

/** The field at the bottom of a chat and a card: a round glass "+" for files, a growing glass field, the round Send. */
struct TalkComposer: View {
  @EnvironmentObject var shell: AppShell
  let placeholder: String
  @Binding var text: String
  @Binding var attachments: [String]
  let send: () -> Void
  @State private var importing = false
  @State private var keyboard = false
  private var canSend: Bool { !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !attachments.isEmpty }
  var body: some View {
    HStack(alignment: .bottom, spacing: 11) {
      Button { importing = true } label: {
        Image(systemName: "plus").font(.system(size: 19, weight: .semibold)).foregroundStyle(Ink.fg).frame(width: 40, height: 40).glass(Circle(), interactive: true)
      }
      .buttonStyle(.plain).accessibilityLabel("Attach")
      VStack(alignment: .leading, spacing: 0) {
        if !attachments.isEmpty {
          ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 6) {
              ForEach(attachments, id: \.self) { id in
                HStack(spacing: 6) {
                  Image(systemName: "paperclip").font(.system(size: 11, weight: .semibold))
                  Text(shell.view.files[id]?.name ?? "File").font(Face.text(13)).lineLimit(1).frame(maxWidth: 140)
                  if let p = shell.view.files[id]?.percent { Text("\(p) %").font(Face.text(11)).monospacedDigit().foregroundStyle(Ink.muted) }
                  Button { attachments.removeAll { $0 == id } } label: { Image(systemName: "xmark").font(.system(size: 10, weight: .bold)).frame(minWidth: 28, minHeight: 36).contentShape(Rectangle()) }.buttonStyle(.plain).accessibilityLabel("Remove")
                }
                .padding(.leading, 10).padding(.trailing, 4).background(Capsule().fill(Ink.sunken))
              }
            }.padding(.horizontal, 10).padding(.top, 8)
          }
        }
        HStack(alignment: .bottom, spacing: 4) {
          TextField(placeholder, text: $text, axis: .vertical)
            .font(Face.text(17)).lineLimit(1...7)
            .padding(.leading, 16).padding(.vertical, 9)
          Button(action: send) {
            Image(systemName: "arrow.up").font(.system(size: 16, weight: .bold))
              .foregroundStyle(Ink.accentFg).frame(width: 32, height: 32).background(Circle().fill(canSend ? Ink.accent : Ink.faint))
              .frame(width: 40, height: 40).contentShape(Rectangle())
          }
          .buttonStyle(.plain).disabled(!canSend).accessibilityLabel("Send")
        }
      }
      .clipShape(RoundedRectangle(cornerRadius: 22, style: .continuous))
      .glass(RoundedRectangle(cornerRadius: 22, style: .continuous))
    }
    .padding(.leading, 27).padding(.trailing, 25).padding(.top, 8).padding(.bottom, keyboard ? 8 : bottomSink(26))
    #if canImport(UIKit)
    .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardWillShowNotification)) { _ in withAnimation(.easeOut(duration: 0.25)) { keyboard = true } }
    .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardWillHideNotification)) { _ in withAnimation(.easeOut(duration: 0.25)) { keyboard = false } }
    #endif
    .fileImporter(isPresented: $importing, allowedContentTypes: [.item], allowsMultipleSelection: true) { result in
      if case .success(let urls) = result { for url in urls { if let id = shell.attach(url) { attachments.append(id) } } }
    }
  }
}

// ---- a session's conversation ----------------------------------------------------------------------------------------

struct SessionScreen: View {
  @EnvironmentObject var shell: AppShell
  let page: CoreView.SessionView
  @State private var text = ""
  @State private var attachments: [String] = []
  private var hue: Int { Pen.hueFor(id: page.slug.isEmpty ? page.id : page.slug, mark: page.mark) }
  var body: some View {
    ScrollViewReader { reader in
      ScrollView {
        LazyVStack(alignment: .leading, spacing: 0) {
          if let earlier = page.earlier {
            Button { shell.act(["do": "show_more", "of": ["list": "talk", "session": page.id]]) } label: {
              Text(earlier.says.en).font(Face.text(14, .semibold)).foregroundStyle(Ink.accent).frame(maxWidth: .infinity).padding(.vertical, 14)
            }.buttonStyle(.plain)
          }
          if let empty = page.empty { EmptyChat(page: page, empty: empty, hue: hue) }
          ForEach(page.talk, id: \.id) { row in
            TalkRow(row: row, who: TalkWho(mark: page.mark, name: page.name.en, hue: hue))
              .padding(.top, row.cont ? 2 : 10)
              .id(row.id)
          }
          SessionStatus(page: page, hue: hue)
          Color.clear.frame(height: 1).id("bottom")
        }
        .padding(.horizontal, 14).padding(.top, 6)
        .frame(maxWidth: 760).frame(maxWidth: .infinity)
      }
      .defaultScrollAnchor(.bottom)
      .scrollDismissesKeyboard(.interactively)
      .onChange(of: page.talk.count) { _, _ in withAnimation(.easeOut(duration: 0.25)) { reader.scrollTo("bottom", anchor: .bottom) } }
      .bottomBar {
        VStack(spacing: 0) {
          if let n = page.note { LinkNoteBox(note: n).padding(.horizontal, 12).padding(.top, 8) }
          TalkComposer(placeholder: page.compose.en, text: $text, attachments: $attachments) {
            shell.act(["do": "write", "session_id": page.id, "text": text, "attachments": attachments]); text = ""; attachments = []
            withAnimation { reader.scrollTo("bottom", anchor: .bottom) }
          }
        }
      }
      .pushedPills(extraTrailing: page.usage == nil ? 0 : 36) { header } trailing: {
        HStack(spacing: 6) {
          if let usage = page.usage { UsageMark(usage: usage) }
          Menu {
            Toggle(isOn: Binding(get: { page.crowned }, set: { _ in shell.act(["do": "crown_session", "session_id": page.id, "on": !page.crowned]) })) { Label("Main Session", systemImage: "crown") }
            Button(role: .destructive) { shell.act(["do": "archive_session", "session_id": page.id, "on": true]) } label: { Label("Archive", systemImage: "archivebox") }
          } label: { Image(systemName: "ellipsis").font(.system(size: 18, weight: .semibold)).foregroundStyle(Ink.fg).pillCircle() }
          .accessibilityLabel("More")
        }
      }
    }
    .background(Ink.bg.ignoresSafeArea())
  }
  /** The title: the session's drawing and name ▾, a glass menu of the desk's sessions to switch to. */
  private var header: some View {
    Menu {
      ForEach(shell.view.sidebar.sessions, id: \.id) { s in
        Button {
          if shell.path.isEmpty { shell.path = [.session(sessionId: s.id)] } else { shell.path[shell.path.count - 1] = .session(sessionId: s.id) }
        } label: {
          Label { Text(s.name.en) } icon: { if s.id == page.id { Image(systemName: "checkmark") } else if s.crowned { Image(systemName: "crown") } }
        }
      }
    } label: {
      HStack(spacing: 7) {
        AgentMark(mark: page.mark, size: 22, crowned: page.crowned, hue: hue)
        Text(page.name.en).font(Face.display(17, .bold)).foregroundStyle(Ink.fg).lineLimit(1).minimumScaleFactor(0.85).truncationMode(.middle)
        if !page.modelLabel.isEmpty { Text(page.modelLabel).font(Face.text(13, .medium)).foregroundStyle(Ink.muted).lineLimit(1).layoutPriority(-1) }
        if page.blocked { Circle().fill(Ink.urgCritical).frame(width: 7, height: 7) }
        else if !page.dot.on { Circle().strokeBorder(Ink.faint, lineWidth: 1.5).frame(width: 7, height: 7) }
        Image(systemName: "chevron.down").font(.system(size: 11, weight: .semibold)).foregroundStyle(Ink.muted)
      }
      .padding(.horizontal, 14).frame(height: 44)
      .glass(Capsule(), interactive: true)
      .contentShape(Capsule())
    }
    .accessibilityLabel("\(page.name.en)\(page.modelLabel.isEmpty ? "" : ", \(page.modelLabel)"), \(page.dot.says.en): switch chat")
    .accessibilityHint(page.quiet?.en ?? "")
  }
}

/** A new conversation: a quiet card from the agent that joined and the starters as rows; a tap sends one. */
struct EmptyChat: View {
  @EnvironmentObject var shell: AppShell
  let page: CoreView.SessionView
  let empty: CoreView.EmptyTalkView
  let hue: Int
  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      HStack(spacing: 12) {
        AgentMark(mark: page.mark, size: 40, hue: hue)
        VStack(alignment: .leading, spacing: 2) {
          Text(page.name.en).font(Face.display(17, .bold)).foregroundStyle(Ink.fg).lineLimit(1)
          if !empty.meta.isEmpty { Text(empty.meta).font(Face.text(12)).foregroundStyle(Ink.muted).lineLimit(1) }
        }
      }
      Text(empty.text.en).font(Face.text(14)).foregroundStyle(Ink.muted).fixedSize(horizontal: false, vertical: true)
      VStack(spacing: 0) {
        Rectangle().fill(Ink.line).frame(height: 1)
        ForEach(Array(empty.starters.enumerated()), id: \.offset) { _, s in
          Button { shell.act(["do": "write", "session_id": page.id, "text": s.en]) } label: {
            HStack {
              Text(s.en).font(Face.text(15, .medium)).foregroundStyle(Ink.fg)
              Spacer(minLength: 8)
              Image(systemName: "arrow.up").font(.system(size: 13, weight: .semibold)).foregroundStyle(Ink.faint)
            }.frame(minHeight: 44).contentShape(Rectangle())
          }.buttonStyle(.plain)
          Rectangle().fill(Ink.line).frame(height: 1)
        }
      }
    }
    .padding(20)
    .frame(maxWidth: 420, alignment: .leading)
    .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(Ink.surface))
    .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(Ink.lineStrong))
    .frame(maxWidth: .infinity).padding(.vertical, 40).padding(.horizontal, 4)
  }
}

/** The session's running work at the end of the chat: "<name> is working" and its status lines as small chips; a tap on
 *  a chip opens its whole text and the way to its card. */
struct SessionStatus: View {
  @EnvironmentObject var shell: AppShell
  let page: CoreView.SessionView
  let hue: Int
  @State private var shown: String? = nil
  var body: some View {
    if page.answering != nil || !page.statusLines.isEmpty {
      VStack(alignment: .leading, spacing: 8) {
        if let answering = page.answering {
          HStack(spacing: 8) { AgentMark(mark: page.mark, size: 16, hue: hue); Text(answering.en).font(Face.text(14)).foregroundStyle(Ink.muted); TypingDots() }
        }
        if !page.statusLines.isEmpty {
          FlowChips(spacing: 6) { ForEach(page.statusLines, id: \.id) { line in chip(line) } }
          if let id = shown, let line = page.statusLines.first(where: { $0.id == id }) { opened(line) }
        }
      }.padding(.vertical, 12)
    }
  }
  @ViewBuilder private func chip(_ t: CoreView.StatusLineView) -> some View {
    let on = shown == t.id
    let quiet = t.state == "later" || t.state == "done" || t.state == "stale"
    Button { withAnimation(.easeOut(duration: 0.18)) { shown = on ? nil : t.id } } label: {
      HStack(spacing: 6) {
        Group {
          if t.state == "working" { WorkingRing(working: true, color: Ink.accent) }
          else if t.state == "stale" { Circle().strokeBorder(Ink.faint, lineWidth: 1.6).frame(width: 11, height: 11) }
          else if t.state == "decision" { Sketch("knock", color: Ink.urgHigh) }
          else if t.state == "later" { Sketch("snooze", color: Ink.faint) }
          else { Sketch("tick", color: Ink.faint) }
        }.frame(width: 16, height: 16)
        Text(t.label).font(Face.text(14, .semibold)).foregroundStyle(quiet ? Ink.muted : Ink.fg).lineLimit(1).frame(maxWidth: 150, alignment: .leading)
        if t.updatedMs > 0 { Text(talkAgoShort(t.updatedMs)).font(Face.text(12)).monospacedDigit().foregroundStyle(Ink.faint) }
      }
      .padding(.leading, 7).padding(.trailing, 10).frame(height: 30)
      .background(Capsule().fill(Ink.sunken))
      .overlay(Capsule().strokeBorder(t.state == "decision" ? Ink.urgHigh.opacity(0.55) : on ? Ink.lineStrong : .clear, lineWidth: 1))
      .contentShape(Capsule())
    }
    .buttonStyle(.plain)
    .accessibilityLabel("\(t.label), \(t.says.en)")
  }
  private func opened(_ t: CoreView.StatusLineView) -> some View {
    VStack(alignment: .leading, spacing: 6) {
      (Text(t.label).font(Face.text(14, .semibold)) + Text(t.detail.isEmpty ? "" : ": \(t.detail)").font(Face.text(14)))
        .foregroundStyle(Ink.fg).fixedSize(horizontal: false, vertical: true)
      HStack(spacing: 12) {
        Text(t.says.en).font(Face.text(12)).foregroundStyle(Ink.muted)
        if let c = t.card {
          Button { shell.focus(.card(cardId: c.id)) } label: {
            HStack(spacing: 6) { Text("Open card").font(Face.text(14, .semibold)).underline(true, color: Ink.accent); Text(c.title).font(Face.text(12)).foregroundStyle(Ink.muted).lineLimit(1) }
              .frame(minHeight: 44).contentShape(Rectangle())
          }.buttonStyle(.plain)
        }
      }
    }
    .padding(.horizontal, 12).padding(.vertical, 10)
    .frame(maxWidth: .infinity, alignment: .leading)
    .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Ink.sunken))
  }
}
/** Chips in rows that wrap. */
struct FlowChips: Layout {
  var spacing: CGFloat = 6
  func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
    let width = proposal.width ?? .infinity
    var x: CGFloat = 0, y: CGFloat = 0, line: CGFloat = 0, widest: CGFloat = 0
    for v in subviews {
      let s = v.sizeThatFits(.unspecified)
      if x > 0 && x + s.width > width { y += line + spacing; x = 0; line = 0 }
      x += s.width + spacing; line = max(line, s.height); widest = max(widest, x - spacing)
    }
    return CGSize(width: min(widest, width), height: y + line)
  }
  func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
    var x = bounds.minX, y = bounds.minY, line: CGFloat = 0
    for v in subviews {
      let s = v.sizeThatFits(.unspecified)
      if x > bounds.minX && x + s.width > bounds.maxX { y += line + spacing; x = bounds.minX; line = 0 }
      v.place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(s))
      x += s.width + spacing; line = max(line, s.height)
    }
  }
}
struct TypingDots: View {
  @State private var phase = 0.0
  var body: some View {
    HStack(spacing: 4) { ForEach(0..<3) { i in Circle().fill(Ink.accent).frame(width: 5, height: 5).offset(y: sin(phase + Double(i) * 0.9) * 2.5) } }
      .onAppear { withAnimation(.linear(duration: 1.3).repeatForever(autoreverses: false)) { phase = .pi * 2 } }
  }
}

// ---- one row of a talk -----------------------------------------------------------------------------------------------

struct TalkRow: View {
  @EnvironmentObject var shell: AppShell
  let row: CoreView.TalkRow
  /** Who speaks for the agent's words (the session in view, a card's session). */
  var who: TalkWho? = nil
  /** On a card's own page: a line about the card does not open it again. */
  var inCard = false
  var body: some View {
    VStack(alignment: .leading, spacing: 6) {
      if let day = row.day { DayLine(text: day.en) }
      switch row.kind {
      case "ask": if let card = row.row { DeskRow(row: card).padding(.vertical, 6) }
      case "work": if let work = row.work { WorkDrawing(work: work, timeMs: row.timeMs) }
      case "event": if let e = row.event, !e.echo { EventLine(event: e, card: row.card, timeMs: row.timeMs, inCard: inCard) }
      case "user", "note": mine
      case "newer":
        HStack(spacing: 8) { Sketch("wake", color: Ink.muted).frame(width: 16, height: 16); Text(row.text.isEmpty ? "This needs a newer version of Trommi." : row.text).font(Face.text(14)).foregroundStyle(Ink.muted) }
          .padding(10).frame(maxWidth: .infinity, alignment: .leading)
          .background(RoundedRectangle(cornerRadius: 10).strokeBorder(Ink.lineStrong, style: StrokeStyle(lineWidth: 1, dash: [4, 3])))
      default: if row.fromMe { mine } else { agent }
      }
    }
    .frame(maxWidth: .infinity, alignment: .leading)
  }
  /** A card the row is about: a small "ABOUT" line that opens it. */
  @ViewBuilder private var about: some View {
    if !inCard, let c = row.card {
      Button { shell.focus(.card(cardId: c.id)) } label: {
        HStack(spacing: 6) { Text("ABOUT").font(Face.text(10, .bold)).kerning(1).foregroundStyle(Ink.faint); Text(c.title).font(Face.text(13)).foregroundStyle(Ink.muted).lineLimit(1) }
      }.buttonStyle(.plain)
    }
  }
  private var chips: some View {
    ForEach(row.chips, id: \.id) { c in
      Button { shell.focus(.card(cardId: c.id)) } label: {
        HStack(spacing: 6) { Text("Nr. \(c.number)").font(Face.text(13, .bold)); Text(c.title).font(Face.text(13)).lineLimit(1) }
          .foregroundStyle(Ink.fg).padding(.horizontal, 10).padding(.vertical, 6).background(Capsule().fill(Ink.sunken))
      }.buttonStyle(.plain)
    }
  }
  /** His words on his side: the green bubble (a note: the yellow slip, taped on), the time beside it. */
  private var mine: some View {
    VStack(spacing: 6) {
      if let deed = row.deed {
        VStack(spacing: 2) {
          HStack(spacing: 6) {
            Image(systemName: "arrow.uturn.backward").font(.system(size: 10, weight: .semibold))
            Text(deed.label.en).font(Face.text(12, .semibold))
            if !inCard, let c = row.card { Text(c.title).font(Face.text(12)).lineLimit(1).truncationMode(.tail) }
            if row.pending { Image(systemName: "clock").font(.system(size: 10)) } else { Text(talkClock(row.timeMs)).font(Face.text(11)).foregroundStyle(Ink.faint) }
          }
          ForEach(Array(deed.said.enumerated()), id: \.offset) { _, s in Text(s.en).font(Face.text(12)).foregroundStyle(Ink.muted) }
        }
        .foregroundStyle(Ink.muted).frame(maxWidth: .infinity, alignment: .center).padding(.vertical, 4).padding(.horizontal, 24)
      }
      if !row.text.isEmpty || !row.files.isEmpty || !row.chips.isEmpty {
        HStack(alignment: .bottom, spacing: 6) {
          Spacer(minLength: 44)
          VStack(alignment: .trailing, spacing: 4) {
            about
            chips
            if row.kind == "note" {
              VStack(alignment: .leading, spacing: 6) {
                HStack(spacing: 6) { NoteGlyph(color: Ink.noteInk, written: true).frame(width: 17, height: 17); Text("Note").font(Face.text(12, .bold)); if let w = row.writtenMs, w > 0 { Text("written \(talkClock(w))").font(Face.text(12)) } }
                  .foregroundStyle(Ink.noteInk.opacity(0.8))
                Text(row.text).font(Face.text(16)).foregroundStyle(Ink.noteInk).accessibilityIdentifier("talk-message")
                FileRows(ids: row.files)
              }
              .padding(14).frame(maxWidth: 320, alignment: .leading)
              .background(Rectangle().fill(Ink.noteYellow).rotationEffect(.degrees(-0.8)).shadow(color: .black.opacity(0.12), radius: 3, y: 2))
              .overlay(alignment: .top) { Rectangle().fill(Color.white.opacity(0.55)).frame(width: 46, height: 14).rotationEffect(.degrees(-3)).offset(y: -7) }
            } else {
              FileRows(ids: row.files)
              if !row.text.isEmpty {
                Text(row.text).font(Face.text(16)).foregroundStyle(Ink.fg).textSelection(.enabled)
                  .padding(.horizontal, 14).padding(.vertical, 10)
                  .background(UnevenRoundedRectangle(topLeadingRadius: 18, bottomLeadingRadius: 18, bottomTrailingRadius: row.cont ? 18 : 6, topTrailingRadius: 18, style: .continuous).fill(Ink.accentSoft))
                  .accessibilityIdentifier("talk-message")
              }
            }
            if row.terminal {
              HStack(alignment: .firstTextBaseline, spacing: 5) { Text(">_").font(.system(size: 10, weight: .medium, design: .monospaced)); Text("from the terminal").font(Face.text(11)) }
                .foregroundStyle(Ink.faint).padding(.trailing, 4)
            }
          }
          VStack {
            if row.pending { Image(systemName: "clock").font(.system(size: 10)).foregroundStyle(Ink.faint) }
            else { Text(talkClock(row.timeMs)).font(Face.text(10)).foregroundStyle(Ink.faint) }
          }.frame(width: 30)
        }
        .contextMenu { if !row.text.isEmpty { Button { copyText(row.text) } label: { Label("Copy", systemImage: "doc.on.doc") } } }
      }
    }
  }
  /** The agent's words: its drawing, name and time over a run's first row, then its light markdown, files and details. */
  private var agent: some View {
    VStack(alignment: .leading, spacing: 6) {
      if !row.cont {
        HStack(spacing: 6) {
          if let who { AgentMark(mark: who.mark, size: 18, hue: who.hue); Text(who.name).font(Face.text(13, .semibold)).foregroundStyle(Ink.muted) }
          Text(talkClock(row.timeMs)).font(Face.text(11)).foregroundStyle(Ink.faint)
          if row.terminal { Text(">_").font(.system(size: 10, weight: .medium, design: .monospaced)).foregroundStyle(Ink.faint) }
        }
      }
      about
      if row.kind == "artifact" {
        HStack(spacing: 10) {
          Sketch("page", color: Ink.accent).frame(width: 22, height: 22)
          Text(row.text).font(Face.text(15, .semibold)).foregroundStyle(Ink.fg).lineLimit(2).accessibilityIdentifier("talk-message")
          Spacer(minLength: 0)
        }
        .padding(12).frame(maxWidth: 420, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Ink.surface))
        .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(Ink.lineStrong))
      } else if !row.text.isEmpty {
        Markdown(text: row.text).accessibilityIdentifier("talk-message")
      }
      if !row.html.isEmpty { InlineHTML(html: row.html).frame(minHeight: 200).clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous)) }
      FileRows(ids: row.files)
      chips
      if !row.details.isEmpty {
        DisclosureGroup { Markdown(text: row.details, size: 15, color: Ink.muted).padding(.top, 4) } label: { Text("Details").font(Face.text(14, .semibold)).foregroundStyle(Ink.muted) }.tint(Ink.muted)
      }
      if let waits = row.waits { Text(waits.en).font(Face.text(13)).italic().foregroundStyle(Ink.faint) }
    }
    .padding(.trailing, 24)
    .frame(maxWidth: .infinity, alignment: .leading)
    .contextMenu { if !row.text.isEmpty { Button { copyText(row.text) } label: { Label("Copy", systemImage: "doc.on.doc") } } }
  }
}

/** The line where a new day begins. */
struct DayLine: View {
  let text: String
  var body: some View {
    HStack(spacing: 10) {
      Rectangle().fill(Ink.line).frame(height: 1)
      Text(text).font(Face.text(12, .semibold)).foregroundStyle(Ink.faint).fixedSize()
      Rectangle().fill(Ink.line).frame(height: 1)
    }.padding(.vertical, 14)
  }
}

/** One quiet line for something that happened to a card: its sign, the label in small capitals, what, the time. */
struct EventLine: View {
  @EnvironmentObject var shell: AppShell
  let event: CoreView.EventView
  let card: CoreView.CardRefView?
  let timeMs: UInt64
  var inCard = false
  var body: some View {
    Button { if let c = card, !inCard { shell.focus(.card(cardId: c.id)) } } label: {
      HStack(alignment: .firstTextBaseline, spacing: 8) {
        Image(systemName: icon).font(.system(size: 13, weight: .medium)).foregroundStyle(tint).frame(width: 18)
        VStack(alignment: .leading, spacing: 2) {
          HStack(spacing: 6) {
            Text(event.label.en.uppercased()).font(Face.text(11, .bold)).kerning(1).foregroundStyle(Ink.muted).fixedSize()
            if !event.about.isEmpty { Text(event.about).font(Face.text(14)).foregroundStyle(Ink.muted).lineLimit(1) }
          }
          if !event.text.isEmpty { Text(event.text).font(Face.text(15, .medium)).foregroundStyle(Ink.fg).lineLimit(2) }
          ForEach(Array(event.said.enumerated()), id: \.offset) { _, s in Text(s.en).font(Face.text(14)).foregroundStyle(Ink.muted).lineLimit(2) }
        }
        Spacer(minLength: 4)
        if timeMs > 0 { Text(talkClock(timeMs)).font(Face.text(11)).foregroundStyle(Ink.faint) }
      }
      .padding(.vertical, 4).contentShape(Rectangle())
    }
    .buttonStyle(.plain)
    .disabled(card == nil || inCard)
  }
  private var icon: String {
    switch event.kind {
    case "decided", "trusted": "checkmark.circle"
    case "done", "read": "checkmark.seal"
    case "shredded": "trash"
    case "reopened", "took_back": "arrow.uturn.backward"
    case "revised": "pencil.circle"
    case "snoozed": "zzz"
    case "handed", "handed_back": "gearshape"
    case "explain": "questionmark.bubble"
    case "info": "doc.text"
    default: "questionmark.circle"
    }
  }
  private var tint: Color { event.kind == "asked" ? Ink.urgHigh : ["decided", "done", "read"].contains(event.kind) ? Ink.accent : Ink.muted }
}

// ---- light markdown --------------------------------------------------------------------------------------------------

/** The light markdown agents write: paragraphs, headings, lists, **bold**, *italic*, `code`, links, __words underlined
 *  with the pen__, fenced code, tables, quotes and rules. */
struct Markdown: View {
  let text: String
  var size: CGFloat = 16
  var color: Color = Ink.fg
  var body: some View {
    VStack(alignment: .leading, spacing: 10) {
      ForEach(Array(MarkBlock.blocks(text).enumerated()), id: \.offset) { _, b in block(b) }
    }
    .frame(maxWidth: .infinity, alignment: .leading)
  }
  @ViewBuilder private func block(_ b: MarkBlock) -> some View {
    switch b {
    case .text(let s):
      Text(MarkBlock.inline(s)).font(Face.text(size)).foregroundStyle(color).lineSpacing(3).textSelection(.enabled).tint(Ink.accent)
        .fixedSize(horizontal: false, vertical: true)
    case .heading(let s, let level):
      Text(MarkBlock.inline(s)).font(Face.display(level <= 1 ? size + 6 : level == 2 ? size + 3 : size + 1, .bold)).foregroundStyle(color)
    case .code(let s):
      ScrollView(.horizontal, showsIndicators: false) {
        Text(s).font(Face.mono(size - 3)).foregroundStyle(Ink.fg).textSelection(.enabled).padding(12)
      }
      .background(RoundedRectangle(cornerRadius: 10).fill(Ink.sunken))
      .overlay(alignment: .topTrailing) { Button { copyText(s) } label: { Image(systemName: "doc.on.doc").font(.system(size: 12)).foregroundStyle(Ink.muted).padding(8) }.buttonStyle(.plain) }
    case .table(let rows):
      ScrollView(.horizontal, showsIndicators: false) {
        Grid(alignment: .leading, horizontalSpacing: 14, verticalSpacing: 6) {
          ForEach(Array(rows.enumerated()), id: \.offset) { i, r in
            GridRow { ForEach(Array(r.enumerated()), id: \.offset) { _, c in Text(MarkBlock.inline(c)).font(Face.text(size - 2, i == 0 ? .semibold : .regular)).foregroundStyle(Ink.fg) } }
            if i == 0 { Rectangle().fill(Ink.fg).frame(height: 1.5).gridCellUnsizedAxes(.horizontal) } else { Rectangle().fill(Ink.lineStrong).frame(height: 1).gridCellUnsizedAxes(.horizontal) }
          }
        }.padding(.vertical, 4)
      }
    case .quote(let s):
      HStack(spacing: 10) { Rectangle().fill(Ink.lineStrong).frame(width: 3); Text(MarkBlock.inline(s)).font(Face.text(size)).foregroundStyle(Ink.muted) }
    case .rule:
      Rectangle().fill(Ink.line).frame(height: 1)
    }
  }
}
enum MarkBlock: Hashable {
  case text(String), heading(String, Int), code(String), table([[String]]), quote(String), rule
  static func blocks(_ text: String) -> [MarkBlock] {
    var out = [MarkBlock](), para = [String]()
    func flush() { if !para.isEmpty { out.append(.text(para.joined(separator: "\n"))); para = [] } }
    let lines = text.replacingOccurrences(of: "\r\n", with: "\n").components(separatedBy: "\n")
    var i = 0
    while i < lines.count {
      let line = lines[i], t = line.trimmingCharacters(in: .whitespaces)
      if t.hasPrefix("```") {
        flush(); var body = [String](); i += 1
        while i < lines.count && !lines[i].trimmingCharacters(in: .whitespaces).hasPrefix("```") { body.append(lines[i]); i += 1 }
        i += 1; out.append(.code(body.joined(separator: "\n"))); continue
      }
      if t.hasPrefix("|") && i + 1 < lines.count && lines[i + 1].trimmingCharacters(in: .whitespaces).range(of: #"^\|?\s*:?-{2,}"#, options: .regularExpression) != nil {
        flush()
        func cells(_ s: String) -> [String] {
          var x = s.trimmingCharacters(in: .whitespaces)
          if x.hasPrefix("|") { x.removeFirst() }
          if x.hasSuffix("|") { x.removeLast() }
          return x.components(separatedBy: "|").map { $0.trimmingCharacters(in: .whitespaces) }
        }
        var rows = [cells(t)]; i += 2
        while i < lines.count && lines[i].trimmingCharacters(in: .whitespaces).hasPrefix("|") { rows.append(cells(lines[i])); i += 1 }
        out.append(.table(rows)); continue
      }
      if let m = t.range(of: #"^#{1,4}\s"#, options: .regularExpression) { flush(); out.append(.heading(String(t[m.upperBound...]), t.prefix(while: { $0 == "#" }).count)); i += 1; continue }
      if t == "---" || t == "***" { flush(); out.append(.rule); i += 1; continue }
      if t.hasPrefix(">") { flush(); out.append(.quote(String(t.dropFirst()).trimmingCharacters(in: .whitespaces))); i += 1; continue }
      if t.isEmpty { flush(); i += 1; continue }
      para.append(line); i += 1
    }
    flush()
    return out
  }
  /** Inline markdown; `__x__` is underlined in the accent ink (the pen's line). */
  static func inline(_ s: String) -> AttributedString {
    var src = s.replacingOccurrences(of: #"(?m)^(\s*)[-*]\s+"#, with: "$1• ", options: .regularExpression)
    src = src.replacingOccurrences(of: #"(?<![\w.])__(?=\S)(.+?)(?<=\S)__(?![\w])"#, with: "⟦$1⟧", options: .regularExpression)
    var a = (try? AttributedString(markdown: src, options: AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(src)
    while let open = a.range(of: "⟦"), let close = a[open.upperBound...].range(of: "⟧") {
      let inner = open.upperBound..<close.lowerBound
      a[inner].underlineStyle = .single
      a[inner].underlineColor = UIColor { $0.userInterfaceStyle == .dark ? UIColor(rgb: 0x6fd0b5) : UIColor(rgb: 0x1b6a57) }
      a.removeSubrange(close)
      a.removeSubrange(open)
    }
    return a
  }
}

// ---- pictures and files ----------------------------------------------------------------------------------------------

struct NativePicture: View {
  @EnvironmentObject var shell: AppShell
  let id: String
  var body: some View {
    Group {
      #if canImport(UIKit)
      if let data = shell.files[id], let image = UIImage(data: data) { Image(uiImage: image).resizable().scaledToFit().accessibilityIdentifier("native-picture") } else { pending }
      #else
      if let data = shell.files[id], let image = NSImage(data: data) { Image(nsImage: image).resizable().scaledToFit().accessibilityIdentifier("native-picture") } else { pending }
      #endif
    }.task(id: id) { shell.act(["do": "open_file", "attachment_id": id, "save": false]) }
  }
  private var pending: some View {
    RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Ink.sunken).frame(height: 160).overlay { ProgressView() }
  }
}
/** A message's or a card's files: a picture small and round-cornered, any other file as a quiet chip with its size. */
struct FileRows: View {
  @EnvironmentObject var shell: AppShell
  let ids: [String]
  var body: some View {
    ForEach(ids, id: \.self) { id in
      if let file = shell.view.files[id] {
        if file.mediaType.hasPrefix("image/") {
          NativePicture(id: id).frame(maxWidth: 280, maxHeight: 280).clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
            .onTapGesture { shell.act(["do": "open_file", "attachment_id": id, "save": true]) }
        } else {
          Button { shell.act(["do": "open_file", "attachment_id": id, "save": true]) } label: {
            HStack(spacing: 10) {
              Image(systemName: "doc").font(.system(size: 15, weight: .medium)).foregroundStyle(Ink.accent)
              VStack(alignment: .leading, spacing: 1) {
                Text(file.name).font(Face.text(15, .medium)).foregroundStyle(Ink.fg).lineLimit(1).truncationMode(.middle)
                if file.size > 0 { Text(ByteCountFormatter.string(fromByteCount: Int64(file.size), countStyle: .file)).font(Face.text(12)).foregroundStyle(Ink.muted) }
              }
              if let p = file.percent { ProgressView(value: Double(p), total: 100).frame(width: 60) }
            }
            .padding(.horizontal, 12).padding(.vertical, 9)
            .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Ink.surface))
            .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(Ink.lineStrong))
          }
          .buttonStyle(.plain)
        }
      }
    }
  }
}
