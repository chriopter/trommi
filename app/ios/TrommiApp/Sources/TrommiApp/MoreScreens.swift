import SwiftUI
import TrommiClient
import UniformTypeIdentifiers

struct OffScreen: View {
  @EnvironmentObject var shell: AppShell
  @State private var query = ""
  var body: some View {
    let off = shell.view.off ?? CoreView.OffListView()
    List {
      ForEach(off.works, id: \.id) { row in Button(row.title) { shell.focus(.card(cardId: row.id)) } }
      ForEach(off.rest, id: \.id) { row in Button { shell.focus(.card(cardId: row.id)) } label: { VStack(alignment: .leading) { Text(row.title); Text(row.says.en).font(Face.text(12)).foregroundStyle(Ink.muted) } } }
      if let none = off.none { Text(none.en) }
      if let more = off.more { Button(more.says.en) { shell.act(["do": "show_more", "of": ["list": "off", "query": query]]) } }
      if let older = off.older { Button(older.en) { shell.act(["do": "show_more", "of": ["list": "off", "query": query]]) } }
    }.navigationTitle("Off your mind").searchable(text: $query).onChange(of: query) { _, text in shell.focus(.off(query: text)) }
  }
}
struct NotesScreen: View {
  @EnvironmentObject var shell: AppShell
  @State private var text = ""
  @State private var attachments: [String] = []
  var body: some View {
    ScrollView { VStack(alignment: .leading, spacing: 20) {
      Text("Note").font(Face.display(36))
      TextEditor(text: $text).font(Face.text(20)).frame(minHeight: 180).padding().scrollContentBackground(.hidden).background(Ink.noteYellow, in: PenBox()).foregroundStyle(Ink.noteInk)
      AttachmentPicker(ids: $attachments)
      Button("Keep in Note") { shell.act(["do": "write_note_files", "text": text, "attachments": attachments]); text = ""; attachments = [] }.buttonStyle(.borderedProminent)
      ForEach(shell.view.notes.rows, id: \.id) { note in
        VStack(alignment: .leading, spacing: 12) {
          Markdown(text: note.text); FileRows(ids: note.files.map(\.fileId))
          if let says = shell.view.notes.sendSays { Button(says.en) { shell.act(["do": "send_note", "note_id": note.id]) } }
          ForEach(shell.view.notes.choose, id: \.sessionId) { to in Button(to.name) { shell.act(["do": "send_note", "note_id": note.id, "to": to.sessionId]) } }
          Button("Delete", role: .destructive) { shell.act(["do": "delete_note", "note_id": note.id]) }
        }.padding(20).background(Ink.noteYellow, in: PenBox()).foregroundStyle(Ink.noteInk)
      }
    }.padding(24).frame(maxWidth: 780).frame(maxWidth: .infinity) }
  }
}
struct SettingsScreen: View {
  @EnvironmentObject var shell: AppShell
  @AppStorage("trommi-theme") private var theme = "system"
  @State private var deskName = ""
  @State private var currentPassword = ""
  @State private var nextPassword = ""
  @State private var accountEmail = ""
  @State private var kitWords = ""
  var body: some View {
    Form {
      if let problem = shell.view.account.problem { Text(problem.en).foregroundStyle(Ink.deny) }
      if shell.view.account.step == "working" { ProgressView() }
      Section("Account") { if let email = shell.view.settings.account { Text(email) }; Button("Pair a device") { shell.account(["step": "invite_device"]) }; Button("Approve a computer") { shell.account(["step": "approve_computer", "label": ""]) }; AccountInvite() }
      Section("Account security") {
        SecureField("Current password", text: $currentPassword)
        SecureField("New password", text: $nextPassword)
        Button("Change password") { shell.account(["step": "set_password", "unlock": ["by": "password", "password": currentPassword], "next": nextPassword]); currentPassword = ""; nextPassword = "" }
        Button("New Emergency Kit") { shell.account(["step": "new_kit", "unlock": ["by": "password", "password": currentPassword]]); currentPassword = "" }
        Button("Replace recovery code") { shell.account(["step": "replace_recovery_code", "unlock": ["by": "password", "password": currentPassword]]); currentPassword = "" }
        TextField("E-mail", text: $accountEmail)
        SecureField("Emergency Kit words", text: $kitWords)
        Button("Set e-mail") { shell.account(["step": "set_email", "email": accountEmail, "words": kitWords]); kitWords = "" }
        Button("Delete account", role: .destructive) { shell.account(["step": "delete_account", "password": currentPassword]); currentPassword = "" }
      }
      Section("Data") {
        Button("Read hub data") { shell.account(["step": "read_usage"]) }
        if let data = shell.view.settings.data {
          if let says = data.says { Text(says.en) }
          ForEach(data.rows, id: \.id) { row in HStack { Text(row.label.en); Spacer(); Text(row.value.en) } }
        }
      }
      Section("Devices") {
        ForEach(shell.view.settings.devices, id: \.id) { device in
          VStack(alignment: .leading) { Text(device.name); Text(device.says.en).font(Face.text(12)); Text(device.fingerprint).font(Face.mono(11)).textSelection(.enabled) }
          Button("Remove", role: .destructive) { shell.account(["step": "remove_devices", "device_ids": [device.id]]) }
        }
      }
      Section("Notifications") {
        Picker("Notifications", selection: Binding(get: { shell.view.settings.push?.level ?? "off" }, set: { shell.act(["do": "push", "level": $0]) })) { Text("Off").tag("off"); Text("All").tag("all"); Text("Only knocking").tag("knocking") }
      }
      Section("Agents") {
        ForEach(shell.view.settings.agentList, id: \.key) { agent in
          DisclosureGroup(agent.name.en) { Text(agent.state.en); Text(agent.count.en); if let usage = agent.usage { UsageDrawing(usage: usage) }; ForEach(agent.sessions, id: \.id) { session in Button(session.name.en) { shell.focus(.session(sessionId: session.id)) } }; if let remove = agent.remove { Button(remove.en, role: .destructive) { shell.account(["step": "remove_devices", "device_ids": agent.devices]) } } }
        }
      }
      Section("Desks") {
        ForEach(shell.view.sidebar.desks, id: \.id) { desk in HStack { Text(desk.name.en); Spacer(); Button("Open") { shell.focus(.desk, desk: desk.id) }; Button("Delete", role: .destructive) { shell.act(["do": "remove_desk", "desk_id": desk.id]) } } }
        TextField("New desk", text: $deskName); Button("Add desk") { shell.act(["do": "add_desk", "name": deskName]); deskName = "" }
      }
      Section("Appearance") { Picker("Theme", selection: $theme) { Text("System").tag("system"); Text("Light").tag("light"); Text("Dark").tag("dark") } }
      Section("Room") { if let room = shell.view.settings.room { Text(room).font(Face.mono(12)).textSelection(.enabled) }; if let hub = shell.view.settings.hub { Text(hub).font(Face.text(14)) } }
      Section { Button("Sign out", role: .destructive) { shell.account(["step": "sign_out"]) } }
    }.navigationTitle("Settings")
  }
}
struct ArtifactsScreen: View {
  @EnvironmentObject var shell: AppShell
  var body: some View { ScrollView { LazyVStack(alignment: .leading, spacing: 20) {
    ForEach(shell.view.artifacts ?? [], id: \.id) { artifact in VStack(alignment: .leading, spacing: 10) { Text(artifact.title).font(Face.display(25)); FileRows(ids: artifact.files) }.padding(20).background(Ink.surface2, in: PenBox()) }
  }.padding(24) }.navigationTitle("Artifacts") }
}
struct Toasts: View {
  @EnvironmentObject var shell: AppShell
  var body: some View {
    VStack(spacing: 8) { ForEach(shell.view.toasts, id: \.id) { toast in
      HStack { VStack(alignment: .leading) { Text(toast.text.en).font(Face.text(15, .medium)); if let line = toast.line { Text(line.en).font(Face.text(12)) } }; Spacer()
        if let action = toast.action { Button(toast.label?.en ?? "Undo") { if let data = try? JSONEncoder().encode(action), let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] { shell.act(json) } } }
        if let go = toast.go { Button(go.label.en) { shell.focus(go.screen) } }
        Button { shell.act(["do": "dismiss", "toast_id": toast.id]) } label: { Image(systemName: "xmark") }
      }.padding(14).background(.regularMaterial, in: RoundedRectangle(cornerRadius: 16)).padding(.horizontal)
      .task(id: toast.id) { try? await Task.sleep(nanoseconds: (toast.ms ?? 6000) * 1_000_000); if !Task.isCancelled { shell.act(["do": "dismiss", "toast_id": toast.id]) } }
    } }
  }
}
struct SaveDocument: FileDocument {
  static var readableContentTypes: [UTType] { [.data] }
  var data: Data
  init(_ data: Data) { self.data = data }
  init(configuration: ReadConfiguration) throws { data = configuration.file.regularFileContents ?? Data() }
  func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper { FileWrapper(regularFileWithContents: data) }
}
struct SaveSheet: View {
  let file: SavedFile
  @State private var exporting = false
  var body: some View { VStack(spacing: 20) { Text(file.name).font(Face.display(24)); Button("Save file") { exporting = true } }.padding(32).fileExporter(isPresented: $exporting, document: SaveDocument(file.data), contentType: .data, defaultFilename: file.name) { _ in } }
}
