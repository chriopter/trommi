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

/**
 * The iPhone's Chats: every team's chats in one list, calm as Messages', each team's name only a small section
 * title over its rows (`sidebar.stacks`, as the wide sidebar's, in the core's order). A tap on the title folds the
 * team to its stack: its faces in one small row and what it holds (`Focus.open_teams`, kept by the shell). In the
 * Search tab, while words are typed, the chats the core found under their teams (`sidebar.found`).
 */
struct ChatsScreen: View {
  @EnvironmentObject var shell: AppShell
  var searching = false
  var body: some View {
    let side = shell.view.sidebar
    let query = !shell.chatsQuery.trimmingCharacters(in: .whitespaces).isEmpty
    List {
      if searching && query {
        ForEach(side.found, id: \.id) { group in
          Section { ForEach(group.rows, id: \.id) { row in ChatRow(row: row) } } header: { TeamTitle(name: group.name.en, open: nil) }
            .listSectionSeparator(.hidden)
        }
        if let none = side.foundNone {
          Text(none.en).font(Face.text(15)).foregroundStyle(Ink.muted).frame(maxWidth: .infinity).padding(.vertical, 40)
            .listRowBackground(Color.clear).listRowSeparator(.hidden)
            .accessibilityIdentifier("chats-none")
        }
      } else {
        ForEach(side.stacks, id: \.id) { stack in
          Section {
            if stack.open { ForEach(stack.sessions, id: \.id) { row in ChatRow(row: row) } }
            else if !stack.sessions.isEmpty { FoldedChats(stack: stack) }
          } header: {
            Button { withAnimation(.snappy) { shell.fold(team: stack.id) } } label: { TeamTitle(name: stack.name.en, open: stack.sessions.isEmpty ? nil : stack.open) }
              .buttonStyle(.plain)
              .disabled(stack.sessions.isEmpty)
              .accessibilityLabel("\(stack.name.en), \(stack.says.en)")
              .accessibilityValue(stack.open ? "Open" : "Folded")
              .accessibilityIdentifier("chat-team-" + stack.id)
          }
          .listSectionSeparator(.hidden)
        }
      }
    }
    .listStyle(.plain)
    .scrollContentBackground(.hidden)
    .environment(\.defaultMinListRowHeight, 1)
    .background(Ink.bg)
    .modifier(ChatsTop(searching: searching))
    .refreshable { shell.act(["do": "retry"]) }
    .accessibilityIdentifier(searching ? "chat-search" : "chat-list")
  }
}

/** A team's name as a plain, quiet section title; with a small chevron where it folds. */
struct TeamTitle: View {
  let name: String
  let open: Bool?
  var body: some View {
    HStack(spacing: 5) {
      Text(name).font(Face.text(14, .semibold)).foregroundStyle(Ink.muted)
      if let open {
        Image(systemName: "chevron.down").font(.system(size: 10, weight: .bold)).foregroundStyle(Ink.faint)
          .rotationEffect(.degrees(open ? 0 : -90))
      }
      Spacer(minLength: 0)
    }
    .padding(.top, 10).padding(.bottom, 2).contentShape(Rectangle())
  }
}

/** A folded team in the Chats list: its faces overlapping in one small row (`faces`), what it holds (`says`), and one
 *  dot while a question of it waits. A tap opens it again. */
struct FoldedChats: View {
  @EnvironmentObject var shell: AppShell
  let stack: CoreView.TeamStackView
  var body: some View {
    Button { withAnimation(.snappy) { shell.fold(team: stack.id) } } label: {
      HStack(spacing: 10) {
        HStack(spacing: -10) {
          ForEach(stack.faces, id: \.id) { face in
            let row = stack.sessions.first { $0.id == face.id }
            let hue = Pen.hueFor(id: (row?.slug).flatMap { $0.isEmpty ? nil : $0 } ?? face.id, mark: face.mark)
            AgentMark(mark: face.mark, size: 18, hue: hue).frame(width: 34, height: 34)
              .background(Circle().fill(Tone.color(hue: hue, .wash))).overlay(Circle().stroke(Ink.bg, lineWidth: 2))
          }
        }
        Text(stack.says.en).font(Face.text(14)).foregroundStyle(Ink.muted).lineLimit(1)
        Spacer(minLength: 0)
        if stack.waiting > 0 { Circle().fill(Ink.accent).frame(width: 10, height: 10) }
      }
      .padding(.vertical, 6).contentShape(Rectangle())
    }
    .buttonStyle(.plain)
    .listRowInsets(EdgeInsets(top: 0, leading: 18, bottom: 0, trailing: 20))
    .listRowBackground(Color.clear).listRowSeparator(.hidden)
    .accessibilityLabel("\(stack.name.en): \(stack.says.en), folded")
    .accessibilityIdentifier("chat-folded-" + stack.id)
  }
}

/** The Chats tab has no bar at the top; the Search tab has the system's, for its title and field. */
private struct ChatsTop: ViewModifier {
  let searching: Bool
  func body(content: Content) -> some View {
    if searching { content.navigationTitle("Search") }
    else { content.toolbar(.hidden, for: .navigationBar).scrollEdgeEffectStyle(.soft, for: [.top, .bottom]) }
  }
}

/** One session of the Chat list, as calm as a messenger's: its drawing large and bare, the name, when it said
 *  something last at the right (faint); under it one grey line of what was said last. A session whose agent is not
 *  connected (`online` false) is grey, face and words. No dots, lines, counts or chips; what is new or waits and its
 *  helpers are said to VoiceOver only. */
struct ChatRow: View {
  @EnvironmentObject var shell: AppShell
  let row: CoreView.SessionRowView
  var body: some View {
    let side: CGFloat = 52
    let hue = Pen.hueFor(id: row.slug.isEmpty ? row.id : row.slug, mark: row.mark)
    let working = row.working
    let teaser = row.last?.text ?? row.cap?.word.en ?? ""
    // The whole row opens the chat (a button under it).
    ZStack {
      Button { shell.focus(.session(sessionId: row.id)) } label: { Rectangle().fill(Color.clear).contentShape(Rectangle()) }
        .buttonStyle(.plain)
        .accessibilityLabel([row.name.en, working ? "working" : row.online ? "" : "not connected", row.signal.isEmpty ? "" : row.says?.en ?? "New message", row.helpers?.says.en ?? "", teaser, row.last?.when.en ?? ""].filter { !$0.isEmpty }.joined(separator: ". "))
        .accessibilityIdentifier("chat-session-" + row.id)
      HStack(alignment: .center, spacing: 14) {
        // (the face bare, no disc behind it)
        AgentMark(mark: row.mark, size: side * 0.78, hue: hue)
          .frame(width: side, height: side)
          // (calm as a messenger's list: no dots; a chat whose agent is not connected is grey, face and words)
          .grayscale(row.online ? 0 : 1).opacity(row.online ? 1 : 0.45)
          .allowsHitTesting(false)
          .accessibilityHidden(true)
        VStack(alignment: .leading, spacing: 4) {
          HStack(alignment: .firstTextBaseline, spacing: 7) {
            Text(row.name.en).font(Face.text(17, .medium)).foregroundStyle(row.online ? Ink.fg : Ink.faint).lineLimit(1)
              .allowsHitTesting(false).accessibilityHidden(true)
            Spacer(minLength: 6)
            if let when = row.last?.when.en {
              Text(when).font(Face.text(14, relativeTo: .footnote)).foregroundStyle(Ink.faint.opacity(0.6)).lineLimit(1).fixedSize()
                .allowsHitTesting(false).accessibilityHidden(true)
            }
          }
          HStack(alignment: .center, spacing: 10) {
            Text(teaser.isEmpty ? " " : teaser).font(Face.text(15, relativeTo: .subheadline))
              .foregroundStyle(!row.online ? Ink.faint : row.last == nil && (row.cap?.kind == "unheard" || row.cap?.kind == "cut") ? Ink.urgHigh : Ink.muted)
              .lineLimit(1)
              .frame(maxWidth: .infinity, alignment: .leading)
              .allowsHitTesting(false)
              .accessibilityIdentifier("chat-teaser-" + row.id)
          }
        }
      }
      .padding(.vertical, 12)
    }
    .listRowInsets(EdgeInsets(top: 0, leading: 18, bottom: 0, trailing: 20))
    .listRowBackground(Color.clear)
    .listRowSeparator(.hidden)
  }
}

// ---- a card's page ---------------------------------------------------------------------------------------------------

/**
 * A card's page, calm (owner's pick of 17 October): who and when in one quiet line, the title and the text in the
 * system's face, the answers as plain rows, the three side ways as one small row, the talk folded. Once answered here
 * the way on stands where the answers stood: one glass button "Next" with the next card's title, the answer that
 * stands quietly under it with Undo. Every word comes from the core (`card.words`, `card.after`).
 */
struct CardScreen: View {
  @EnvironmentObject var shell: AppShell
  let card: CoreView.CardView
  @State private var note = ""
  @State private var choices: Set<String> = []
  @State private var attachments: [String] = []
  /** Answered on this page just now: the page then offers the way on (`card.after`). */
  @State private var answeredHere = false
  /** The card's talk unfolded. */
  @State private var showTalk = false
  /** The pictures open over the whole screen. */
  @State private var showing: PictureShow?
  @Namespace private var zoom
  private func ratioOf(_ p: CoreView.PictureView) -> CGFloat? {
    guard let w = p.width, let h = p.height, w > 0, h > 0 else { return nil }
    return CGFloat(w) / CGFloat(h)
  }
  private var urgency: String { cardUrgency(card.urgency) }
  private var who: TalkWho? { card.from.map { TalkWho(mark: $0.mark, name: $0.name, hue: Pen.hueFor(id: $0.sessionId, mark: $0.mark)) } }
  /** The quiet fill of a row and a side way. */
  private var calmFill: Color { Ink.fg.opacity(0.055) }
  var body: some View {
    ScrollView {
      VStack(alignment: .leading, spacing: 22) {
        lead
        if !card.pictures.isEmpty {
          VStack(spacing: 12) {
            ForEach(Array(card.pictures.enumerated()), id: \.offset) { i, picture in
              CardPicture(fileId: picture.fileId, ratio: ratioOf(picture), zoom: zoom) {
                showing = PictureShow(shots: card.pictures.map { Shot(fileId: $0.fileId, caption: $0.caption) }, start: i)
              }
            }
          }
        }
        FileRows(ids: card.files)
        answers
        if card.answer.mode == "options" && !card.sections.isEmpty { sections }
        if let n = card.note { LinkNoteBox(note: n) }
        thread
      }
      .padding(.horizontal, 18).padding(.top, 8).padding(.bottom, 24)
      .frame(maxWidth: 760).frame(maxWidth: .infinity)
    }
    .scrollDismissesKeyboard(.interactively)
    .background(Ink.bg.ignoresSafeArea())
    .pictureViewer($showing, zoom: zoom)
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
    .onChange(of: card.status) { was, now in withAnimation(.snappy) { answeredHere = was == "open" && now != "open" } }
    .onChange(of: note) { _, value in shell.act(["do": "draft", "place": card.id, "text": value]) }
  }

  // ---- the head: who and when, the work strip, the title, the text ----

  @ViewBuilder private var lead: some View {
    VStack(alignment: .leading, spacing: 12) {
      if let work = card.work { CardWorkStrip(work: work) }
      if let from = card.from {
        Button { shell.focus(.session(sessionId: from.sessionId)) } label: {
          HStack(spacing: 7) {
            // (a card that knocks: a small dot in its urgency's colour, the words for a screen reader)
            if let knock = card.knock { Circle().fill(Ink.urgency(urgency)).frame(width: 7, height: 7).accessibilityLabel(knock.en) }
            AgentMark(from: from, size: 18)
            Text(from.name).font(.subheadline.weight(.semibold)).foregroundStyle(Ink.fg)
            if card.askedMs > 0 { Text(talkAgo(card.askedMs)).font(.subheadline).foregroundStyle(Ink.muted) }
          }
          .lineLimit(1)
          .frame(minHeight: 32, alignment: .leading).contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel("From \(from.path.joined(separator: ", "))")
      }
      Text(card.title).font(.system(size: 26, weight: .bold)).foregroundStyle(Ink.fg).fixedSize(horizontal: false, vertical: true).textSelection(.enabled)
        .accessibilityIdentifier("card-title")
      if let into = card.mergedInto {
        Button { shell.focus(.card(cardId: into.id)) } label: { Text("Nr. \(into.number): \(into.title)").font(.subheadline.weight(.medium)).foregroundStyle(Ink.accent).multilineTextAlignment(.leading) }.buttonStyle(.plain)
      }
      if !card.mergedFrom.isEmpty {
        VStack(alignment: .leading, spacing: 4) {
          ForEach(card.mergedFrom, id: \.id) { o in
            Button { shell.focus(.card(cardId: o.id)) } label: { Text("Nr. \(o.number) · \(o.title)").font(.subheadline).foregroundStyle(Ink.accent).lineLimit(1) }.buttonStyle(.plain)
          }
        }
      }
      if !card.text.isEmpty { Markdown(text: card.text, size: 17, system: true) }
      if !card.html.isEmpty { InlineHTML(html: card.html).frame(minHeight: 240).clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous)) }
    }
  }

  // ---- the answers ----

  @ViewBuilder private var answers: some View {
    let a = card.answer
    let w = card.words
    VStack(alignment: .leading, spacing: 10) {
      switch a.mode {
      case "options":
        ForEach(card.options, id: \.key) { o in option(o) }
        if card.multiple {
          Button { answer(Array(choices)) } label: {
            Text("Send the answer").font(.body.weight(.semibold)).foregroundStyle(Ink.accentFg).frame(maxWidth: .infinity, minHeight: 54)
              .background(RoundedRectangle(cornerRadius: 18, style: .continuous).fill(Ink.accent))
          }
          .buttonStyle(TalkPressStyle()).disabled(choices.isEmpty).opacity(choices.isEmpty ? 0.45 : 1)
        }
        if card.kind == "decision" { sideWays(duck: true).padding(.top, 2) }
      case "read":
        row(w.gotIt.en, prominent: true) { shell.act(["do": "read", "card_id": card.id]) }
        sideWays(duck: false).padding(.top, 2)
      case "request":
        HStack(spacing: 8) {
          Button { verdict(false) } label: { Text("Deny").font(.body.weight(.semibold)).frame(maxWidth: .infinity, minHeight: 56) }.buttonStyle(TileStyle(lead: false, urgency: urgency))
          Button { verdict(true) } label: { Text("Allow").font(.body.weight(.semibold)).frame(maxWidth: .infinity, minHeight: 56) }.buttonStyle(TileStyle(lead: true, urgency: urgency))
        }
      case "said":
        // the way on where the answers stood, the answer quietly under it
        if answeredHere, let after = card.after { nextButton(after).transition(.opacity.combined(with: .scale(scale: 0.97))) }
        said(a)
        ForEach(Array(a.optionNotes.enumerated()), id: \.offset) { _, n in still(n.label, n.note.en) }
        if let settled = a.settled { still("✓ Settled by your answer", settled.en) }
        if !a.summary.isEmpty { still("Done by the agent", a.summary) }
        if a.finished { sideCapsule(label: { Text(w.what.en) }) { shell.act(["do": "hand_back", "card_id": card.id]) } }
      default:
        if let label = a.label { still(label.en, a.detail?.en ?? "") }
        else if let says = a.says { Text(says.en).font(.subheadline).foregroundStyle(Ink.muted) }
        if a.did != nil || !a.back.isEmpty { said(a) }
      }
    }
  }
  /** One answer: a plain row; the advised one with a hair of the accent round it and "Recommended" at its end. */
  @ViewBuilder private func option(_ o: CoreView.OptionView) -> some View {
    let on = choices.contains(o.key)
    Button {
      if card.multiple { if on { choices.remove(o.key) } else { choices.insert(o.key) } }
      else { answer([o.key]) }
    } label: {
      HStack(alignment: .center, spacing: 12) {
        if card.multiple { Image(systemName: on ? "checkmark.circle.fill" : "circle").font(.system(size: 22)).foregroundStyle(on ? Ink.accent : Ink.muted) }
        VStack(alignment: .leading, spacing: 3) {
          Text(o.label).font(.body.weight(.semibold)).foregroundStyle(Ink.fg).multilineTextAlignment(.leading)
          if !o.detail.isEmpty { Text(o.detail).font(.subheadline).foregroundStyle(Ink.muted).multilineTextAlignment(.leading) }
          if !o.note.isEmpty { Text(o.note).font(.subheadline).italic().foregroundStyle(Ink.muted) }
        }
        Spacer(minLength: 6)
        if o.recommended { Text(card.words.recommended.en).font(.caption.weight(.semibold)).foregroundStyle(Ink.accent) }
      }
      .padding(.horizontal, 18).padding(.vertical, 15).frame(maxWidth: .infinity, minHeight: 56, alignment: .leading)
      .background(RoundedRectangle(cornerRadius: 18, style: .continuous).fill(calmFill))
      .overlay(RoundedRectangle(cornerRadius: 18, style: .continuous).strokeBorder(on ? Ink.accent : o.recommended ? Ink.accent.opacity(0.55) : .clear, lineWidth: 1.5))
      .contentShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
    }
    .buttonStyle(TalkPressStyle())
    .accessibilityLabel(o.tip.map { "\(o.label), \($0.en)" } ?? o.label)
    .accessibilityAddTraits(on ? .isSelected : [])
  }
  private func row(_ label: String, prominent: Bool, _ action: @escaping () -> Void) -> some View {
    Button(action: action) {
      Text(label).font(.body.weight(.semibold)).foregroundStyle(prominent ? Ink.accentFg : Ink.fg)
        .padding(.horizontal, 18).frame(maxWidth: .infinity, minHeight: 56)
        .background(RoundedRectangle(cornerRadius: 18, style: .continuous).fill(prominent ? Ink.accent : calmFill))
        .contentShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
    }
    .buttonStyle(TalkPressStyle())
  }
  /** The side ways as one quiet row: the duck (a decision), What??, Rework. */
  private func sideWays(duck: Bool) -> some View {
    let w = card.words
    return HStack(spacing: 8) {
      if duck {
        sideCapsule(label: {
          HStack(spacing: 6) { PenMark(doc: PenStore.doc("sketch:duck"), inks: PenInks(stroke: Ink.fg, duck: true, water: Ink.fg)).frame(width: 24, height: 20); Text(w.duck.en) }
        }) { shell.act(["do": "answer", "card_id": card.id, "choices": card.options.filter(\.recommended).map(\.key), "trusted": true, "note": note]) }
      }
      sideCapsule(label: { Text(w.what.en) }) { shell.act(["do": "hand_back", "card_id": card.id, "note": note]) }
      sideCapsule(label: { HStack(spacing: 6) { Image(systemName: "arrow.uturn.left"); Text(w.rework.en) } }) {
        shell.act(["do": "hand_back", "card_id": card.id, "note": note, "rework": true])
      }
    }
  }
  private func sideCapsule<L: View>(@ViewBuilder label: () -> L, _ action: @escaping () -> Void) -> some View {
    Button(action: action) {
      label().font(.subheadline.weight(.medium)).foregroundStyle(Ink.fg).lineLimit(1)
        .frame(maxWidth: .infinity, minHeight: 44).background(Capsule().fill(calmFill)).contentShape(Capsule())
    }
    .buttonStyle(TalkPressStyle())
  }
  /** The options' own paragraphs, after the answers: one block per option, open. */
  private var sections: some View {
    VStack(alignment: .leading, spacing: 14) {
      ForEach(Array(card.sections.enumerated()), id: \.offset) { _, s in
        VStack(alignment: .leading, spacing: 4) {
          Text("\(Text(s.label).font(.headline))\(Text(s.recommended ? "  \(card.words.recommended.en)" : "").font(.caption.weight(.semibold)).foregroundColor(Ink.accent))")
          if !s.text.isEmpty { Markdown(text: s.text, size: 15, system: true) }
          if !s.html.isEmpty { InlineHTML(html: s.html).frame(minHeight: 160) }
        }
      }
    }
    .padding(.top, 4)
  }
  private func still(_ label: String, _ detail: String = "") -> some View {
    VStack(alignment: .leading, spacing: 3) {
      Text(label).font(.body.weight(.semibold)).foregroundStyle(Ink.fg)
      if !detail.isEmpty { Text(detail).font(.subheadline).foregroundStyle(Ink.muted) }
    }
    .padding(.horizontal, 18).padding(.vertical, 14).frame(maxWidth: .infinity, alignment: .leading)
    .background(RoundedRectangle(cornerRadius: 18, style: .continuous).fill(calmFill))
  }
  /** What was said, in one quiet line: a tick, the answer, when, and Undo at its end. */
  private func said(_ a: CoreView.AnswerView) -> some View {
    HStack(spacing: 8) {
      if a.label != nil { Image(systemName: "checkmark.circle.fill").foregroundStyle(Ink.accent).accessibilityHidden(true) }
      VStack(alignment: .leading, spacing: 2) {
        Text(a.label?.en ?? a.did?.en ?? "").font(.subheadline.weight(.semibold)).foregroundStyle(Ink.fg).lineLimit(2)
        if let d = a.detail?.en, !d.isEmpty { Text(d).font(.footnote).foregroundStyle(Ink.muted).lineLimit(2) }
      }
      if let at = a.atMs { Text(talkAgo(at)).font(.subheadline).foregroundStyle(Ink.muted).lineLimit(1) }
      Spacer(minLength: 6)
      if a.back == "reopen" || a.back == "takeback" {
        Button(card.words.undo.en) { shell.act(["do": a.back == "reopen" ? "decide_again" : "take_back", "card_id": card.id]) }
          .font(.subheadline.weight(.semibold)).foregroundStyle(Ink.muted).frame(minWidth: 44, minHeight: 44)
          .accessibilityLabel(a.back == "reopen" ? "Take back your answer" : "Take back the card")
      }
    }
    .padding(.horizontal, 6)
    .accessibilityElement(children: .combine)
  }
  /** The way on: one glass button in the accent, "Next" and the next card's title; "All done" when none waits. */
  private func nextButton(_ after: CoreView.AfterView) -> some View {
    Button {
      if !shell.path.isEmpty { shell.path.removeLast() }
      if let n = after.next { shell.focus(.card(cardId: n.id)) }
    } label: {
      HStack(spacing: 12) {
        VStack(alignment: .leading, spacing: 2) {
          Text(after.go.en).font(.headline)
          if let n = after.next { Text(n.title).font(.subheadline).opacity(0.78).lineLimit(1) }
        }
        Spacer(minLength: 8)
        Image(systemName: after.next != nil ? "chevron.right" : "checkmark").font(.system(size: 17, weight: .semibold))
      }
      .foregroundStyle(Ink.accentFg)
      .padding(.horizontal, 22).frame(maxWidth: .infinity, minHeight: 60, alignment: .leading)
      .glass(Capsule(), interactive: true, tint: Ink.accent)
      .contentShape(Capsule())
    }
    .buttonStyle(.plain)
    .accessibilityLabel(after.next.map { "\(after.says.en): \($0.title)" } ?? after.says.en)
  }

  // ---- the card's own talk, folded into one quiet row ----

  @ViewBuilder private var thread: some View {
    if card.words.talk != nil || !card.lead.isEmpty || card.earlier != nil {
      VStack(alignment: .leading, spacing: 10) {
        ForEach(Array(card.lead.enumerated()), id: \.offset) { _, l in
          VStack(alignment: .leading, spacing: 6) {
            Text(l.label.en).font(.footnote.weight(.semibold)).foregroundStyle(Ink.muted)
            if !l.text.isEmpty { Markdown(text: l.text, size: 15, system: true) }
            if !l.html.isEmpty { InlineHTML(html: l.html).frame(minHeight: 200) }
            ForEach(l.links, id: \.url) { link in
              if let url = URL(string: link.url) { Link(destination: url) { Text("\(Text(link.label).foregroundColor(Ink.fg))  \(Text(link.short).foregroundColor(Ink.accent))").font(.subheadline) } }
            }
          }
          .padding(12).frame(maxWidth: .infinity, alignment: .leading)
          .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(calmFill))
        }
        if let fold = card.words.talk {
          Button { withAnimation(.snappy) { showTalk.toggle() } } label: {
            HStack(spacing: 6) {
              Image(systemName: "bubble.left.and.text.bubble.right").font(.subheadline)
              Text(fold.en).font(.subheadline.weight(.medium))
              Spacer()
              Image(systemName: showTalk ? "chevron.up" : "chevron.down").font(.caption.weight(.semibold))
            }
            .foregroundStyle(Ink.muted).frame(minHeight: 44).contentShape(Rectangle())
          }
          .buttonStyle(.plain)
          .accessibilityAddTraits(showTalk ? .isSelected : [])
        }
        if showTalk || card.words.talk == nil {
          if let earlier = card.earlier {
            Button(earlier.says.en) { shell.act(["do": "show_more", "of": ["list": "comments", "card": card.id]]) }.font(.subheadline.weight(.semibold)).foregroundStyle(Ink.accent)
          }
          ForEach(Array(card.talk.enumerated()), id: \.element.id) { i, row in TalkRow(row: row, who: who, inCard: true, last: i + 1 == card.talk.count || !card.talk[i + 1].cont) }
          ForEach(card.versions, id: \.n) { v in RevisionRow(version: v) }
        }
      }
      .padding(.top, 6)
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

/** The finger on an option: a little darker and a hair smaller, flat. */
struct TalkPressStyle: ButtonStyle {
  func makeBody(configuration: Configuration) -> some View {
    configuration.label
      .background(Ink.fg.opacity(configuration.isPressed ? 0.07 : 0))
      .scaleEffect(configuration.isPressed ? 0.97 : 1).animation(.easeOut(duration: 0.1), value: configuration.isPressed)
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

/** The field at the bottom of a chat and a card: a round glass "+" for photos and files, a growing glass field, the round Send. */
struct TalkComposer: View {
  @EnvironmentObject var shell: AppShell
  let placeholder: String
  @Binding var text: String
  @Binding var attachments: [String]
  /** The chat this field belongs to: with it, sealed dictation (spec/v1.md 18) lives in the send button's place. */
  var session: String? = nil
  let send: () -> Void
  @State private var keyboard = false
  @State private var said = ""
  @State private var taken = ""
  @State private var levels: [Double] = []
  private var canSend: Bool { !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !attachments.isEmpty }
  /** The core's dictation as this field shows it: another chat's runs elsewhere, so here it is idle; none without a chat. */
  private var dictation: String {
    guard let session else { return "off" }
    let d = shell.view.dictation
    if d.state.isEmpty || d.state == "off" { return "off" }
    return d.session.isEmpty || d.session == session ? d.state : "idle"
  }
  private func dictate(_ step: [String: Any]) { shell.act(["do": "dictate", "step": step]) }
  var body: some View {
    VStack(alignment: .leading, spacing: 6) {
      if !said.isEmpty {
        Text(said).font(Face.text(13)).foregroundStyle(Ink.urgCritical).padding(.leading, 27 + 51).padding(.trailing, 25)
      }
      field
    }
    .onChange(of: shell.view.dictation) { _, d in took(d) }
  }
  /** The words of a dictation into this field (once), a problem said above it. */
  private func took(_ d: CoreView.DictationView) {
    guard let session, d.session == session else { return }
    if d.state == "done", let t = d.text, t.id != taken {
      taken = t.id
      let gap = text.isEmpty || text.hasSuffix(" ") || text.hasSuffix("\n") ? "" : " "
      text += gap + t.text
      dictate(["step": "taken", "id": t.id])
    }
    if d.state == "failed", let p = d.problem {
      said = p.en
      dictate(["step": "dismiss"])
      Task { try? await Task.sleep(for: .seconds(8)); said = "" }
    }
  }
  @ViewBuilder private var recordingRow: some View {
    HStack(spacing: 10) {
      Button { dictate(["step": "cancel"]) } label: { Image(systemName: "trash").font(.system(size: 16)).foregroundStyle(Ink.muted).frame(width: 36, height: 40) }
        .buttonStyle(.plain).accessibilityLabel("Throw the recording away")
      if dictation == "working" {
        Text("Writing…").font(Face.text(16)).foregroundStyle(Ink.muted)
        Spacer(minLength: 0)
      } else {
        Circle().fill(Ink.urgCritical).frame(width: 9, height: 9)
        TimelineView(.periodic(from: .now, by: 0.12)) { tl in
          HStack(spacing: 10) {
            let since = Double(shell.view.dictation.sinceMs) / 1000
            let sec = max(0, Int(tl.date.timeIntervalSince1970 - since))
            Text(String(format: "%d:%02d", sec / 60, sec % 60)).font(Face.text(16)).monospacedDigit()
            // (drawn into whatever width is left, the newest at the right: the bars never widen the row)
            let levels = bars(tl.date)
            Canvas { context, size in
              let step: CGFloat = 5, fits = max(0, Int(size.width / step))
              for (i, v) in levels.suffix(fits).enumerated() {
                let h = 3 + 17 * CGFloat(v)
                context.fill(Path(roundedRect: CGRect(x: CGFloat(i) * step, y: (size.height - h) / 2, width: 3, height: h), cornerRadius: 1.5), with: .color(Ink.muted))
              }
            }
            .frame(minWidth: 0, maxWidth: .infinity).frame(height: 22)
            .accessibilityHidden(true)
          }
        }
        .frame(minWidth: 0, maxWidth: .infinity)
      }
      Image(systemName: "lock").font(.system(size: 12)).foregroundStyle(Ink.faint)
        .accessibilityLabel(shell.view.dictation.hint?.en ?? "Sealed")
      Button { dictate(["step": "stop"]) } label: {
        RoundedRectangle(cornerRadius: 3).fill(Color.white).frame(width: 12, height: 12)
          .frame(width: 32, height: 32).background(Circle().fill(Ink.urgCritical)).opacity(dictation == "working" ? 0.5 : 1)
          .frame(width: 40, height: 40).contentShape(Rectangle())
      }.buttonStyle(.plain).disabled(dictation == "working").accessibilityLabel("Stop and write it").accessibilityIdentifier("composer-dictate-stop")
    }
    .padding(.leading, 6).frame(minWidth: 0, maxWidth: .infinity, minHeight: 40)
  }
  private func bars(_ now: Date) -> [Double] {
    DispatchQueue.main.async {
      levels.append(NativeDictation.shared.level)
      if levels.count > 60 { levels.removeFirst(levels.count - 60) }
    }
    return levels
  }
  private var field: some View {
    HStack(alignment: .bottom, spacing: 11) {
      AttachMenu(ids: $attachments) {
        Image(systemName: "plus").font(.system(size: 19, weight: .semibold)).foregroundStyle(Ink.fg).frame(width: 40, height: 40).glass(Circle(), interactive: true)
      }
      VStack(alignment: .leading, spacing: 0) {
        if !attachments.isEmpty {
          ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 6) {
              ForEach(attachments, id: \.self) { id in
                if shell.view.files[id]?.picture == true { PictureChip(id: id) { attachments.removeAll { $0 == id } } } else {
                HStack(spacing: 6) {
                  Image(systemName: "paperclip").font(.system(size: 11, weight: .semibold))
                  Text(shell.view.files[id]?.name ?? "File").font(Face.text(13)).lineLimit(1).frame(maxWidth: 140)
                  if let p = shell.view.files[id]?.percent { Text("\(p) %").font(Face.text(11)).monospacedDigit().foregroundStyle(Ink.muted) }
                  Button { attachments.removeAll { $0 == id } } label: { Image(systemName: "xmark").font(.system(size: 10, weight: .bold)).frame(minWidth: 28, minHeight: 36).contentShape(Rectangle()) }.buttonStyle(.plain).accessibilityLabel("Remove")
                }
                .padding(.leading, 10).padding(.trailing, 4).background(Capsule().fill(Ink.sunken))
                }
              }
            }.padding(.horizontal, 10).padding(.top, 8)
          }
        }
        if dictation == "recording" || dictation == "working" { recordingRow } else {
        HStack(alignment: .bottom, spacing: 4) {
          TextField(placeholder, text: $text, axis: .vertical)
            .accessibilityIdentifier("composer-field")
            .font(Face.text(17)).lineLimit(1...7)
            .padding(.leading, 16).padding(.vertical, 9)
          if dictation != "off" && !canSend {
            Button { said = ""; levels = []; dictate(dictation == "starting" ? ["step": "cancel"] : ["step": "start", "session": session ?? ""]) } label: {
              Image(systemName: "mic").font(.system(size: 16, weight: .bold))
                .foregroundStyle(Ink.accentFg).frame(width: 32, height: 32).background(Circle().fill(Ink.accent)).opacity(dictation == "starting" ? 0.6 : 1)
                .frame(width: 40, height: 40).contentShape(Rectangle())
            }
            .buttonStyle(.plain).accessibilityLabel("Dictate (sealed)").accessibilityIdentifier("composer-dictate")
          } else {
          Button(action: send) {
            Image(systemName: "arrow.up").font(.system(size: 16, weight: .bold))
              .foregroundStyle(Ink.accentFg).frame(width: 32, height: 32).background(Circle().fill(canSend ? Ink.accent : Ink.faint))
              .frame(width: 40, height: 40).contentShape(Rectangle())
          }
          .buttonStyle(.plain).disabled(!canSend).accessibilityLabel("Send").accessibilityIdentifier("composer-send")
          }
        }
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
  }
}

// ---- a session's conversation ----------------------------------------------------------------------------------------

struct SessionScreen: View {
  @EnvironmentObject var shell: AppShell
  let page: CoreView.SessionView
  @State private var text = ""
  @State private var attachments: [String] = []
  /** The switch "All | Questions": the talk, or only the questions (the web's /chat/<id>/questions). */
  @State private var onlyQuestions = false
  @State private var renaming = false
  @State private var newName = ""
  @State private var drawing = false
  @State private var workDetails = false
  private var hue: Int { Pen.hueFor(id: page.slug.isEmpty ? page.id : page.slug, mark: page.mark) }
  var body: some View {
    ScrollViewReader { reader in
      ScrollView {
        LazyVStack(alignment: .leading, spacing: 0) {
          if onlyQuestions { SessionQuestions(page: page) } else {
          if let earlier = page.earlier {
            Button { shell.act(["do": "show_more", "of": ["list": "talk", "session": page.id]]) } label: {
              Text(earlier.says.en).font(Face.text(14, .semibold)).foregroundStyle(Ink.accent).frame(maxWidth: .infinity).padding(.vertical, 14)
            }.buttonStyle(.plain)
          }
          if let empty = page.empty { EmptyChat(page: page, empty: empty, hue: hue) }
          ForEach(Array(page.talk.enumerated()), id: \.element.id) { i, row in
            // (a run of the agent's bubbles ends where the next row does not go on from it: its time stands there)
            TalkRow(row: row, who: TalkWho(mark: page.mark, name: page.name.en, hue: hue), last: i + 1 == page.talk.count || !page.talk[i + 1].cont)
              .padding(.top, row.cont ? 2 : 10)
              .id(row.id)
          }
          if !AppShell.phone { SessionStatus(page: page, hue: hue) }
          // (an iPhone, the calm chat: the agent at work is only its face drawing itself where its answer will come;
          //  a tap opens the details)
          if AppShell.phone, let working = page.working {
            Button { workDetails = true } label: { DrawingFace(mark: page.mark, hue: hue).padding(.vertical, 8).padding(.trailing, 24).contentShape(Rectangle()) }
              .buttonStyle(.plain).padding(.top, 12)
              .accessibilityLabel(working.says.en).accessibilityHint("Shows what it works on")
              .accessibilityIdentifier("working-face")
          }
          }
          Color.clear.frame(height: 1).id("bottom")
        }
        .padding(.horizontal, 14).padding(.top, 6)
        .frame(maxWidth: 760).frame(maxWidth: .infinity)
      }
      .defaultScrollAnchor(onlyQuestions ? .top : .bottom)
      .safeAreaInset(edge: .top, spacing: 0) {
        if page.open > 0 || onlyQuestions { QuestionsChip(onlyQuestions: $onlyQuestions, count: page.open).padding(.bottom, 4) }
      }
      .scrollDismissesKeyboard(.interactively)
      .onChange(of: page.talk.count) { _, _ in withAnimation(.easeOut(duration: 0.25)) { reader.scrollTo("bottom", anchor: .bottom) } }
      .bottomBar {
        VStack(spacing: 0) {
          if let n = page.note { LinkNoteBox(note: n).padding(.horizontal, 12).padding(.top, 8) }
          TalkComposer(placeholder: page.compose.en, text: $text, attachments: $attachments, session: page.id) {
            shell.act(["do": "write", "session_id": page.id, "text": text, "attachments": attachments]); text = ""; attachments = []
            withAnimation { reader.scrollTo("bottom", anchor: .bottom) }
          }
        }
      }
      .pushedPills(solid: AppShell.phone) { header } trailing: { EmptyView() }
    }
    .background(Ink.bg.ignoresSafeArea())
    .alert("Rename Session", isPresented: $renaming) {
      TextField("Name", text: $newName).accessibilityIdentifier("session-rename-field")
      Button("Cancel", role: .cancel) {}
      Button("Rename") { let n = newName.trimmingCharacters(in: .whitespaces); if !n.isEmpty { shell.act(["do": "rename_session", "session_id": page.id, "name": n]) } }
    }
    .sheet(isPresented: $workDetails) { WorkDetails(page: page) }
    .sheet(isPresented: $drawing) { DrawingPicker(page: page) { mark in shell.act(["do": "session_icon", "session_id": page.id, "icon": mark]); drawing = false } }
  }
  /** The title in its own glass pill in the middle of the top row (back is a round button of its own; no ⋯): the
   *  session's drawing and its name, both grey while it is not connected (the core's `online`), and a small chevron;
   *  a tap opens the chat's menu. */
  private var header: some View {
    Menu { actions } label: {
      HStack(spacing: 7) {
        AgentMark(mark: page.mark, size: 22, hue: hue).grayscale(page.online ? 0 : 1).opacity(page.online ? 1 : 0.5)
        Text(page.name.en).font(Face.text(17, .semibold)).foregroundStyle(page.online ? Ink.fg : Ink.faint).lineLimit(1).minimumScaleFactor(0.85).truncationMode(.middle)
        Image(systemName: "chevron.down").font(.system(size: 11, weight: .bold)).foregroundStyle(Ink.faint)
      }
      .padding(.horizontal, 16).frame(height: 44)
      .glass(Capsule(), interactive: true)
      .contentShape(Capsule())
    }
    .buttonStyle(.plain)
    .accessibilityLabel("\(page.name.en)\(page.modelLabel.isEmpty ? "" : ", \(page.modelLabel)"), \(page.dot.says.en)")
    .accessibilityHint(page.quiet?.en ?? "Opens the chat's menu")
    .accessibilityIdentifier("session-title")
  }
  /** The chat's menu (under its title). */
  @ViewBuilder private var actions: some View {
    Toggle(isOn: $onlyQuestions) { Label(page.open > 0 ? "Questions Only (\(page.open))" : "Questions Only", systemImage: "questionmark.circle") }
    Button { newName = page.name.en; renaming = true } label: { Label("Rename…", systemImage: "pencil") }
    Button { drawing = true } label: { Label("Change Icon…", systemImage: "scribble") }
    Toggle(isOn: Binding(get: { page.crowned }, set: { _ in shell.act(["do": "crown_session", "session_id": page.id, "on": !page.crowned]) })) { Label("Main Session", systemImage: "crown") }
    let others = shell.view.sidebar.teams.filter { $0.id != page.teamId }
    if !others.isEmpty {
      Menu { ForEach(others, id: \.id) { team in Button(team.name.en) { shell.act(["do": "move_session", "session_id": page.id, "team_id": team.id]) } } } label: { Label("Move to Team…", systemImage: "rectangle.stack") }
    }
    Button(role: .destructive) { shell.act(["do": "archive_session", "session_id": page.id, "on": true]) } label: { Label("Archive", systemImage: "archivebox") }
    // (what it runs on and how much of its plan is used: in the menu, not in the top row)
    if !page.modelLabel.isEmpty || !(page.usage?.meters.isEmpty ?? true) {
      Section(page.modelLabel) {
        ForEach(page.usage?.meters ?? [], id: \.key) { m in Label("\(m.long): \(m.used) %", systemImage: m.hot ? "gauge.with.dots.needle.100percent" : "gauge.with.dots.needle.33percent") }
      }
    }
  }
}

/** Under the top row, only while questions wait (or while they are shown): a small chip with how many; a tap shows
 *  only the questions (the web's /chat/<id>/questions), a second tap all messages again. */
struct QuestionsChip: View {
  @Binding var onlyQuestions: Bool
  let count: UInt32
  var body: some View {
    Button { withAnimation(.snappy) { onlyQuestions.toggle() } } label: {
      HStack(spacing: 6) {
        if !onlyQuestions && count > 0 {
          Text("\(count)").font(Face.text(11, .bold)).foregroundStyle(Ink.bg).monospacedDigit()
            .padding(.horizontal, 5).frame(minWidth: 17, minHeight: 17).background(Capsule().fill(Ink.urgHigh))
        }
        Text(onlyQuestions ? "All messages" : "Questions").font(Face.text(13, .semibold)).foregroundStyle(Ink.fg)
      }
      .padding(.horizontal, 11).frame(height: 30)
      .glass(Capsule(), interactive: true)
      .frame(minHeight: 44).contentShape(Rectangle())
    }
    .buttonStyle(.plain)
    .frame(maxWidth: .infinity)
    .accessibilityIdentifier(onlyQuestions ? "session-mode-all" : "session-mode-questions")
  }
}

/** Only the questions of a session: what waits for him as the Desk's cards (`SessionView.waiting`, what the count
 *  counts), then every question it asked (the web's `questionList`, from `SessionView.questions`). */
struct SessionQuestions: View {
  @EnvironmentObject var shell: AppShell
  let page: CoreView.SessionView
  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      Text(page.openWord?.en ?? "No question waits for you.").font(Face.display(22, .bold)).foregroundStyle(Ink.fg).padding(.top, 10)
      ForEach(page.waiting, id: \.id) { row in DeskRow(row: row) }
      if !page.questions.isEmpty {
        HStack(spacing: 8) {
          Text("ALL QUESTIONS").font(Face.text(12, .semibold)).kerning(0.9).foregroundStyle(Ink.muted)
          Text("\(page.questions.count)").font(Face.text(12, .semibold)).foregroundStyle(Ink.faint)
          DashedRule().frame(height: 1)
        }.padding(.top, 12)
        ForEach(page.questions, id: \.id) { q in
          Button { shell.focus(.card(cardId: q.id)) } label: {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
              Text(q.number > 0 ? "Nr. \(q.number)" : "").font(Face.text(13, .medium)).foregroundStyle(Ink.faint).frame(minWidth: 44, alignment: .leading)
              Text(q.title).font(Face.text(16)).foregroundStyle(Ink.fg).lineLimit(2).multilineTextAlignment(.leading)
              Spacer(minLength: 0)
            }.padding(.vertical, 8).contentShape(Rectangle())
          }
          .buttonStyle(.plain)
          .accessibilityIdentifier("session-question-" + q.id)
        }
      }
    }
    .accessibilityElement(children: .contain)
    .accessibilityIdentifier("session-questions")
  }
}

/** The drawings a session can have, as a grid; a tap gives it that one ("draw:<name>"). */
struct DrawingPicker: View {
  let page: CoreView.SessionView
  let pick: (String) -> Void
  var body: some View {
    NavigationStack {
      ScrollView {
        LazyVGrid(columns: [GridItem(.adaptive(minimum: 56), spacing: 10)], spacing: 10) {
          ForEach(Pen.drawings, id: \.self) { name in
            let mark = "draw:" + name
            Button { pick(mark) } label: {
              AgentMark(mark: mark, size: 30, hue: Pen.hueFor(id: page.slug.isEmpty ? page.id : page.slug, mark: mark))
                .frame(width: 56, height: 56)
                .background(RoundedRectangle(cornerRadius: 12).fill(page.mark == mark ? Ink.accentSoft : Ink.surface))
                .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(page.mark == mark ? Ink.accent : Ink.line))
            }
            .buttonStyle(.plain).accessibilityLabel(name).accessibilityAddTraits(page.mark == mark ? .isSelected : [])
          }
        }.padding(16)
      }
      .background(Ink.bg)
      .navigationTitle("Change Icon")
      #if os(iOS)
      .navigationBarTitleDisplayMode(.inline)
      #endif
    }
    .presentationDetents([.medium, .large])
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

/** A session's helpers (its agent's subagents), above its status lines: "3 helpers", then a row each with a dot while
 *  it works, its name and how it stands. No way into a chat of their own: their cards and words are the session's. */
struct HelperList: View {
  let list: CoreView.HelperListView
  var body: some View {
    VStack(alignment: .leading, spacing: 6) {
      (Text(list.head.en).font(Face.text(14, .bold)).foregroundColor(Ink.fg) + Text("  " + list.sub.en).font(Face.text(12)).foregroundColor(Ink.muted))
      ForEach(Array(list.rows.enumerated()), id: \.offset) { _, h in
        HStack(spacing: 8) {
          Circle().fill(h.working ? Ink.accent : Color.clear).overlay(Circle().stroke(h.working ? Color.clear : Ink.faint, lineWidth: 1.4)).frame(width: 8, height: 8)
          Text(h.name).font(Face.text(14, .semibold)).foregroundStyle(Ink.fg).lineLimit(1)
          Text(h.says.en).font(Face.text(13)).foregroundStyle(Ink.muted).lineLimit(1)
        }
        .accessibilityElement(children: .combine)
      }
    }
    .padding(12).frame(maxWidth: .infinity, alignment: .leading)
    .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(Ink.surface))
    .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).stroke(Ink.lineStrong, lineWidth: 1))
    .accessibilityIdentifier("session-helpers")
  }
}

/** The session's running work at the end of the chat: "<name> is working" and its status lines as small chips; a tap on
 *  a chip opens its whole text and the way to its card. */
struct SessionStatus: View {
  @EnvironmentObject var shell: AppShell
  let page: CoreView.SessionView
  let hue: Int
  @State private var shown: String? = nil
  @State private var more = false
  /** One calm line (as the web's): the drawing, "… is working" with its dots while it answers, the status line its
   *  agent set last (the core's first) and the core's "+N" for the rest, which a tap opens under the line. A tap on a
   *  line opens its whole text and the way to its card. */
  var body: some View { wide }
  @ViewBuilder private var wide: some View {
    if page.answering != nil || !page.statusLines.isEmpty || page.helpers != nil {
      VStack(alignment: .leading, spacing: 8) {
        if let helpers = page.helpers { HelperList(list: helpers) }
        HStack(spacing: 8) {
          AgentMark(mark: page.mark, size: 16, hue: hue)
          if let answering = page.answering {
            Text(answering.en).font(Face.text(14)).foregroundStyle(Ink.muted).lineLimit(1).layoutPriority(1)
            TypingDots()
            if page.statusLines.first != nil { Text("·").font(Face.text(14)).foregroundStyle(Ink.faint) }
          }
          if let first = page.statusLines.first { chip(first) }
          if let plus = page.statusMore {
            Button { withAnimation(.easeOut(duration: 0.18)) { more.toggle() } } label: {
              Text(plus.en).font(Face.text(13, .semibold)).monospacedDigit().foregroundStyle(Ink.muted)
                .padding(.horizontal, 9).frame(height: 30).background(Capsule().fill(Ink.sunken.opacity(more ? 1 : 0.6)))
                .frame(minHeight: 44).contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("\(page.statusLines.count - 1) more")
            .accessibilityIdentifier("status-more")
          }
          Spacer(minLength: 0)
        }
        if more, page.statusLines.count > 1 {
          FlowChips(spacing: 6) { ForEach(page.statusLines.dropFirst(), id: \.id) { line in chip(line) } }
        }
        if let id = shown, let line = page.statusLines.first(where: { $0.id == id }) { opened(line) }
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
        if let helper = t.helper { Text(helper).font(Face.text(12, .semibold)).foregroundStyle(Ink.muted).lineLimit(1) }
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
/** The agent at work as only its face, drawing itself with the pen: drawn on in a little over two seconds, held, faded,
 *  again (as calm as a messenger's typing sign). Reduced motion: the face stands still. */
struct DrawingFace: View {
  let mark: String
  let hue: Int
  var size: CGFloat = 30
  @Environment(\.accessibilityReduceMotion) private var still
  var body: some View {
    if still {
      AgentMark(mark: mark, size: size, hue: hue)
    } else {
      TimelineView(.animation) { tl in
        let cycle = 3.6, t = tl.date.timeIntervalSinceReferenceDate.truncatingRemainder(dividingBy: cycle)
        let drawn = t < 2.2 ? CGFloat(0.5 - cos(.pi * t / 2.2) / 2) : 1
        PenMark(doc: PenStore.mark(mark), inks: PenInks(stroke: Tone.mark(hue), drawn: drawn))
          .frame(width: size, height: size)
          .opacity(t > 3.1 ? 1 - (t - 3.1) / 0.5 : 1)
      }
    }
  }
}

/** What the face opens (`SessionView.working`): "Working · <step>", the lines it works on (a tap opens a line's card),
 *  and the turn's trail. */
struct WorkDetails: View {
  @EnvironmentObject var shell: AppShell
  @Environment(\.dismiss) private var dismiss
  let page: CoreView.SessionView
  var body: some View {
    NavigationStack {
      List {
        if let w = page.working {
          Section { Text(w.head.en).font(Face.text(16, .semibold)).foregroundStyle(Ink.fg) }
        }
        if !page.statusLines.isEmpty {
          Section("Working on") {
            ForEach(page.statusLines, id: \.id) { l in
              Button { if let c = l.card { dismiss(); shell.focus(.card(cardId: c.id)) } } label: {
                VStack(alignment: .leading, spacing: 2) {
                  (Text(l.helper.map { "\($0) · " } ?? "").foregroundColor(Ink.muted) + Text(l.label).foregroundColor(Ink.fg)).font(Face.text(16, .medium))
                  if !l.detail.isEmpty { Text(l.detail).font(Face.text(14)).foregroundStyle(Ink.muted) }
                  Text(l.card.map { "\(l.says.en) · Nr. \($0.number)" } ?? l.says.en).font(Face.text(13)).foregroundStyle(Ink.faint)
                }
              }
              .buttonStyle(.plain).disabled(l.card == nil)
            }
          }
        }
        if let trail = page.working?.trail {
          Section("Steps") { WorkDrawing(work: trail).listRowInsets(EdgeInsets(top: 6, leading: 12, bottom: 6, trailing: 12)) }
        }
      }
      .navigationTitle(page.name.en)
      .navigationBarTitleDisplayMode(.inline)
    }
    .presentationDetents([.medium, .large])
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
  /** The last of a run of the agent's words: its time stands under it. */
  var last = true
  var body: some View {
    VStack(alignment: .leading, spacing: 6) {
      if let day = row.day { DayLine(text: day.en) }
      switch row.kind {
      case "ask": if let card = row.row { DeskRow(row: card).padding(.vertical, 6) }
      case "work": if let work = row.work { WorkDrawing(work: work, timeMs: row.timeMs) }
      case "event": if let e = row.event, !e.echo { EventLine(event: e, card: row.card, timeMs: row.timeMs, inCard: inCard) }
      case "events": EventChips(chips: row.events)
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
              FileRows(ids: row.files, small: true)
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
          // (the time beside his words; an iPhone shows only that it is not sent yet)
          if !AppShell.phone || row.pending {
            VStack {
              if row.pending { Image(systemName: "clock").font(.system(size: 10)).foregroundStyle(Ink.faint) }
              else { Text(talkClock(row.timeMs)).font(Face.text(10)).foregroundStyle(Ink.faint) }
            }.frame(width: AppShell.phone ? 16 : 30)
          }
        }
        .contextMenu { if !row.text.isEmpty { Button { copyText(row.text) } label: { Label("Copy", systemImage: "doc.on.doc") } } }
      }
    }
  }
  /** The agent's words as a grey bubble on the left (as the web's): its drawing beside the first of a run, its name
   *  above it, the time under the last; an Artifact as its own card. */
  private var agent: some View {
    HStack(alignment: .top, spacing: 8) {
      Group {
        // (its face bare, no disc behind it; an iPhone shows none: the title above has it)
        if !row.cont, let who, !AppShell.phone {
          AgentMark(mark: who.mark, size: 22, hue: who.hue).frame(width: 30, height: 30)
        } else { Color.clear }
      }
      .frame(width: AppShell.phone ? 0 : 30, height: row.cont ? 1 : 30)
      .padding(.top, row.cont || AppShell.phone ? 0 : 20)
      .accessibilityHidden(true)
      VStack(alignment: .leading, spacing: 4) {
        if !row.cont && (!AppShell.phone || row.helper != nil) {
          HStack(spacing: 6) {
            if let who, !AppShell.phone { Text(who.name).font(Face.text(13, .semibold)).foregroundStyle(Ink.muted) }
            if let helper = row.helper { Text(helper).font(Face.text(12, .semibold)).foregroundStyle(Ink.muted).padding(.horizontal, 7).padding(.vertical, 1).background(Capsule().fill(Ink.sunken)) }
            if row.terminal && !AppShell.phone { Text(">_").font(.system(size: 10, weight: .medium, design: .monospaced)).foregroundStyle(Ink.faint) }
          }
          .padding(.leading, 4)
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
        } else if !row.text.isEmpty || !row.html.isEmpty || !row.files.isEmpty || !row.details.isEmpty {
          VStack(alignment: .leading, spacing: 8) {
            if !row.text.isEmpty { Markdown(text: row.text).accessibilityIdentifier("talk-message") }
            if !row.html.isEmpty { InlineHTML(html: row.html).frame(minHeight: 200).clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous)) }
            FileRows(ids: row.files)
            if !row.details.isEmpty {
              DisclosureGroup { Markdown(text: row.details, size: 15, color: Ink.muted).padding(.top, 4) } label: { Text("Details").font(Face.text(14, .semibold)).foregroundStyle(Ink.muted) }.tint(Ink.muted)
            }
          }
          .padding(.horizontal, 14).padding(.vertical, 10)
          .background(UnevenRoundedRectangle(topLeadingRadius: row.cont ? 18 : 6, bottomLeadingRadius: 18, bottomTrailingRadius: 18, topTrailingRadius: 18, style: .continuous).fill(Ink.fg.opacity(0.07)))
          .contextMenu { if !row.text.isEmpty { Button { copyText(row.text) } label: { Label("Copy", systemImage: "doc.on.doc") } } }
        }
        chips
        if let waits = row.waits { Text(waits.en).font(Face.text(13)).italic().foregroundStyle(Ink.faint) }
        if last && !AppShell.phone { Text(talkClock(row.timeMs)).font(Face.text(11)).foregroundStyle(Ink.faint).padding(.leading, 4) }
      }
      Spacer(minLength: 28)
    }
    .frame(maxWidth: .infinity, alignment: .leading)
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

/** An iPhone's run of card events (`kind` "events"): a small chip each, its sign and the card's title; a tap opens it. */
struct EventChips: View {
  @EnvironmentObject var shell: AppShell
  let chips: [CoreView.EventChipView]
  var body: some View {
    FlowChips(spacing: 6) {
      ForEach(chips, id: \.id) { c in
        Button { if let card = c.card { shell.focus(.card(cardId: card.id)) } } label: {
          HStack(spacing: 6) {
            Image(systemName: EventLine.icon(c.kind)).font(.system(size: 11, weight: .semibold)).foregroundStyle(EventLine.tint(c.kind))
            Text(c.text).font(Face.text(13, .medium)).foregroundStyle(Ink.fg).lineLimit(1)
          }
          .padding(.horizontal, 10).frame(height: 28).frame(maxWidth: 300).background(Capsule().fill(Ink.sunken))
          .frame(minHeight: 36).contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(c.card == nil)
        .accessibilityLabel(c.says.en)
      }
    }
    .padding(.vertical, 2)
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
  private var icon: String { Self.icon(event.kind) }
  private var tint: Color { Self.tint(event.kind) }
  static func icon(_ kind: String) -> String {
    switch kind {
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
  static func tint(_ kind: String) -> Color { kind == "asked" ? Ink.urgHigh : ["decided", "done", "read"].contains(kind) ? Ink.accent : Ink.muted }
}

// ---- light markdown --------------------------------------------------------------------------------------------------

/** The light markdown agents write: paragraphs, headings, lists, **bold**, *italic*, `code`, links, __words underlined
 *  with the pen__, fenced code, tables, quotes and rules. */
struct Markdown: View {
  let text: String
  var size: CGFloat = 16
  var color: Color = Ink.fg
  /** The system's face instead of the app's (a card's calm page). */
  var system = false
  private func face(_ size: CGFloat, _ weight: Font.Weight = .regular) -> Font { system ? .system(size: size, weight: weight) : Face.text(size, weight) }
  var body: some View {
    VStack(alignment: .leading, spacing: 10) {
      ForEach(Array(MarkBlock.blocks(text).enumerated()), id: \.offset) { _, b in block(b) }
    }
    .frame(maxWidth: .infinity, alignment: .leading)
  }
  @ViewBuilder private func block(_ b: MarkBlock) -> some View {
    switch b {
    case .text(let s):
      Text(MarkBlock.inline(s)).font(face(size)).foregroundStyle(color).lineSpacing(3).textSelection(.enabled).tint(Ink.accent)
        .fixedSize(horizontal: false, vertical: true)
    case .heading(let s, let level):
      Text(MarkBlock.inline(s)).font(system ? .system(size: level <= 1 ? size + 6 : level == 2 ? size + 3 : size + 1, weight: .bold) : Face.display(level <= 1 ? size + 6 : level == 2 ? size + 3 : size + 1, .bold)).foregroundStyle(color)
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
            GridRow { ForEach(Array(r.enumerated()), id: \.offset) { _, c in Text(MarkBlock.inline(c)).font(face(size - 2, i == 0 ? .semibold : .regular)).foregroundStyle(Ink.fg) } }
            if i == 0 { Rectangle().fill(Ink.fg).frame(height: 1.5).gridCellUnsizedAxes(.horizontal) } else { Rectangle().fill(Ink.lineStrong).frame(height: 1).gridCellUnsizedAxes(.horizontal) }
          }
        }.padding(.vertical, 4)
      }
    case .quote(let s):
      HStack(spacing: 10) { Rectangle().fill(Ink.lineStrong).frame(width: 3); Text(MarkBlock.inline(s)).font(face(size)).foregroundStyle(Ink.muted) }
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
  /** Fill its frame (a small square), cut at the edges, instead of fitting in it. */
  var fill = false
  var body: some View {
    Group {
      #if canImport(UIKit)
      if let data = shell.files[id], let image = UIImage(data: data) { Image(uiImage: image).resizable().aspectRatio(contentMode: fill ? .fill : .fit).accessibilityIdentifier("native-picture") } else { pending }
      #else
      if let data = shell.files[id], let image = NSImage(data: data) { Image(nsImage: image).resizable().aspectRatio(contentMode: fill ? .fill : .fit).accessibilityIdentifier("native-picture") } else { pending }
      #endif
    }.task(id: id) { shell.act(["do": "open_file", "attachment_id": id, "save": false]) }
  }
  private var pending: some View {
    RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Ink.sunken).frame(height: 160).overlay { ProgressView() }
  }
}
/** A picture about to be sent: a small square of it, its upload's progress over it, an × in the corner. */
struct PictureChip: View {
  @EnvironmentObject var shell: AppShell
  let id: String
  let remove: () -> Void
  var body: some View {
    ZStack(alignment: .topTrailing) {
      thumb.frame(width: 56, height: 56).clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay { if let p = shell.view.files[id]?.percent, p < 100 { ProgressView(value: Double(p), total: 100).progressViewStyle(.circular).padding(14).background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 10, style: .continuous)) } }
      Button(action: remove) { Image(systemName: "xmark.circle.fill").font(.system(size: 18)).symbolRenderingMode(.palette).foregroundStyle(.white, .black.opacity(0.6)) }
        .buttonStyle(.plain).offset(x: 6, y: -6).accessibilityLabel("Remove")
    }
    .padding(.top, 6)
    .accessibilityElement(children: .contain).accessibilityLabel(shell.view.files[id]?.name ?? "Picture")
    .accessibilityIdentifier("attach-picture")
  }
  @ViewBuilder private var thumb: some View {
    #if canImport(UIKit)
    if let data = shell.files[id], let image = UIImage(data: data) { Image(uiImage: image).resizable().scaledToFill() } else { placeholder }
    #else
    if let data = shell.files[id], let image = NSImage(data: data) { Image(nsImage: image).resizable().scaledToFill() } else { placeholder }
    #endif
  }
  private var placeholder: some View { RoundedRectangle(cornerRadius: 10, style: .continuous).fill(Ink.sunken).overlay { Image(systemName: "photo").foregroundStyle(.secondary) } }
}
/** A message's or a card's files: a picture small and round-cornered, any other file as a quiet chip with its size. */
struct FileRows: View {
  @EnvironmentObject var shell: AppShell
  let ids: [String]
  /** His own message: its pictures as small squares (as in the composer), a tap opens them big. */
  var small = false
  /** Its pictures open over the whole screen. */
  @State private var showing: PictureShow?
  @Namespace private var zoom
  private var shots: [Shot] {
    ids.compactMap { id in shell.view.files[id].flatMap { $0.mediaType.hasPrefix("image/") ? Shot(fileId: id, caption: $0.name) : nil } }
  }
  var body: some View {
    ForEach(ids, id: \.self) { id in
      if let file = shell.view.files[id] {
        if file.mediaType.hasPrefix("image/") {
          Button {
            let all = shots
            showing = PictureShow(shots: all, start: all.firstIndex { $0.fileId == id } ?? 0)
          } label: {
            Group {
              if small { NativePicture(id: id, fill: true).frame(width: 120, height: 120).clipped() }
              else { NativePicture(id: id).frame(maxWidth: 280, maxHeight: 280) }
            }.clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
          }
          .buttonStyle(.plain)
          .contextMenu { SavePictureButton(id: id) }
          .zoomSource(id: id, in: zoom)
          // (one cover for all of them, on the first)
          .pictureViewer($showing, zoom: zoom, on: id == shots.first?.fileId)
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
