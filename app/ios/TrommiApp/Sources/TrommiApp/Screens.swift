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
    width == .regular || !AppShell.phone
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
        // iPhone: one navigation stack, the teams as folders at its root (PhoneFolders.swift); no tab bar. The note
        // lies over everything as a yellow page.
        #if os(iOS)
        PhoneFrame().overlay(alignment: .top) { TopLines() }
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
      let screen = path.last ?? (shell.noteOpen ? .notes : .group)
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
    case .group: DeskScreen()
    case .card(let id): if let card = shell.view.cards[id] { CardScreen(card: card).id(id) } else { Loading() }
    case .session(let id): if let page = shell.view.sessions[id] { SessionScreen(page: page).id(id) } else { Loading() }
    case .notes: NotesScreen()
    case .off: OffScreen()
    case .settings(let page): SettingsScreen(page: page)
    case .artifacts: ArtifactsScreen()
    case .knowledge: KnowledgeScreen()
    case .wakeups: WakeupsScreen()
    case .scribble: ScribbleScreen()
    }
  }
}

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
      Section("Teams") {
        if let none = side.noTeam { HomeItem(home: none) { shell.pick(.desk); shell.focus(.group, team: none.id) } }
        ForEach(side.teams, id: \.id) { teamItem($0) }
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
    Button { shell.pick(.desk); shell.focus(.group, team: team.id) } label: {
      Label { Text(team.cards > 0 ? "\(team.name.en) · \(team.cards)" : team.name.en) } icon: { if team.selected { Image(systemName: "checkmark") } else { PenImage.desk(waiting: team.waiting > 0) } }
    }
    .accessibilityLabel(team.cards > 0 ? "\(team.name.en), \(team.cards) open" : team.name.en)
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
        if let none = side.noTeam { HomeItem(home: none) { shell.focus(.group, team: none.id) } }
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
          ForEach(side.root, id: \.id) { row in SessionRow(row: row) }
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
        Button { shell.focus(.wakeups) } label: { Label("Wakeups", systemImage: "alarm") }
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
    Button { shell.focus(.group, team: team.id) } label: {
      Label { Text(team.cards > 0 ? "\(team.name.en) · \(team.cards)" : team.name.en) } icon: { if team.selected { Image(systemName: "checkmark") } else { PenImage.desk(waiting: team.waiting > 0) } }
    }
    .accessibilityLabel(team.cards > 0 ? "\(team.name.en), \(team.cards) open" : team.name.en)
  }
}

/** The chats on no team in the team menu, above the teams (`sidebar.no_team`, while there are any): their group page. */
struct HomeItem: View {
  let home: CoreView.TeamTab
  let pick: () -> Void
  var body: some View {
    Button(action: pick) {
      Label { Text(home.cards > 0 ? "\(home.name.en) · \(home.cards)" : home.name.en) } icon: { if home.selected { Image(systemName: "checkmark") } else { PenImage.desk(waiting: home.waiting > 0) } }
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
        SessionBadge(open: row.cards, stopped: row.blocked, says: row.blocked ? row.says?.en : nil)
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

// ---- the group page ------------------------------------------------------------------------------------------------

/** The group page (`view.group`): the group's name, a chip per chat, the questions, then what is only to read. The
 *  iPhone's look is being redrawn on its own; this draws the core's GroupView with the components there are. */
struct DeskScreen: View {
  @EnvironmentObject var shell: AppShell
  @Environment(\.horizontalSizeClass) private var hSize
  @State private var speedReport: String?
  var body: some View {
    let desk = shell.view.group
    // (the iPhone's page is calm: no name (the bar says it) nor goals)
    let calm = hSize == .compact
    ScrollView {
      LazyVStack(alignment: .leading, spacing: calm ? 16 : 12) {
        if desk.mode == "invite" { FirstAgent() }
        else if calm { Color.clear.frame(height: 4) }
        else if let head = desk.head {
          VStack(alignment: .leading, spacing: 0) {
            Greeting(text: head.name.en)
            if let goals = desk.goals { GoalsDrawing(goals: goals).id(goals.teamId) }
          }.padding(.top, 18).padding(.bottom, 6)
        }
        if !desk.chips.isEmpty { GroupChips(chips: desk.chips) }
        if let reading = desk.reading { Text(reading.en).font(Face.text(14)).foregroundStyle(Ink.muted).accessibilityIdentifier("reading-history") }
        if !desk.slip.isEmpty { CutSlip(slips: desk.slip) }
        if let says = desk.questionsSays { GroupPart(says: says) }
        ForEach(desk.questions, id: \.id) { row in DeskRow(row: row) }
        if let says = desk.readsSays { GroupPart(says: says) }
        ForEach(desk.reads, id: \.id) { row in DeskRow(row: row) }
        if calm, let empty = desk.empty {
          ContentUnavailableView(empty.en, systemImage: "checkmark.circle").frame(maxWidth: .infinity).padding(.vertical, 40)
        } else if let empty = desk.empty {
          VStack(spacing: 10) {
            Sketch("desk", color: Ink.faint).frame(width: 64, height: 64)
            Text(empty.en).font(Face.text(15)).foregroundStyle(Ink.muted).multilineTextAlignment(.center)
          }.frame(maxWidth: .infinity).padding(.vertical, 28)
        }
      }
      .padding(.horizontal, 16).padding(.bottom, 120)
      .frame(maxWidth: 760).frame(maxWidth: .infinity)
    }
    .accessibilityIdentifier("desk-content")
    .accessibilityValue(speedReport ?? String(desk.stack.count))
    .onAppear { speedReport = NativeDemo.ready(rows: desk.stack.count) }
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
      content.navigationBarTitleDisplayMode(.inline).toolbar { MainBar.items(title: AnyView(PlacePill(title: true)), desk: true) }
    }
    #endif
  }
}

/**
 * The top of the iPhone's main screens, as the wide sidebar's top and foot: the person's picture at the left (the
 * foot's menu: notifications, the pages, Settings, the demo), search and + at the right (+: an agent, a team, the
 * note; on the Desk also the duck for all and Blitz). The place or the page's name in the middle.
 */
enum MainBar {
  @ToolbarContentBuilder static func items(title: AnyView, desk: Bool) -> some ToolbarContent {
    ToolbarItem(placement: .topBarLeading) { MePicture() }
    ToolbarItem(placement: .principal) { title }
    ToolbarItemGroup(placement: .topBarTrailing) {
      SearchButton()
      PlusMenu(desk: desk)
    }
  }
}
struct SearchButton: View {
  @EnvironmentObject var shell: AppShell
  var body: some View {
    Button { shell.searchOpen = true } label: { Image(systemName: "magnifyingglass") }
      .accessibilityLabel("Search").accessibilityIdentifier("side-search")
  }
}
/** The person's picture: the core's initial (`sidebar.me`), a plain figure until the account is read; its menu. */
struct MePicture: View {
  @EnvironmentObject var shell: AppShell
  @AppStorage("trommi-theme") private var theme = "system"
  var body: some View {
    let side = shell.view.sidebar
    Menu {
      if shell.isDemo { Section("Demo") { Button { shell.leaveDemo() } label: { Label("Leave Demo", systemImage: "rectangle.portrait.and.arrow.right") } } }
      if let account = shell.view.settings.account { Text(account) }
      Section {
        if AppShell.phone {
          Button { shell.teamPage = nil; shell.path = [.group]; shell.show(.group) } label: { Label("Group", systemImage: "tray") }
            .accessibilityIdentifier("phone-open-desk")
        }
        Button { go(.scribble(boardId: side.team)) } label: { Label("Scribble", systemImage: "scribble.variable") }
        Button { go(.knowledge(query: "", filter: "", open: "")) } label: { Label("Knowledge", systemImage: "book") }
        Button { go(.wakeups) } label: { Label("Wakeups", systemImage: "alarm") }
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
      Group {
        if side.me.isEmpty { Image(systemName: "person.crop.circle.fill").font(.system(size: 28)).foregroundStyle(.secondary) }
        else { Text(side.me).font(.subheadline.weight(.semibold)).foregroundStyle(.white).frame(width: 30, height: 30).background(Circle().fill(Ink.accent)) }
      }.frame(width: 32, height: 32)
    }
    .accessibilityLabel("You, Menu").accessibilityIdentifier("side-avatar")
    .task { if shell.view.settings.account == nil && !shell.isDemo { shell.account(["step": "read_account"]) } }
  }
  private func go(_ screen: CoreView.Screen) { shell.closeNote(); shell.focus(screen) }
}
/** + at the top right: a new chat with a chat agent, an agent, a team, the note; on the Desk the duck for all (asked once) and Blitz. */
struct PlusMenu: View {
  @EnvironmentObject var shell: AppShell
  let desk: Bool
  @State private var asking = false
  var body: some View {
    let d = shell.view.group
    Menu {
      // (a chat agent such as Hermes starts a new chat when asked: `SidebarView.newChat`)
      if !shell.view.sidebar.newChat.isEmpty {
        Section("New chat with") {
          ForEach(shell.view.sidebar.newChat, id: \.sessionId) { n in
            Menu {
              ForEach(Array(shell.view.sidebar.places.enumerated()), id: \.offset) { _, place in
                Button(place.name.en) { shell.act(["do": "new_chat", "session_id": n.sessionId, "team_id": place.teamId as Any? ?? NSNull()]) }
              }
            } label: { Label(n.name.en, systemImage: "square.and.pencil") }
              .accessibilityIdentifier("new-chat-" + n.slug)
          }
        }
      }
      Button { shell.focus(.settings(page: "agents")) } label: { Label("Add agent", systemImage: "laptopcomputer") }.accessibilityIdentifier("add-agent")
      Button { shell.focus(.settings(page: "teams")) } label: { Label("Add team", systemImage: "person.2") }.accessibilityIdentifier("add-team")
      Button("Note", systemImage: "square.and.pencil") { shell.focus(.notes) }.accessibilityIdentifier("note-open")
      if desk, d.duck != nil || d.blitz != nil {
        Section {
          if let duck = d.duck {
            // (the yellow duck and its short name; its question follows at once and says the rest)
            Button { asking = true } label: { Label { Text(duck.short.en) } icon: { PenImage.duck } }
              .accessibilityLabel(duck.tip.en).accessibilityHint(duck.what.en).accessibilityIdentifier("desk-duck")
          }
          if let blitz = d.blitz {
            Button { shell.blitzAll() } label: { Label("\(blitz.says.en), \(blitz.scope.en)", systemImage: "bolt") }.accessibilityIdentifier("desk-blitz")
          }
        }
      }
    } label: { Image(systemName: "plus") }
    .accessibilityLabel("Add").accessibilityIdentifier("side-plus")
    .alert(d.duck?.ask.en ?? "", isPresented: $asking) {
      if let duck = d.duck {
        Button(duck.no.en, role: .cancel) {}
        Button(duck.yes.en) { shell.act(["do": "batch", "way": "sweep", "card_ids": duck.cardIds]) }
          .keyboardShortcut(.defaultAction).accessibilityHint(duck.what.en)
      }
    } message: { Text(d.duck?.what.en ?? "") }
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
    let desk = shell.view.group
    HStack(spacing: 10) {
      if let head = desk.head, !head.menu.isEmpty { GroupMenu(head: head) }
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
      if let blitz = desk.blitz {
        Button { shell.blitzAll() } label: {
          PenMark("desk:BOLT").frame(width: 20, height: 20).pillCircle()
            .overlay(alignment: .topTrailing) {
              Text(blitz.says.args["n"] ?? "").font(Face.text(10, .bold)).foregroundStyle(Ink.bg).monospacedDigit()
                .padding(.horizontal, 4).frame(minWidth: 17, minHeight: 17).background(Capsule().fill(Ink.fg)).offset(x: 3, y: -2)
            }
        }
        .buttonStyle(.plain)
        .accessibilityLabel("\(blitz.says.en), \(blitz.scope.en)").accessibilityIdentifier("desk-blitz")
      }
    }
  }
}

/** The group's ⋯ menu (`GroupHeadView.menu`): Knowledge, Wakeups, Scribble Board, each with its count; Rename asks for
 *  the new name. */
struct GroupMenu: View {
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
    } label: { Image(systemName: "ellipsis").frame(width: 20, height: 20).pillCircle() }
    .accessibilityLabel(head.menuSays.en).accessibilityIdentifier("group-menu")
    .alert(head.menu.first { $0.key == "rename" }?.label.en ?? "", isPresented: $renaming) {
      TextField(head.name.en, text: $name)
      Button("Cancel", role: .cancel) {}
      Button("Save") { let n = name.trimmingCharacters(in: .whitespaces); if !n.isEmpty, n != head.name.en { shell.act(["do": "rename_team", "team_id": head.teamId, "name": n]) } }
    }
  }
}

/** The chips of the group page (`GroupView.chips`): "All" first, then each chat; a tap shows only that chat's cards,
 *  the chosen one opens its chat. The core orders them; every one is drawn (the row scrolls). */
struct GroupChips: View {
  @EnvironmentObject var shell: AppShell
  let chips: [CoreView.ChipView]
  var body: some View {
    ScrollView(.horizontal, showsIndicators: false) {
      HStack(spacing: 8) {
        ForEach(chips, id: \.sessionId) { chip in
          Button {
            if chip.selected, !chip.sessionId.isEmpty { shell.focus(.session(sessionId: chip.sessionId)) }
            else { shell.chip(chip.sessionId.isEmpty ? nil : chip.sessionId) }
          } label: {
            HStack(spacing: 6) {
              Text(chip.name.en).font(Face.text(14, .semibold)).lineLimit(1)
              if chip.count > 0 { Text("\(chip.count)").font(Face.text(14)).foregroundStyle(chip.selected ? Ink.bg : Ink.muted) }
              if chip.needs { Circle().fill(chip.selected ? Ink.bg : Ink.accent).frame(width: 7, height: 7) }
              if let go = chip.openChat { Text(go.en).font(Face.text(14, .bold)) }
            }
            .foregroundStyle(chip.selected ? Ink.bg : Ink.fg)
            .padding(.horizontal, 12).frame(height: 34)
            .background(Capsule().fill(chip.selected ? Ink.fg : Ink.surface))
            .overlay(Capsule().stroke(Ink.line, lineWidth: chip.selected ? 0 : 1))
          }
          .buttonStyle(.plain)
          .accessibilityLabel(chip.says.en)
        }
      }
    }
    .accessibilityIdentifier("group-chips")
  }
}

/** A part's heading on the group page: "Waits for you", "To read", with how many. */
struct GroupPart: View {
  let says: CoreView.Text
  var body: some View {
    HStack(spacing: 6) {
      Text(says.en.uppercased()).font(Face.text(12, .bold)).foregroundStyle(Ink.muted)
      Text(says.args["n"] ?? "").font(Face.text(12)).foregroundStyle(Ink.faint)
    }.padding(.top, 6)
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

/** The team's goals under the greeting: a quiet line or a few; a tap writes in place; one toggle folds them. */
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
