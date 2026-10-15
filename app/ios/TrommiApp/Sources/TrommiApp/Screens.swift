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
    NavigationStack {
      Group {
        if shell.view.phase == .signedOut || shell.view.phase == .starting || shell.view.account.step == "keep_kit" { AccountScreen() }
        else if wide {
          HStack(spacing: 0) { Sidebar().frame(width: 258); Divider(); page.frame(maxWidth: .infinity) }
        } else { page }
      }
      .background(Ink.bg).foregroundStyle(Ink.fg)
      .preferredColorScheme(theme == "light" ? .light : theme == "dark" ? .dark : nil)
      .toolbar {
        ToolbarItem(placement: .navigation) {
          Menu { SidebarMenu() } label: { Label(shell.view.sidebar.place.en, systemImage: "bell") }
        }
        ToolbarItem(placement: .primaryAction) {
          Menu {
            if shell.isDemo { Button("Leave demo") { shell.leaveDemo() } }
            Button("Note", systemImage: "note.text") { shell.focus(.notes) }
            Button("Off your mind", systemImage: "tray") { shell.focus(.off(query: "")) }
            Button("Artifacts", systemImage: "photo.on.rectangle") { shell.focus(.artifacts) }
            Button("Scribble Board", systemImage: "pencil.tip") { shell.focus(.scribble(boardId: shell.view.sidebar.desk)) }
            Button("Settings", systemImage: "gearshape") { shell.focus(.settings); shell.account(["step": "read_account"]) }
          } label: { Image(systemName: "ellipsis.circle") }
        }
      }
      .safeAreaInset(edge: .bottom) { Toasts() }
      .sheet(item: $shell.saved) { file in SaveSheet(file: file) }
      .overlay(alignment: .top) { if let text = shell.view.status { Text(text.en).font(Face.text(13)).padding(8).background(.regularMaterial, in: Capsule()) } }
      .alert("Trommi", isPresented: Binding(get: { shell.platformError != nil }, set: { if !$0 { shell.platformError = nil } })) { Button("OK") { shell.platformError = nil } } message: { Text(shell.platformError ?? "") }
    }
  }
  @ViewBuilder var page: some View {
    switch shell.screen {
    case .desk: DeskScreen()
    case .card(let id): if let card = shell.view.cards[id] { CardScreen(card: card).id(id) } else { ProgressView() }
    case .session(let id): if let page = shell.view.sessions[id] { SessionScreen(page: page).id(id) } else { ProgressView() }
    case .notes: NotesScreen()
    case .off: OffScreen()
    case .settings: SettingsScreen()
    case .artifacts: ArtifactsScreen()
    case .scribble: ScribbleScreen()
    }
  }
}
struct SidebarMenu: View {
  @EnvironmentObject var shell: AppShell
  var body: some View {
    Button("Desk") { shell.focus(.desk) }
    if let all = shell.view.sidebar.all { Button(all.name.en) { shell.focus(.desk, desk: all.id) } }
    ForEach(shell.view.sidebar.desks, id: \.id) { desk in Button(desk.name.en) { shell.focus(.desk, desk: desk.id) } }
    Divider()
    ForEach(shell.view.sidebar.sessions, id: \.id) { row in Button(row.name.en) { shell.focus(.session(sessionId: row.id)) } }
  }
}
struct Sidebar: View {
  @EnvironmentObject var shell: AppShell
  var body: some View {
    ScrollView {
      VStack(alignment: .leading, spacing: 16) {
        Text("Trommi").font(Face.display(30)).padding(.bottom, 12)
        Button("Desk") { shell.focus(.desk) }
        if let all = shell.view.sidebar.all { Button(all.name.en) { shell.focus(.desk, desk: all.id) } }
        ForEach(shell.view.sidebar.desks, id: \.id) { desk in Button(desk.name.en) { shell.focus(.desk, desk: desk.id) } }
        Divider()
        ForEach(shell.view.sidebar.sessions, id: \.id) { row in
          if let heading = row.heading { Text(heading.text.en).font(Face.text(12)).foregroundStyle(Ink.muted) }
          Button { shell.focus(.session(sessionId: row.id)) } label: {
            HStack(spacing: 12) {
              AgentMark(mark: row.mark, crowned: row.crowned)
              VStack(alignment: .leading, spacing: 4) { Text(row.name.en).font(Face.text(15, .medium)); if let cap = row.cap { Text(cap.word.en).font(Face.text(12)).foregroundStyle(Ink.muted) } }
              Spacer(); WorkingRing(working: row.working).frame(width: 18, height: 18)
            }.padding(.leading, CGFloat(row.depth) * 12).opacity(row.hushed ? 0.6 : 1)
          }.buttonStyle(.plain)
        }
      }.padding(22)
    }.background(Ink.surface2)
  }
}
struct DeskScreen: View {
  @EnvironmentObject var shell: AppShell
  var body: some View {
    ScrollView {
      LazyVStack(alignment: .leading, spacing: 20) {
        if let hello = shell.view.desk.hello { Text(hello.en).font(Face.display(38)).overlay(alignment: .bottom) { PenUnderline().stroke(Ink.accent, lineWidth: 2).frame(height: 5).offset(y: 6) }.padding(.bottom, 20) }
        if let reading = shell.view.desk.reading { Text(reading.en).font(Face.text(14)).foregroundStyle(Ink.muted) }
        if shell.view.desk.mode == "invite" { Button("Invite an agent", systemImage: "plus") { shell.account(["step": "invite_agent", "label": "", "desk": shell.view.sidebar.desk]) }.buttonStyle(.borderedProminent) }
        if let goals = shell.view.desk.goals { GoalsDrawing(goals: goals).id(goals.deskId) }
        ForEach(shell.view.desk.rows, id: \.id) { row in DeskRow(row: row) }
        if let empty = shell.view.desk.empty { VStack(spacing: 12) { Sketch("desk", color: Ink.faint).frame(width: 64, height: 64); Text(empty.en).font(Face.text(16)).foregroundStyle(Ink.muted) }.frame(maxWidth: .infinity).padding(.vertical, 40) }
        if let dock = shell.view.desk.starred { Dock(dock: dock) }
        if let dock = shell.view.desk.running { Dock(dock: dock) }
        if let off = shell.view.desk.off { Button { shell.focus(.off(query: "")) } label: { Label(off.says.en, systemImage: "tray") }.font(Face.display(22)).padding(.top, 24) }
      }.padding(24).frame(maxWidth: 820).frame(maxWidth: .infinity)
    }.refreshable { shell.act(["do": "retry"]) }
  }
}
struct Dock: View {
  @EnvironmentObject var shell: AppShell
  let dock: CoreView.DockView
  var body: some View {
    DisclosureGroup(dock.says.en) { ForEach(dock.rows, id: \.id) { row in Button(row.title) { shell.focus(.card(cardId: row.id)) }.padding(8) } }
      .padding().background(Ink.surface2, in: PenBox())
  }
}
struct DeskRow: View {
  @EnvironmentObject var shell: AppShell
  let row: CoreView.DeskRowView
  var body: some View {
    VStack(alignment: .leading, spacing: 14) {
      if let from = row.from { HStack { AgentMark(mark: from.mark, size: 22); Text(from.path.joined(separator: " › ")).font(Face.text(13)).foregroundStyle(Ink.muted) } }
      Button { shell.focus(.card(cardId: row.id)) } label: { VStack(alignment: .leading, spacing: 10) { Text(row.title).font(Face.display(24)).foregroundStyle(Ink.fg); Text(row.words).font(Face.text(16)).foregroundStyle(Ink.muted).lineLimit(3) }.frame(maxWidth: .infinity, alignment: .leading) }.buttonStyle(.plain).accessibilityIdentifier("desk-card-" + row.id)
      ForEach(Array(row.notes.enumerated()), id: \.offset) { Text($0.element.en).font(Face.text(12)).foregroundStyle(Ink.muted) }
      HStack(spacing: 12) {
        ForEach(row.tiles.tiles, id: \.key) { tile in
          Button { tileTap(tile.key) } label: {
            Group { if tile.thumbs { Image(systemName: tile.thumb == "yes" ? "hand.thumbsup" : "hand.thumbsdown") } else { Text(tile.label) } }.font(Face.text(17, .medium)).frame(maxWidth: .infinity).padding(16)
          }.background(tile.lead ? Ink.lead : Ink.surface2, in: PenBox()).foregroundStyle(tile.lead ? Ink.leadFg : Ink.fg).accessibilityLabel(tile.says.en)
        }
        if row.tiles.kind == "read" { Button("Got it") { shell.act(["do": "read", "card_id": row.id]) }.buttonStyle(.borderedProminent) }
        if row.tiles.kind == "choose" || row.tiles.kind == "advised" { Button(row.tiles.says?.en ?? "Choose") { shell.focus(.card(cardId: row.id)) }.buttonStyle(.bordered) }
      }
    }.padding(20).background(Ink.surface, in: PenBox()).overlay(PenBox().stroke(Ink.lineStrong, lineWidth: 1))
      .contextMenu { CardWays(id: row.id, ways: row.ways) }
  }
  private func tileTap(_ key: String) {
    if row.kind == "request" { shell.act(["do": "verdict", "request_id": row.id, "allow": key == "allow"]) }
    else { shell.act(["do": "answer", "card_id": row.id, "choices": [key]]) }
  }
}
struct CardWays: View {
  @EnvironmentObject var shell: AppShell
  let id: String; let ways: [String]
  var body: some View {
    ForEach(ways, id: \.self) { way in
      switch way {
      case "later": Button("Later", systemImage: "clock") { shell.act(["do": "snooze", "card_id": id]) }
      case "shred": Button("Shred", systemImage: "trash", role: .destructive) { shell.act(["do": "shred", "card_id": id]) }
      case "reverse": Button("What??") { shell.act(["do": "hand_back", "card_id": id]) }
      default: EmptyView()
      }
    }
  }
}
struct CardScreen: View {
  @EnvironmentObject var shell: AppShell
  let card: CoreView.CardView
  @State private var note = ""
  @State private var choices: Set<String> = []
  @State private var attachments: [String] = []
  var body: some View {
    ScrollView {
      VStack(alignment: .leading, spacing: 20) {
        if let from = card.from { Text(from.path.joined(separator: " › ")).font(Face.text(14)).foregroundStyle(Ink.muted) }
        HStack { Text(card.nr.en).font(Face.mono(13)); Spacer(); Button { shell.act(["do": "star", "card_id": card.id, "on": !card.starred]) } label: { StarMark(on: card.starred).frame(width: 24, height: 24) }.accessibilityLabel(card.starSays.en) }
        if let work = card.work { Text(work.says.en).font(Face.text(15)); Text(work.words.en).foregroundStyle(Ink.muted) }
        Text(card.title).font(Face.display(36)).accessibilityIdentifier("card-title")
        Markdown(text: card.text)
        if !card.html.isEmpty { InlineHTML(html: card.html).frame(minHeight: 240) }
        FileRows(ids: card.files)
        ForEach(card.pictures, id: \.fileId) { picture in NativePicture(id: picture.fileId) }
        if card.answer.mode == "options" {
          ForEach(card.options, id: \.key) { option in
            Button {
              if card.multiple { if choices.contains(option.key) { choices.remove(option.key) } else { choices.insert(option.key) } }
              else { answer([option.key]) }
            } label: {
              HStack { VStack(alignment: .leading, spacing: 5) { Text(option.label).font(Face.text(18, .medium)); if !option.detail.isEmpty { Text(option.detail).font(Face.text(14)) } }; Spacer(); if choices.contains(option.key) { Image(systemName: "checkmark.circle.fill") } }
                .padding(18).frame(maxWidth: .infinity, alignment: .leading)
            }.background(option.recommended ? Ink.lead : Ink.surface2, in: PenBox()).foregroundStyle(option.recommended ? Ink.leadFg : Ink.fg)
          }
          if card.multiple { Button("Answer") { answer(Array(choices)) }.buttonStyle(.borderedProminent) }
        } else if card.answer.mode == "read" { Button("Got it") { shell.act(["do": "read", "card_id": card.id]) }.buttonStyle(.borderedProminent) }
        else if card.answer.mode == "request" { HStack { Button("Deny") { verdict(false) }; Button("Allow") { verdict(true) }.buttonStyle(.borderedProminent) } }
        if let label = card.answer.label { Text(label.en).font(Face.display(24)) }
        if let detail = card.answer.detail { Text(detail.en).font(Face.text(16)) }
        if card.answer.back == "reopen" { Button("Decide again") { shell.act(["do": "decide_again", "card_id": card.id]) } }
        if card.answer.back == "takeback" { Button("Take back") { shell.act(["do": "take_back", "card_id": card.id]) } }
        AttachmentPicker(ids: $attachments)
        TextField(card.ask.en, text: $note, axis: .vertical).textFieldStyle(.roundedBorder)
          .onChange(of: note) { _, value in shell.act(["do": "draft", "place": card.id, "text": value]) }
        Button("Send") { shell.act(["do": "write", "session_id": card.sessionId ?? "", "card_id": card.id, "text": note, "attachments": attachments]); note = ""; attachments = [] }
        ForEach(card.talk, id: \.id) { row in TalkRow(row: row) }
        ForEach(card.versions, id: \.n) { version in DisclosureGroup(version.title) { Markdown(text: version.text) } }
      }.padding(24).frame(maxWidth: 780).frame(maxWidth: .infinity)
    }.navigationTitle(card.nr.en).onAppear { note = card.draft; choices = Set(card.options.filter(\.chosen).map(\.key)) }
  }
  private func answer(_ keys: [String]) { shell.act(["do": "answer", "card_id": card.id, "choices": keys, "note": note, "attachments": attachments]) }
  private func verdict(_ allow: Bool) { shell.act(["do": "verdict", "request_id": card.id, "allow": allow]) }
}
struct SessionScreen: View {
  @EnvironmentObject var shell: AppShell
  let page: CoreView.SessionView
  @State private var text = ""
  @State private var attachments: [String] = []
  var body: some View {
    ScrollViewReader { reader in
      ScrollView { LazyVStack(alignment: .leading, spacing: 18) {
        HStack { AgentMark(mark: page.mark, size: 42, crowned: page.crowned); Text(page.name.en).font(Face.display(30)) }
        Text(page.modelLabel).font(Face.text(13)).foregroundStyle(Ink.muted)
        if let usage = page.usage { UsageDrawing(usage: usage) }
        ForEach(page.statusLines, id: \.id) { line in
          HStack {
            WorkingRing(working: line.state == "working").frame(width: 18, height: 18)
            VStack(alignment: .leading) {
              Text(line.label).font(Face.text(15, .medium)); Text(line.detail).font(Face.text(13))
              Text(line.says.en).font(Face.text(12)).foregroundStyle(Ink.muted)
              if let card = line.card { Button(card.title) { shell.focus(.card(cardId: card.id)) } }
            }
          }
        }
        if let note = page.note { if let text = note.line { Text(text.en).font(Face.text(14)).foregroundStyle(Ink.muted) }; if let receipt = note.receipt { Text(receipt.en).font(Face.text(13)) } }
        if let empty = page.empty { Text(empty.text.en).font(Face.text(18)); ForEach(Array(empty.starters.enumerated()), id: \.offset) { item in Button(item.element.en) { shell.act(["do": "write", "session_id": page.id, "text": item.element.en]) } } }

        if let earlier = page.earlier { Button(earlier.says.en) { shell.act(["do": "show_more", "of": ["list": "talk", "session": page.id]]) } }
        ForEach(page.talk, id: \.id) { row in TalkRow(row: row) }
        Color.clear.frame(height: 1).id("bottom")
      }.padding(24).frame(maxWidth: 780).frame(maxWidth: .infinity) }
      .defaultScrollAnchor(.bottom)
      .safeAreaInset(edge: .bottom) {
        VStack { AttachmentPicker(ids: $attachments); HStack { TextField(page.compose.en, text: $text, axis: .vertical).textFieldStyle(.roundedBorder); Button("Send", systemImage: "arrow.up") { shell.act(["do": "write", "session_id": page.id, "text": text, "attachments": attachments]); text = ""; attachments = [] } } }.padding().background(.regularMaterial)
      }
    }.navigationTitle(page.name.en)
      .toolbar { Menu("Session") { Button("Archive", role: .destructive) { shell.act(["do": "archive_session", "session_id": page.id, "on": true]) }; Button("Crown") { shell.act(["do": "crown_session", "session_id": page.id, "on": !page.crowned]) } } }
  }
}
struct TalkRow: View {
  @EnvironmentObject var shell: AppShell
  let row: CoreView.TalkRow
  var body: some View {
    VStack(alignment: .leading, spacing: 10) {
      if let day = row.day { Text(day.en).font(Face.text(12)).foregroundStyle(Ink.muted).frame(maxWidth: .infinity) }
      if let card = row.row { DeskRow(row: card) }
      if let work = row.work { WorkDrawing(work: work) }
      if let deed = row.deed { Text(deed.label.en).font(Face.text(13)); ForEach(Array(deed.said.enumerated()), id: \.offset) { Text($0.element.en).font(Face.text(13)) } }
      if let event = row.event { Text(event.label.en).font(Face.text(13)).foregroundStyle(Ink.muted) }
      if !row.text.isEmpty { Markdown(text: row.text).accessibilityIdentifier("talk-message").padding(14).background(row.fromMe ? Ink.accentSoft : Ink.surface2, in: PenBox()) }
      FileRows(ids: row.files)
    }.frame(maxWidth: .infinity, alignment: row.fromMe ? .trailing : .leading)
  }
}
struct Markdown: View {
  let text: String
  var body: some View { Text((try? AttributedString(markdown: text, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(text)).font(Face.text(17)).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading) }
}
struct NativePicture: View {
  @EnvironmentObject var shell: AppShell
  let id: String
  var body: some View {
    Group {
      #if canImport(UIKit)
      if let data = shell.files[id], let image = UIImage(data: data) { Image(uiImage: image).resizable().scaledToFit().accessibilityIdentifier("native-picture") } else { ProgressView() }
      #else
      if let data = shell.files[id], let image = NSImage(data: data) { Image(nsImage: image).resizable().scaledToFit().accessibilityIdentifier("native-picture") } else { ProgressView() }
      #endif
    }.task(id: id) { shell.act(["do": "open_file", "attachment_id": id, "save": false]) }
  }
}
struct FileRows: View {
  @EnvironmentObject var shell: AppShell
  let ids: [String]
  var body: some View {
    ForEach(ids, id: \.self) { id in
      if let file = shell.view.files[id] { Button(file.name, systemImage: "doc") { shell.act(["do": "open_file", "attachment_id": id, "save": true]) } }
    }
  }
}
