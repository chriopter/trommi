// Screens.swift: the frame around every board screen and the Desk, as the app drew them before the shared app core:
// on the iPhone the glass pill at the bottom (Chat · Desk · Note as drawings, Note as a full yellow page over the
// list), the place pill (the menu) at the top left; on the iPad and the Mac the sessions as a sidebar column beside
// the stack. The Desk: one big greeting with its last word underlined by the pen, the team's goals, the open questions
// as calm cards with their answer tiles, Off your mind under them, the two docked stacks. What is said, in what order
// and what shows is the core's view model; this file only draws it.
import SwiftUI
import TrommiClient
import UniformTypeIdentifiers

struct RootView: View {
  @EnvironmentObject var shell: AppShell
  @Environment(\.horizontalSizeClass) private var width
  @AppStorage("trommi-theme") private var theme = "system"
  private var wide: Bool {
    #if os(macOS)
    true
    #else
    width == .regular
    #endif
  }
  var body: some View {
    let _ = NativeDemo.mark("root.body")
    Group {
      if shell.view.phase == .starting { Starting() }
      else if shell.view.phase == .signedOut || shell.view.account.step == "keep_kit" { AccountScreen() }
      else if wide {
        NavigationSplitView {
          Sidebar().navigationSplitViewColumnWidth(min: 260, ideal: 300, max: 360).toolbar(.hidden, for: .navigationBar)
        } detail: { stack(DeskScreen()) }
        .sheet(isPresented: noteBinding) { NavigationStack { NotesScreen() }.presentationDetents([.large]) }
        .overlay(alignment: .top) { TopLines() }
      } else {
        // iPhone: the system's tab bar, Desk · Chats · Search; it shrinks while a list scrolls down. The note lies
        // over everything as a yellow page.
        #if os(iOS)
        PhoneTabs().overlay(alignment: .top) { TopLines() }
        #endif
      }
    }
    .overlay { ToastHost() }
    .background(Ink.bg.ignoresSafeArea())
    .foregroundStyle(Ink.fg)
    .preferredColorScheme(theme == "light" ? .light : theme == "dark" ? .dark : nil)
    .sheet(item: $shell.saved) { file in SaveSheet(file: file) }
    .alert("Trommi", isPresented: Binding(get: { shell.platformError != nil }, set: { if !$0 { shell.platformError = nil } })) { Button("OK") { shell.platformError = nil } } message: { Text(shell.platformError ?? "") }
  }
  private var noteBinding: Binding<Bool> { Binding(get: { shell.noteOpen }, set: { if !$0 { shell.closeNote() } }) }
  private func stack<Root: View>(_ root: Root) -> some View {
    NavigationStack(path: $shell.path) {
      root.toolbarBackground(.hidden, for: .navigationBar)
        .navigationDestination(for: CoreView.Screen.self) { screen in page(screen).toolbarBackground(.hidden, for: .navigationBar) }
    }
    .onChange(of: shell.path) { _, path in
      let screen = path.last ?? (shell.noteOpen ? .notes : .desk)
      if shell.screen != screen { shell.show(screen) }
    }
  }
  @ViewBuilder func page(_ screen: CoreView.Screen) -> some View { CorePage(screen: screen) }
}

/** A screen of the core's pushed onto a navigation stack. */
struct CorePage: View {
  @EnvironmentObject var shell: AppShell
  let screen: CoreView.Screen
  var body: some View {
    switch screen {
    case .desk: DeskScreen()
    case .card(let id): if let card = shell.view.cards[id] { CardScreen(card: card).id(id) } else { Loading() }
    case .session(let id): if let page = shell.view.sessions[id] { SessionScreen(page: page).id(id) } else { Loading() }
    case .notes: NotesScreen()
    case .off: OffScreen()
    case .settings(let page): SettingsScreen(page: page)
    case .artifacts: ArtifactsScreen()
    case .knowledge: KnowledgeScreen()
    case .scribble: ScribbleScreen()
    }
  }
}

#if os(iOS)
/**
 * The iPhone's frame: the system's tab bar with Desk (its number is the core's `sidebar.badge`), Chats (every team's
 * chats in one list) and Search (the same list under the words searched for; the core filters). Each tab keeps its
 * own navigation stack; only the tab in front follows `shell.path`, and a pushed screen hides the bar (it has its own
 * controls at the bottom).
 */
struct PhoneTabs: View {
  @EnvironmentObject var shell: AppShell
  var body: some View {
    TabView(selection: Binding(get: { shell.tab }, set: { shell.pick($0) })) {
      // (icons only, in black: the tray for the Desk, full while something waits, never a red count; the message for
      // the chats; the system fills the one chosen)
      Tab(value: NativeTab.desk) { stack(.desk) { DeskScreen() } } label: {
        Image(systemName: shell.view.sidebar.badge > 0 ? "tray.full" : "tray").accessibilityLabel("Desk")
      }
      Tab(value: NativeTab.chat) { stack(.chat) { ChatsScreen() } } label: { Image(systemName: "message").accessibilityLabel("Chats") }
      Tab(value: NativeTab.search, role: .search) {
        stack(.search) {
          // (the system's search field, always shown under the title; the core filters the chats by its words)
          ChatsScreen(searching: true)
            .searchable(text: $shell.chatsQuery, placement: .navigationBarDrawer(displayMode: .always), prompt: "Search chats")
            .onChange(of: shell.chatsQuery) { _, _ in shell.show(shell.screen) }
        }
      } label: { Image(systemName: "magnifyingglass").accessibilityLabel("Search") }
    }
    .tint(Color.primary)
    .tabBarMinimizeBehavior(.onScrollDown)
    // (the note lies over everything as a yellow page)
    .overlay { if shell.noteOpen { NotePage().transition(.move(edge: .bottom)) } }
    .animation(.snappy, value: shell.noteOpen)
    .onChange(of: shell.path) { _, path in
      let screen = path.last ?? (shell.noteOpen ? .notes : .desk)
      if shell.screen != screen { shell.show(screen) }
    }
  }
  private func stack<Root: View>(_ tab: NativeTab, @ViewBuilder _ root: () -> Root) -> some View {
    NavigationStack(path: Binding(get: { shell.tab == tab ? shell.path : [] }, set: { if shell.tab == tab { shell.path = $0 } })) {
      // (the bar's black stays the bar's: the pages keep the app's green; lists fade softly under both bars)
      root().toolbarBackground(.hidden, for: .navigationBar).scrollEdgeEffectStyle(.soft, for: .all).tint(Ink.accent)
        .navigationDestination(for: CoreView.Screen.self) { screen in
          CorePage(screen: screen).toolbarBackground(.hidden, for: .navigationBar).toolbar(.hidden, for: .tabBar)
            .scrollEdgeEffectStyle(.soft, for: .all)
        }
    }
  }
}
#endif

/** The lines over the board, under the top row: the core's line (`ViewModel.status`: while it opens, what it does;
 *  offline or stopped, that what shows is what this device last had) and a sign-in that was cut off
 *  (`AccountView.resume`), with the way to log in again. Nothing while the core says nothing. */
struct TopLines: View {
  @EnvironmentObject var shell: AppShell
  @State private var loggingIn = false
  var body: some View {
    VStack(spacing: 8) {
      if let line = shell.view.status {
        Text(line.en).font(Face.text(13, .medium)).foregroundStyle(Ink.fg).lineLimit(2).multilineTextAlignment(.center)
          .padding(.horizontal, 14).padding(.vertical, 7).glass(Capsule())
          .allowsHitTesting(false)
          .accessibilityIdentifier("core-status")
      }
      if let resume = shell.view.account.resume {
        HStack(spacing: 10) {
          Text(resume.en).font(Face.text(14, .medium)).foregroundStyle(Ink.fg).fixedSize(horizontal: false, vertical: true)
          Button("Log in") { loggingIn = true }.font(Face.text(14, .bold)).foregroundStyle(Ink.accent).buttonStyle(.plain)
            .accessibilityIdentifier("resume-login")
        }
        .padding(.horizontal, 14).padding(.vertical, 9).glass(Capsule())
        .accessibilityElement(children: .contain).accessibilityIdentifier("resume-line")
      }
    }
    .padding(.horizontal, 16).padding(.top, 58)
    .sheet(isPresented: $loggingIn) { AccountScreen(form: .login) }
    // (the core takes the line away once the way in was given: the sheet goes with it)
    .onChange(of: shell.view.account.resume == nil) { _, gone in if gone { loggingIn = false } }
  }
}

/** The app before its first picture of the room: the bell, calm. */
struct Starting: View {
  var body: some View {
    VStack(spacing: 16) {
      PenMark("ui:BELL", color: Ink.fg).frame(width: 56, height: 56)
      ProgressView()
    }.frame(maxWidth: .infinity, maxHeight: .infinity).background(Ink.bg.ignoresSafeArea())
      .accessibilityLabel("Trommi")
  }
}
struct Loading: View {
  var body: some View { ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity).background(Ink.bg.ignoresSafeArea()) }
}

/** The note on the iPhone: a full page of yellow paper over the list, the tab pill stays and is lit on Note; a drag
 *  down on the handle, or a tab, leaves it. */
struct NotePage: View {
  @EnvironmentObject var shell: AppShell
  @State private var drag: CGFloat = 0
  var body: some View {
    VStack(spacing: 0) {
      Capsule().fill(Ink.noteInk.opacity(0.35)).frame(width: 38, height: 5).padding(.top, 8).padding(.bottom, 9)
        .frame(maxWidth: .infinity).contentShape(Rectangle())
        .gesture(DragGesture().onChanged { drag = max(0, $0.translation.height) }.onEnded { v in
          withAnimation(.snappy) { if v.translation.height > 80 || v.predictedEndTranslation.height > 200 { hideKeyboard(); shell.closeNote() }; drag = 0 }
        })
        .onTapGesture { hideKeyboard(); shell.closeNote() }
        .accessibilityLabel("Close the note").accessibilityAddTraits(.isButton).accessibilityIdentifier("note-close")
        .accessibilityAction { shell.closeNote() }
      NotesScreen()
    }
    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
    .background(Ink.noteYellow.ignoresSafeArea())
    .offset(y: drag)
  }
}

/**
 * The place pill at the top left of the iPhone's pages: the team's drawing and name with a small chevron. It is the
 * menu: the switch of the team in view (each team with its open questions), Scribble and Artifacts, notifications,
 * Settings, the demo.
 */
struct PlacePill: View {
  @EnvironmentObject var shell: AppShell
  @AppStorage("trommi-theme") private var theme = "system"
  /** Round: only the team's drawing in a round glass button (the Chat list); else the drawing with the team's name. */
  var round = false
  /** The iPhone Desk's centered title: the place's name in the system's font with a chevron, nothing drawn. */
  var title = false
  var body: some View {
    let side = shell.view.sidebar
    Menu {
      if shell.isDemo { Section("Demo") { Button { shell.leaveDemo() } label: { Label("Leave Demo", systemImage: "rectangle.portrait.and.arrow.right") } } }
      Section("Teams") {
        HomeItem(home: side.home) { shell.pick(.desk); shell.home() }
        ForEach(side.teams, id: \.id) { teamItem($0) }
      }
      Section {
        Button { go(.scribble(boardId: side.team)) } label: { Label("Scribble", systemImage: "scribble.variable") }
        Button { go(.knowledge(query: "", filter: "", open: "")) } label: { Label("Knowledge", systemImage: "book") }
        Button { go(.artifacts(kind: "")) } label: { Label("Artifacts", systemImage: "photo.on.rectangle") }
        Button { go(.off(query: "")) } label: { Label("Off your mind", systemImage: "tray") }
      }
      Section {
        if let push = shell.view.settings.push {
          Picker(selection: Binding(get: { push.level }, set: { shell.act(["do": "push", "level": $0]) })) {
            ForEach(shell.view.settings.pushChoices, id: \.level) { c in
              Label(c.title.en, systemImage: c.level == "off" ? "bell.slash" : c.level == "knocking" ? "bell.badge" : "bell").tag(c.level)
            }
          } label: { Label("Notifications", systemImage: push.level == "off" ? "bell.slash" : push.level == "knocking" ? "bell.badge" : "bell") }
          .pickerStyle(.menu)
        }
        Picker(selection: $theme) {
          Text("System").tag("system"); Text("Light").tag("light"); Text("Dark").tag("dark")
        } label: { Label("Theme", systemImage: theme == "dark" ? "moon" : "sun.max") }
        .pickerStyle(.menu)
        Button { go(.settings(page: "")); shell.account(["step": "read_account"]) } label: { Label("Settings", systemImage: "gearshape") }
        if !shell.isDemo { Button { shell.enterDemo() } label: { Label("Demo", systemImage: "play.rectangle") } }
        Link(destination: URL(string: "https://app.trommi.com/whats-new")!) { Label("What's new", systemImage: "sparkles") }
        Button { go(.settings(page: "logout")); shell.account(["step": "read_account"]) } label: { Label(shell.view.settings.logout.title.en, systemImage: "rectangle.portrait.and.arrow.right") }
      }
    } label: {
      if title {
        HStack(spacing: 4) {
          Text(side.place.en).font(.headline).foregroundStyle(.primary).lineLimit(1)
          Image(systemName: "chevron.down").font(.caption.weight(.semibold)).foregroundStyle(.secondary)
        }.fixedSize()
      } else if round {
        PenMark("sketch:desk", color: Ink.fg).frame(width: 22, height: 22)
          .overlay(alignment: .topLeading) { if side.waiting > 0 { Circle().fill(Ink.yellow).frame(width: 7, height: 7).offset(x: 3, y: 1) } }
          .frame(width: 44, height: 44)
          .glass(Circle(), interactive: true)
      } else {
        HStack(spacing: 7) {
          PenMark("sketch:desk", color: Ink.fg).frame(width: 22, height: 22)
            .overlay(alignment: .topLeading) { if side.waiting > 0 { Circle().fill(Ink.yellow).frame(width: 7, height: 7).offset(x: 3, y: 1) } }
          Text(side.place.en).font(Face.display(17, .bold)).foregroundStyle(Ink.fg).lineLimit(1)
          Image(systemName: "chevron.down").font(.system(size: 11, weight: .semibold)).foregroundStyle(Ink.muted)
        }
        .padding(.horizontal, 14).frame(height: 44)
        .glass(Capsule(), interactive: true)
      }
    }
    .accessibilityLabel("\(side.place.en), Menu")
    .accessibilityIdentifier("place-pill")
  }
  private func teamItem(_ team: CoreView.TeamTab) -> some View {
    Button { shell.pick(.desk); shell.focus(.desk, team: team.id) } label: {
      Label { Text(team.waiting > 0 ? "\(team.name.en) · \(team.waiting)" : team.name.en) } icon: { if team.selected { Image(systemName: "checkmark") } else { PenImage.desk(waiting: team.waiting > 0) } }
    }
    .accessibilityLabel(team.waiting > 0 ? "\(team.name.en), \(team.waiting) waiting" : team.name.en)
  }
  private func go(_ screen: CoreView.Screen) { shell.closeNote(); shell.focus(screen) }
}

// ---- the sidebar (iPad, Mac) ------------------------------------------------------------------------------

struct Sidebar: View {
  @EnvironmentObject var shell: AppShell
  @AppStorage("trommi-theme") private var theme = "system"
  var body: some View {
    let side = shell.view.sidebar
    VStack(spacing: 0) {
      // The top: only the switch of the team in view, a menu of the teams, each with its open questions.
      Menu {
        HomeItem(home: side.home) { shell.home() }
        ForEach(side.teams, id: \.id) { teamItem($0) }
      } label: {
        HStack(spacing: 12) {
          PenMark("sketch:desk", color: Ink.fg).frame(width: 34, height: 34)
            .overlay(alignment: .topLeading) { if side.waiting > 0 { Circle().fill(Ink.yellow).frame(width: 8, height: 8).offset(x: 4, y: 2) } }
          Text(side.place.en).font(Face.display(26, .heavy)).foregroundStyle(Ink.fg).lineLimit(1).minimumScaleFactor(0.6)
          Spacer(minLength: 4)
          PenMark("sketch:unfold", color: Ink.muted).frame(width: 18, height: 18)
        }
        .padding(.horizontal, 14).padding(.vertical, 12)
        .background(PenBox(r: 14).stroke(Ink.fg, lineWidth: 2))
        .contentShape(Rectangle())
      }
      .buttonStyle(.plain)
      .padding(.horizontal, 10).padding(.top, 10).padding(.bottom, 6)
      .menuIndicator(.hidden)
      .accessibilityLabel("Team: \(side.place.en). Choose the team")
      .accessibilityIdentifier("team-switch")
      ScrollView {
        LazyVStack(alignment: .leading, spacing: 2) {
          ForEach(side.sessions, id: \.id) { row in SessionRow(row: row) }
          Button { shell.focus(.settings(page: "")); shell.account(["step": "read_account"]) } label: {
            HStack(spacing: 12) { PenMark("ui:PLUS").frame(width: 22, height: 22).padding(.leading, 6); Text("New Agent…").font(Face.text(16, .medium)) }
              .foregroundStyle(side.sessions.isEmpty ? Ink.accent : Ink.muted).padding(.vertical, 10).padding(.horizontal, 12)
          }.buttonStyle(.plain)
        }.padding(.vertical, 8)
      }
      Divider().overlay(Ink.line)
      Menu {
        Button { shell.focus(.settings(page: "")); shell.account(["step": "read_account"]) } label: { Label("Settings", systemImage: "key") }
        Button { shell.focus(.scribble(boardId: side.team)) } label: { Label("Scribble", systemImage: "scribble.variable") }
        Button { shell.focus(.knowledge(query: "", filter: "", open: "")) } label: { Label("Knowledge", systemImage: "book") }
        Button { shell.focus(.artifacts(kind: "")) } label: { Label("Artifacts", systemImage: "photo.on.rectangle") }
        Button { shell.focus(.off(query: "")) } label: { Label("Off your mind", systemImage: "tray") }
        Button { shell.focus(.notes) } label: { Label("Note", systemImage: "note.text") }
        if shell.isDemo { Button { shell.leaveDemo() } label: { Label("Leave Demo", systemImage: "rectangle.portrait.and.arrow.right") } }
        else { Button { shell.enterDemo() } label: { Label("Demo", systemImage: "play.rectangle") } }
        Picker("Theme", selection: $theme) { Text("System").tag("system"); Text("Light").tag("light"); Text("Dark").tag("dark") }
        Link(destination: URL(string: "https://app.trommi.com/whats-new")!) { Label("What's new", systemImage: "sparkles") }
        Button { shell.focus(.settings(page: "logout")); shell.account(["step": "read_account"]) } label: { Label(shell.view.settings.logout.title.en, systemImage: "rectangle.portrait.and.arrow.right") }
      } label: {
        HStack(spacing: 12) {
          PenMark("ui:BELL", color: Ink.fg).frame(width: 30, height: 30)
          Text("Trommi").font(Face.display(22, .heavy)).foregroundStyle(Ink.fg)
          Spacer()
          PenMark("sketch:unfold", color: Ink.muted).frame(width: 16, height: 16).rotationEffect(.degrees(180))
        }.padding(.horizontal, 20).padding(.vertical, 14).contentShape(Rectangle())
      }
      .buttonStyle(.plain)
    }
    .background(Ink.bg)
  }
  private func teamItem(_ team: CoreView.TeamTab) -> some View {
    Button { shell.focus(.desk, team: team.id) } label: {
      Label { Text(team.waiting > 0 ? "\(team.name.en) · \(team.waiting)" : team.name.en) } icon: { if team.selected { Image(systemName: "checkmark") } else { PenImage.desk(waiting: team.waiting > 0) } }
    }
    .accessibilityLabel(team.waiting > 0 ? "\(team.name.en), \(team.waiting) waiting" : team.name.en)
  }
}

/** Home in the team menu, above the teams: "All teams", the Desk of every team, with what waits on all of them. */
struct HomeItem: View {
  let home: CoreView.TeamTab
  let pick: () -> Void
  var body: some View {
    Button(action: pick) {
      Label { Text(home.waiting > 0 ? "\(home.name.en) · \(home.waiting)" : home.name.en) } icon: { if home.selected { Image(systemName: "checkmark") } else { PenImage.desk(waiting: home.waiting > 0) } }
    }
    .accessibilityLabel(home.waiting > 0 ? "\(home.name.en), \(home.waiting) waiting" : home.name.en)
    .accessibilityIdentifier("team-home")
  }
}

/** One session in the sidebar: its mark, its name, how it hears, its helpers as dots, what waits. */
struct SessionRow: View {
  @EnvironmentObject var shell: AppShell
  let row: CoreView.SessionRowView
  var body: some View {
    let current = shell.path.last == .session(sessionId: row.id)
    Button { shell.focus(.session(sessionId: row.id)) } label: {
      HStack(spacing: 12) {
        AgentMark(row: row, size: 30).opacity(row.online ? 1 : 0.6)
        VStack(alignment: .leading, spacing: 1) {
          Text(row.name.en).font(Face.text(17, .medium)).foregroundStyle(row.online ? Ink.fg : Ink.muted).lineLimit(1)
          if let cap = row.cap {
            Text(cap.word.en).font(Face.text(13)).foregroundStyle(cap.kind == "unheard" ? Ink.urgHigh : Ink.muted).lineLimit(1)
          }
          // its helpers (its agent's subagents): a dot each, filled while that helper works, and "3 helpers"
          if let h = row.helpers {
            HStack(spacing: 3) {
              ForEach(Array(h.working.enumerated()), id: \.offset) { _, on in
                Circle().fill(on ? Ink.accent : Color.clear).overlay(Circle().stroke(on ? Color.clear : Ink.faint, lineWidth: 1.2)).frame(width: 6, height: 6)
              }
              Text(h.says.en).font(Face.text(12)).foregroundStyle(Ink.muted).lineLimit(1).padding(.leading, 2)
            }
            .accessibilityElement(children: .ignore).accessibilityLabel(h.says.en)
          }
        }
        Spacer(minLength: 4)
        if row.working && !row.blocked && row.open == 0 { WorkingRing(working: true).frame(width: 18, height: 18) }
        SessionBadge(open: row.open, stopped: row.blocked, says: row.says?.en)
        if row.unread, row.open == 0 { Circle().fill(Ink.accent).frame(width: 8, height: 8).accessibilityLabel("New message") }
      }
      .padding(.horizontal, 14).padding(.vertical, 8)
      .contentShape(Rectangle())
    }
    .buttonStyle(.plain)
    .accessibilityIdentifier("sidebar-session-" + row.id)
    .background(RoundedRectangle(cornerRadius: 12).fill(current ? Ink.accentSoft : .clear))
    .padding(.horizontal, 6)
  }
}

// ---- the Desk ----------------------------------------------------------------------------------------------------

struct DeskScreen: View {
  @EnvironmentObject var shell: AppShell
  @Environment(\.horizontalSizeClass) private var hSize
  @State private var speedReport: String?
  var body: some View {
    let desk = shell.view.desk
    // (the iPhone's Desk is calm: only the questions, then one quiet line; no greeting, goals or hint)
    let calm = hSize == .compact
    ScrollView {
      LazyVStack(alignment: .leading, spacing: calm ? 16 : 12) {
        if desk.mode == "invite" { FirstAgent() }
        else if calm { Color.clear.frame(height: 4) }
        else if let hello = desk.hello {
          VStack(alignment: .leading, spacing: 0) {
            Greeting(text: hello.en)
            if let goals = desk.goals { GoalsDrawing(goals: goals).id(goals.teamId) }
            if let know = desk.knowledge { KnowledgeChip(chip: know) }
          }.padding(.top, 18).padding(.bottom, 6)
        }
        if let reading = desk.reading { Text(reading.en).font(Face.text(14)).foregroundStyle(Ink.muted).accessibilityIdentifier("reading-history") }
        if !desk.slip.isEmpty { CutSlip(slips: desk.slip) }
        ForEach(desk.rows, id: \.id) { row in DeskRow(row: row) }
        if calm, let empty = desk.empty {
          ContentUnavailableView(empty.en, systemImage: "checkmark.circle").frame(maxWidth: .infinity).padding(.vertical, 40)
        } else if let empty = desk.empty {
          VStack(spacing: 10) {
            Sketch("desk", color: Ink.faint).frame(width: 64, height: 64)
            Text(empty.en).font(Face.text(15)).foregroundStyle(Ink.muted).multilineTextAlignment(.center)
          }.frame(maxWidth: .infinity).padding(.vertical, 28)
        }
        if calm, !desk.tiles.isEmpty { DeskTiles(tiles: desk.tiles).padding(.top, 8) }
        else if calm { if let off = desk.off { OffLine(off: off).padding(.top, 8) } }
        else if desk.off != nil || desk.starred != nil { OffHint(off: desk.off, starred: desk.starred) }
      }
      .padding(.horizontal, 16).padding(.bottom, 120)
      .frame(maxWidth: 760).frame(maxWidth: .infinity)
    }
    .accessibilityIdentifier("desk-content")
    .accessibilityValue(speedReport ?? String(desk.rows.count))
    .onAppear { speedReport = NativeDemo.ready(rows: desk.rows.count) }
    .refreshable { shell.act(["do": "retry"]) }
    .background(Ink.bg)
    .modifier(DeskTop(regular: hSize == .regular))
  }
}

/** The Desk's top: the floating place pill on the iPhone; on the iPad and the Mac the team's name in the bar. */
struct DeskTop: ViewModifier {
  @EnvironmentObject var shell: AppShell
  let regular: Bool
  func body(content: Content) -> some View {
    #if os(macOS)
    content.navigationTitle(shell.view.sidebar.place.en).toolbar { ToolbarItem(placement: .primaryAction) { DeskWays() } }
    #else
    if regular {
      content.navigationBarTitleDisplayMode(.inline).toolbar {
        ToolbarItem(placement: .principal) {
          HStack(spacing: 8) { PenMark("sketch:desk").frame(width: 24, height: 24); Text(shell.view.sidebar.place.en).font(Face.display(19, .bold)).foregroundStyle(Ink.fg) }
        }
        ToolbarItem(placement: .primaryAction) { DeskWays() }
      }
    } else {
      // (the iPhone: the system's bar, calm: the place as a centered title that switches the team, then Blitz and one
      // ⋯ menu, SF Symbols in one glass group)
      content.navigationBarTitleDisplayMode(.inline).toolbar {
        ToolbarItem(placement: .principal) { PlacePill(title: true) }
        ToolbarItemGroup(placement: .topBarTrailing) { DeskBar() }
      }
    }
    #endif
  }
}

/**
 * The iPhone Desk's two buttons at the top right, in the system's bar: Blitz (the first open card; its count is said,
 * not drawn) and one ⋯ menu with the duck for all (the core's `desk.duck`, asked once) and the note.
 */
struct DeskBar: View {
  @EnvironmentObject var shell: AppShell
  @State private var asking = false
  var body: some View {
    let desk = shell.view.desk
    if let blitz = desk.blitz, let first = desk.rows.first {
      Button { shell.focus(.card(cardId: first.id)) } label: { Image(systemName: "bolt") }
        .accessibilityLabel(blitz.en).accessibilityIdentifier("desk-blitz")
    }
    Menu {
      if let duck = desk.duck {
        // (the yellow duck and its short name; its question follows at once and says the rest)
        Button { asking = true } label: { Label { Text(duck.short.en) } icon: { PenImage.duck } }
          .accessibilityLabel(duck.tip.en).accessibilityHint(duck.what.en).accessibilityIdentifier("desk-duck")
      }
      Button("Note", systemImage: "square.and.pencil") { shell.focus(.notes) }.accessibilityIdentifier("note-open")
    } label: { Image(systemName: "ellipsis") }
    .accessibilityLabel("More").accessibilityIdentifier("desk-more")
    .alert(desk.duck?.ask.en ?? "", isPresented: $asking) {
      if let duck = desk.duck {
        Button(duck.no.en, role: .cancel) {}
        Button(duck.yes.en) { shell.act(["do": "batch", "way": "sweep", "card_ids": duck.cardIds]) }
          .keyboardShortcut(.defaultAction).accessibilityHint(duck.what.en)
      }
    } message: { Text(desk.duck?.what.en ?? "") }
  }
}

/**
 * The Desk's two ways at the top right (beside the place pill): the duck for all and Blitz with its count, as two
 * small round glass buttons. The duck asks once, in the core's words (`desk.duck`), "Cancel" as the quiet way; Blitz
 * opens the first open card, whose page walks on to the next.
 */
struct DeskWays: View {
  @EnvironmentObject var shell: AppShell
  @State private var asking = false
  var body: some View {
    let desk = shell.view.desk
    HStack(spacing: 10) {
      if let duck = desk.duck {
        Button { asking = true } label: {
          PenMark("sketch:duck", color: Ink.fg, duck: false).frame(width: 24, height: 20).pillCircle()
        }
        .buttonStyle(.plain)
        .accessibilityLabel(duck.tip.en).accessibilityHint(duck.what.en).accessibilityIdentifier("desk-duck")
        .alert(duck.ask.en, isPresented: $asking) {
          Button(duck.no.en, role: .cancel) {}
          Button(duck.yes.en) { shell.act(["do": "batch", "way": "sweep", "card_ids": duck.cardIds]) }
            .keyboardShortcut(.defaultAction).accessibilityHint(duck.what.en)
        } message: { Text(duck.what.en) }
      }
      if let blitz = desk.blitz, let first = desk.rows.first {
        Button { shell.focus(.card(cardId: first.id)) } label: {
          PenMark("desk:BOLT").frame(width: 20, height: 20).pillCircle()
            .overlay(alignment: .topTrailing) {
              Text(blitz.args["n"] ?? "").font(Face.text(10, .bold)).foregroundStyle(Ink.bg).monospacedDigit()
                .padding(.horizontal, 4).frame(minWidth: 17, minHeight: 17).background(Capsule().fill(Ink.fg)).offset(x: 3, y: -2)
            }
        }
        .buttonStyle(.plain)
        .accessibilityLabel(blitz.en).accessibilityIdentifier("desk-blitz")
      }
    }
  }
}

/** A new account's Desk: invite the first agent. */
struct FirstAgent: View {
  @EnvironmentObject var shell: AppShell
  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      Sketch("heads", color: Ink.fg).frame(width: 64, height: 64)
      Text("Invite your first agent").font(Face.display(28, .heavy))
      Text("You get one command for any computer with Claude Code: run it in the project folder, then start Claude Code there with plain claude. Its questions land here.")
        .font(Face.text(16)).foregroundStyle(Ink.muted).fixedSize(horizontal: false, vertical: true)
      Button { shell.account(["step": "approve_computer", "label": ""]) } label: {
        HStack { PenMark("ui:PLUS", color: Ink.accentFg).frame(width: 20, height: 20); Text("Approve a computer").font(Face.text(17, .semibold)) }
          .foregroundStyle(Ink.accentFg).padding(.horizontal, 18).padding(.vertical, 12).background(Capsule().fill(Ink.accent))
      }.buttonStyle(.plain)
      AccountInvite()
    }.padding(.top, 30)
  }
}

/** The slip above the questions: the sessions that are cut off, each with the step in its terminal. */
struct CutSlip: View {
  @EnvironmentObject var shell: AppShell
  let slips: [CoreView.SlipView]
  var body: some View {
    VStack(alignment: .leading, spacing: 10) {
      ForEach(slips, id: \.sessionId) { slip in
        Button { shell.focus(.session(sessionId: slip.sessionId)) } label: {
          VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .top, spacing: 8) {
              Sketch("ear-off", color: Ink.urgHigh).frame(width: 20, height: 20)
              Text(slip.text.en).font(Face.text(15)).foregroundStyle(Ink.fg).multilineTextAlignment(.leading)
            }
            HStack(spacing: 6) {
              Text(slip.fix.say.en).font(Face.text(13)).foregroundStyle(Ink.muted)
              if !slip.fix.code.isEmpty { CodeChip(text: slip.fix.code) }
            }.padding(.leading, 28)
          }
        }.buttonStyle(.plain)
      }
    }
    .padding(14)
    .frame(maxWidth: .infinity, alignment: .leading)
    .background(RoundedRectangle(cornerRadius: 14).fill(Ink.urgHighSoft))
  }
}
struct CodeChip: View {
  let text: String
  @State private var copied = false
  var body: some View {
    Button { copyText(text); copied = true } label: {
      Text(copied ? "Copied" : text).font(Face.mono(13, .medium)).foregroundStyle(Ink.fg).padding(.horizontal, 8).padding(.vertical, 4)
        .background(RoundedRectangle(cornerRadius: 6).fill(Ink.sunken))
    }.buttonStyle(.plain)
  }
}

/** A question on the Desk: its title with the asking session's drawing at the end, and its answer tiles. */
struct DeskRow: View {
  @EnvironmentObject var shell: AppShell
  let row: CoreView.DeskRowView
  var body: some View {
    VStack(alignment: .leading, spacing: 10) {
      HStack(alignment: .top, spacing: 8) {
        Button { shell.focus(.card(cardId: row.id)) } label: {
          HStack(alignment: .firstTextBaseline, spacing: 6) {
            if row.sign == "block" { PenMark("hand", color: Ink.surface, blocked: true).frame(width: 24, height: 24).alignmentGuide(.firstTextBaseline) { $0[.bottom] - 4 } }
            else if row.sign == "knock" { Sketch("knock", color: Ink.urgHigh).frame(width: 20, height: 20).alignmentGuide(.firstTextBaseline) { $0[.bottom] - 3 } }
            if row.starred { StarMark().frame(width: 18, height: 18).alignmentGuide(.firstTextBaseline) { $0[.bottom] - 3 }.accessibilityLabel("Starred") }
            Text(row.title.isEmpty ? " " : row.title).font(Face.display(20, .bold)).foregroundStyle(row.loading ? Ink.muted : Ink.fg).lineLimit(3).multilineTextAlignment(.leading)
          }
          .frame(maxWidth: .infinity, alignment: .leading)
          .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityHint(row.signSays?.en ?? "")
        .accessibilityIdentifier("desk-card-" + row.id)
        Group { if let from = row.from { AgentMark(from: from, size: 22).accessibilityLabel(from.name) } }.frame(width: 26, height: 26)
      }
      ForEach(Array(row.notes.enumerated()), id: \.offset) { Text($0.element.en).font(Face.text(13)).foregroundStyle(Ink.muted) }
      if row.newer { UnsupportedLine() } else { Tiles(row: row) }
    }
    .cardBox()
    .contextMenu { CardWays(id: row.id, ways: row.ways) }
  }
}

/** A calm stand-in for something only a newer Trommi can show. */
struct UnsupportedLine: View {
  var body: some View {
    HStack(spacing: 8) {
      Sketch("wake", color: Ink.muted).frame(width: 16, height: 16)
      Text("This needs a newer version of Trommi. Update to see it.").font(Face.text(14)).foregroundStyle(Ink.muted)
    }
    .padding(10).frame(maxWidth: .infinity, alignment: .leading)
    .background(RoundedRectangle(cornerRadius: 10).strokeBorder(Ink.lineStrong, style: StrokeStyle(lineWidth: 1, dash: [4, 3])))
  }
}

/** The answers of a row, as the core lays them out (`row.tiles`). */
struct Tiles: View {
  @EnvironmentObject var shell: AppShell
  let row: CoreView.DeskRowView
  var body: some View {
    let tiles = row.tiles
    let cols = [GridItem(.flexible(), spacing: 8), GridItem(.flexible(), spacing: 8)]
    switch tiles.kind {
    case "read":
      LazyVGrid(columns: cols, spacing: 8) {
        tile(lead: false, label: nil, drawing: "what") { shell.act(["do": "hand_back", "card_id": row.id]) }.accessibilityLabel("What??")
        tile(lead: true, label: "Got it", drawing: "tick") { shell.act(["do": "read", "card_id": row.id]) }
      }
    case "pair":
      LazyVGrid(columns: cols, spacing: 8) {
        ForEach(tiles.tiles, id: \.key) { t in
          tile(lead: t.lead, label: t.label.isEmpty ? nil : t.label, drawing: t.thumbs ? t.thumb : nil, advised: t.advised, final: t.settles, small: t.small) { answer(t.key) }
            .accessibilityLabel(t.says.en)
        }
      }
    case "advised":
      LazyVGrid(columns: cols, spacing: 8) {
        Button { shell.focus(.card(cardId: row.id)) } label: {
          HStack(spacing: 6) { Text("+\(tiles.others)").font(Face.display(20, .heavy)); Text("other ways").font(Face.text(15, .medium)) }
            .foregroundStyle(Ink.fg).frame(maxWidth: .infinity, minHeight: 58)
        }
        .buttonStyle(TileStyle(lead: false, urgency: row.urgency))
        .accessibilityLabel(tiles.says?.en ?? "Open the card")
        ForEach(tiles.tiles, id: \.key) { t in
          tile(lead: true, label: t.label, drawing: nil, advised: true, final: t.settles, small: t.small) { answer(t.key) }.accessibilityLabel(t.says.en)
        }
      }
    case "choose":
      Button { shell.focus(.card(cardId: row.id)) } label: {
        HStack(spacing: 10) { Sketch("choose", color: Ink.surface).frame(width: 22, height: 22); Text("Choose").font(Face.text(17, .semibold)) }
          .frame(maxWidth: .infinity, minHeight: 58)
      }
      .buttonStyle(TileStyle(lead: true, urgency: row.urgency))
      .accessibilityLabel(tiles.says?.en ?? "Choose")
    default: EmptyView()
    }
  }
  private func answer(_ key: String) {
    if row.kind == "request" { shell.act(["do": "verdict", "request_id": row.id, "allow": key == "allow"]) }
    else { shell.act(["do": "answer", "card_id": row.id, "choices": [key]]) }
  }
  private func tile(lead: Bool, label: String?, drawing: String?, advised: Bool = false, final: Bool = false, small: Bool = false, action: @escaping () -> Void) -> some View {
    Button(action: action) {
      VStack(spacing: 4) {
        if let d = drawing {
          if d == "what" { PenMark("sketch:what", color: lead ? Ink.accentFg : Ink.fg, width: 2.2).frame(width: 64, height: 22) }
          else { Sketch(d, color: lead ? Ink.accentFg : Ink.accent).frame(width: 24, height: 24) }
        }
        if let l = label {
          Text(l).font(Face.text(small ? 13 : 15, .semibold)).multilineTextAlignment(.center).lineLimit(3).minimumScaleFactor(0.85)
            .padding(.horizontal, 6)
            .background(alignment: .bottom) { if advised && !lead { PenUnderline().stroke(Ink.urgHigh.opacity(0.7), lineWidth: 2).frame(height: 6).offset(y: 4) } }
        }
      }
      .frame(maxWidth: .infinity, minHeight: 58)
      .overlay(alignment: .topTrailing) { if final { Sketch("tick", color: lead ? Ink.surface : Ink.accent, width: 2.2).frame(width: 13, height: 13).padding(6) } }
    }
    .buttonStyle(TileStyle(lead: lead, urgency: row.urgency))
  }
}

/** The ways out of a row, held: those the core offers for it. */
struct CardWays: View {
  @EnvironmentObject var shell: AppShell
  let id: String; let ways: [String]
  var body: some View {
    Button { shell.focus(.card(cardId: id)) } label: { Label("Open", systemImage: "doc.text") }
    ForEach(ways, id: \.self) { way in
      switch way {
      case "later": Button { shell.act(["do": "snooze", "card_id": id]) } label: { Label("Later", systemImage: "zzz") }
      case "reverse": Button { shell.act(["do": "hand_back", "card_id": id]) } label: { Label("What??", systemImage: "questionmark.bubble") }
      case "shred": Button(role: .destructive) { shell.act(["do": "shred", "card_id": id]) } label: { Label("Shred", systemImage: "trash") }
      default: EmptyView()
      }
    }
  }
}

// ---- under the stack ---------------------------------------------------------------------------------------------

/** The section Off your mind on the Desk: what runs at its head (each card with its agent and status line, no stack
 *  floating over the Desk), then the first rows put off or closed, fading, and how many there are; it opens the
 *  whole list. */
struct OffHint: View {
  @EnvironmentObject var shell: AppShell
  let off: CoreView.OffHintView?
  /** His starred cards: at the head of Off your mind, above the running ones (one design with the web's Desk). */
  var starred: CoreView.DockView? = nil
  var body: some View {
    VStack(alignment: .leading, spacing: 0) {
      EndDivider(title: "Off your mind", more: { shell.focus(.off(query: "")) }).padding(.bottom, 4)
      if let starred, !starred.rows.isEmpty {
        VStack(alignment: .leading, spacing: 2) {
          ForEach(starred.rows, id: \.id) { r in headRow(r, star: true) }
        }
        .padding(.bottom, 6)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(starred.says.en)
        .accessibilityIdentifier("off-starred")
      }
      if let off, !off.running.isEmpty {
        VStack(alignment: .leading, spacing: 2) {
          ForEach(off.running, id: \.id) { r in headRow(r, star: false) }
          if let more = off.runningMore {
            Button { shell.focus(.off(query: "")) } label: {
              Text(more.en).font(Face.text(13, .semibold)).foregroundStyle(Ink.muted).frame(minHeight: 32).contentShape(Rectangle())
            }.buttonStyle(.plain).padding(.leading, 36)
          }
        }
        .padding(.bottom, off.rows.isEmpty ? 0 : 10)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("off-running")
      }
      if let off, !off.rows.isEmpty || off.putOff > 0 || off.closed > 0 {
      Button { shell.focus(.off(query: "")) } label: {
        VStack(alignment: .leading, spacing: 4) {
          ForEach(Array(off.rows.enumerated()), id: \.element.id) { i, r in
            HStack(spacing: 8) {
              OffMark(mark: r.mark).frame(width: 12, height: 12)
              Text(r.title).font(Face.text(13, .medium)).foregroundStyle(Ink.muted).lineLimit(1).strikethrough(r.mark == "shred")
            }.opacity(i == 0 ? 1 : i == 1 ? 0.7 : 0.4)
          }
          Text(off.says.en).font(Face.text(13, .semibold)).foregroundStyle(Ink.fg).underline(true, color: Ink.accent).padding(.top, 3)
        }
        .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading).contentShape(Rectangle())
      }
      .buttonStyle(.plain)
      .accessibilityLabel("Off your mind: \(off.says.en)")
      }
    }.padding(.top, 8)
  }
  /** A starred card or one its agent works on: the agent's drawing (in its turning ring while it runs), the title
   *  (a star before a starred one's), who and what (its status line). */
  private func headRow(_ r: CoreView.DockRowView, star: Bool) -> some View {
    Button { shell.focus(.card(cardId: r.id)) } label: {
      HStack(spacing: 10) {
        AgentMark(from: r.from, size: 18)
          .frame(width: 26, height: 26)
          .overlay { if r.running { WorkingRing(working: true, color: Ink.accent).frame(width: 28, height: 28) } }
        VStack(alignment: .leading, spacing: 1) {
          HStack(spacing: 5) {
            if star { StarMark().frame(width: 13, height: 13) }
            Text(r.title).font(Face.text(14, .semibold)).foregroundStyle(Ink.fg).lineLimit(1)
          }
          Text(r.line.en).font(Face.text(12)).foregroundStyle(Ink.muted).lineLimit(1)
        }
        Spacer(minLength: 0)
      }
      .frame(minHeight: 44).contentShape(Rectangle())
    }
    .buttonStyle(.plain)
    .accessibilityLabel("\(r.title), \(r.line.en)")
    .accessibilityIdentifier((star ? "off-starred-" : "off-running-") + r.id)
  }
}
/**
 * The iPhone Desk's tiles under the questions (`desk.tiles`, the core's: order, words, what each opens, which share a
 * row on a phone): Liquid Glass, SF Symbols, black. A tile that holds nothing yet is quiet, its line only.
 */
struct DeskTiles: View {
  @EnvironmentObject var shell: AppShell
  let tiles: [CoreView.DeskTileView]
  var body: some View {
    let rows = Dictionary(grouping: tiles, by: \.phoneRow).sorted { $0.key < $1.key }
    VStack(spacing: 12) {
      ForEach(rows, id: \.key) { row in
        HStack(alignment: .top, spacing: 12) { ForEach(row.value, id: \.key) { DeskTile(tile: $0) } }
      }
    }
  }
}
struct DeskTile: View {
  @EnvironmentObject var shell: AppShell
  let tile: CoreView.DeskTileView
  private static let symbols = ["work": "tray.full", "artifacts": "photo.on.rectangle", "board": "scribble.variable", "knowledge": "book.closed"]
  var body: some View {
    if tile.key == "work", !tile.empty, !runningRows.isEmpty { work } else { plain }
  }
  /** Working / Done with its running cards: its name and numbers in short, then a line each, the agent's face beside the
   *  title without its status word (`short`). */
  private var work: some View {
    Button { shell.focus(tile.opens) } label: {
      VStack(alignment: .leading, spacing: 10) {
        HStack(spacing: 8) {
          Text(tile.title.en).font(.subheadline.weight(.semibold)).lineLimit(1)
          Spacer(minLength: 0)
          if let brief = tile.brief { Text(brief.en).font(.subheadline).foregroundStyle(.secondary).monospacedDigit() }
          Image(systemName: "chevron.right").font(.caption.weight(.semibold)).foregroundStyle(.tertiary)
        }
        ForEach(runningRows.prefix(3), id: \.id) { r in
          HStack(spacing: 10) {
            AgentMark(from: r.from, size: 18).frame(width: 22, height: 22)
            Text(r.short.isEmpty ? r.title : r.short).font(.subheadline).lineLimit(1)
          }
        }
      }
      .foregroundStyle(.primary)
      .padding(14).frame(maxWidth: .infinity, alignment: .topLeading)
      .glass(RoundedRectangle(cornerRadius: 22, style: .continuous), interactive: true)
    }
    .buttonStyle(.plain)
    .accessibilityElement(children: .combine)
    .accessibilityIdentifier("desk-tile-" + tile.key)
  }
  /** The running cards it names, by id; one no longer running is left out. */
  private var runningRows: [CoreView.DockRowView] { tile.items.compactMap { id in shell.view.desk.off?.running.first { $0.id == id } } }
  private var plain: some View {
    Button { shell.focus(tile.opens) } label: {
      VStack(alignment: .leading, spacing: 6) {
        HStack(spacing: 8) {
          Image(systemName: Self.symbols[tile.key] ?? "square").font(.body)
          Text(tile.title.en).font(.subheadline.weight(.semibold)).lineLimit(1)
          Spacer(minLength: 0)
          Image(systemName: "chevron.right").font(.caption.weight(.semibold)).foregroundStyle(.tertiary)
        }.foregroundStyle(.primary)
        Text(tile.line.en).font(.footnote).foregroundStyle(.secondary).lineLimit(1)
        if !tile.empty {
          ForEach(Array(lines.prefix(3).enumerated()), id: \.offset) { _, line in
            HStack(spacing: 6) {
              if tile.key == "work" { Circle().fill(Color.primary.opacity(0.6)).frame(width: 5, height: 5) }
              Text(line).font(.footnote).foregroundStyle(.primary).lineLimit(1)
            }
          }
        }
      }
      .padding(14).frame(maxWidth: .infinity, alignment: .topLeading)
      .glass(RoundedRectangle(cornerRadius: 22, style: .continuous), interactive: true)
      .opacity(tile.empty ? 0.7 : 1)
    }
    .buttonStyle(.plain)
    .accessibilityElement(children: .combine)
    .accessibilityIdentifier("desk-tile-" + tile.key)
  }
  /** What it shows small: the running cards' and the newest artifacts' titles by their ids (one that is gone is left
   *  out), the Knowledge entries' names. */
  private var lines: [String] {
    switch tile.key {
    case "work": return tile.items.compactMap { id in shell.view.desk.off?.running.first { $0.id == id }?.title }
    case "artifacts": return tile.items.compactMap { id in shell.view.artifacts?.first { $0.id == id }?.title }
    default: return tile.names
    }
  }
}

/** The iPhone's calm Desk ends in one quiet row: what runs, is put off and is done (`off.line`), opening the list. */
struct OffLine: View {
  @EnvironmentObject var shell: AppShell
  let off: CoreView.OffHintView
  var body: some View {
    Button { shell.focus(off.opens) } label: {
      HStack(spacing: 10) {
        Image(systemName: "tray").font(.body).foregroundStyle(.secondary)
        Text(off.line.en).font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
        Spacer(minLength: 4)
        Image(systemName: "chevron.right").font(.footnote.weight(.semibold)).foregroundStyle(.tertiary)
      }
      .padding(.horizontal, 16).frame(minHeight: 48).contentShape(Rectangle())
      .glass(RoundedRectangle(cornerRadius: 24, style: .continuous), interactive: true)
    }
    .buttonStyle(.plain)
    .accessibilityLabel("Off your mind: \(off.line.en)")
    .accessibilityIdentifier("off-line")
  }
}
/** The team's goals under the greeting: a quiet line or a few; a tap writes in place; one toggle folds them. */
/** The chip under a team's goals that opens its page Knowledge (`DeskView.knowledge`): "Knowledge" and how many. */
struct KnowledgeChip: View {
  @EnvironmentObject var shell: AppShell
  let chip: CoreView.KnowledgeChipView
  var body: some View {
    Button { shell.focus(.knowledge(query: "", filter: "", open: "")) } label: {
      HStack(spacing: 6) {
        Sketch("book", color: Ink.muted).frame(width: 15, height: 15)
        Text(chip.label.en).font(Face.text(13, .semibold)).foregroundStyle(Ink.muted)
        Text("\(chip.count)").font(Face.text(13, .bold)).foregroundStyle(Ink.fg).monospacedDigit()
      }
      .padding(.horizontal, 10).frame(minHeight: 30)
      .background(Capsule().stroke(Ink.line, lineWidth: 1))
      .contentShape(Capsule())
    }
    .buttonStyle(.plain)
    .padding(.top, 6)
    .accessibilityLabel(chip.says.en)
    .accessibilityIdentifier("desk-knowledge")
  }
}

struct GoalsDrawing: View {
  @EnvironmentObject var shell: AppShell
  let goals: CoreView.GoalsView
  @State private var draft: String? = nil
  @FocusState private var focused: Bool
  private let face = Face.text(16, .medium)
  var body: some View {
    Group {
      if let d = draft {
        TextEditor(text: Binding(get: { d }, set: { draft = $0 }))
          .font(face).foregroundStyle(Ink.fg).lineSpacing(3)
          .scrollContentBackground(.hidden).scrollDisabled(true)
          .focused($focused)
          .fixedSize(horizontal: false, vertical: true)
          .frame(minHeight: 44)
          .background(PenBox(r: 8).fill(Ink.sunken))
          .overlay(PenBox(r: 8).stroke(Ink.lineStrong, lineWidth: 1.5))
          .padding(.horizontal, -5)
          .accessibilityLabel("This team’s goals")
          .onChange(of: focused) { _, on in if !on { done() } }
          .onDisappear { done() }
      } else {
        let shown = (goals.folded ? Array(goals.lines.prefix(1)) : goals.lines).joined(separator: "\n")
        HStack(alignment: .top, spacing: 4) {
          Button { draft = goals.lines.joined(separator: "\n"); focused = true } label: {
            Text(goals.lines.isEmpty ? "Goals…" : shown)
              .font(face).italic(goals.lines.isEmpty).foregroundStyle(goals.lines.isEmpty ? Ink.faint : Ink.muted).lineSpacing(3)
              .multilineTextAlignment(.leading).frame(maxWidth: .infinity, alignment: .leading)
              .padding(.vertical, 8).contentShape(Rectangle())
          }
          .buttonStyle(.plain)
          .accessibilityLabel(goals.lines.isEmpty ? "This team’s goals: none yet" : "This team’s goals: \(shown)")
          if let toggle = goals.toggle {
            Button { withAnimation(.easeOut(duration: 0.18)) { shell.act(["do": "fold_goals", "team_id": goals.teamId, "folded": !goals.folded]) } } label: {
              Image(systemName: "chevron.down").font(.system(size: 13, weight: .semibold))
                .rotationEffect(.degrees(goals.folded ? -90 : 0))
                .foregroundStyle(goals.folded ? Ink.muted : Ink.faint)
                .frame(width: 44, height: 40).contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(toggle.en)
          }
        }
      }
    }
    .frame(maxWidth: 600, alignment: .leading)
  }
  private func done() {
    guard let d = draft else { return }
    draft = nil; focused = false
    if d != goals.lines.joined(separator: "\n") { shell.act(["do": "set_goal", "team_id": goals.teamId, "text": d]) }
  }
}

// ---- the passing toast ---------------------------------------------------------------------------------------------

/**
 * The passing word: ONE glass capsule at the bottom, centred, just above whatever stands there (the tab pill, a
 * composer). The words at the left; the core's action (Undo, Retry) as the round button at the right, its ring running
 * down for the toast's time; its way to a page as a link. A tap on the words or a swipe down lets it go.
 */
struct ToastHost: View {
  @EnvironmentObject var shell: AppShell
  @ObservedObject private var chrome = BottomChrome.shared
  @State private var drag: CGFloat = 0
  @State private var progress: CGFloat = 1
  var body: some View {
    GeometryReader { g in
      let floor = g.frame(in: .global).maxY
      let lift = max(0, floor - (chrome.top ?? floor))
      VStack(spacing: 0) {
        Spacer(minLength: 0)
        if let t = shell.view.toasts.last {
          capsule(t)
            .task(id: t.id) {
              let ms = t.ms ?? (t.action != nil ? 5000 : 3500)
              progress = 1
              withAnimation(.linear(duration: Double(ms) / 1000)) { progress = 0 }
              try? await Task.sleep(nanoseconds: ms * 1_000_000)
              if !Task.isCancelled { dismiss(t) }
            }
            .transition(.move(edge: .bottom).combined(with: .opacity))
        }
      }
      .frame(maxWidth: .infinity)
      .padding(.bottom, lift + 8)
      .animation(.spring(response: 0.35, dampingFraction: 0.85), value: shell.view.toasts.last?.id)
      .animation(.snappy, value: lift)
    }
    .ignoresSafeArea(.keyboard)
  }
  private func dismiss(_ t: CoreView.Toast) { shell.act(["do": "dismiss", "toast_id": t.id]) }
  private func capsule(_ t: CoreView.Toast) -> some View {
    HStack(spacing: 10) {
      HStack(spacing: 8) {
        if t.alert { Image(systemName: "exclamationmark.circle").foregroundStyle(Ink.urgCritical) }
        Text("\(Text(t.text.en).font(Face.text(15, .semibold)).foregroundColor(t.alert ? Ink.urgCritical : Ink.fg))\(Text(t.line.map { "  " + $0.en } ?? "").font(Face.text(14)).foregroundColor(Ink.muted))")
          .lineLimit(1).truncationMode(.tail)
      }
      .frame(minHeight: 44)
      .contentShape(Rectangle())
      .onTapGesture { dismiss(t) }
      .accessibilityElement(children: .combine)
      if let go = t.go {
        Button(go.label.en) { dismiss(t); shell.focus(go.screen) }.font(Face.text(14, .semibold)).foregroundStyle(Ink.accent)
      }
      if let action = t.action {
        Spacer(minLength: 2)
        Button {
          dismiss(t)
          if let data = try? JSONEncoder().encode(action), let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] { shell.act(json) }
        } label: {
          ZStack {
            Circle().fill(Ink.fg.opacity(0.08))
            Circle().trim(from: 0, to: progress).stroke(Ink.fg, style: StrokeStyle(lineWidth: 2, lineCap: .round)).rotationEffect(.degrees(-90)).padding(1)
            Image(systemName: "arrow.uturn.backward").font(.system(size: 14, weight: .bold)).foregroundStyle(Ink.fg)
          }
          .frame(width: 34, height: 34).frame(width: 44, height: 44).contentShape(Circle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel("\(t.label?.en ?? "Undo"): \(t.text.en)")
      }
    }
    .padding(.leading, 18).padding(.trailing, t.action != nil ? 3 : 18)
    .glass(Capsule())
    .frame(maxWidth: 380)
    .padding(.horizontal, 16)
    .offset(y: max(0, drag))
    .gesture(DragGesture(minimumDistance: 6).onChanged { drag = $0.translation.height }.onEnded { v in
      withAnimation(.snappy) { if v.translation.height > 24 { dismiss(t) }; drag = 0 }
    })
  }
}
