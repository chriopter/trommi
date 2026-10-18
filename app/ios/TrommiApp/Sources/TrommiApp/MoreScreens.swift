// MoreScreens.swift: the pages off the Desk, drawn as the app looked before the shared app core: Off your mind (the
// big heading with the pen's line, the cards with their agents in a drawn border, the put-off and closed ones as small
// quiet rows with their marks), the note (a full page of yellow paper: the words, the files, one row of paperclip ·
// bin · "To: …" · the envelope), Settings (one list on paper, its groups as iOS Settings draws them) and the
// Artifacts. Every word that depends on the state comes from the core's view model.
import SwiftUI
import TrommiClient
import UniformTypeIdentifiers

/** When something moved, short ("5 min ago"): the time the core hands over, written by the shell. */
func offAgo(_ ms: UInt64) -> String {
  let f = RelativeDateTimeFormatter(); f.unitsStyle = .short; f.locale = Locale(identifier: "en_GB")
  return f.localizedString(for: Date(timeIntervalSince1970: Double(ms) / 1000), relativeTo: Date())
}

/** A page's big heading in the display face, its last word underlined with the pen (as the Desk's greeting). */
struct PageHeading: View {
  let text: String
  var body: some View { Greeting(text: text, size: 34).padding(.top, 10).padding(.bottom, 8) }
}

/** A search line on paper (the page has no navigation bar to carry the system's search field). */
struct PaperSearch: View {
  @Binding var text: String
  var prompt = "Search the list"
  var body: some View {
    HStack(spacing: 8) {
      Sketch("search", color: Ink.muted).frame(width: 18, height: 18)
      TextField(prompt, text: $text).textInputAutocapitalization(.never).autocorrectionDisabled().font(Face.text(16))
      if !text.isEmpty { Button { text = "" } label: { Image(systemName: "xmark.circle.fill").foregroundStyle(Ink.faint) }.buttonStyle(.plain).accessibilityLabel("Clear") }
    }
    .padding(.horizontal, 12).frame(minHeight: 42)
    .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Ink.sunken))
  }
}

// ---- Off your mind ---------------------------------------------------------------------------------------------

struct OffScreen: View {
  @EnvironmentObject var shell: AppShell
  @State private var query = ""
  var body: some View {
    let off = shell.view.off ?? CoreView.OffListView()
    ScrollView {
      LazyVStack(alignment: .leading, spacing: 6) {
        PageHeading(text: "Off your mind")
        PaperSearch(text: $query).padding(.bottom, 8)
        ForEach(off.works, id: \.id) { row in work(row) }
        if !off.rest.isEmpty {
          VStack(spacing: 4) { ForEach(off.rest, id: \.id) { row in rest(row) } }.padding(.top, 4)
        }
        if let none = off.none { Text(none.en).font(Face.text(15)).foregroundStyle(Ink.muted).padding(.vertical, 20) }
        if let more = off.more { Button(more.says.en) { shell.act(["do": "show_more", "of": ["list": "off", "query": query]]) }.buttonStyle(ObLink()).frame(maxWidth: .infinity) }
        if let older = off.older { Button(older.en) { shell.act(["do": "show_more", "of": ["list": "off", "query": query]]) }.buttonStyle(ObLink()).frame(maxWidth: .infinity) }
        if off.loading { ProgressView().frame(maxWidth: .infinity).padding(.vertical, 12) }
      }
      .padding(.horizontal, 16).padding(.bottom, 120)
      .frame(maxWidth: 760).frame(maxWidth: .infinity)
    }
    .background(Ink.bg)
    .scrollDismissesKeyboard(.interactively)
    .pushedPills(center: { EmptyView() }, trailing: { EmptyView() })
    .onChange(of: query) { _, text in shell.focus(.off(query: text)) }
  }
  /** A card with its agent: who is on it, the title, its status line, how long. */
  private func work(_ row: CoreView.OffWorkView) -> some View {
    Button { shell.focus(.card(cardId: row.id)) } label: {
      HStack(alignment: .center, spacing: 10) {
        AgentMark(from: row.from, size: 28)
        VStack(alignment: .leading, spacing: 2) {
          HStack(spacing: 4) {
            if row.starred { StarMark().frame(width: 14, height: 14).accessibilityHidden(true) }
            Text(row.title).font(Face.text(15, .semibold)).foregroundStyle(Ink.fg).lineLimit(2).multilineTextAlignment(.leading)
          }
          (Text(row.from.name).font(Face.text(12, .semibold)).foregroundColor(Ink.fg) + Text(" · \(row.label.en)").font(Face.text(12)).foregroundColor(Ink.muted)).lineLimit(1)
        }
        Spacer(minLength: 6)
        if row.atMs > 0 { Text(offAgo(row.atMs)).font(Face.text(11)).foregroundStyle(Ink.faint).frame(maxHeight: .infinity, alignment: .top) }
      }
      .padding(.horizontal, 14).padding(.vertical, 11)
      .background(RoundedRectangle(cornerRadius: 13, style: .continuous).fill(Ink.surface))
      .overlay(RoundedRectangle(cornerRadius: 13, style: .continuous).strokeBorder(Ink.fg.opacity(0.55), lineWidth: 1.5))
      .contentShape(RoundedRectangle(cornerRadius: 13))
    }
    .buttonStyle(.plain)
    .accessibilityLabel(row.says.en)
  }
  /** A put-off or closed card, small and quiet: its mark, the title, when. */
  private func rest(_ row: CoreView.OffRowView) -> some View {
    Button { shell.focus(.card(cardId: row.id)) } label: {
      HStack(spacing: 8) {
        OffMark(mark: row.mark).frame(width: 13, height: 13).accessibilityHidden(true)
        if row.starred { StarMark().frame(width: 13, height: 13).accessibilityHidden(true) }
        Text(row.title.isEmpty ? "(no title)" : row.title).font(Face.text(13, .medium)).foregroundStyle(Ink.muted)
          .strikethrough(row.mark == "shred", color: Ink.faint)
          .lineLimit(1).truncationMode(.tail)
          .frame(maxWidth: .infinity, alignment: .leading)
        if let until = row.until { Text(until.en).font(Face.text(11)).foregroundStyle(Ink.faint) }
        else if row.atMs > 0 { Text(offAgo(row.atMs)).font(Face.text(11)).foregroundStyle(Ink.faint) }
      }
      .padding(.horizontal, 12).frame(minHeight: 36)
      .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(Ink.surface))
      .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(Ink.lineStrong, lineWidth: 1))
      .contentShape(Rectangle())
    }
    .buttonStyle(.plain)
    .accessibilityLabel("\(row.title), \(row.says.en)")
  }
}
/** The mark of a card off the Desk: put off the three Z, shredded the shredder, withdrawn the return arrow, else the tick. */
struct OffMark: View {
  let mark: String
  var body: some View {
    switch mark {
    case "snooze": Sketch("snooze", color: Ink.stampLater)
    case "shred", "withdraw": Sketch(mark, color: Ink.faint)
    default: Sketch("tick", color: Ink.accent)
    }
  }
}

// ---- the note ----------------------------------------------------------------------------------------------------

/**
 * The note: a full page of yellow paper over the list he came from (the shell lays it there, the tab pill stays).
 * From the top: the words, the files, and one row: paperclip and bin at the left, "To: …" and the envelope at the
 * right. The words are the corner's note (the core's); older notes of other devices stand below as small slips.
 */
struct NotesScreen: View {
  @EnvironmentObject var shell: AppShell
  @State private var text = ""
  @State private var attachments: [String] = []
  @State private var loadedId: String?
  /** The keep that waits for him to pause typing (the web's 600 ms). */
  @State private var pause: Task<Void, Never>?
  @FocusState private var focused: Bool
  private var notes: CoreView.NotesView { shell.view.notes }
  private var corner: CoreView.NoteView? { notes.corner.flatMap { id in notes.rows.first { $0.id == id } } }
  var body: some View {
    let empty = text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && attachments.isEmpty
    GeometryReader { geo in ScrollView {
      VStack(alignment: .leading, spacing: 10) {
        TextField("", text: $text, prompt: Text("Write a note…").foregroundStyle(Ink.noteInk.opacity(0.45)), axis: .vertical)
          .font(Face.text(18)).foregroundStyle(Ink.noteInk).lineSpacing(4)
          .focused($focused)
          .accessibilityIdentifier("note-text")
          // (the paper below the words is the field's too: a vertical field's empty lines take no tap, so its
          //  height is a frame around it that starts writing)
          .frame(maxWidth: .infinity, minHeight: 100, alignment: .topLeading)
          .contentShape(Rectangle())
          .onTapGesture { focused = true }
          .padding(.top, 8)
        if !attachments.isEmpty {
          ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
              ForEach(attachments, id: \.self) { id in
                ZStack(alignment: .topTrailing) {
                  NoteThumb(id: id)
                  Button { attachments.removeAll { $0 == id } } label: { Image(systemName: "xmark.circle.fill").foregroundStyle(Ink.noteInk) }
                    .buttonStyle(.plain).offset(x: 6, y: -6).accessibilityLabel("Remove")
                }
              }
            }.padding(.top, 6).padding(.trailing, 8)
          }
        }
        Spacer(minLength: 12)
        HStack(spacing: 2) {
          AttachMenu(ids: $attachments) { Image(systemName: "paperclip").font(.system(size: 19, weight: .medium)).frame(width: 44, height: 44) }
          Button { bin() } label: { Image(systemName: "trash").font(.system(size: 17)).frame(width: 44, height: 44) }
            .buttonStyle(.plain).accessibilityLabel("Delete Note").disabled(corner == nil && empty)
          Spacer(minLength: 4)
          recipient
          send.disabled(empty || notes.to == nil).opacity(empty || notes.to == nil ? 0.5 : 1)
        }
        .foregroundStyle(Ink.noteInk)
        if let none = notes.noCrown { Text(none.en).font(Face.text(13)).foregroundStyle(Ink.noteInk.opacity(0.7)) }
        // the notes besides the corner's (written on other devices): small slips
        ForEach(notes.rows.filter { $0.id != notes.corner }, id: \.id) { note in
          VStack(alignment: .leading, spacing: 8) {
            Text(note.text).font(Face.text(15)).foregroundStyle(Ink.noteInk).lineLimit(4)
            HStack(spacing: 14) {
              if let says = notes.sendSays { Button(says.en) { shell.act(["do": "send_note", "note_id": note.id]) }.font(Face.text(14, .semibold)) }
              Spacer()
              Button { shell.act(["do": "delete_note", "note_id": note.id]) } label: { Image(systemName: "trash") }.accessibilityLabel("Delete Note")
            }.buttonStyle(.plain).foregroundStyle(Ink.noteInk)
          }
          .padding(12).frame(maxWidth: .infinity, alignment: .leading)
          .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(Color.white.opacity(0.35)))
        }
      }
      .padding(.horizontal, 20).padding(.top, 14).padding(.bottom, 100)
      .frame(minHeight: geo.size.height, alignment: .top)
      .frame(maxWidth: 760).frame(maxWidth: .infinity, alignment: .leading)
    } }
    .scrollDismissesKeyboard(.interactively)
    .scrollContentBackground(.hidden)
    .background(Ink.noteYellow.ignoresSafeArea())
    .onAppear { load() }
    // (a note written on another device fills an empty page; the one this page made is the core's to follow)
    .onChange(of: notes.corner) { _, _ in if text.isEmpty && attachments.isEmpty { load() } }
    .onChange(of: text) { _, _ in later() }
    .onChange(of: attachments) { _, _ in later() }
    .onDisappear { keep(); focused = false; hideKeyboard() }
  }
  private func load() {
    guard let c = corner, loadedId != c.id else { return }
    loadedId = c.id; text = c.text; attachments = c.files.map(\.fileId)
  }
  /** The note as it stands, for the core to keep (`KeepNote`: it decides whether that makes, changes or bins one). */
  private func written(send: Bool = false, to: String? = nil) -> [String: Any] {
    var action: [String: Any] = ["do": "keep_note", "text": text, "attachments": attachments]
    if let id = loadedId { action["note_id"] = id }
    if send { action["send"] = true; if let to { action["to"] = to } }
    return action
  }
  /** Saved by itself: a moment after he stops typing, and when he leaves the page. */
  private func later() {
    pause?.cancel()
    pause = Task { @MainActor in
      try? await Task.sleep(nanoseconds: 600_000_000)
      if !Task.isCancelled { keep() }
    }
  }
  private func keep() { pause?.cancel(); shell.act(written()) }
  private func bin() {
    pause?.cancel()
    if let id = loadedId ?? notes.corner { shell.act(["do": "delete_note", "note_id": id]) }
    text = ""; attachments = []; loadedId = nil; focused = false; hideKeyboard()
  }
  private func sendTo(_ to: String?) {
    pause?.cancel()
    shell.act(written(send: true, to: to))
    text = ""; attachments = []; loadedId = nil
    pause?.cancel()
    focused = false; hideKeyboard()
  }
  /** "To: …": the crowned session of the team in view, the one the note goes to. */
  private var recipient: some View {
    HStack(spacing: 4) {
      Text("To:").foregroundStyle(Ink.noteInk.opacity(0.7))
      Text(notes.to?.name ?? "No Session").fontWeight(.semibold).foregroundStyle(Ink.noteInk).lineLimit(1).truncationMode(.middle)
    }
    .lineLimit(1).font(Face.text(15)).padding(.horizontal, 14).frame(minHeight: 40)
    .glass(Capsule(), interactive: true).contentShape(Capsule())
    .accessibilityLabel("Recipient: \(notes.to?.name ?? "none")")
  }
  private var send: some View {
    Button { sendTo(nil) } label: { NoteEnvelope().frame(width: 58, height: 44).contentShape(Rectangle()) }
      .buttonStyle(.plain).accessibilityLabel(notes.sendSays?.en ?? "Send")
  }
}

/** A file on the note: a picture as a small square, any other file by its name. */
struct NoteThumb: View {
  @EnvironmentObject var shell: AppShell
  let id: String
  var body: some View {
    let file = shell.view.files[id]
    Group {
      if file?.mediaType.hasPrefix("image/") == true, let data = shell.files[id], let image = UIImage(data: data) {
        Image(uiImage: image).resizable().scaledToFill().frame(width: 72, height: 72).clipShape(RoundedRectangle(cornerRadius: 8))
          .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(Ink.noteInk.opacity(0.4), lineWidth: 1))
      } else {
        VStack(spacing: 4) {
          Sketch("clip", color: Ink.noteInk).frame(width: 18, height: 18)
          Text(file?.name ?? "File").font(Face.text(11)).foregroundStyle(Ink.noteInk).lineLimit(2).multilineTextAlignment(.center)
          if let percent = file?.percent { ProgressView(value: Double(percent), total: 100).tint(Ink.noteInk).padding(.horizontal, 8) }
        }
        .frame(width: 72, height: 72).background(RoundedRectangle(cornerRadius: 8).fill(Color.white.opacity(0.5)))
      }
    }
    .task(id: id) { if file?.mediaType.hasPrefix("image/") == true && shell.files[id] == nil { shell.act(["do": "open_file", "attachment_id": id, "save": false]) } }
  }
}

/** The note's send control: an envelope drawn with the board's pen on cream paper, the crown its seal on the flap. */
struct NoteEnvelope: View {
  var body: some View {
    ZStack(alignment: .topLeading) {
      Canvas { ctx, size in
        let k = min(size.width / 58, size.height / 44)
        let t = CGAffineTransform(a: k, b: 0, c: 0, d: k, tx: (size.width - 58 * k) / 2 + k, ty: (size.height - 44 * k) / 2 + 2 * k)
        func pt(_ x: CGFloat, _ y: CGFloat) -> CGPoint { CGPoint(x: x, y: y) }
        var paper = Path()
        paper.move(to: pt(3, 7.4)); paper.addLine(to: pt(52.6, 6.6)); paper.addLine(to: pt(53, 36.4)); paper.addLine(to: pt(3.4, 37)); paper.closeSubpath()
        var edge = Path()
        edge.move(to: pt(3, 7.4))
        edge.addQuadCurve(to: pt(52.6, 6.6), control: pt(28, 6.4))
        edge.addQuadCurve(to: pt(53, 36.4), control: pt(53.4, 22))
        edge.addQuadCurve(to: pt(3.4, 37), control: pt(28, 37.4))
        edge.addQuadCurve(to: pt(3, 7.4), control: pt(2.6, 22))
        var flap = Path()
        flap.move(to: pt(3.4, 8))
        flap.addQuadCurve(to: pt(27.6, 24.2), control: pt(16, 18.6))
        flap.addQuadCurve(to: pt(52.4, 7.4), control: pt(40, 18))
        let ink = Color(hex: 0x1d1a12)
        ctx.fill(paper.applying(t), with: .color(Color(hex: 0xfdf2c4)))
        ctx.stroke(edge.applying(t), with: .color(ink), style: StrokeStyle(lineWidth: 1.6 * k, lineCap: .round, lineJoin: .round))
        ctx.stroke(flap.applying(t), with: .color(ink), style: StrokeStyle(lineWidth: 1.5 * k, lineCap: .round, lineJoin: .round))
      }
      GeometryReader { g in
        let k = min(g.size.width / 58, g.size.height / 44)
        PenMark("crown", color: Color(hex: 0x1d1a12)).frame(width: 26 * k, height: 19 * k)
          .rotationEffect(.degrees(-6))
          .offset(x: (g.size.width - 58 * k) / 2 + 15 * k, y: (g.size.height - 44 * k) / 2 + 16 * k)
      }
    }
    .rotationEffect(.degrees(-4))
    .accessibilityHidden(true)
  }
}

// ---- Settings ------------------------------------------------------------------------------------------------------

/** A group of rows as iOS Settings draws it: a caption, a sheet of paper with hairlines, a note under it. */
struct SetGroup<Content: View>: View {
  var header: String? = nil
  var footer: String? = nil
  @ViewBuilder var content: Content
  var body: some View {
    VStack(alignment: .leading, spacing: 7) {
      if let h = header { Text(h.uppercased()).font(Face.text(12, .semibold)).kerning(0.9).foregroundStyle(Ink.muted).padding(.horizontal, 16) }
      VStack(spacing: 0) { content }
        .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(Ink.surface))
        .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(Ink.line))
        .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
      if let f = footer { Text(f).font(Face.text(13)).foregroundStyle(Ink.muted).padding(.horizontal, 16).fixedSize(horizontal: false, vertical: true) }
    }
  }
}
/** The hairline between two rows, from under the icon to the edge. */
struct SetRule: View {
  var inset: CGFloat = 56
  var body: some View { Rectangle().fill(Ink.line).frame(height: 1).padding(.leading, inset) }
}
/** One row: a drawn icon, the title, a detail on the right, a chevron or a tick. */
struct SetRow<Icon: View>: View {
  let title: String
  var detail: String? = nil
  var tint: Color = Ink.fg
  var chevron = false
  var ticked = false
  @ViewBuilder var icon: Icon
  var body: some View {
    HStack(spacing: 14) {
      icon.frame(width: 26, height: 26)
      Text(title).font(Face.text(17, .medium)).foregroundStyle(tint).lineLimit(1)
      Spacer(minLength: 8)
      if let d = detail { Text(d).font(Face.text(15)).foregroundStyle(Ink.muted).lineLimit(1) }
      if chevron { Image(systemName: "chevron.right").font(.system(size: 13, weight: .semibold)).foregroundStyle(Ink.faint) }
      if ticked { Sketch("tick", color: Ink.accent).frame(width: 20, height: 20) }
    }
    .padding(.horizontal, 16).frame(minHeight: 52)
    .contentShape(Rectangle())
  }
}
/** Sealed dictation on this device: the switch, and the enclave's state as the core says it (a dot and a line). */
private struct DictationRow: View {
  @EnvironmentObject var shell: AppShell
  var body: some View {
    let d = shell.view.dictation
    HStack(spacing: 14) {
      Sketch("mic").frame(width: 26, height: 26)
      VStack(alignment: .leading, spacing: 2) {
        Text("Dictation").font(Face.text(17, .medium)).foregroundStyle(Ink.fg).lineLimit(1)
        HStack(spacing: 6) {
          Circle().fill(d.statusKind == "connected" ? Color(red: 0.18, green: 0.62, blue: 0.36) : d.statusKind == "failed" ? Ink.urgCritical : Ink.faint).frame(width: 7, height: 7)
          Text(d.status?.en ?? "").font(Face.text(13)).foregroundStyle(Ink.muted).lineLimit(2)
        }
      }
      Spacer(minLength: 8)
      Toggle("Dictation", isOn: Binding(get: { d.on }, set: { on in shell.act(["do": "dictate", "step": ["step": "switch", "on": on]]) }))
        .labelsHidden().tint(Ink.accent)
    }
    .padding(.horizontal, 16).padding(.vertical, 8).frame(minHeight: 52)
    .accessibilityElement(children: .combine)
  }
}

/** A field inside a group: one outlined line on the paper. */
struct SetField: View {
  let title: String
  @Binding var text: String
  var secure = false
  var body: some View {
    Group { if secure { SecureField(title, text: $text) } else { TextField(title, text: $text).textInputAutocapitalization(.never).autocorrectionDisabled() } }
      .font(Face.text(16)).padding(.horizontal, 12).frame(minHeight: 44)
      .background(RoundedRectangle(cornerRadius: 10).strokeBorder(Ink.lineStrong))
      .padding(.horizontal, 16).padding(.vertical, 6)
  }
}
/** A device's drawing by the core's icon name. */
private struct DeviceIcon: View {
  let icon: String
  var body: some View { if icon == "phone" { PenMark("draw:phone", color: Ink.fg) } else { Sketch(icon.isEmpty ? "laptop" : icon) } }
}

/**
 * Settings as the iPhone drew it before: a list (the invitations as two tiles, then a row per page with what it holds
 * on the right) and a page of its own for Agents, Teams, Devices, Account, Theme, Data and Security
 * (`Screen.settings(page:)`). What each row and page says is the core's; which page is open is the stack's.
 */
struct SettingsScreen: View {
  @EnvironmentObject var shell: AppShell
  let page: String
  @AppStorage("trommi-theme") private var theme = "system"
  @State private var teamName = ""
  @State private var currentPassword = ""
  @State private var nextPassword = ""
  @State private var accountEmail = ""
  @State private var kitWords = ""
  @State private var deletingTeam: CoreView.TeamTab?
  @State private var deleting = false
  @State private var removing: CoreView.AgentView?
  /** The page a web address names (`/settings/<page>`); "" for the list. Navigation only. */
  static func page(of part: String?) -> String {
    switch part {
    case "sessions": return "agents"
    case "agents", "teams", "devices", "account", "theme", "data", "security", "proof": return part ?? ""
    default: return ""
    }
  }
  private var title: String {
    switch page {
    case "agents": "Agents"
    case "teams": "Teams"
    case "devices": "Devices"
    case "account": "Account"
    case "theme": "Theme"
    case "data": "Data"
    case "security": "Security"
    case "proof": "Self Test"
    case "logout": shell.view.settings.logout.title.en
    default: page.hasPrefix("device:") ? (device?.name ?? "Devices") : "Settings"
    }
  }
  /** The device a page "device:<id>" shows: one that is in, or one removed. */
  private var device: CoreView.DeviceRow? {
    let id = String(page.dropFirst("device:".count)), s = shell.view.settings
    return (s.devices + s.removed).first { $0.id == id }
  }
  var body: some View {
    let s = shell.view.settings
    ScrollView {
      VStack(alignment: .leading, spacing: 26) {
        PageHeading(text: title).accessibilityIdentifier("settings-" + (page.isEmpty ? "list" : page))
        if let problem = shell.view.account.problem { Text(problem.en).font(Face.text(15)).foregroundStyle(Ink.urgCritical) }
        if shell.view.account.step == "working" { ProgressView().frame(maxWidth: .infinity) }
        switch page {
        case "agents": agents(s)
        case "teams": teams
        case "devices": devices(s); pushGroup
        case "account": account(s)
        case "theme": themeGroup
        case "data": dataGroup(s)
        case "security": SecurityPage(security: s.security)
        case "proof": SelfTestPage()
        case "logout": LogoutPage().padding(.horizontal, -16)
        case let p where p.hasPrefix("device:"): DevicePage(device: device)
        default: home(s)
        }
      }
      .padding(.horizontal, 16).padding(.bottom, 90)
      .frame(maxWidth: 680).frame(maxWidth: .infinity)
    }
    .background(Ink.bg.ignoresSafeArea())
    .scrollDismissesKeyboard(.interactively)
    .pushedPills(center: { EmptyView() }, trailing: { EmptyView() })
    .sheet(isPresented: Binding(get: { deletingTeam != nil }, set: { if !$0 { deletingTeam = nil } })) {
      if let team = deletingTeam { TeamDeleteSheet(teamId: team.id, name: team.name.en).environmentObject(shell) }
    }
    .confirmationDialog("Delete Your Account?", isPresented: $deleting, titleVisibility: .visible) {
      Button("Delete account", role: .destructive) { shell.account(["step": "delete_account", "password": currentPassword]); currentPassword = "" }
    } message: { Text("Your account and everything Trommi keeps for it on the hub are deleted. This cannot be undone.") }
    .confirmationDialog(removing?.name.en ?? "", isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }), titleVisibility: .visible) {
      Button("Remove from This Account", role: .destructive) { if let agent = removing { shell.account(["step": "remove_devices", "device_ids": agent.devices]) }; removing = nil }
    } message: { Text(removing?.remove?.en ?? "") }
  }

  /** A row that opens a page of Settings. */
  private func link<Icon: View>(_ to: String, _ title: String, detail: String?, @ViewBuilder icon: () -> Icon) -> some View {
    Button { shell.focus(.settings(page: to)) } label: { SetRow(title: title, detail: detail, chevron: true) { icon() } }
      .buttonStyle(.plain).accessibilityIdentifier("settings-row-" + to)
  }

  /** The list: the invitations, then a row per page. */
  @ViewBuilder private func home(_ s: CoreView.SettingsView) -> some View {
    invites
    SetGroup {
      link("agents", "Agents", detail: s.agents?.en) { Sketch("terminal") }
      SetRule()
      link("teams", "Teams", detail: s.teams?.en) { Sketch("desk") }
      SetRule()
      link("devices", "Devices", detail: s.devicesSays?.en) { PenMark("draw:phone", color: Ink.fg) }
      SetRule()
      link("account", "Account", detail: shell.isDemo ? "Demo" : s.account ?? s.login?.email) { Sketch("key") }
    }
    SetGroup {
      link("theme", "Theme", detail: theme == "light" ? "Light" : theme == "dark" ? "Dark" : "System") { Sketch(theme == "dark" ? "moon" : "sun") }
      if shell.view.dictation.available {
        SetRule()
        DictationRow()
      }
    }
    SetGroup {
      link("data", "Data", detail: nil) { Image(systemName: "chart.bar").font(.system(size: 19)).foregroundStyle(Ink.fg) }
      SetRule()
      link("security", "Security", detail: "Keys, devices, self test") { Sketch("lock") }
      SetRule()
      // (App Review 5.1.1(i): the privacy policy reachable inside the app)
      Link(destination: URL(string: "https://trommi.com/privacy")!) { SetRow(title: "Privacy Policy", chevron: true) { Image(systemName: "hand.raised").font(.system(size: 20)).foregroundStyle(Ink.fg) } }.buttonStyle(.plain)
    }
    SetGroup {
      Button { shell.focus(.settings(page: "logout")); shell.account(["step": "read_account"]) } label: { SetRow(title: s.logout.go.en, tint: Ink.urgCritical, chevron: true) { Sketch("logout", color: Ink.urgCritical) } }
        .buttonStyle(.plain).accessibilityIdentifier("settings-logout")
    }
    roomLine(s)
  }

  /** The account: who, its ways in, its e-mail, deleting it. */
  @ViewBuilder private func account(_ s: CoreView.SettingsView) -> some View {
    SetGroup(header: "Account") {
      SetRow(title: s.account ?? s.login?.email ?? "Account", detail: nil) { Sketch(s.account == nil ? "key" : "letter") }
      if let login = s.login {
        SetRule()
        Text("Account ID \(login.account)").font(Face.mono(12)).foregroundStyle(Ink.muted).textSelection(.enabled)
          .padding(.horizontal, 16).padding(.vertical, 10).frame(maxWidth: .infinity, alignment: .leading)
      }
    }
    SetGroup(header: "Ways In", footer: "Changing how your account opens asks for your current password again: being logged in here is not enough.") {
      SetField(title: "Current password", text: $currentPassword, secure: true)
      SetField(title: "New password", text: $nextPassword, secure: true)
      SetRule(inset: 16)
      way("Change password", "lock") { shell.account(["step": "set_password", "unlock": ["by": "password", "password": currentPassword], "next": nextPassword]); currentPassword = ""; nextPassword = "" }
      SetRule()
      way("New Emergency Kit", "page") { shell.account(["step": "new_kit", "unlock": ["by": "password", "password": currentPassword]]); currentPassword = "" }
      SetRule()
      way("Replace recovery code", "key") { shell.account(["step": "replace_recovery_code", "unlock": ["by": "password", "password": currentPassword]]); currentPassword = "" }
    }
    SetGroup(header: "E-mail", footer: "With the twelve words of your Emergency Kit.") {
      SetField(title: "E-mail", text: $accountEmail)
      SetField(title: "Emergency Kit words", text: $kitWords, secure: true)
      way("Set e-mail", "letter") { shell.account(["step": "set_email", "email": accountEmail, "words": kitWords]); kitWords = "" }
    }
    SetGroup {
      Button { shell.focus(.settings(page: "logout")); shell.account(["step": "read_account"]) } label: { SetRow(title: s.logout.go.en, tint: Ink.urgCritical, chevron: true) { Sketch("logout", color: Ink.urgCritical) } }
        .buttonStyle(.plain).accessibilityIdentifier("settings-logout")
    }
    SetGroup(footer: "Deletes your account and everything Trommi keeps for it. Your current password above confirms it. This cannot be undone.") {
      Button { deleting = true } label: { SetRow(title: "Delete account…", tint: Ink.urgCritical) { Sketch("bin", color: Ink.urgCritical) } }.buttonStyle(.plain)
    }
  }
  private var pushGroup: some View {
    SetGroup(header: "Push on This Device") {
      ForEach(Array(shell.view.settings.pushChoices.enumerated()), id: \.element.level) { i, c in
        if i > 0 { SetRule() }
        pushRow(c)
      }
    }
  }
  private var themeGroup: some View {
    SetGroup(header: "Theme") {
      themeRow("system", "System", "sun"); SetRule()
      themeRow("light", "Light", "sun"); SetRule()
      themeRow("dark", "Dark", "moon")
    }
  }
  private func dataGroup(_ s: CoreView.SettingsView) -> some View {
    SetGroup(header: "Data", footer: s.data?.says?.en) {
      Button { shell.account(["step": "read_usage"]) } label: { SetRow(title: "Read hub data", chevron: true) { Image(systemName: "chart.bar").font(.system(size: 19)).foregroundStyle(Ink.fg) } }.buttonStyle(.plain)
      ForEach(s.data?.rows ?? [], id: \.id) { row in SetRule(); SetRow(title: row.label.en, detail: row.value.en) { EmptyView() } }
    }
  }
  private func roomLine(_ s: CoreView.SettingsView) -> some View {
    VStack(spacing: 4) {
      if let room = s.room { Text("room \(room) · key epoch \(s.keyEpoch)").font(Face.mono(12)).foregroundStyle(Ink.faint).textSelection(.enabled) }
      if let hub = s.hub { Text(hub).font(Face.mono(12)).foregroundStyle(Ink.faint) }
    }.frame(maxWidth: .infinity).multilineTextAlignment(.center)
  }

  /** The invitations as two equal tiles, then what an invite in progress shows (AccountInvite). */
  private var invites: some View {
    VStack(alignment: .leading, spacing: 12) {
      tile("Pair a device", "A phone or another computer scans the code. Made when you ask, works once.", "draw:phone") { shell.account(["step": "invite_device"]) }
      tile("Approve a computer", "Claude Code or Codex on one of your computers joins with one command.", "sketch:terminal") { shell.account(["step": "approve_computer", "label": ""]) }
      Text("Nobody is added before you confirm.").font(Face.text(13)).foregroundStyle(Ink.muted).padding(.horizontal, 16)
      VStack(alignment: .leading, spacing: 12) { AccountInvite() }
    }
  }
  private func tile(_ title: String, _ line: String, _ mark: String, _ go: @escaping () -> Void) -> some View {
    Button(action: go) {
      HStack(spacing: 14) {
        PenMark(mark, color: Ink.fg).frame(width: 30, height: 30).frame(width: 56, height: 56)
          .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Ink.sunken))
          .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(Ink.fg, lineWidth: 1.5))
        VStack(alignment: .leading, spacing: 4) {
          Text(title).font(Face.text(17, .semibold)).foregroundStyle(Ink.fg)
          Text(line).font(Face.text(14)).foregroundStyle(Ink.muted).fixedSize(horizontal: false, vertical: true).multilineTextAlignment(.leading)
        }
        Spacer(minLength: 0)
      }
      .padding(14).frame(maxWidth: .infinity, alignment: .leading)
      .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(Ink.surface))
      .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(Ink.line))
      .contentShape(Rectangle())
    }
    .buttonStyle(.plain)
    .accessibilityLabel(title)
  }
  private func way(_ title: String, _ icon: String, _ go: @escaping () -> Void) -> some View {
    Button(action: go) { SetRow(title: title, tint: Ink.accent) { Sketch(icon, color: Ink.accent) } }.buttonStyle(.plain)
  }
  private func pushRow(_ c: CoreView.PushChoice) -> some View {
    let on = (shell.view.settings.push?.level ?? "off") == c.level
    return Button { shell.act(["do": "push", "level": c.level]) } label: { SetRow(title: c.title.en, detail: c.detail?.en, ticked: on) { Sketch(c.icon) } }
      .buttonStyle(.plain).accessibilityAddTraits(on ? .isSelected : [])
  }
  private func themeRow(_ key: String, _ title: String, _ icon: String) -> some View {
    Button { theme = key } label: { SetRow(title: title, ticked: theme == key) { Sketch(icon) } }
      .buttonStyle(.plain).accessibilityAddTraits(theme == key ? .isSelected : [])
  }

  /** The person's devices: each its drawing, its name, its fingerprint, whether it is here. */
  private func devices(_ s: CoreView.SettingsView) -> some View {
    SetGroup(header: "Devices", footer: "The fingerprint comes from the device's own key: it must look the same on every device.") {
      ForEach(Array(s.devices.enumerated()), id: \.element.id) { i, device in
        if i > 0 { SetRule() }
        HStack(spacing: 14) {
          Button { shell.focus(.settings(page: "device:" + device.id)) } label: {
            HStack(spacing: 14) {
              DeviceIcon(icon: device.icon).frame(width: 26, height: 26)
              VStack(alignment: .leading, spacing: 2) {
                Text(device.name).font(Face.text(17, .medium)).foregroundStyle(device.active ? Ink.fg : Ink.muted).lineLimit(1)
                Text(device.fingerprint).font(Face.mono(12)).foregroundStyle(Ink.muted).lineLimit(1)
                if let seen = device.seen { Text(seen.en).font(Face.text(12)).foregroundStyle(Ink.muted) }
              }
            }.contentShape(Rectangle())
          }.buttonStyle(.plain).accessibilityIdentifier("device-row-" + device.id)
          Spacer(minLength: 8)
          if device.active { Circle().fill(device.online ? Ink.stDone : Ink.lineStrong).frame(width: 8, height: 8) }
          Text(device.says.en).font(Face.text(15)).foregroundStyle(Ink.muted).lineLimit(1)
          if !device.thisDevice {
            Menu { Button("Remove", role: .destructive) { shell.account(["step": "remove_devices", "device_ids": [device.id]]) } } label: {
              Image(systemName: "ellipsis").font(.system(size: 16, weight: .semibold)).foregroundStyle(Ink.fg).frame(width: 36, height: 36)
            }.accessibilityLabel("More for \(device.name)")
          }
        }
        .padding(.horizontal, 16).padding(.vertical, 8).frame(minHeight: 60)
      }
      if let remove = s.removeUnfinished {
        SetRule()
        Button { shell.account(["step": "remove_devices", "device_ids": s.unfinished]) } label: { SetRow(title: remove.en, tint: Ink.urgCritical) { Sketch("logout", color: Ink.urgCritical) } }.buttonStyle(.plain)
      }
      ForEach(s.removed, id: \.id) { device in
        SetRule()
        SetRow(title: device.name, detail: device.says.en, tint: Ink.muted) { Sketch("archive", color: Ink.muted) }
      }
    }
  }

  /** Each agent as a group, its main sessions under it as the branches of a tree. */
  @ViewBuilder private func agents(_ s: CoreView.SettingsView) -> some View {
    VStack(alignment: .leading, spacing: 14) {
      Text("AGENTS").font(Face.text(12, .semibold)).kerning(0.9).foregroundStyle(Ink.muted).padding(.horizontal, 16)
      if let lead = s.agentsLead { Text(lead.en).font(Face.text(15)).foregroundStyle(Ink.muted).padding(.horizontal, 16) }
      ForEach(s.agentList, id: \.key) { agent in
        SetGroup {
          HStack(spacing: 12) {
            Sketch("terminal").frame(width: 24, height: 24).frame(width: 40, height: 40)
              .background(RoundedRectangle(cornerRadius: 10).fill(Ink.surface)).overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Ink.fg, lineWidth: 1.5))
            VStack(alignment: .leading, spacing: 3) {
              Text(agent.name.en).font(Face.display(17, .bold)).foregroundStyle(Ink.fg).lineLimit(1)
              HStack(spacing: 5) {
                Circle().fill(agent.online ? Ink.stDone : Ink.lineStrong).frame(width: 7, height: 7)
                Text("\(agent.state.en) · \(agent.count.en)").font(Face.text(12)).foregroundStyle(Ink.muted).lineLimit(1)
              }
            }
            Spacer(minLength: 0)
            if agent.remove != nil {
              Menu { Button("Remove from This Account…", role: .destructive) { removing = agent } } label: {
                Image(systemName: "ellipsis").font(.system(size: 17, weight: .semibold)).foregroundStyle(Ink.fg).frame(width: 40, height: 40)
              }.accessibilityLabel("More for \(agent.name.en)")
            }
          }
          .padding(.horizontal, 14).padding(.vertical, 12).background(Ink.sunken)
          if let usage = agent.usage { UsageDrawing(usage: usage).padding(16).frame(maxWidth: .infinity, alignment: .leading); SetRule(inset: 16) }
          ForEach(Array(agent.sessions.enumerated()), id: \.element.id) { i, session in
            Button { shell.focus(.session(sessionId: session.id)) } label: { branch(session, last: i == agent.sessions.count - 1) }.buttonStyle(.plain)
          }
        }
      }
      if s.agentList.isEmpty {
        SetGroup { Text("No agent yet.").font(Face.text(15)).foregroundStyle(Ink.muted).padding(16).frame(maxWidth: .infinity, alignment: .leading) }
      }
    }
  }
  private func branch(_ b: CoreView.AgentSessionView, last: Bool) -> some View {
    HStack(spacing: 12) {
      ZStack(alignment: .topLeading) {
        Rectangle().fill(Ink.lineStrong).frame(width: 1.4).frame(maxHeight: last ? 30 : .infinity, alignment: .top)
        Rectangle().fill(Ink.lineStrong).frame(width: 12, height: 1.4).offset(y: 29)
      }.frame(width: 14, alignment: .leading).frame(maxHeight: .infinity, alignment: .top)
      AgentMark(mark: b.mark, size: 24, hue: Pen.hueFor(id: b.slug.isEmpty ? b.id : b.slug, mark: b.mark)).frame(width: 24, height: 24)
      VStack(alignment: .leading, spacing: 2) {
        Text(b.name.en).font(Face.text(16, .semibold)).foregroundStyle(Ink.fg).lineLimit(1)
        Text(b.folder.isEmpty ? b.team.en : "\(b.team.en) · \(b.folder)").font(Face.text(12)).foregroundStyle(Ink.muted).lineLimit(1).truncationMode(.tail)
      }
      Spacer(minLength: 4)
      if let helpers = b.helpersSays { Text(helpers.en).font(Face.text(12, .semibold)).foregroundStyle(Ink.muted).padding(.horizontal, 7).padding(.vertical, 3).overlay(Capsule().strokeBorder(Ink.lineStrong)) }
      Circle().fill(b.online ? Ink.stDone : Ink.lineStrong).frame(width: 8, height: 8).accessibilityLabel(b.state.en)
      Image(systemName: "chevron.right").font(.system(size: 13, weight: .semibold)).foregroundStyle(Ink.faint)
    }
    .padding(.leading, 34).padding(.trailing, 16).frame(height: 60).contentShape(Rectangle())
  }

  /** The teams: each opens its Desk; its "⋯" deletes it; a new one by name. */
  private var teams: some View {
    SetGroup(header: "Teams") {
      ForEach(Array(shell.view.sidebar.teams.enumerated()), id: \.element.id) { i, team in
        if i > 0 { SetRule() }
        HStack(spacing: 0) {
          Button { shell.focus(.desk, team: team.id) } label: { SetRow(title: team.name.en, detail: team.waiting > 0 ? "\(team.waiting)" : nil, chevron: true) { Sketch("desk") } }.buttonStyle(.plain)
          if let p = shell.view.settings.deleting.first(where: { $0.teamId == team.id }) {
            ProgressView(value: Double(p.done), total: Double(max(p.total, 1))).frame(width: 44).padding(.trailing, 12)
          }
          Menu { Button("Delete \(team.name.en)…", role: .destructive) { deletingTeam = team }.disabled(shell.view.sidebar.teams.count < 2) } label: {
            Image(systemName: "ellipsis").font(.system(size: 16, weight: .semibold)).foregroundStyle(Ink.fg).frame(width: 40, height: 44)
          }.padding(.trailing, 6).accessibilityLabel("More for \(team.name.en)")
        }
      }
      SetRule()
      HStack(spacing: 10) {
        PenMark("ui:PLUS", color: Ink.accent).frame(width: 22, height: 22)
        TextField("Name of the new team", text: $teamName).font(Face.text(16)).onSubmit(addTeam)
        Button("Add team", action: addTeam).font(Face.text(15, .semibold)).foregroundStyle(Ink.accent).buttonStyle(.plain)
      }.padding(.horizontal, 16).frame(minHeight: 52)
    }
  }
  private func addTeam() { shell.act(["do": "add_team", "name": teamName]); teamName = "" }
}

// ---- Knowledge -----------------------------------------------------------------------------------------------------

/**
 * The page Knowledge of the team in view (`ViewModel.knowledge`, the core's KnowledgeView): the search, the filter All ·
 * Pinned, the entries in the core's order. A tap opens an entry (all of its text); its menu has Edit, Pin or Unpin and
 * Delete. "Add entry" opens the editor. The core keeps the entries, writes them and says every word.
 */
struct KnowledgeScreen: View {
  @EnvironmentObject var shell: AppShell
  @State private var query = ""
  @State private var open: String?
  @State private var editing: KnowledgeDraft?
  @State private var deleting: CoreView.KnowledgeRowView?
  var body: some View {
    ScrollView {
      LazyVStack(alignment: .leading, spacing: 12) {
        if let k = shell.view.knowledge {
          VStack(alignment: .leading, spacing: 4) {
            teamSwitch(k)
            HStack(alignment: .firstTextBaseline, spacing: 8) {
              PageHeading(text: k.title.en)
              Text("(\(k.count))").font(Face.display(26, .bold)).foregroundStyle(Ink.muted).accessibilityLabel(k.summary.en)
            }
            Text(k.lead.en).font(Face.text(15)).foregroundStyle(Ink.muted)
          }
          PaperSearch(text: $query, prompt: k.search.en)
          HStack(spacing: 10) {
            filters(k)
            Spacer(minLength: 0)
            Button { editing = KnowledgeDraft(teamId: k.teamId) } label: {
              Label(k.words.add.en, systemImage: "plus").font(Face.text(15, .semibold)).padding(.horizontal, 14).frame(minHeight: 40)
                .background(Capsule().stroke(Ink.lineStrong, lineWidth: 1))
            }
            .buttonStyle(.plain).disabled(k.full)
            .accessibilityIdentifier("knowledge-add")
          }
          if k.full { Text(k.words.full.en).font(Face.text(13)).foregroundStyle(Ink.muted) }
          if !k.rows.isEmpty {
            VStack(spacing: 0) {
              ForEach(Array(k.rows.enumerated()), id: \.element.id) { i, row in
                if i > 0 { Divider().overlay(Ink.line) }
                entry(row, k)
              }
            }
            .background(RoundedRectangle(cornerRadius: 18, style: .continuous).fill(Ink.surface))
            .overlay(RoundedRectangle(cornerRadius: 18, style: .continuous).stroke(Ink.line, lineWidth: 1))
          }
          if let none = k.none { Text(none.en).font(Face.text(15)).foregroundStyle(Ink.muted).padding(.vertical, 20).frame(maxWidth: .infinity).multilineTextAlignment(.center).accessibilityIdentifier("knowledge-none") }
        } else {
          Loading()
        }
      }
      .padding(.horizontal, 16).padding(.bottom, 120)
      .frame(maxWidth: 760).frame(maxWidth: .infinity)
    }
    .background(Ink.bg)
    .scrollDismissesKeyboard(.interactively)
    .pushedPills(center: { EmptyView() }, trailing: { EmptyView() })
    .onAppear { query = shell.view.knowledge?.query ?? "" }
    .onChange(of: query) { _, text in pick(query: text, filter: shell.view.knowledge?.filter ?? "") }
    .sheet(item: $editing) { draft in KnowledgeEditor(draft: draft, words: shell.view.knowledge?.words ?? CoreView.KnowledgeWordsView(), titleMax: Int(shell.view.knowledge?.titleMax ?? 80), textMax: Int(shell.view.knowledge?.textMax ?? 600)).presentationDetents([.medium, .large]) }
    .confirmationDialog(shell.view.knowledge?.words.deleteAsk.en ?? "", isPresented: Binding(get: { deleting != nil }, set: { if !$0 { deleting = nil } }), titleVisibility: .visible) {
      if let row = deleting { Button(shell.view.knowledge?.words.delete.en ?? "Delete", role: .destructive) { shell.act(["do": "knowledge_delete", "entry_id": row.id]); deleting = nil } }
    }
  }
  /** One entry: its drawing, the title, the text (two lines, all of it when open), who brought it and when, the pin. */
  private func entry(_ row: CoreView.KnowledgeRowView, _ k: CoreView.KnowledgeView) -> some View {
    let isOpen = open == row.id
    return Button { withAnimation(.easeOut(duration: 0.15)) { open = isOpen ? nil : row.id } } label: {
      HStack(alignment: .top, spacing: 12) {
        Sketch("book", color: Ink.muted).frame(width: 18, height: 18).frame(width: 32, height: 32)
          .background(RoundedRectangle(cornerRadius: 9).fill(Ink.sunken))
        VStack(alignment: .leading, spacing: 3) {
          Text(row.title).font(Face.text(16, .semibold)).foregroundStyle(Ink.fg).multilineTextAlignment(.leading)
          if !row.text.isEmpty { Text(row.text).font(Face.text(14)).foregroundStyle(Ink.fg).lineLimit(isOpen ? nil : 2).multilineTextAlignment(.leading) }
          Text(row.by.en).font(Face.text(12)).foregroundStyle(Ink.faint)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        if row.pinned { Sketch("pin", color: Ink.accent).frame(width: 16, height: 16).accessibilityLabel("Pinned") }
      }
      .padding(14).contentShape(Rectangle())
      .background(isOpen ? Ink.accentSoft : .clear)
    }
    .buttonStyle(.plain)
    .contextMenu {
      Button { editing = KnowledgeDraft(teamId: k.teamId, row: row) } label: { Label(k.words.edit.en, systemImage: "pencil") }
      Button { shell.act(["do": "knowledge_pin", "entry_id": row.id, "on": !row.pinned]) } label: { Label(row.pinned ? k.words.unpin.en : k.words.pin.en, systemImage: row.pinned ? "pin.slash" : "pin") }
      Button(role: .destructive) { deleting = row } label: { Label(k.words.delete.en, systemImage: "trash") }
    }
    .overlay(alignment: .bottomTrailing) {
      if isOpen {
        HStack(spacing: 8) {
          Button(k.words.edit.en) { editing = KnowledgeDraft(teamId: k.teamId, row: row) }
          Button(row.pinned ? k.words.unpin.en : k.words.pin.en) { shell.act(["do": "knowledge_pin", "entry_id": row.id, "on": !row.pinned]) }
          Button(k.words.delete.en, role: .destructive) { deleting = row }
        }
        .font(Face.text(13, .semibold)).buttonStyle(.bordered).controlSize(.small).padding(10)
      }
    }
    .padding(.bottom, isOpen ? 40 : 0)
    .accessibilityIdentifier("knowledge-\(row.id)")
  }
  /** Whose knowledge this is: the team's name; with two teams or more, a menu that shows another team's. */
  private func teamSwitch(_ k: CoreView.KnowledgeView) -> some View {
    let teams = shell.view.sidebar.teams
    return Menu {
      ForEach(teams, id: \.id) { team in
        Button { shell.show(.knowledge(query: query.trimmingCharacters(in: .whitespaces), filter: k.filter), team: team.id) } label: {
          Label { Text(team.name.en) } icon: { if team.id == k.teamId { Image(systemName: "checkmark") } }
        }
      }
    } label: {
      HStack(spacing: 6) {
        PenMark("sketch:desk", color: Ink.muted).frame(width: 16, height: 16)
        Text(k.team.en).font(Face.text(14, .semibold)).foregroundStyle(Ink.muted)
        if teams.count > 1 { Image(systemName: "chevron.down").font(.system(size: 10, weight: .semibold)).foregroundStyle(Ink.faint) }
      }
      .padding(.top, 10)
    }
    .disabled(teams.count < 2)
    .accessibilityLabel("Team: \(k.team.en)")
    .accessibilityIdentifier("knowledge-team")
  }
  /** The filter as one quiet segmented track: All · Pinned, each with its count (the core's choices). */
  private func filters(_ k: CoreView.KnowledgeView) -> some View {
    HStack(spacing: 4) {
      ForEach(k.filters, id: \.key) { f in
        Button { pick(query: query, filter: f.key) } label: {
          Text("\(f.label.en) \(f.count)").font(Face.text(15, f.on ? .semibold : .regular)).foregroundStyle(Ink.fg)
            .padding(.horizontal, 12).frame(minHeight: 36)
            .background { if f.on { Capsule().fill(Ink.surface).shadow(color: .black.opacity(0.08), radius: 2, y: 1) } }
            .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(f.on ? .isSelected : [])
        .accessibilityIdentifier("knowledge-filter-" + (f.key.isEmpty ? "all" : f.key))
      }
    }
    .padding(4).background(Capsule().fill(Ink.sunken))
  }
  private func pick(query: String, filter: String) {
    let screen = CoreView.Screen.knowledge(query: query.trimmingCharacters(in: .whitespaces), filter: filter)
    if case .knowledge = shell.path.last { shell.path[shell.path.count - 1] = screen; shell.show(screen) } else { shell.show(screen) }
  }
}

/** What the editor writes: a new entry of the team (no id), or one anew. */
struct KnowledgeDraft: Identifiable {
  let id = UUID()
  let teamId: String
  var entryId: String? = nil
  var title = ""
  var text = ""
  var pinned = false
  init(teamId: String, row: CoreView.KnowledgeRowView? = nil) {
    self.teamId = teamId
    if let row { entryId = row.id; title = row.title; text = row.text; pinned = row.pinned }
  }
}

/** The editor of an entry: its title, its text, pinned; Save sends it to the core (Action::KnowledgeSave). */
struct KnowledgeEditor: View {
  @EnvironmentObject var shell: AppShell
  @Environment(\.dismiss) private var dismiss
  @State var draft: KnowledgeDraft
  let words: CoreView.KnowledgeWordsView
  let titleMax: Int
  let textMax: Int
  var body: some View {
    NavigationStack {
      Form {
        TextField(words.titleHint.en, text: $draft.title).font(Face.text(17, .semibold))
          .onChange(of: draft.title) { _, t in if t.count > titleMax { draft.title = String(t.prefix(titleMax)) } }
        TextField(words.textHint.en, text: $draft.text, axis: .vertical).lineLimit(4...12).font(Face.text(15))
          .onChange(of: draft.text) { _, t in if t.count > textMax { draft.text = String(t.prefix(textMax)) } }
        Toggle(words.pin.en, isOn: $draft.pinned)
      }
      .toolbar {
        ToolbarItem(placement: .cancellationAction) { Button(words.cancel.en) { dismiss() } }
        ToolbarItem(placement: .confirmationAction) {
          Button(words.save.en) {
            var a: [String: Any] = ["do": "knowledge_save", "team_id": draft.teamId, "title": draft.title, "text": draft.text, "pinned": draft.pinned]
            if let id = draft.entryId { a["entry_id"] = id }
            shell.act(a)
            dismiss()
          }
          .disabled(draft.title.trimmingCharacters(in: .whitespaces).isEmpty)
        }
      }
    }
  }
}

// ---- Artifacts -----------------------------------------------------------------------------------------------------

struct ArtifactsScreen: View {
  @EnvironmentObject var shell: AppShell
  var body: some View {
    ScrollView {
      LazyVStack(alignment: .leading, spacing: 14) {
        PageHeading(text: "Artifacts")
        if let g = shell.view.gallery {
          if !g.kinds.isEmpty { kinds(g) } else { Text(g.lead.en).font(Face.text(15)).foregroundStyle(Ink.muted) }
          if let none = g.none { Text(none.en).font(Face.text(15)).foregroundStyle(Ink.muted).padding(.vertical, 20).frame(maxWidth: .infinity).accessibilityIdentifier("artifacts-none") }
        }
        ForEach(shell.view.artifacts ?? [], id: \.id) { artifact in ArtifactCard(artifact: artifact) }
      }
      .padding(.horizontal, 16).padding(.bottom, 40)
      .frame(maxWidth: 760).frame(maxWidth: .infinity)
    }
    .background(Ink.bg)
    .pushedPills(center: { EmptyView() }, trailing: { EmptyView() })
  }
  /** The filter as one quiet segmented track: All · Media · Pages (the core's choices). */
  private func kinds(_ g: CoreView.GalleryView) -> some View {
    HStack(spacing: 4) {
      ForEach(g.kinds, id: \.key) { k in
        Button { pick(k.key) } label: {
          Text(k.label.en).font(Face.text(16, k.on ? .semibold : .regular)).foregroundStyle(Ink.fg)
            .frame(maxWidth: .infinity, minHeight: 40)
            .background { if k.on { Capsule().fill(Ink.surface).shadow(color: .black.opacity(0.08), radius: 2, y: 1) } }
            .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(k.on ? .isSelected : [])
        .accessibilityIdentifier("artifacts-kind-" + (k.key.isEmpty ? "all" : k.key))
      }
    }
    .padding(4).background(Capsule().fill(Ink.sunken))
  }
  private func pick(_ kind: String) {
    let screen = CoreView.Screen.artifacts(kind: kind)
    if case .artifacts = shell.path.last { shell.path[shell.path.count - 1] = screen } else { shell.show(screen) }
  }
}
/** One published thing: its kind's drawing, the kind, the title, who sent it, its files. */
struct ArtifactCard: View {
  @EnvironmentObject var shell: AppShell
  let artifact: CoreView.ArtifactView
  var body: some View {
    let glyph = ["page": "html", "html": "html", "image": "image", "picture": "image", "video": "video", "audio": "audio"][artifact.kind] ?? "file"
    VStack(alignment: .leading, spacing: 8) {
      HStack(spacing: 12) {
        PenMark("glyph:\(glyph)", color: Ink.fg).frame(width: 30, height: 30).frame(width: 64, height: 48)
          .background(RoundedRectangle(cornerRadius: 8).fill(Ink.sunken))
        VStack(alignment: .leading, spacing: 2) {
          if !artifact.kind.isEmpty { Text(artifact.kind.uppercased()).font(Face.text(10, .bold)).kerning(1).foregroundStyle(Ink.faint) }
          Text(artifact.title).font(Face.text(15, .semibold)).foregroundStyle(Ink.fg).lineLimit(2)
          if !artifact.session.isEmpty { Text(artifact.session).font(Face.text(13)).foregroundStyle(Ink.muted).lineLimit(1) }
        }
        Spacer(minLength: 0)
      }
      ForEach(artifact.files, id: \.self) { id in
        if let file = shell.view.files[id] {
          Button { shell.act(["do": "open_file", "attachment_id": id, "save": true]) } label: {
            HStack(spacing: 8) { Sketch("clip", color: Ink.accent).frame(width: 16, height: 16); Text(file.name).font(Face.text(14, .medium)).foregroundStyle(Ink.accent).lineLimit(1) }
              .frame(minHeight: 32).contentShape(Rectangle())
          }.buttonStyle(.plain)
        }
      }
    }
    .padding(8)
    .background(RoundedRectangle(cornerRadius: 12).fill(Ink.surface)).overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Ink.lineStrong))
  }
}

// ---- a file to save --------------------------------------------------------------------------------------------------

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
  var body: some View {
    VStack(spacing: 20) {
      Sketch("page").frame(width: 48, height: 48)
      Text(file.name).font(Face.display(24, .heavy)).foregroundStyle(Ink.fg).multilineTextAlignment(.center)
      Button("Save file") { exporting = true }.buttonStyle(ObGo())
    }
    .padding(32).frame(maxWidth: 440)
    .fileExporter(isPresented: $exporting, document: SaveDocument(file.data), contentType: .data, defaultFilename: file.name) { _ in }
  }
}
