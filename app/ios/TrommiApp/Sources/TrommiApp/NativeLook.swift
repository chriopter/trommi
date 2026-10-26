// NativeLook.swift: the iPhone's native look, drawing only. A messenger's grammar in the system's own parts: round
// faces, bold names, two grey lines of what was said, inset separators, a compact bar with the title in the middle, and
// one row of controls at the bottom (the system's search, the bolt over every team, the +). Bubbles in the chat; the
// agent's words keep the full Markdown. The iPad and the Mac never reach it. Rows, order, counts and words belong to
// the core.
import SwiftUI
import TrommiClient
#if os(iOS)
import UIKit
#endif

@MainActor enum NativeLook {
  /** The native look is the iPhone's; the iPad and the Mac keep theirs. */
  static var on: Bool { AppShell.phone }
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
        }
    } else {
      self.pushedPills(solid: AppShell.phone) { pill() } trailing: { EmptyView() }
    }
    #else
    self.pushedPills(solid: AppShell.phone) { pill() } trailing: { EmptyView() }
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
/** The controls of the chat lists, one row at the bottom: the system's search, then the bolt over every team and the
 *  +, each its own round button. No tab bar, nothing at the top right. The words go to the core. */
struct ListControls: ToolbarContent {
  var body: some ToolbarContent {
    DefaultToolbarItem(kind: .search, placement: .bottomBar)
    ToolbarSpacer(.fixed, placement: .bottomBar)
    ToolbarItem(placement: .bottomBar) { BlitzAllButton() }
    ToolbarSpacer(.fixed, placement: .bottomBar)
    ToolbarItem(placement: .bottomBar) { PlusMenu(desk: false) }
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

extension View {
  /** A chat list's frame: the plain list, the compact bar with the title in the middle, the system's search whose
   *  words the core filters by, the controls at the bottom. */
  func nativeChatList(title: String, query: Binding<String>, onQuery: @escaping () -> Void) -> some View {
    self.listStyle(.plain)
      .navigationTitle(title)
      .navigationBarTitleDisplayMode(.inline)
      // (the system's bar stays, also when coming back from a page that hid it for its pills)
      .toolbar(.visible, for: .navigationBar)
      .searchable(text: query, prompt: "Search")
      .onChange(of: query.wrappedValue) { _, _ in onQuery() }
  }
}

/** The root: chats on no team first, then every team as a folder, in the core's order; a folder pushes its chats. */
struct NativeRoot: View {
  @EnvironmentObject var shell: AppShell
  var body: some View {
    let side = shell.view.sidebar
    List {
      if NativeFound.searching(side) {
        NativeFound()
      } else {
        ForEach(side.root, id: \.id) { row in NativeChatRow(row: row) }
          .onMove(perform: chatMove(side.root) { shell.act($0) })
        ForEach(side.stacks, id: \.id) { stack in NativeTeamRow(stack: stack) }
      }
    }
    .nativeChatList(title: "Chats", query: $shell.chatsQuery) { shell.show(shell.screen) }
    .toolbar {
      if shell.path.isEmpty {
        ToolbarItem(placement: .topBarLeading) { MePicture() }
        ListControls()
      }
    }
    .refreshable { shell.act(["do": "retry"]) }
    .accessibilityIdentifier("team-folders")
  }
}

/** A team's page: its chats in the core's order (newest first); the same controls at the bottom (the bolt
 *  still walks every team). */
struct NativeTeamChats: View {
  @EnvironmentObject var shell: AppShell
  var body: some View {
    let side = shell.view.sidebar
    let stack = side.stacks.first { $0.id == (shell.teamPage ?? side.team) }
    let rows = stack?.sessions ?? []
    List {
      if NativeFound.searching(side) {
        NativeFound()
      } else {
        ForEach(rows, id: \.id) { row in NativeChatRow(row: row) }
          .onMove(perform: chatMove(rows) { shell.act($0) })
      }
    }
    .nativeChatList(title: stack?.name.en ?? "", query: $shell.chatsQuery) { shell.show(shell.screen) }
    .toolbar { ListControls() }
    .refreshable { shell.act(["do": "retry"]) }
    .accessibilityIdentifier("team-chats-" + (stack?.id ?? ""))
  }
}

/** The margin before a face: a dot while something new waits (as Messages has it), otherwise empty, so every face and
 *  name stands in one column. */
private struct NewDot: View {
  let on: Bool
  var body: some View { Circle().fill(on ? Ink.accent : Color.clear).frame(width: 10, height: 10).padding(.trailing, -4) }
}

/** A chat in the list: face, name, time, the core's teaser in two lines; a dot while something new waits, the count
 *  of its cards. The separator starts under the words. */
struct NativeChatRow: View {
  @EnvironmentObject var shell: AppShell
  let row: CoreView.SessionRowView
  var body: some View {
    let hue = Pen.hueFor(id: row.slug.isEmpty ? row.id : row.slug, mark: row.mark)
    let teaser = row.last?.text ?? row.cap?.word.en ?? ""
    let alert = row.last == nil && (row.cap?.kind == "unheard" || row.cap?.kind == "cut")
    Button { shell.focus(.session(sessionId: row.id)) } label: {
      HStack(alignment: .center, spacing: 12) {
        NewDot(on: !row.signal.isEmpty)
        NativeAvatar(mark: row.mark, hue: hue, size: 48, online: row.online)
        VStack(alignment: .leading, spacing: 2) {
          HStack(alignment: .firstTextBaseline, spacing: 6) {
            Text(row.name.en).font(.body.weight(.semibold))
              .foregroundStyle(row.online ? Color.primary : Color.secondary).lineLimit(1)
            Spacer(minLength: 6)
            if let when = row.last?.when.en {
              Text(when).font(.subheadline).foregroundStyle(.secondary).lineLimit(1).fixedSize()
            }
            Image(systemName: "chevron.right").font(.footnote.weight(.semibold)).foregroundStyle(.tertiary)
          }
          HStack(alignment: .top, spacing: 8) {
            // (two full lines: line breaks of the message only set as spaces, never cut off at the first one)
            Text(teaser.replacingOccurrences(of: "\n", with: " ")).font(.subheadline)
              .foregroundStyle(alert ? Ink.urgHigh : Color.secondary)
              .lineLimit(2, reservesSpace: true).truncationMode(.tail)
              .multilineTextAlignment(.leading)
              .fixedSize(horizontal: false, vertical: true)
              .frame(maxWidth: .infinity, alignment: .leading)
              .accessibilityIdentifier("chat-teaser-" + row.id)
            if row.cards > 0 { NativeCount(n: Int(row.cards)) }
          }
        }
        .alignmentGuide(.listRowSeparatorLeading) { d in d[.leading] }
      }
      .padding(.vertical, 4)
      .contentShape(Rectangle())
    }
    .accessibilityElement(children: .ignore)
    // (only the core's words: its badge, why it is stopped, its link's sentence, its helpers, the teaser, the time)
    .accessibilityLabel([row.name.en, row.says?.en, row.hand?.text.en, row.cap?.line?.en, row.helpers?.says.en, teaser, row.last?.when.en].compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: ". "))
    .accessibilityAddTraits(.isButton)
    .accessibilityIdentifier("chat-session-" + row.id)
  }
}

/** The calm count of open cards: a small grey capsule (never red), the number only; the row's label says it in the
 *  core's words. */
struct NativeCount: View {
  let n: Int
  var body: some View {
    Text("\(n)").font(.caption.weight(.semibold)).monospacedDigit().foregroundStyle(.secondary)
      .padding(.horizontal, 7).frame(minWidth: 22, minHeight: 20).background(Capsule().fill(NativeLook.bubble))
      .accessibilityHidden(true)
  }
}

/** A team as a folder, in a chat's grammar: its faces in one round picture, its name, what it says, a dot while a
 *  question of it waits, its count; a push. */
struct NativeTeamRow: View {
  @EnvironmentObject var shell: AppShell
  let stack: CoreView.TeamStackView
  var body: some View {
    Button { shell.openTeam(stack.id) } label: {
      HStack(alignment: .center, spacing: 12) {
        NewDot(on: stack.waiting > 0)
        faces
        VStack(alignment: .leading, spacing: 2) {
          Text(stack.name.en).font(.body.weight(.semibold)).foregroundStyle(.primary).lineLimit(1)
          Text(stack.says.en).font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
        }
        .alignmentGuide(.listRowSeparatorLeading) { d in d[.leading] }
        Spacer(minLength: 4)
        if stack.cards > 0 { NativeCount(n: Int(stack.cards)) }
        Image(systemName: "chevron.right").font(.footnote.weight(.semibold)).foregroundStyle(.tertiary)
      }
      .padding(.vertical, 4)
      .contentShape(Rectangle())
    }
    .accessibilityElement(children: .ignore)
    .accessibilityLabel("\(stack.name.en): \(stack.says.en)")
    .accessibilityAddTraits(.isButton)
    .accessibilityIdentifier("chat-folder-" + stack.id)
  }
  private func hue(id: String, mark: String) -> Int {
    let row = stack.sessions.first { $0.id == id }
    return Pen.hueFor(id: (row?.slug).flatMap { $0.isEmpty ? nil : $0 } ?? id, mark: mark)
  }
  /** Up to three faces overlapping in one round well, a group's picture. */
  private var faces: some View {
    let list = Array(stack.faces.prefix(3))
    return ZStack {
      Circle().fill(NativeLook.bubble)
      if list.isEmpty { Image(systemName: "folder.fill").font(.title3).foregroundStyle(.secondary) }
      HStack(spacing: -10) {
        ForEach(list, id: \.id) { face in
          NativeAvatar(mark: face.mark, hue: hue(id: face.id, mark: face.mark), size: list.count > 2 ? 21 : 26)
            .overlay(Circle().stroke(NativeLook.bubble, lineWidth: 1.5))
        }
      }
    }
    .frame(width: 48, height: 48)
  }
}
#endif
