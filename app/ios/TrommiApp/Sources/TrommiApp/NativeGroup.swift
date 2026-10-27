// NativeGroup.swift: the iPhone's group page in the native look, drawing only (the core's `view.group`). The group's
// name as the large title, its ⋯ menu in the bar, a face per chat to filter by, then two inset groups: the questions
// with their answers as two round buttons, then what is only to read. The same controls at the bottom as the list.
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
        if !group.chips.isEmpty {
          NativeChips(chips: group.chips)
            .listRowBackground(Color.clear).listRowInsets(EdgeInsets()).listRowSeparator(.hidden)
        }
        if let reading = group.reading {
          Text(reading.en).font(.footnote).foregroundStyle(.secondary).listRowBackground(Color.clear)
            .accessibilityIdentifier("reading-history")
        }
        if !group.slip.isEmpty { CutSlip(slips: group.slip).listRowBackground(Color.clear).listRowInsets(EdgeInsets()) }
        if !group.questions.isEmpty {
          Section {
            ForEach(group.questions, id: \.id) { row in NativeQuestionRow(row: row) }
          } header: { if let says = group.questionsSays { NativePartHead(says: says) } }
        }
        if !group.reads.isEmpty {
          Section {
            ForEach(group.reads, id: \.id) { row in NativeReadRow(row: row) }
          } header: { if let says = group.readsSays { NativePartHead(says: says) } }
        }
        if let empty = group.empty, group.mode != "invite" {
          ContentUnavailableView(empty.en, systemImage: "checkmark.circle")
            .frame(maxWidth: .infinity).padding(.vertical, 40).listRowBackground(Color.clear)
        }
      }
    }
    .nativeList(title: group.head?.name.en ?? side.place.en, query: $shell.chatsQuery) { shell.show(shell.screen) }
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

/** A part's heading: the core's "Waits for you" or "To read", with how many beside it. */
private struct NativePartHead: View {
  let says: CoreView.Text
  var body: some View {
    Text(says.args["n"].map { "\(says.en) · \($0)" } ?? says.en)
  }
}

/** The group's ⋯ in the bar (`GroupHeadView.menu`): Knowledge, Wakeups, Scribble Board with their counts; Rename asks
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

/** The faces of the group's chats under the title (`GroupView.chips`), "All" first with its calm count: a tap shows
 *  only that chat's cards, the chosen one is ringed and a second tap opens its chat. Every chip is drawn; the row
 *  scrolls. */
private struct NativeChips: View {
  @EnvironmentObject var shell: AppShell
  let chips: [CoreView.ChipView]
  var body: some View {
    ScrollView(.horizontal, showsIndicators: false) {
      HStack(alignment: .top, spacing: 14) {
        ForEach(chips, id: \.sessionId) { chip in
          Button {
            if chip.selected, !chip.sessionId.isEmpty { shell.focus(.session(sessionId: chip.sessionId)) }
            else { shell.chip(chip.sessionId.isEmpty ? nil : chip.sessionId) }
          } label: {
            VStack(spacing: 5) {
              picture(chip)
                .padding(3)
                .overlay(Circle().stroke(chip.selected ? Ink.accent : Color.clear, lineWidth: 2))
                .overlay(alignment: .topTrailing) {
                  if chip.needs { Circle().fill(NativeLook.waits).frame(width: 11, height: 11).overlay(Circle().stroke(Color(uiColor: .systemGroupedBackground), lineWidth: 2)) }
                }
              Text(chip.name.en).font(.caption).foregroundStyle(chip.selected ? Color.primary : Color.secondary)
                .lineLimit(2).multilineTextAlignment(.center)
              if let go = chip.openChat { Text(go.en).font(.caption2.weight(.semibold)).foregroundStyle(Ink.accent).lineLimit(1) }
            }
            .frame(width: 66)
            .contentShape(Rectangle())
          }
          .buttonStyle(.plain)
          .accessibilityLabel(chip.says.en)
          .accessibilityIdentifier("group-chip-" + (chip.sessionId.isEmpty ? "all" : chip.sessionId))
          .accessibilityAddTraits(chip.selected ? .isSelected : [])
        }
      }
      .padding(.horizontal, 16).padding(.vertical, 2)
    }
    .accessibilityIdentifier("group-chips")
  }
  @ViewBuilder private func picture(_ chip: CoreView.ChipView) -> some View {
    if chip.sessionId.isEmpty {
      // ("All": a quiet well with the group's calm count)
      Circle().fill(NativeLook.bubble).frame(width: 50, height: 50)
        .overlay { Text(chip.count > 0 ? "\(chip.count)" : "").font(.headline).monospacedDigit().foregroundStyle(.secondary) }
    } else {
      NativeAvatar(mark: chip.mark, hue: Pen.hueFor(id: chip.slug.isEmpty ? chip.sessionId : chip.slug, mark: chip.mark), size: 50)
    }
  }
}

/** Who a row is from and how long ago, small above its title: the face, its name, the core's age. */
private struct NativeFromLine: View {
  let row: CoreView.DeskRowView
  var body: some View {
    HStack(spacing: 6) {
      if let from = row.from {
        NativeAvatar(mark: from.mark, hue: Pen.hueFor(id: from.sessionId, mark: from.mark), size: 20)
      }
      Text([row.from?.name, row.age?.en].compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: " · "))
        .font(.footnote).foregroundStyle(.secondary).lineLimit(1)
    }
  }
}

/** A question: who asks, its title in the system's bold, the core's notes, and its answers as round buttons. A tap
 *  on the words opens the card; held, the ways the core offers. */
private struct NativeQuestionRow: View {
  @EnvironmentObject var shell: AppShell
  let row: CoreView.DeskRowView
  var body: some View {
    VStack(alignment: .leading, spacing: 10) {
      Button { shell.focus(.card(cardId: row.id)) } label: {
        VStack(alignment: .leading, spacing: 5) {
          NativeFromLine(row: row)
          HStack(alignment: .firstTextBaseline, spacing: 6) {
            if row.sign == "block" { PenMark("hand", color: Ink.surface, blocked: true).frame(width: 20, height: 20).alignmentGuide(.firstTextBaseline) { $0[.bottom] - 4 } }
            else if row.sign == "knock" { Sketch("knock", color: Ink.urgHigh).frame(width: 17, height: 17).alignmentGuide(.firstTextBaseline) { $0[.bottom] - 3 } }
            if row.starred { StarMark().frame(width: 16, height: 16).alignmentGuide(.firstTextBaseline) { $0[.bottom] - 3 }.accessibilityLabel("Starred") }
            Text(row.title.isEmpty ? " " : row.title).font(.body.weight(.semibold))
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
      if row.newer { UnsupportedLine() } else { NativeAnswers(row: row) }
    }
    .padding(.vertical, 6)
    .contextMenu { CardWays(id: row.id, ways: row.ways) }
  }
}

/** The answers of a question as the core lays them out (`row.tiles`): two round buttons side by side, the lead one
 *  filled; "+N other ways" and "Choose" open the card. */
private struct NativeAnswers: View {
  @EnvironmentObject var shell: AppShell
  let row: CoreView.DeskRowView
  var body: some View {
    let tiles = row.tiles
    switch tiles.kind {
    case "pair":
      HStack(spacing: 8) {
        ForEach(tiles.tiles, id: \.key) { t in
          Button { answer(t.key) } label: { label(t) }
            .buttonStyle(NativeAnswerStyle(lead: t.lead, urgency: row.urgency))
            .accessibilityLabel(t.says.en)
        }
      }
    case "advised":
      HStack(spacing: 8) {
        Button { shell.focus(.card(cardId: row.id)) } label: { Text("+\(tiles.others) other ways") }
          .buttonStyle(NativeAnswerStyle(lead: false, urgency: row.urgency))
          .accessibilityLabel(tiles.says?.en ?? "Open the card")
        ForEach(tiles.tiles, id: \.key) { t in
          Button { answer(t.key) } label: { label(t) }
            .buttonStyle(NativeAnswerStyle(lead: true, urgency: row.urgency))
            .accessibilityLabel(t.says.en)
        }
      }
    case "choose":
      Button { shell.focus(.card(cardId: row.id)) } label: { Text("Choose") }
        .buttonStyle(NativeAnswerStyle(lead: true, urgency: row.urgency))
        .accessibilityLabel(tiles.says?.en ?? "Choose")
    case "read":
      HStack(spacing: 8) {
        Button { shell.act(["do": "hand_back", "card_id": row.id]) } label: { Text("What??") }
          .buttonStyle(NativeAnswerStyle(lead: false, urgency: row.urgency))
        Button { shell.act(["do": "read", "card_id": row.id]) } label: { Text("Got it") }
          .buttonStyle(NativeAnswerStyle(lead: true, urgency: row.urgency))
      }
    default: EmptyView()
    }
  }
  @ViewBuilder private func label(_ t: CoreView.TileView) -> some View {
    HStack(spacing: 5) {
      if t.thumbs, t.label.isEmpty { Image(systemName: t.thumb == "yes" ? "hand.thumbsup.fill" : "hand.thumbsdown") }
      if !t.label.isEmpty { Text(t.label).font(t.small ? .footnote.weight(t.lead ? .semibold : .medium) : .subheadline.weight(t.lead ? .semibold : .medium)) }
      if t.settles { Image(systemName: "checkmark").font(.caption2.weight(.bold)) }
    }
  }
  private func answer(_ key: String) {
    if row.kind == "request" { shell.act(["do": "verdict", "request_id": row.id, "allow": key == "allow"]) }
    else { shell.act(["do": "answer", "card_id": row.id, "choices": [key]]) }
  }
}

/** A round answer button: the lead filled in the accent (the urgent colour on a knock), the other a quiet fill with
 *  the accent's words. */
private struct NativeAnswerStyle: ButtonStyle {
  let lead: Bool
  var urgency = "normal"
  func makeBody(configuration: Configuration) -> some View {
    let knock = urgency == "high" || urgency == "critical"
    let fill = lead ? (knock ? Ink.urgency(urgency) : Ink.accent) : Color(uiColor: .tertiarySystemFill)
    configuration.label
      .font(.subheadline.weight(lead ? .semibold : .medium))
      .lineLimit(2).multilineTextAlignment(.center).minimumScaleFactor(0.85)
      .foregroundStyle(lead ? Ink.accentFg : Ink.accent)
      .padding(.horizontal, 10)
      .frame(maxWidth: .infinity, minHeight: 38)
      .background(Capsule().fill(fill))
      .opacity(configuration.isPressed ? 0.7 : 1)
      .contentShape(Capsule())
  }
}

/** Something to read: the page's drawing in a quiet square, its title, who sent it and how long ago. A tap opens it;
 *  a swipe says Got it; held, the ways the core offers. */
private struct NativeReadRow: View {
  @EnvironmentObject var shell: AppShell
  let row: CoreView.DeskRowView
  var body: some View {
    Button { shell.focus(.card(cardId: row.id)) } label: {
      HStack(alignment: .center, spacing: 12) {
        Sketch("page", color: Color.secondary).frame(width: 20, height: 20)
          .frame(width: 30, height: 30).background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(Color(uiColor: .tertiarySystemFill)))
        VStack(alignment: .leading, spacing: 1) {
          Text(row.title.isEmpty ? " " : row.title).font(.body.weight(.medium))
            .foregroundStyle(row.loading ? Color.secondary : Color.primary).lineLimit(1)
          Text([row.from?.name, row.age?.en].compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: " · "))
            .font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .alignmentGuide(.listRowSeparatorLeading) { d in d[.leading] }
      }
      .padding(.vertical, 2)
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
