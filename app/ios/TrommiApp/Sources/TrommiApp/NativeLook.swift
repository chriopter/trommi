// NativeLook.swift: the iPhone's native look, drawing only. A messenger's grammar in the system's own parts: round
// faces, bold names, one grey line of what was said, separators under the words, two inset groups (the chats, the
// teams) under the large title, and one row of controls at the bottom (the system's search; the bolt over every team
// and the + in one glass group). Bubbles in the chat; the agent's words keep the full Markdown. Only the large title
// stands in Bricolage. The iPad and the Mac never reach it. Rows, order, counts and words belong to the core.
import SwiftUI
import TrommiClient
#if os(iOS)
import UIKit
#endif

@MainActor enum NativeLook {
  /** The native look is the iPhone's; the iPad and the Mac keep theirs. */
  static var on: Bool { AppShell.phone }
  /** At the start: the large title in Bricolage on the iPhone (the system's face everywhere else). */
  static func dressIfPhone() {
    #if os(iOS)
    dress()
    #endif
  }
  /** The system's quiet fill (the agent's bubble, the count, a group's picture). */
  static var bubble: Color {
    #if os(iOS)
    Color(uiColor: .systemGray5)
    #else
    Color.secondary.opacity(0.16)
    #endif
  }
  static var tile: Color {
    #if os(iOS)
    Color(uiColor: .secondarySystemBackground)
    #else
    Color.secondary.opacity(0.08)
    #endif
  }
}

/** A face as the system draws a contact: round; grey while not connected. */
struct NativeAvatar: View {
  let mark: String
  let hue: Int
  let size: CGFloat
  var online = true
  var body: some View {
    AgentMark(mark: mark, size: size * 0.6, hue: hue)
      .frame(width: size, height: size)
      .background(Circle().fill(Tone.color(hue: hue, .wash)))
      .clipShape(Circle())
      .grayscale(online ? 0 : 1).opacity(online ? 1 : 0.5)
      .accessibilityHidden(true)
  }
}

// ---- the talk ---------------------------------------------------------------------------------------------------------

/** One message of a talk as a bubble; everything it does not draw (cards, work, events, notes, deeds, artifacts) stays
 *  with TalkRow. The agent's words keep the full Markdown (code, links, tables), selection and Copy. */
struct NativeTalkLine: View {
  @EnvironmentObject var shell: AppShell
  let row: CoreView.TalkRow
  let inCard: Bool
  static func handles(_ row: CoreView.TalkRow) -> Bool {
    guard row.deed == nil else { return false }
    switch row.kind {
    case "ask", "work", "event", "events", "note", "newer", "artifact": return false
    default: return true
    }
  }
  private var mine: Bool { row.kind == "user" || row.fromMe }
  var body: some View {
    VStack(alignment: .leading, spacing: 6) {
      if let day = row.day {
        Text(day.en).font(.caption.weight(.medium)).foregroundStyle(.secondary).frame(maxWidth: .infinity).padding(.vertical, 8)
      }
      if mine { own } else { agent }
    }
    .frame(maxWidth: .infinity, alignment: .leading)
  }
  @ViewBuilder private var about: some View {
    if !inCard, row.quote == nil, let c = row.card {
      Button { shell.focus(.card(cardId: c.id)) } label: { Label(c.title, systemImage: "doc.text").font(.footnote).foregroundStyle(.secondary).lineLimit(1) }
        .buttonStyle(.plain)
    }
  }
  private var chips: some View {
    ForEach(row.chips, id: \.id) { c in
      Button { shell.focus(.card(cardId: c.id)) } label: {
        HStack(spacing: 6) { Text("No. \(c.number)").fontWeight(.semibold); Text(c.title).lineLimit(1) }
          .font(.footnote).foregroundStyle(.primary)
          .padding(.horizontal, 10).padding(.vertical, 6)
          .background(Capsule().fill(NativeLook.bubble))
      }
      .buttonStyle(.plain)
    }
  }
  @ViewBuilder private var copy: some View {
    if !row.text.isEmpty { Button { copyText(row.text) } label: { Label("Copy", systemImage: "doc.on.doc") } }
  }

  /** His words: the accent bubble at the right, as he wrote them; only a clock while not sent (the title has the rest). */
  private var own: some View {
    HStack(alignment: .bottom, spacing: 6) {
      Spacer(minLength: 56)
      VStack(alignment: .trailing, spacing: 4) {
        about
        chips
        FileRows(ids: row.files, small: true)
        if !row.text.isEmpty {
          VStack(alignment: .leading, spacing: 8) {
            if let q = row.quote { QuoteChip(quote: q, onAccent: true) }
            Text(row.text).font(.body).foregroundStyle(Ink.accentFg).textSelection(.enabled)
          }
          .padding(.horizontal, 13).padding(.vertical, 8)
          .background(RoundedRectangle(cornerRadius: 18, style: .continuous).fill(Ink.accent))
          .contextMenu { copy }
          .accessibilityIdentifier("talk-message")
        }
        if row.terminal {
          HStack(alignment: .firstTextBaseline, spacing: 5) { Text(">_").font(.caption2.monospaced()); Text("from the terminal").font(.caption2) }
            .foregroundStyle(.tertiary).padding(.trailing, 4)
        }
      }
      if row.pending { Image(systemName: "clock").font(.caption2).foregroundStyle(.tertiary) }
    }
  }
  /** The agent's words: the system's grey bubble at the left, the full Markdown in the system's face. */
  private var agent: some View {
    HStack(alignment: .top, spacing: 0) {
      VStack(alignment: .leading, spacing: 4) {
        if !row.cont, let helper = row.helper { Text(helper).font(.caption).foregroundStyle(.secondary).padding(.leading, 12) }
        about
        if !row.text.isEmpty || !row.html.isEmpty || !row.files.isEmpty || !row.details.isEmpty {
          VStack(alignment: .leading, spacing: 8) {
            if let q = row.quote { QuoteChip(quote: q) }
            if !row.text.isEmpty { Markdown(text: row.text, size: 17, system: true).accessibilityIdentifier("talk-message") }
            if !row.html.isEmpty { InlineHTML(html: row.html).frame(minHeight: 200).clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous)) }
            FileRows(ids: row.files)
            if !row.details.isEmpty {
              DisclosureGroup {
                Markdown(text: row.details, size: 15, color: Ink.muted, system: true).padding(.top, 4)
              } label: { Text("Details").font(.subheadline.weight(.semibold)).foregroundStyle(.secondary) }
                .tint(.secondary)
            }
          }
          .padding(.horizontal, 13).padding(.vertical, 8)
          .background(RoundedRectangle(cornerRadius: 18, style: .continuous).fill(NativeLook.bubble))
          .contextMenu { copy }
        }
        chips
        if let waits = row.waits { Text(waits.en).font(.footnote).foregroundStyle(.secondary).padding(.leading, 12) }
      }
      Spacer(minLength: 40)
    }
  }
}

extension View {
  /** A chat's top: on the iPhone the system's bar (standard back, the face and name in the middle as the menu);
   *  elsewhere the pills. */
  @ViewBuilder func sessionChrome<H: View, M: View>(page: CoreView.SessionView, hue: Int, @ViewBuilder pill: () -> H, @ViewBuilder menu: () -> M) -> some View {
    #if os(iOS)
    if NativeLook.on {
      let items = menu()
      self.navigationTitle(page.name.en)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar(.visible, for: .navigationBar)
        .toolbar {
          ToolbarItem(placement: .principal) {
            Menu { items } label: { NativeSessionTitle(page: page, hue: hue) }
              .accessibilityLabel("\(page.name.en)\(page.modelLabel.isEmpty ? "" : ", \(page.modelLabel)"), \(page.dot.says.en)")
              .accessibilityHint(page.quiet?.en ?? "Opens the chat's menu")
              .accessibilityIdentifier("session-title")
          }
          ToolbarItem(placement: .topBarTrailing) { BoardButton(round: false) }
        }
    } else {
      self.pushedPills(solid: AppShell.phone) { pill() } trailing: { BoardButton() }
    }
    #else
    self.pushedPills(solid: AppShell.phone) { pill() } trailing: { BoardButton() }
    #endif
  }
}

/** The title of a chat in the bar, as a messenger's: the face above the name, a small chevron. */
struct NativeSessionTitle: View {
  let page: CoreView.SessionView
  let hue: Int
  var body: some View {
    VStack(spacing: 1) {
      NativeAvatar(mark: page.mark, hue: hue, size: 24, online: page.online)
      HStack(spacing: 2) {
        Text(page.name.en).font(.caption.weight(.semibold)).foregroundStyle(page.online ? Color.primary : Color.secondary)
          .lineLimit(1).truncationMode(.middle)
        Image(systemName: "chevron.right").font(.system(size: 8, weight: .bold)).foregroundStyle(.tertiary)
      }
    }
    .frame(maxWidth: 220)
  }
}

// ---- the lists --------------------------------------------------------------------------------------------------------

#if os(iOS)
/** The controls of the iPhone's lists, one row at the bottom: the system's search, and beside it the bolt over every
 *  team and the + in one glass group. Nothing else at the bottom; the person's picture is a small button at the top
 *  right. The words go to the core. */
struct ListControls: ToolbarContent {
  var desk = false
  var body: some ToolbarContent {
    DefaultToolbarItem(kind: .search, placement: .bottomBar)
    ToolbarSpacer(.fixed, placement: .bottomBar)
    ToolbarItemGroup(placement: .bottomBar) {
      BlitzAllButton()
      PlusMenu(desk: desk)
    }
  }
}

/** What the core found under the words, every team's chats under its name; or its line for nothing. */
struct NativeFound: View {
  @EnvironmentObject var shell: AppShell
  /** The core answers a search: its chats under the words, or its line for nothing. The shell reads no words itself. */
  static func searching(_ side: CoreView.SidebarView) -> Bool { !side.found.isEmpty || side.foundNone != nil }
  var body: some View {
    let side = shell.view.sidebar
    ForEach(side.found, id: \.id) { group in
      Section { ForEach(group.rows, id: \.id) { row in NativeChatRow(row: row) } } header: { Text(group.name.en) }
    }
    if let none = side.foundNone {
      Text(none.en).font(.body).foregroundStyle(.secondary).frame(maxWidth: .infinity).padding(.vertical, 40)
        .listRowBackground(Color.clear).listRowSeparator(.hidden)
        .accessibilityIdentifier("chats-none")
    }
  }
}

extension NativeLook {
  /** A cell's own fill in the grouped list (the ring between a team's two faces). */
  static var cell: Color { Color(uiColor: .secondarySystemGroupedBackground) }
  /** The quiet yellow of a team where a question waits. */
  static let waits = Color.dyn(0xe8b400, 0xf5d64a)
  /** The large title alone stands in Bricolage; every other word is the system's. */
  static func dress() {
    guard on, let face = UIFont(name: "BricolageGrotesque-Bold", size: 34) else { return }
    UINavigationBar.appearance().largeTitleTextAttributes = [.font: UIFontMetrics(forTextStyle: .largeTitle).scaledFont(for: face)]
  }
}

extension View {
  /** A list of the iPhone: inset and grouped, the large title, the system's search whose words the core filters by,
   *  the controls at the bottom. */
  func nativeList(title: String, query: Binding<String>, onQuery: @escaping () -> Void) -> some View {
    self.listStyle(.insetGrouped)
      .navigationTitle(title)
      .navigationBarTitleDisplayMode(.large)
      // (the system's bar stays, also when coming back from a page that hid it for its pills)
      .toolbar(.visible, for: .navigationBar)
      .searchable(text: query, prompt: "Search")
      .onChange(of: query.wrappedValue) { _, _ in onQuery() }
  }
}

/** The root, two inset groups: the chats on no team, then the teams, each in the core's order; a team opens its page
 *  of open cards. */
struct NativeRoot: View {
  @EnvironmentObject var shell: AppShell
  var body: some View {
    let side = shell.view.sidebar
    List {
      if NativeFound.searching(side) {
        NativeFound()
      } else {
        if !side.root.isEmpty {
          Section {
            ForEach(side.root, id: \.id) { row in NativeChatRow(row: row) }
              .onMove(perform: chatMove(side.root) { shell.act($0) })
          }
        }
        if !side.stacks.isEmpty {
          Section { ForEach(side.stacks, id: \.id) { stack in NativeTeamRow(stack: stack) } } header: { Text("Teams") }
        }
      }
    }
    .nativeList(title: "Chats", query: $shell.chatsQuery) { shell.show(shell.screen) }
    .toolbar {
      if shell.path.isEmpty {
        ToolbarItem(placement: .topBarTrailing) { MePicture() }
        ListControls()
      }
    }
    .refreshable { shell.act(["do": "retry"]) }
    .accessibilityIdentifier("team-folders")
  }
}

/** A row's one quiet sign at the end of its second line: new words (the accent), a question waiting (yellow), stopped
 *  (red); nothing otherwise. */
private struct RowDot: View {
  let signal: String
  var body: some View {
    let color: Color? = signal == "unread" ? Ink.accent : signal == "waiting" ? NativeLook.waits : signal == "stopped" ? Ink.urgCritical : nil
    if let color { Circle().fill(color).frame(width: 10, height: 10).padding(.top, 4) }
  }
}

/** The calm count of open cards: a grey number, never red, no pill; the row's label says it in the core's words. */
struct NativeCount: View {
  let n: Int
  var body: some View {
    Text("\(n)").font(.subheadline).monospacedDigit().foregroundStyle(.secondary).lineLimit(1).fixedSize()
      .accessibilityHidden(true)
  }
}

/** A chat in the list, compact: face, name, time, the core's teaser in one line; the count of its cards and its one
 *  quiet sign at the end. The separator starts under the words. */
struct NativeChatRow: View {
  @EnvironmentObject var shell: AppShell
  let row: CoreView.SessionRowView
  var body: some View {
    let hue = Pen.hueFor(id: row.slug.isEmpty ? row.id : row.slug, mark: row.mark)
    let teaser = row.last?.text ?? row.cap?.word.en ?? ""
    let alert = row.last == nil && (row.cap?.kind == "unheard" || row.cap?.kind == "cut")
    Button { shell.focus(.session(sessionId: row.id)) } label: {
      HStack(alignment: .center, spacing: 12) {
        NativeAvatar(mark: row.mark, hue: hue, size: 44, online: row.online)
        VStack(alignment: .leading, spacing: 1) {
          HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(row.name.en).font(.body.weight(.semibold))
              .foregroundStyle(row.online ? Color.primary : Color.secondary).lineLimit(1)
            Spacer(minLength: 6)
            if let when = row.last?.when.en {
              Text(when).font(.subheadline).foregroundStyle(.secondary).lineLimit(1).fixedSize()
            }
          }
          HStack(alignment: .top, spacing: 8) {
            Text(teaser.replacingOccurrences(of: "\n", with: " ")).font(.subheadline)
              .foregroundStyle(alert ? Ink.urgHigh : Color.secondary)
              .lineLimit(1).truncationMode(.tail)
              .frame(maxWidth: .infinity, alignment: .leading)
              .accessibilityIdentifier("chat-teaser-" + row.id)
            if row.cards > 0 { NativeCount(n: Int(row.cards)) }
            RowDot(signal: row.signal)
          }
        }
        .alignmentGuide(.listRowSeparatorLeading) { d in d[.leading] }
      }
      .padding(.vertical, 2)
      .contentShape(Rectangle())
    }
    .buttonStyle(.plain)
    .accessibilityElement(children: .ignore)
    // (only the core's words: its badge, why it is stopped, its link's sentence, its helpers, the teaser, the time)
    .accessibilityLabel([row.name.en, row.says?.en, row.hand?.text.en, row.cap?.line?.en, row.helpers?.says.en, teaser, row.last?.when.en].compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: ". "))
    .accessibilityAddTraits(.isButton)
    .accessibilityIdentifier("chat-session-" + row.id)
  }
}

/** A team in the list, in a chat's grammar: two of its faces stacked, its name and the count of its open cards, what
 *  was said last in it under the name, a yellow dot while a question of it waits. A tap opens its page of open cards. */
struct NativeTeamRow: View {
  @EnvironmentObject var shell: AppShell
  let stack: CoreView.TeamStackView
  var body: some View {
    Button { shell.openTeam(stack.id) } label: {
      HStack(alignment: .center, spacing: 12) {
        NativeFaces(faces: stack.faces, size: 44) { id in stack.sessions.first { $0.id == id }?.slug }
        VStack(alignment: .leading, spacing: 1) {
          HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(stack.name.en).font(.body.weight(.semibold)).foregroundStyle(.primary).lineLimit(1)
            Spacer(minLength: 6)
            if stack.cards > 0 { NativeCount(n: Int(stack.cards)) }
          }
          HStack(alignment: .top, spacing: 8) {
            Text((stack.last?.text ?? stack.says.en).replacingOccurrences(of: "\n", with: " ")).font(.subheadline)
              .foregroundStyle(.secondary).lineLimit(1).truncationMode(.tail)
              .frame(maxWidth: .infinity, alignment: .leading)
            RowDot(signal: stack.waiting > 0 ? "waiting" : "")
          }
        }
        .alignmentGuide(.listRowSeparatorLeading) { d in d[.leading] }
      }
      .padding(.vertical, 2)
      .contentShape(Rectangle())
    }
    .buttonStyle(.plain)
    .accessibilityElement(children: .ignore)
    .accessibilityLabel([stack.name.en, stack.says.en, stack.last?.text].compactMap { $0 }.joined(separator: ". "))
    .accessibilityAddTraits(.isButton)
    .accessibilityIdentifier("chat-folder-" + stack.id)
  }
}

/** A group's picture: one face alone, or two cleanly stacked (the second up at the right, the first in front at the
 *  bottom left, a ring of the cell between them); a quiet placeholder for none. */
struct NativeFaces: View {
  let faces: [CoreView.StackEdgeView]
  let size: CGFloat
  /** A face's short id, for its hue as its row has it. */
  var slug: (String) -> String? = { _ in nil }
  var body: some View {
    let two = Array(faces.prefix(2))
    let small = (size * 0.66).rounded()
    ZStack {
      if two.isEmpty {
        Circle().fill(NativeLook.bubble).overlay { Image(systemName: "person.2.fill").font(.callout).foregroundStyle(.secondary) }
      } else if two.count == 1 {
        face(two[0], size)
      } else {
        face(two[1], small).frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topTrailing)
        face(two[0], small)
          .background(Circle().fill(NativeLook.cell).padding(-2))
          .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomLeading)
      }
    }
    .frame(width: size, height: size)
    .accessibilityHidden(true)
  }
  private func face(_ f: CoreView.StackEdgeView, _ s: CGFloat) -> some View {
    let key = slug(f.id).flatMap { $0.isEmpty ? nil : $0 } ?? f.id
    return NativeAvatar(mark: f.mark, hue: Pen.hueFor(id: key, mark: f.mark), size: s)
  }
}
#endif
