// IOSProposal.swift: the iPhone's frame. One navigation stack and no tab bar: at its root chats on no team and every
// team as a folder (the core's `sidebar.root` and `sidebar.stacks`, in drawing order); a folder pushes that team's
// chats on a page of their own (`Focus.team`); a chat opens its page as on every device, and back returns home. The
// bolt at the top right walks every open card, including chats on no team (`sidebar.global_blitz`). Search, the note
// and the person's menu remain as before. Drawing only: rows, order, counts and words belong to the core.
import SwiftUI
import TrommiClient

#if os(iOS)
struct PhoneFrame: View {
  @EnvironmentObject var shell: AppShell
  var body: some View {
    NavigationStack(path: $shell.path) {
      TeamFolders().toolbarBackground(.hidden, for: .navigationBar).scrollEdgeEffectStyle(.soft, for: .all).tint(Ink.accent)
        .navigationDestination(for: CoreView.Screen.self) { screen in
          Group {
            // (`.desk` pushed is a team's page, its chats: the phone draws no Desk of its own)
            if screen == .desk, shell.teamPage != nil { TeamChats() } else { CorePage(screen: screen) }
          }
          .toolbarBackground(.hidden, for: .navigationBar).scrollEdgeEffectStyle(.soft, for: .all)
        }
    }
    // (the search, from the top right: every team's chats under the words; the core filters)
    .sheet(isPresented: $shell.searchOpen, onDismiss: { shell.chatsQuery = ""; shell.show(shell.screen) }) {
      NavigationStack {
        ChatsScreen(searching: true)
          .searchable(text: $shell.chatsQuery, placement: .navigationBarDrawer(displayMode: .always), prompt: "Search chats")
          .onChange(of: shell.chatsQuery) { _, _ in shell.show(shell.screen) }
      }
    }
    // (the note lies over everything as a yellow page)
    .overlay { if shell.noteOpen { NotePage().transition(.move(edge: .bottom)) } }
    .animation(.snappy, value: shell.noteOpen)
    .onChange(of: shell.path) { _, path in
      if path.isEmpty { shell.teamPage = nil }
      // (back on a team's page from every team's Blitz: that team in view again)
      if path.last == .desk, let team = shell.teamPage, shell.deskAll { shell.show(.desk, team: team); return }
      let screen = path.last ?? (shell.noteOpen ? .notes : .desk)
      if shell.screen != screen { shell.show(screen) }
    }
  }
}

/** The root: chats on no team alongside every team as a folder; a folder pushes its chats. */
struct TeamFolders: View {
  @EnvironmentObject var shell: AppShell
  var body: some View {
    List {
      ForEach(shell.view.sidebar.root, id: \.id) { row in ChatRow(row: row) }
        .onMove { from, to in if let a = placeChat(shell.view.sidebar.root, from: from, to: to) { shell.act(a) } }
      ForEach(shell.view.sidebar.stacks, id: \.id) { stack in
        Button { shell.openTeam(stack.id) } label: {
          HStack(spacing: 8) {
            TeamFolder(stack: stack)
            Image(systemName: "chevron.right").font(.footnote.weight(.semibold)).foregroundStyle(Ink.faint)
          }
          .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(stack.name.en): \(stack.says.en)")
        .accessibilityAddTraits(.isButton)
        .accessibilityIdentifier("chat-folder-" + stack.id)
      }
    }
    .listStyle(.insetGrouped)
    .scrollContentBackground(.hidden)
    .background(Ink.bg)
    .navigationBarTitleDisplayMode(.inline)
    .toolbar(.visible, for: .navigationBar)
    .toolbar {
      if shell.path.isEmpty {
        MainBar.items(title: AnyView(Text("Chats").font(.headline)), desk: false)
        ToolbarSpacer(.fixed, placement: .topBarTrailing)
        ToolbarItem(placement: .topBarTrailing) { BlitzAllButton() }
      }
    }
    .refreshable { shell.act(["do": "retry"]) }
    .accessibilityIdentifier("team-folders")
  }
}

/** A team's page: its chats (the core's rows of that team's stack), held and dragged to move as in the Chats list. */
struct TeamChats: View {
  @EnvironmentObject var shell: AppShell
  var body: some View {
    let side = shell.view.sidebar
    let stack = side.stacks.first { $0.id == (shell.teamPage ?? side.team) }
    let rows = stack?.sessions ?? []
    List {
      ForEach(rows, id: \.id) { row in ChatRow(row: row) }
        .onMove { from, to in if let a = placeChat(rows, from: from, to: to) { shell.act(a) } }
    }
    .listStyle(.plain)
    .scrollContentBackground(.hidden)
    .environment(\.defaultMinListRowHeight, 1)
    .background(Ink.bg)
    .navigationTitle(stack?.name.en ?? "")
    .navigationBarTitleDisplayMode(.inline)
    .toolbar(.visible, for: .navigationBar)
    .toolbar {
      ToolbarItemGroup(placement: .topBarTrailing) {
        SearchButton()
        PlusMenu(desk: false)
      }
      ToolbarSpacer(.fixed, placement: .topBarTrailing)
      ToolbarItem(placement: .topBarTrailing) { BlitzAllButton() }
    }
    .refreshable { shell.act(["do": "retry"]) }
    .accessibilityIdentifier("team-chats-" + (stack?.id ?? ""))
  }
}

/** A team as a folder row: its faces (the core's `faces`), its name, what it says, a dot while a question of it
 *  waits, the calm count of what its Blitz walks. */
struct TeamFolder: View {
  let stack: CoreView.TeamStackView
  var body: some View {
    HStack(spacing: 12) {
      ZStack {
        if stack.faces.isEmpty { Image(systemName: "folder").font(.system(size: 18)).foregroundStyle(Ink.muted) }
        HStack(spacing: -12) {
          ForEach(stack.faces, id: \.id) { face in
            let row = stack.sessions.first { $0.id == face.id }
            let hue = Pen.hueFor(id: (row?.slug).flatMap { $0.isEmpty ? nil : $0 } ?? face.id, mark: face.mark)
            AgentMark(mark: face.mark, size: 16, hue: hue).frame(width: 30, height: 30)
              .background(Circle().fill(Tone.color(hue: hue, .wash))).overlay(Circle().stroke(Ink.bg, lineWidth: 2))
          }
        }
      }
      .frame(minWidth: 30, alignment: .leading)
      VStack(alignment: .leading, spacing: 1) {
        Text(stack.name.en).font(Face.text(17, .medium)).foregroundStyle(Ink.fg).lineLimit(1)
        Text(stack.says.en).font(Face.text(13)).foregroundStyle(Ink.muted).lineLimit(1)
      }
      Spacer(minLength: 4)
      if stack.waiting > 0 { Circle().fill(Ink.accent).frame(width: 9, height: 9) }
      if stack.cards > 0 { QuietCount(n: Int(stack.cards)) }
    }
    .padding(.vertical, 4)
  }
}

/** The bolt at the top right: Blitz over every team's open cards (`sidebar.global_blitz`), with its count; off while
 *  nothing is open. The card's page walks on to the next. */
struct BlitzAllButton: View {
  @EnvironmentObject var shell: AppShell
  var body: some View {
    let blitz = shell.view.sidebar.globalBlitz
    Button { shell.blitzAll() } label: {
      HStack(spacing: 4) {
        Image(systemName: blitz != nil ? "bolt.fill" : "bolt")
        if let n = blitz?.says.args["n"] { Text(n).font(.caption.weight(.semibold)).monospacedDigit() }
      }
    }
    .disabled(blitz == nil)
    .accessibilityLabel(blitz?.says.en ?? "Blitz")
    .accessibilityIdentifier("chats-blitz-all")
  }
}
#endif
