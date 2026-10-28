// NativeGroup.swift: the iPhone's group page in the native look, drawing only (the core's `view.group`). The group's
// name as the large title, its ⋯ menu in the bar, then on a plain white page with no boxes and no lines, chat by chat:
// a small grey head (the chat's face and name) is the way into the chat, under it its questions, each with one quiet
// round button for the advised answer, the other answers as plain words beside it and how long ago, then what is only
// to read. Nothing open: the calm word. The same controls at the bottom as the list.
// Order, headings, answers, ages and words are the core's.
import SwiftUI
import TrommiClient

#if os(iOS)
struct NativeGroupPage: View {
  @EnvironmentObject var shell: AppShell
  @State private var speedReport: String?
  var body: some View {
    let group = shell.view.group
    let side = shell.view.sidebar
    List {
      if NativeFound.searching(side) {
        NativeFound()
      } else {
        if group.mode == "invite" { FirstAgent().listRowBackground(Color.clear).listRowInsets(EdgeInsets(top: 0, leading: 4, bottom: 0, trailing: 4)) }
        if let summary = group.summary {
          Text(summary.en).font(.subheadline).foregroundStyle(.secondary).calmRow(top: 0, bottom: 0)
            .accessibilityIdentifier("group-summary")
        }
        if let reading = group.reading {
          Text(reading.en).font(.footnote).foregroundStyle(.secondary).calmRow(top: 2, bottom: 0)
            .accessibilityIdentifier("reading-history")
        }
        if !group.slip.isEmpty { CutSlip(slips: group.slip).listRowBackground(Color.clear).listRowInsets(EdgeInsets()) }
        ForEach(group.blocks, id: \.sessionId) { block in
          NativeChatHead(block: block).calmRow(top: 18, bottom: 2)
          ForEach(block.questions, id: \.id) { row in NativeQuestionRow(row: row).calmRow() }
          ForEach(block.reads, id: \.id) { row in NativeReadRow(row: row).calmRow() }
        }
        if let empty = group.empty, group.mode != "invite" {
          GroupCalm(empty: empty).listRowBackground(Color.clear).listRowSeparator(.hidden)
        }
      }
    }
    .nativeList(title: group.head?.name.en ?? side.place.en, plain: true, query: $shell.chatsQuery) { shell.show(shell.screen) }
    .toolbar {
      if let head = group.head, !head.menu.isEmpty {
        ToolbarItem(placement: .topBarTrailing) { NativeGroupMenu(head: head) }
      }
      ListControls(desk: true)
    }
    .refreshable { shell.act(["do": "retry"]) }
    .accessibilityIdentifier("desk-content")
    .accessibilityValue(speedReport ?? String(group.stack.count))
    .onAppear { speedReport = NativeDemo.ready(rows: group.stack.count) }
  }
}

/** The group's ⋯ in the bar (`GroupHeadView.menu`): Knowledge and Wakeups with their counts; Rename asks
 *  for the new name. */
private struct NativeGroupMenu: View {
  @EnvironmentObject var shell: AppShell
  let head: CoreView.GroupHeadView
  @State private var renaming = false
  @State private var name = ""
  var body: some View {
    Menu {
      ForEach(head.menu, id: \.key) { item in
        if let to = item.opens {
          Button { shell.focus(to) } label: { Text(item.count.map { "\(item.label.en) · \($0.en)" } ?? item.label.en) }
        } else if item.key == "rename" {
          Button { name = head.name.en; renaming = true } label: { Text(item.label.en) }
        }
      }
    } label: { Image(systemName: "ellipsis") }
    .accessibilityLabel(head.menuSays.en).accessibilityIdentifier("group-menu")
    .alert(head.menu.first { $0.key == "rename" }?.label.en ?? "", isPresented: $renaming) {
      TextField(head.name.en, text: $name)
      Button("Cancel", role: .cancel) {}
      Button("Save") { let n = name.trimmingCharacters(in: .whitespaces); if !n.isEmpty, n != head.name.en { shell.act(["do": "rename_team", "team_id": head.teamId, "name": n]) } }
    }
  }
}

extension View {
  /** A row of the calm page: no box, no line, the page's own white, the system's side margin. */
  func calmRow(top: CGFloat = 8, bottom: CGFloat = 8) -> some View {
    self.listRowBackground(Color.clear).listRowSeparator(.hidden)
      .listRowInsets(EdgeInsets(top: top, leading: 20, bottom: bottom, trailing: 20))
  }
}

/** A chat's head on the group page: its face and name, small and grey; a tap opens the chat. */
private struct NativeChatHead: View {
  @EnvironmentObject var shell: AppShell
  let block: CoreView.ChatBlockView
  var body: some View {
    Button { shell.focus(.session(sessionId: block.sessionId)) } label: {
      HStack(spacing: 8) {
        NativeAvatar(mark: block.mark, hue: Pen.hueFor(id: block.slug.isEmpty ? block.sessionId : block.slug, mark: block.mark), size: 22)
        Text(block.name.en).font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
        Spacer(minLength: 0)
      }
      .contentShape(Rectangle())
    }
    .buttonStyle(.plain)
    .accessibilityLabel(block.says.en)
    .accessibilityIdentifier("group-block-" + block.sessionId)
  }
}

/** How long ago, quiet at the end of a row (its chat is the head above it). */
private struct NativeAge: View {
  let row: CoreView.DeskRowView
  var body: some View {
    Text(row.age?.en ?? "").font(.subheadline).foregroundStyle(.tertiary).lineLimit(1).layoutPriority(1)
  }
}

/** A question: its title in the system's body, the core's notes, then its answers (one quiet round button, the
 *  others as words) and how long ago. A tap on the words opens the card; held, the ways the core offers. */
private struct NativeQuestionRow: View {
  @EnvironmentObject var shell: AppShell
  let row: CoreView.DeskRowView
  var body: some View {
    VStack(alignment: .leading, spacing: 10) {
      Button { shell.focus(.card(cardId: row.id)) } label: {
        VStack(alignment: .leading, spacing: 4) {
          HStack(alignment: .firstTextBaseline, spacing: 6) {
            if row.sign == "block" { PenMark("hand", color: Ink.surface, blocked: true).frame(width: 20, height: 20).alignmentGuide(.firstTextBaseline) { $0[.bottom] - 4 } }
            else if row.sign == "knock" { Sketch("knock", color: Ink.urgHigh).frame(width: 17, height: 17).alignmentGuide(.firstTextBaseline) { $0[.bottom] - 3 } }
            if row.starred { StarMark().frame(width: 16, height: 16).alignmentGuide(.firstTextBaseline) { $0[.bottom] - 3 }.accessibilityLabel("Starred") }
            Text(row.title.isEmpty ? " " : row.title).font(.body)
              .foregroundStyle(row.loading ? Color.secondary : Color.primary).lineLimit(3).multilineTextAlignment(.leading)
          }
          ForEach(Array(row.notes.enumerated()), id: \.offset) { Text($0.element.en).font(.footnote).foregroundStyle(.secondary) }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .contentShape(Rectangle())
      }
      .buttonStyle(.plain)
      .accessibilityHint(row.signSays?.en ?? "")
      .accessibilityIdentifier("desk-card-" + row.id)
      HStack(alignment: .center, spacing: 14) {
        if row.newer { UnsupportedLine() } else { NativeAnswers(row: row) }
        Spacer(minLength: 0)
        NativeAge(row: row)
      }
    }
    .contextMenu { CardWays(id: row.id, ways: row.ways) }
  }
}

/** The answers of a question as the core lays them out (`row.tiles`): the lead one as a quiet round button with a
 *  check, the other as plain words beside it (both answer at once); "+N other ways" and "Choose" open the card. */
private struct NativeAnswers: View {
  @EnvironmentObject var shell: AppShell
  let row: CoreView.DeskRowView
  var body: some View {
    let tiles = row.tiles
    HStack(spacing: 14) {
      switch tiles.kind {
      case "pair":
        // the lead first; with no lead both stand as buttons
        let anyLead = tiles.tiles.contains { $0.lead }
        ForEach(tiles.tiles.sorted { $0.lead && !$1.lead }, id: \.key) { t in
          Button { answer(t.key) } label: { label(t) }
            .buttonStyle(NativeAnswerStyle(lead: t.lead || !anyLead, urgency: row.urgency))
            .accessibilityLabel(t.says.en)
        }
      case "advised":
        ForEach(tiles.tiles, id: \.key) { t in
          Button { answer(t.key) } label: { label(t) }
            .buttonStyle(NativeAnswerStyle(lead: true, urgency: row.urgency))
            .accessibilityLabel(t.says.en)
        }
        Button { shell.focus(.card(cardId: row.id)) } label: { Text("+\(tiles.others) other ways") }
          .buttonStyle(NativeAnswerStyle(lead: false, urgency: row.urgency))
          .accessibilityLabel(tiles.says?.en ?? "Open the card")
      case "choose":
        Button { shell.focus(.card(cardId: row.id)) } label: { Text("Choose") }
          .buttonStyle(NativeAnswerStyle(lead: true, urgency: row.urgency))
          .accessibilityLabel(tiles.says?.en ?? "Choose")
      case "read":
        Button { shell.act(["do": "read", "card_id": row.id]) } label: { Text("Got it") }
          .buttonStyle(NativeAnswerStyle(lead: true, urgency: row.urgency))
        Button { shell.act(["do": "hand_back", "card_id": row.id]) } label: { Text("What??") }
          .buttonStyle(NativeAnswerStyle(lead: false, urgency: row.urgency))
      default: EmptyView()
      }
    }
  }
  @ViewBuilder private func label(_ t: CoreView.TileView) -> some View {
    HStack(spacing: 5) {
      // (a choice that settles the card: a small grey check, as the core's `says` tells a screen reader)
      if t.settles { Image(systemName: "checkmark").font(.caption2.weight(.bold)).foregroundStyle(.secondary) }
      if t.thumbs, t.label.isEmpty { Image(systemName: t.thumb == "yes" ? "hand.thumbsup" : "hand.thumbsdown") }
      if !t.label.isEmpty { Text(t.label) }
    }
  }
  private func answer(_ key: String) {
    if row.kind == "request" { shell.act(["do": "verdict", "request_id": row.id, "allow": key == "allow"]) }
    else { shell.act(["do": "answer", "card_id": row.id, "choices": [key]]) }
  }
}

/** An answer: the lead a quiet grey round button in the text's own colour (on a knock in the urgent colour), the
 *  other plain grey words. Both as wide as their words. */
private struct NativeAnswerStyle: ButtonStyle {
  let lead: Bool
  var urgency = "normal"
  func makeBody(configuration: Configuration) -> some View {
    configuration.label
      .font(.subheadline.weight(lead ? .medium : .regular))
      .lineLimit(lead ? 2 : 1).multilineTextAlignment(.leading)
      .foregroundStyle(lead ? (urgency == "high" || urgency == "critical" ? Ink.urgency(urgency) : Color.primary) : Color.secondary)
      .padding(.horizontal, lead ? 14 : 0)
      .frame(minHeight: 34)
      .background(Capsule().fill(lead ? Color(uiColor: .tertiarySystemFill) : Color.clear))
      .opacity(configuration.isPressed ? 0.6 : 1)
      .contentShape(Capsule())
  }
}

/** Something to read: the page's small grey drawing, its title and how long ago (its chat heads it). A tap opens it;
 *  a swipe says Got it; held, the ways the core offers. */
private struct NativeReadRow: View {
  @EnvironmentObject var shell: AppShell
  let row: CoreView.DeskRowView
  var body: some View {
    Button { shell.focus(.card(cardId: row.id)) } label: {
      HStack(alignment: .firstTextBaseline, spacing: 10) {
        Sketch("page", color: Color.secondary).frame(width: 18, height: 18).alignmentGuide(.firstTextBaseline) { $0[.bottom] - 3 }
        Text(row.title.isEmpty ? " " : row.title).font(.body)
          .foregroundStyle(row.loading ? Color.secondary : Color.primary).lineLimit(2).multilineTextAlignment(.leading)
          .frame(maxWidth: .infinity, alignment: .leading)
        NativeAge(row: row)
      }
      .contentShape(Rectangle())
    }
    .buttonStyle(.plain)
    .accessibilityHint(row.signSays?.en ?? "")
    .accessibilityIdentifier("desk-card-" + row.id)
    .swipeActions(edge: .trailing) {
      if row.tiles.kind == "read" { Button("Got it") { shell.act(["do": "read", "card_id": row.id]) }.tint(Ink.accent) }
    }
    .contextMenu { CardWays(id: row.id, ways: row.ways) }
  }
}
#endif
