// Screens.swift: the frame around every board screen and the Desk, as the app drew them before the shared app core:
// on the iPhone the glass pill at the bottom (Chat · Desk · Note as drawings, Note as a full yellow page over the
// list), the place pill (the menu) at the top left; on the iPad and the Mac the sessions as a sidebar column beside
// the stack. The Desk: one big greeting with its last word underlined by the pen, the desk's goals, the open questions
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
      } else {
        // iPhone: Chat · Desk · Note in one glass pill at the bottom. The bar shows on the two lists only: a pushed
        // screen has its own controls at the bottom. Both pages stay alive, so each keeps its scroll position.
        stack(ZStack {
          DeskScreen().opacity(shell.tab == .desk ? 1 : 0).allowsHitTesting(shell.tab == .desk).accessibilityHidden(shell.tab != .desk)
          ChatsScreen().opacity(shell.tab == .chat ? 1 : 0).allowsHitTesting(shell.tab == .chat).accessibilityHidden(shell.tab != .chat)
        }
        .overlay { if shell.noteOpen { NotePage() .transition(.move(edge: .bottom)) } }
        .animation(.snappy, value: shell.noteOpen)
        .safeAreaBar(edge: .bottom, spacing: 0) { if shell.path.isEmpty { NativeTabPill() } })
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
  @ViewBuilder func page(_ screen: CoreView.Screen) -> some View {
    switch screen {
    case .desk: DeskScreen()
    case .card(let id): if let card = shell.view.cards[id] { CardScreen(card: card).id(id) } else { Loading() }
    case .session(let id): if let page = shell.view.sessions[id] { SessionScreen(page: page).id(id) } else { Loading() }
    case .notes: NotesScreen()
    case .off: OffScreen()
    case .settings: SettingsScreen()
    case .artifacts: ArtifactsScreen()
    case .scribble: ScribbleScreen()
    }
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
        .accessibilityLabel("Close the note").accessibilityAddTraits(.isButton)
        .accessibilityAction { shell.closeNote() }
      NotesScreen()
    }
    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
    .background(Ink.noteYellow.ignoresSafeArea())
    .offset(y: drag)
  }
}

/**
 * The iPhone's bar: Chat · Desk · Note in one clear glass pill. The page he is on: full ink on a raised lens; the
 * others a little muted. The Desk's number is the core's (sidebar.badge).
 */
struct NativeTabPill: View {
  @EnvironmentObject var shell: AppShell
  private let wide: CGFloat = 84
  private var lit: Int { shell.noteOpen ? 2 : shell.tab == .chat ? 0 : 1 }
  var body: some View {
    let waiting = shell.view.sidebar.badge
    HStack(spacing: 8) {
      if shell.isDemo {
        Button { shell.leaveDemo() } label: {
          HStack(spacing: 6) {
            Image(systemName: "rectangle.portrait.and.arrow.right").font(.system(size: 15, weight: .bold))
            Text("Leave demo").font(Face.text(14, .bold)).lineLimit(1).fixedSize()
          }
          .foregroundStyle(Color(hex: 0x2d2406))
          .padding(.horizontal, 13).frame(height: 58)
          .background(Capsule().fill(Ink.yellow))
          .overlay(Capsule().strokeBorder(Color(hex: 0x2d2406), lineWidth: 1.6))
          .contentShape(Capsule())
        }
        .buttonStyle(.plain).accessibilityLabel("Leave Demo")
      }
      HStack(spacing: 0) {
        item(0, "sketch:bubble", 39, 24, "Chat", id: "tab-chat") { shell.closeNote(); shell.path = []; shell.tab = .chat; shell.show(.desk) }
        item(1, "sketch:desk", 34, 24, waiting > 0 ? "Desk, \(waiting) waiting" : "Desk", id: "tab-desk", badge: waiting) { shell.closeNote(); shell.path = []; shell.tab = .desk; shell.show(.desk) }
        item(2, "sidebar:NOTE_ICON", 36, 52, "Note", id: "tab-note") { if shell.noteOpen { shell.closeNote() } else { shell.focus(.notes) } }
      }
      .background(alignment: .leading) {
        Capsule().fill(Ink.tabLens).shadow(color: .black.opacity(0.18), radius: 7, y: 2)
          .overlay(Capsule().strokeBorder(Ink.fg.opacity(0.08), lineWidth: 0.5))
          .frame(width: itemWide, height: 50)
          .offset(x: CGFloat(lit) * itemWide)
          .allowsHitTesting(false)
      }
      .padding(4)
      .glass(Capsule())
      .accessibilityElement(children: .contain).accessibilityAddTraits(.isTabBar)
    }
    .bottomChrome()
    .animation(.snappy, value: lit)
    .padding(.top, 6).padding(.bottom, bottomSink(23))
  }
  private var itemWide: CGFloat { shell.isDemo ? 66 : wide }
  private func item(_ i: Int, _ key: String, _ side: CGFloat, _ box: CGFloat, _ label: String, id: String, badge: UInt32 = 0, tap: @escaping () -> Void) -> some View {
    let sel = lit == i
    return Button(action: tap) {
      Group {
        if i == 2 { NoteGlyph(color: Ink.fg.opacity(sel ? 1 : 0.6), width: 2.4 * box / side, written: false) }
        else { PenMark(key, color: Ink.fg.opacity(sel ? 1 : 0.6), width: 2.4 * box / side) }
      }
      .frame(width: side, height: side)
      .frame(width: itemWide, height: 50)
      .overlay {
        if badge > 0 {
          Text("\(badge)").font(Face.text(11, .bold)).foregroundStyle(Ink.bg).padding(.horizontal, 5).frame(minWidth: 18, minHeight: 18).background(Capsule().fill(Ink.fg)).offset(x: 18, y: -13)
        }
      }
      .contentShape(Capsule())
    }
    .buttonStyle(.plain)
    .accessibilityLabel(label).accessibilityIdentifier(id).accessibilityAddTraits(sel ? .isSelected : [])
  }
}

/**
 * The place pill at the top left of the iPhone's pages: the desk's drawing and name with a small chevron. It is the
 * menu: the desks, Scribble and Artifacts, notifications, Settings, the demo.
 */
struct PlacePill: View {
  @EnvironmentObject var shell: AppShell
  @State private var askDesk = false
  @State private var deskName = ""
  var body: some View {
    let side = shell.view.sidebar
    Menu {
      if shell.isDemo { Section("Demo") { Button { shell.leaveDemo() } label: { Label("Leave Demo", systemImage: "rectangle.portrait.and.arrow.right") } } }
      Section("Desks") {
        if let all = side.all { deskItem(all) }
        ForEach(side.desks, id: \.id) { deskItem($0) }
        Button { deskName = ""; askDesk = true } label: { Label("New Desk…", systemImage: "plus") }
      }
      Section {
        Button { go(.scribble(boardId: side.desk)) } label: { Label("Scribble", systemImage: "scribble.variable") }
        Button { go(.artifacts) } label: { Label("Artifacts", systemImage: "photo.on.rectangle") }
        Button { go(.off(query: "")) } label: { Label("Off your mind", systemImage: "tray") }
      }
      Section {
        if let push = shell.view.settings.push {
          Picker(selection: Binding(get: { push.level }, set: { shell.act(["do": "push", "level": $0]) })) {
            Label("Everything", systemImage: "bell").tag("all")
            Label("Only Knocking", systemImage: "bell.badge").tag("knocking")
            Label("Off", systemImage: "bell.slash").tag("off")
          } label: { Label("Notifications", systemImage: push.level == "off" ? "bell.slash" : push.level == "knocking" ? "bell.badge" : "bell") }
          .pickerStyle(.menu)
        }
        Button { go(.settings); shell.account(["step": "read_account"]) } label: { Label("Settings", systemImage: "gearshape") }
        if !shell.isDemo { Button { shell.enterDemo() } label: { Label("Demo", systemImage: "play.rectangle") } }
      }
    } label: {
      HStack(spacing: 7) {
        PenMark("sketch:desk", color: Ink.fg).frame(width: 22, height: 22)
          .overlay(alignment: .topLeading) { if side.waiting > 0 { Circle().fill(Ink.yellow).frame(width: 7, height: 7).offset(x: 3, y: 1) } }
        Text(side.place.en).font(Face.display(17, .bold)).foregroundStyle(Ink.fg).lineLimit(1)
        Image(systemName: "chevron.down").font(.system(size: 11, weight: .semibold)).foregroundStyle(Ink.muted)
      }
      .padding(.horizontal, 14).frame(height: 44)
      .glass(Capsule(), interactive: true)
    }
    .accessibilityLabel("\(side.place.en), Menu")
    .accessibilityIdentifier("place-pill")
    .alert("New Desk", isPresented: $askDesk) {
      TextField("Name", text: $deskName)
      Button("Cancel", role: .cancel) {}
      Button("Create") { let n = deskName.trimmingCharacters(in: .whitespaces); if !n.isEmpty { shell.act(["do": "add_desk", "name": n]) } }
    }
  }
  private func deskItem(_ desk: CoreView.DeskTab) -> some View {
    Button { shell.closeNote(); shell.tab = .desk; shell.focus(.desk, desk: desk.id) } label: {
      Label { Text(desk.waiting > 0 ? "\(desk.name.en) · \(desk.waiting)" : desk.name.en) } icon: { if desk.selected { Image(systemName: "checkmark") } else { PenImage.desk(waiting: desk.waiting > 0) } }
    }
  }
  private func go(_ screen: CoreView.Screen) { shell.closeNote(); shell.focus(screen) }
}

// ---- the sidebar (iPad, Mac) ------------------------------------------------------------------------------

struct Sidebar: View {
  @EnvironmentObject var shell: AppShell
  @State private var switching = false
  @AppStorage("trommi-theme") private var theme = "system"
  var body: some View {
    let side = shell.view.sidebar
    VStack(spacing: 0) {
      Button { withAnimation(.snappy) { switching.toggle() } } label: {
        HStack(spacing: 12) {
          PenMark("sketch:desk", color: Ink.fg).frame(width: 34, height: 34)
            .overlay(alignment: .topLeading) { if side.waiting > 0 { Circle().fill(Ink.yellow).frame(width: 8, height: 8).offset(x: 4, y: 2) } }
          Text(side.place.en).font(Face.display(26, .heavy)).foregroundStyle(Ink.fg).lineLimit(1).minimumScaleFactor(0.6)
          Spacer(minLength: 4)
          PenMark("sketch:unfold", color: Ink.muted).frame(width: 18, height: 18).rotationEffect(.degrees(switching ? 180 : 0))
        }
        .padding(.horizontal, 14).padding(.vertical, 12)
        .background(PenBox(r: 14).stroke(Ink.fg, lineWidth: 2))
        .contentShape(Rectangle())
      }
      .buttonStyle(.plain)
      .padding(.horizontal, 10).padding(.top, 10).padding(.bottom, 6)
      .accessibilityLabel("Desk: \(side.place.en). Choose the desk")
      ScrollView {
        LazyVStack(alignment: .leading, spacing: 2) {
          if switching {
            if let all = side.all { deskItem(all) }
            ForEach(side.desks, id: \.id) { deskItem($0) }
            DashedRule().frame(height: 1).padding(.horizontal, 14).padding(.vertical, 8)
          }
          ForEach(side.sessions, id: \.id) { row in
            if let heading = row.heading {
              HStack(spacing: 10) {
                Text(heading.text.en.uppercased()).font(Face.text(12, .semibold)).kerning(1.2).foregroundStyle(Ink.muted).fixedSize()
                DashedRule().frame(height: 1)
              }.padding(.horizontal, 16).padding(.top, 14).padding(.bottom, 4)
            }
            SessionRow(row: row).padding(.leading, CGFloat(row.depth) * 18)
          }
          Button { shell.focus(.settings); shell.account(["step": "read_account"]) } label: {
            HStack(spacing: 12) { PenMark("ui:PLUS").frame(width: 22, height: 22).padding(.leading, 6); Text("New Agent…").font(Face.text(16, .medium)) }
              .foregroundStyle(side.sessions.isEmpty ? Ink.accent : Ink.muted).padding(.vertical, 10).padding(.horizontal, 12)
          }.buttonStyle(.plain)
        }.padding(.vertical, 8)
      }
      Divider().overlay(Ink.line)
      Menu {
        Button { shell.focus(.settings); shell.account(["step": "read_account"]) } label: { Label("Settings", systemImage: "key") }
        Button { shell.focus(.scribble(boardId: side.desk)) } label: { Label("Scribble", systemImage: "scribble.variable") }
        Button { shell.focus(.artifacts) } label: { Label("Artifacts", systemImage: "photo.on.rectangle") }
        Button { shell.focus(.off(query: "")) } label: { Label("Off your mind", systemImage: "tray") }
        Button { shell.focus(.notes) } label: { Label("Note", systemImage: "note.text") }
        if shell.isDemo { Button { shell.leaveDemo() } label: { Label("Leave Demo", systemImage: "rectangle.portrait.and.arrow.right") } }
        else { Button { shell.enterDemo() } label: { Label("Demo", systemImage: "play.rectangle") } }
        Picker("Theme", selection: $theme) { Text("System").tag("system"); Text("Light").tag("light"); Text("Dark").tag("dark") }
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
  private func deskItem(_ desk: CoreView.DeskTab) -> some View {
    Button { shell.focus(.desk, desk: desk.id); withAnimation(.snappy) { switching = false } } label: {
      HStack(spacing: 10) {
        PenMark("sketch:desk", color: desk.selected ? Ink.fg : Ink.muted).frame(width: 22, height: 22)
        Text(desk.name.en).font(Face.text(16, desk.selected ? .semibold : .regular)).foregroundStyle(desk.selected ? Ink.fg : Ink.muted)
        if desk.waiting > 0 { Circle().fill(Ink.urgHigh).frame(width: 7, height: 7) }
        Spacer()
        if desk.selected { PenMark("sketch:tick", color: Ink.accent).frame(width: 18, height: 18) }
      }.padding(.horizontal, 18).padding(.vertical, 9).contentShape(Rectangle())
    }.buttonStyle(.plain)
  }
}

/** One session in the sidebar: its mark, its name, how it hears, what waits. */
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
        }
        Spacer(minLength: 4)
        if row.working && !row.blocked && row.open == 0 { WorkingRing(working: true).frame(width: 18, height: 18) }
        SessionBadge(open: row.open, stopped: row.blocked, says: row.says?.en)
      }
      .padding(.horizontal, 14).padding(.vertical, 8)
      .background(RoundedRectangle(cornerRadius: 12).fill(current ? Ink.accentSoft : .clear))
      .contentShape(Rectangle())
      .opacity(row.hushed ? 0.6 : 1)
    }
    .buttonStyle(.plain)
    .padding(.horizontal, 6)
    .accessibilityIdentifier("sidebar-session-" + row.id)
  }
}

// ---- the Desk ----------------------------------------------------------------------------------------------------

struct DeskScreen: View {
  @EnvironmentObject var shell: AppShell
  @Environment(\.horizontalSizeClass) private var hSize
  @State private var speedReport: String?
  var body: some View {
    let desk = shell.view.desk
    ScrollView {
      LazyVStack(alignment: .leading, spacing: 12) {
        if desk.mode == "invite" { FirstAgent() }
        else if let hello = desk.hello {
          VStack(alignment: .leading, spacing: 0) {
            Greeting(text: hello.en)
            if let goals = desk.goals { GoalsDrawing(goals: goals).id(goals.deskId) }
          }.padding(.top, 18).padding(.bottom, 6)
        }
        if let reading = desk.reading { Text(reading.en).font(Face.text(14)).foregroundStyle(Ink.muted).accessibilityIdentifier("reading-history") }
        if !desk.slip.isEmpty { CutSlip(slips: desk.slip) }
        ForEach(desk.rows, id: \.id) { row in DeskRow(row: row) }
        if let empty = desk.empty {
          VStack(spacing: 10) {
            Sketch("desk", color: Ink.faint).frame(width: 64, height: 64)
            Text(empty.en).font(Face.text(15)).foregroundStyle(Ink.muted).multilineTextAlignment(.center)
          }.frame(maxWidth: .infinity).padding(.vertical, 28)
        }
        if let off = desk.off { OffHint(off: off) }
      }
      .padding(.horizontal, 16).padding(.bottom, 120 + (desk.starred != nil || desk.running != nil ? 54 : 0))
      .frame(maxWidth: 760).frame(maxWidth: .infinity)
    }
    .accessibilityIdentifier("desk-content")
    .accessibilityValue(speedReport ?? String(desk.rows.count))
    .onAppear { speedReport = NativeDemo.ready(rows: desk.rows.count) }
    .refreshable { shell.act(["do": "retry"]) }
    .background(Ink.bg)
    .overlay(alignment: .bottom) { DeskDock(starred: desk.starred, running: desk.running).padding(.bottom, hSize == .regular ? 20 : 84) }
    .modifier(DeskTop(regular: hSize == .regular))
  }
}

/** The Desk's top: the floating place pill on the iPhone; on the iPad and the Mac the desk's name in the bar. */
struct DeskTop: ViewModifier {
  @EnvironmentObject var shell: AppShell
  let regular: Bool
  func body(content: Content) -> some View {
    #if os(macOS)
    content.navigationTitle(shell.view.sidebar.place.en)
    #else
    if regular {
      content.navigationBarTitleDisplayMode(.inline).toolbar {
        ToolbarItem(placement: .principal) {
          HStack(spacing: 8) { PenMark("sketch:desk").frame(width: 24, height: 24); Text(shell.view.sidebar.place.en).font(Face.display(19, .bold)).foregroundStyle(Ink.fg) }
        }
      }
    } else {
      content.topPills { PlacePill() } trailing: { EmptyView() }
    }
    #endif
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

/** The section Off your mind on the Desk: the first rows, fading, and how many there are; it opens the whole list. */
struct OffHint: View {
  @EnvironmentObject var shell: AppShell
  let off: CoreView.OffHintView
  var body: some View {
    VStack(alignment: .leading, spacing: 0) {
      EndDivider(title: "Off your mind", more: { shell.focus(.off(query: "")) }).padding(.bottom, 4)
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
    }.padding(.top, 8)
  }
}
/** The dock at the bottom of the Desk: two stacks, his starred cards and the cards still running, each a small tab;
 *  a tap fans the cards upward. Which card lies where is the core's (`desk.starred`, `desk.running`). */
struct DeskDock: View {
  @EnvironmentObject var shell: AppShell
  let starred: CoreView.DockView?
  let running: CoreView.DockView?
  @State private var open: String? = nil
  var body: some View {
    if starred != nil || running != nil {
      VStack(alignment: open == "run" && starred != nil ? .trailing : .leading, spacing: 4) {
        if open == "star", let starred { fan(starred) }
        if open == "run", let running { fan(running) }
        HStack(spacing: 10) {
          if let starred { tab(starred, word: "Starred") }
          if let running { tab(running, word: "Running") }
        }
      }
      .padding(.horizontal, 8)
      .frame(maxWidth: 640)
    }
  }
  private func tab(_ dock: CoreView.DockView, word: String) -> some View {
    let on = open == dock.key
    return Button { withAnimation(.easeOut(duration: 0.2)) { open = on ? nil : dock.key } } label: {
      HStack(spacing: 8) {
        if dock.key == "star" { StarMark().frame(width: 15, height: 15) }
        else { WorkingRing(working: true, color: Ink.muted).frame(width: 15, height: 15) }
        Text(word).font(Face.text(13, .semibold)).foregroundStyle(Ink.fg)
        Text("\(dock.count)").font(Face.text(11, .bold)).monospacedDigit().foregroundStyle(Ink.bg)
          .padding(.horizontal, 6).padding(.vertical, 3).frame(minWidth: 18).background(Capsule().fill(Ink.fg))
      }
      .padding(.horizontal, 14).frame(height: 36)
      .background(Capsule().fill(on ? Ink.sunken : Ink.surface))
      .overlay(Capsule().strokeBorder(Ink.lineStrong, lineWidth: 1))
      .shadow(color: .black.opacity(0.18), radius: 8, y: 4)
      .frame(minHeight: 44).contentShape(Rectangle())
    }
    .buttonStyle(.plain)
    .accessibilityLabel(dock.says.en)
  }
  private func fan(_ dock: CoreView.DockView) -> some View {
    VStack(alignment: .leading, spacing: 7) {
      ScrollView {
        VStack(alignment: .leading, spacing: 7) {
          ForEach(Array(dock.rows.enumerated()), id: \.element.id) { i, r in
            Button { open = nil; shell.focus(.card(cardId: r.id)) } label: { mini(r).frame(width: 300) }
              .buttonStyle(.plain)
              .rotationEffect(.degrees(i % 2 == 0 ? -1.4 : 1.1)).offset(x: i % 2 == 0 ? 0 : 5)
              .shadow(color: .black.opacity(0.16), radius: 7, y: 4)
              .accessibilityLabel(r.title)
          }
        }.padding(.horizontal, 10).padding(.vertical, 8)
      }
      .frame(maxHeight: 420).fixedSize(horizontal: true, vertical: true)
      if let all = dock.all {
        Button { open = nil; shell.focus(.off(query: "")) } label: {
          HStack(spacing: 5) { Text(all.en).font(Face.text(14, .semibold)); Image(systemName: "arrow.right").font(.system(size: 12, weight: .semibold)) }
            .foregroundStyle(Ink.fg).padding(.horizontal, 14).frame(minHeight: 36)
            .background(Capsule().fill(Ink.surface)).overlay(Capsule().strokeBorder(Ink.fg, lineWidth: 1.5))
            .frame(minHeight: 44).contentShape(Rectangle())
        }.buttonStyle(.plain).padding(.leading, 10)
      }
    }
    .transition(.move(edge: .bottom).combined(with: .opacity))
  }
  private func mini(_ r: CoreView.DockRowView) -> some View {
    HStack(spacing: 10) {
      AgentMark(from: r.from, size: 24)
      VStack(alignment: .leading, spacing: 2) {
        Text(r.title).font(Face.text(13, .semibold)).foregroundStyle(Ink.fg).lineLimit(2).multilineTextAlignment(.leading)
        Text(r.line.en).font(Face.text(11)).foregroundStyle(Ink.muted).lineLimit(1)
      }
      Spacer(minLength: 0)
    }
    .padding(.horizontal, 14).padding(.vertical, 10)
    .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Ink.surface))
    .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(Ink.lineStrong, lineWidth: 1))
    .contentShape(RoundedRectangle(cornerRadius: 12))
  }
}

/** The desk's goals under the greeting: a quiet line or a few; a tap writes in place; one toggle folds them. */
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
          .accessibilityLabel("This desk’s goals")
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
          .accessibilityLabel(goals.lines.isEmpty ? "This desk’s goals: none yet" : "This desk’s goals: \(shown)")
          if let toggle = goals.toggle {
            Button { withAnimation(.easeOut(duration: 0.18)) { shell.act(["do": "fold_goals", "desk_id": goals.deskId, "folded": !goals.folded]) } } label: {
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
    if d != goals.lines.joined(separator: "\n") { shell.act(["do": "set_goal", "desk_id": goals.deskId, "text": d]) }
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
