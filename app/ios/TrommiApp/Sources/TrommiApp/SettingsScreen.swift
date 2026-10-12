// SettingsScreen.swift: Settings as one list, the way iOS Settings reads, on Trommi's paper (agents.mjs, auth.mjs): on
// top the two invitations as equal tiles (Invite a Device: its QR code blurred until Show Code, the invite made only
// then, six emoji and They Match; Invite an Agent: the command for a terminal), then a row for each page: Agents
// (each a program on a computer, its sessions under it as a tree; route "sessions", the web's /settings/agents), Devices
// (only the person's: each a page of its own; Push on this phone: Yes, Only Knocking, No), Account (who, the ways in, each a page of its own that asks "Confirm It Is You"; Log Out), Theme (the Demo is a row of the pill menu, under Settings). Route .settings(<page>): "" or "agents" is the list itself.
import SwiftUI
import TrommiClient
#if canImport(CoreImage)
import CoreImage.CIFilterBuiltins
#endif

/** What every change of how the account opens says: the way in of now is asked for once more (the hub checks it). */
private let confirmPassword = "To change how your account opens, enter your current password. Being logged in on this iPhone is not enough."
private let confirmPasskey = "To change how your account opens, confirm with one of your passkeys. Being logged in on this iPhone is not enough."

struct SettingsScreen: View {
  @EnvironmentObject var model: BoardModel
  let tab: String
  init(tab: String) { self.tab = tab }
  var body: some View {
    switch tab {
    case "sessions": AgentsPage()   // (the page of the web's /settings/agents; the old Sessions page leads here)
    case "devices": DevicesPage()
    case let t where t.hasPrefix("devices/"): DevicePage(deviceId: String(t.dropFirst("devices/".count)))
    case "account": AccountPage()
    case "account/password", "account/passkeys", "account/kit": AccountWayPage(way: String(tab.dropFirst("account/".count)))
    case "theme": ThemePage()
    case "whats-new": WhatsNewPage()
    case "data": DataPage()
    case "security": SecurityPage()
    case "mls-proof": CoreProofPage()   // CoreProofScreen.swift: the self test, a row of Security
    default: SettingsHome()
    }
  }
}

// ---- the parts every page uses ---------------------------------------------------------------------------------

/** A page of Settings: paper, a large title, the groups one under the other. */
struct SettingsPage<Content: View>: View {
  let title: String
  @ViewBuilder var content: Content
  var body: some View {
    ScrollView {
      VStack(alignment: .leading, spacing: 26) { content }
        .padding(.horizontal, 16).padding(.top, 8).padding(.bottom, 90)
        .frame(maxWidth: 680).frame(maxWidth: .infinity)
    }
    .background(Ink.bg.ignoresSafeArea())
    .navigationTitle(title)
    .navigationBarTitleDisplayMode(.large)
  }
}

/** A group of rows as iOS Settings draws it: a caption, a sheet of paper with hairlines, a note under it. */
struct SettingsGroup<Content: View>: View {
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
struct RowRule: View {
  var inset: CGFloat = 56
  var body: some View { Rectangle().fill(Ink.line).frame(height: 1).padding(.leading, inset) }
}

/** One row: a drawn icon, the title, a detail on the right, a chevron when it opens a page. */
struct SettingsRow<Icon: View>: View {
  let title: String
  var detail: String? = nil
  var tint: Color = Ink.fg
  var chevron = false
  @ViewBuilder var icon: Icon
  var body: some View {
    HStack(spacing: 14) {
      icon.frame(width: 26, height: 26)
      Text(title).font(Face.text(17, .medium)).foregroundStyle(tint).lineLimit(1)
      Spacer(minLength: 8)
      if let d = detail { Text(d).font(Face.text(15)).foregroundStyle(Ink.muted).lineLimit(1) }
      if chevron { Image(systemName: "chevron.right").font(.system(size: 13, weight: .semibold)).foregroundStyle(Ink.faint) }
    }
    .padding(.horizontal, 16).frame(minHeight: 52)
    .contentShape(Rectangle())
  }
}

// ---- the list ---------------------------------------------------------------------------------------------------

struct SettingsHome: View {
  @EnvironmentObject var model: BoardModel
  @State private var inviteDevice = false
  @State private var askAgentName = false
  @State private var agentName = ""
  @State private var inviteAgent = false
  @State private var account: AccountStatus?
  @State private var hubBytes: Int64?
  var body: some View {
    let _ = model.version
    let agents = concreteAgents(model.desk?.agents ?? [], members: model.board?.members ?? [:])
    let sessions = agents.reduce(0) { $0 + $1.sessions.count }
    let devices = (model.board?.members.values.filter { $0.isActive && $0.deviceRole == "human" }.count) ?? 0
    SettingsPage(title: "Settings") {
      InviteTiles(device: { inviteDevice = true }, agent: { agentName = ""; askAgentName = true })
      SettingsGroup {
        link("sessions") { SettingsRow(title: "Agents", detail: agents.isEmpty ? nil : "\(agents.count) · \(sessions) sessions", chevron: true) { Sketch("terminal") } }
        RowRule()
        link("devices") { SettingsRow(title: "Devices", detail: devices > 0 ? "\(devices)" : nil, chevron: true) { PenMark("draw:phone", color: Ink.fg) } }
        RowRule()
        link("account") { SettingsRow(title: "Account", detail: model.demo ? "Demo" : account?.email, chevron: true) { Sketch("key") } }
      }
      SettingsGroup {
        link("theme") { SettingsRow(title: "Theme", detail: model.theme.word, chevron: true) { Sketch(model.theme == .dark ? "moon" : "sun") } }
      }
      SettingsGroup {
        link("data") { SettingsRow(title: "Data", detail: hubBytes.map { "\(megabytes($0)) on the hub" }, chevron: true) { Image(systemName: "chart.bar").font(.system(size: 19)).foregroundStyle(Ink.fg) } }
        RowRule()
        link("security") { SettingsRow(title: "Security", detail: "Keys, devices, self test", chevron: true) { Sketch("lock") } }
        RowRule()
        // (App Review 5.1.1(i): the privacy policy reachable inside the app, the same page App Store Connect names)
        Link(destination: URL(string: "https://trommi.com/privacy")!) { SettingsRow(title: "Privacy Policy", chevron: true) { Image(systemName: "hand.raised").font(.system(size: 20)).foregroundStyle(Ink.fg) } }.buttonStyle(.plain)
      }
      if let r = model.room {
        Text("Trommi \(HubClient.appVersion) · room \(String(r.record.roomId.prefix(12)))… · key epoch \(r.state.epoch)").font(Face.mono(12)).foregroundStyle(Ink.faint)
          .frame(maxWidth: .infinity).textSelection(.enabled)
      }
    }
    .sheet(isPresented: $inviteDevice) { InviteDeviceSheet() }
    .sheet(isPresented: $inviteAgent) { AgentInviteSheet(label: agentName.trimmingCharacters(in: .whitespaces)) }
    // (a state of the demo's list opens its sheet here: DemoMode.swift openDemoScreen)
    .onAppear {
      switch model.demoSheet { case "pair": inviteDevice = true; case "invite": agentName = "Website"; inviteAgent = true; default: break }
      model.demoSheet = nil
    }
    .alert("Invite Agent", isPresented: $askAgentName) {
      TextField("Name of the Session (e.g. Website)", text: $agentName)
      Button("Cancel", role: .cancel) {}
      Button("Continue") { inviteAgent = true }
    } message: { Text("The session gets this name on the board. You can leave it empty and rename it later.") }
    .task {
      if account == nil, !model.demo { account = try? await model.room?.accountStatus() }
      if model.demo { hubBytes = AccountUsage.demo.totalBytes } else if let u = try? await model.room?.accountUsage() { hubBytes = u.totalBytes }
    }
  }
  private func link<L: View>(_ page: String, @ViewBuilder _ label: () -> L) -> some View {
    Button { model.path.append(.settings(page)) } label: { label() }.buttonStyle(.plain)
  }
}

/** The two invites as equals: a device's code and an agent's command, each drawn blurred until asked for (only then
 *  is the invite made). Side by side where there is room (iPad), one above the other on a phone, as on the web. */
struct InviteTiles: View {
  let device: () -> Void
  let agent: () -> Void
  @Environment(\.horizontalSizeClass) private var size
  var body: some View {
    let wide = size == .regular
    let layout = wide ? AnyLayout(HStackLayout(alignment: .top, spacing: 14)) : AnyLayout(VStackLayout(spacing: 12))
    VStack(alignment: .leading, spacing: 7) {
      layout {
        tile(wide, "Invite a Device", "A phone or another computer scans the code. Made when you ask, works once.", "Show Code", device) {
          ZStack { Color.white; QRCode(text: "https://app.trommi.com/join#v1.invite.drawn.blurred").padding(10).blur(radius: 4).opacity(0.75) }
        }
        tile(wide, "Invite an Agent", "Claude Code or Codex runs one command in your project and joins.", "Show Command", agent) {
          ZStack(alignment: .topLeading) {
            Color(hex: 0x1d211f)
            VStack(alignment: .leading, spacing: 7) {
              HStack(spacing: 5) { ForEach(0..<3, id: \.self) { _ in Circle().fill(Color(hex: 0x5a625e)).frame(width: 7, height: 7) } }
              Text("$ claude").font(Face.mono(12)); Text("> join trommi ab3f…9c2e").font(Face.mono(12)); Text("> check the six emoji").font(Face.mono(12))
            }.foregroundStyle(Color(hex: 0xd8e4dc)).padding(10).blur(radius: 2.2).opacity(0.85)
          }
        }
      }
      Text("Nobody is added before you confirm.").font(Face.text(13)).foregroundStyle(Ink.muted).padding(.horizontal, 16)
    }
  }
  private func tile<Art: View>(_ wide: Bool, _ title: String, _ line: String, _ word: String, _ go: @escaping () -> Void, @ViewBuilder art: () -> Art) -> some View {
    let picture = ZStack {
      art()
      Text(word).font(Face.text(12, .bold)).lineLimit(1).fixedSize().foregroundStyle(Ink.bg).padding(.horizontal, 8).padding(.vertical, 6).background(Capsule().fill(Ink.fg))
    }
    .frame(width: wide ? nil : 104, height: wide ? 150 : 104).frame(maxWidth: wide ? .infinity : nil)
    .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
    .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(Ink.fg, lineWidth: 1.5))
    let words = VStack(alignment: .leading, spacing: 4) {
      Text(title).font(Face.text(17, .semibold)).foregroundStyle(Ink.fg)
      Text(line).font(Face.text(14)).foregroundStyle(Ink.muted).fixedSize(horizontal: false, vertical: true)
    }
    return Button(action: go) {
      Group {
        if wide { VStack(alignment: .leading, spacing: 12) { picture; words } } else { HStack(spacing: 14) { picture; words; Spacer(minLength: 0) } }
      }
      .padding(14).frame(maxWidth: .infinity, alignment: .leading)
      .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(Ink.surface))
      .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(Ink.line))
      .contentShape(Rectangle())
    }
    .buttonStyle(.plain)
    .accessibilityLabel("\(title): \(word)")
  }
}

// ---- Agents -----------------------------------------------------------------------------------------------------
// Each agent (a program on a computer, Agents.swift) as a group, its main sessions under it as the branches of a tree:
// name, "<desk> desk · folder", "+N helpers", connected or away; a tap opens the session. Its "…" removes it from the
// account (the agent devices of its sessions). Renaming an agent and updating its connector have no way on the hub
// yet: the connector names itself and updates itself.

struct AgentsPage: View {
  @EnvironmentObject var model: BoardModel
  @State private var removing: ConcreteAgent?
  @State private var askAgentName = false
  @State private var agentName = ""
  @State private var inviteAgent = false
  var body: some View {
    let _ = model.version
    let agents = concreteAgents(model.desk?.agents ?? [], members: model.board?.members ?? [:], me: model.board?.myDeviceId)
    let desks = model.desk?.desks ?? []
    SettingsPage(title: "Agents") {
      if !agents.isEmpty {
        let n = agents.reduce(0) { $0 + $1.sessions.count }, computers = Set(agents.map(\.host)).count
        Text("\(agents.count) \(agents.count == 1 ? "agent" : "agents") on \(computers) \(computers == 1 ? "computer" : "computers") · \(n) \(n == 1 ? "session" : "sessions")").font(Face.text(15)).foregroundStyle(Ink.muted)
      }
      ForEach(agents) { g in
        SettingsGroup {
          head(g)
          if let acc = g.sessions.lazy.compactMap({ usageOf(model, $0) }).first { UsageCard(acc: acc); RowRule(inset: 16) }
          ForEach(Array(g.tops.enumerated()), id: \.element.agent.deviceId) { i, b in
            Button { model.path.append(.session(b.agent.id)) } label: { branch(b, last: i == g.tops.count - 1, desk: desks.first { $0.id == (b.agent.desk ?? desks.first?.id) }?.name ?? "Personal") }.buttonStyle(.plain)
          }
        }
      }
      SettingsGroup(footer: "An agent is Claude Code or Codex on one of your computers, connected by the Trommi connector. Its sessions stand under it; a session's helpers are counted beside it.") {
        if agents.isEmpty { Text("No agent yet.").font(Face.text(15)).foregroundStyle(Ink.muted).padding(16).frame(maxWidth: .infinity, alignment: .leading); RowRule() }
        Button { agentName = ""; askAgentName = true } label: { SettingsRow(title: "Invite an Agent…", tint: Ink.accent) { PenMark("ui:PLUS", color: Ink.accent) } }.buttonStyle(.plain)
      }
    }
    .sheet(isPresented: $inviteAgent) { AgentInviteSheet(label: agentName.trimmingCharacters(in: .whitespaces)) }
    .alert("Invite Agent", isPresented: $askAgentName) {
      TextField("Name of the Session (e.g. Website)", text: $agentName)
      Button("Cancel", role: .cancel) {}
      Button("Continue") { inviteAgent = true }
    } message: { Text("The session gets this name on the board. You can leave it empty and rename it later.") }
    .confirmationDialog(removing.map { "Remove \($0.program) on \(computer($0))?" } ?? "", isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }), titleVisibility: .visible) {
      Button("Remove from This Account", role: .destructive) {
        if let g = removing, let room = model.acting() { model.act("Not removed") { try await room.removeDevices(g.devices); model.say("Removed", "\(g.program) on \(computer(g))") } }
      }
    } message: { Text(removing.map { "It and its \($0.sessions.count) sessions can read nothing new after this. The others get a new key; the history stays." } ?? "") }
  }
  private func computer(_ g: ConcreteAgent) -> String { g.host.isEmpty ? "an unnamed computer" : g.host }
  private func head(_ g: ConcreteAgent) -> some View {
    let helpers = g.sessions.count - g.tops.count
    return HStack(spacing: 12) {
      Sketch("terminal").frame(width: 24, height: 24).frame(width: 40, height: 40)
        .background(RoundedRectangle(cornerRadius: 10).fill(Ink.surface)).overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Ink.fg, lineWidth: 1.5))
      VStack(alignment: .leading, spacing: 3) {
        (Text(g.program).font(Face.display(17, .bold)) + Text(" on \(computer(g))").font(Face.text(16, .medium)).foregroundColor(Ink.muted)).lineLimit(1)
        HStack(spacing: 5) {
          Circle().fill(g.online ? Ink.stDone : Ink.lineStrong).frame(width: 7, height: 7)
          Text("\(g.online ? "connected" : "away") · \(g.tops.count) \(g.tops.count == 1 ? "session" : "sessions")\(helpers > 0 ? ", \(helpers) \(helpers == 1 ? "helper" : "helpers")" : "")").font(Face.text(12)).foregroundStyle(Ink.muted).lineLimit(1)
        }
      }
      Spacer(minLength: 0)
      if !g.devices.isEmpty && !model.demo {
        Menu { Button("Remove from This Account…", role: .destructive) { removing = g } } label: {
          Image(systemName: "ellipsis").font(.system(size: 17, weight: .semibold)).foregroundStyle(Ink.fg).frame(width: 40, height: 40)
        }.accessibilityLabel("More for \(g.program) on \(computer(g))")
      }
    }
    .padding(.horizontal, 14).padding(.vertical, 12).background(Ink.sunken)
  }
  private func branch(_ b: ConcreteAgent.Branch, last: Bool, desk: String) -> some View {
    HStack(spacing: 12) {
      // the branch of the tree: down from the agent, across to the session
      ZStack(alignment: .topLeading) {
        Rectangle().fill(Ink.lineStrong).frame(width: 1.4).frame(maxHeight: last ? 30 : .infinity, alignment: .top)
        Rectangle().fill(Ink.lineStrong).frame(width: 12, height: 1.4).offset(y: 29)
      }.frame(width: 14, alignment: .leading).frame(maxHeight: .infinity, alignment: .top)
      AgentMark(agent: b.agent, size: 24, crown: false).frame(width: 24, height: 24)
      VStack(alignment: .leading, spacing: 2) {
        Text(b.agent.name).font(Face.text(16, .semibold)).foregroundStyle(Ink.fg).lineLimit(1)
        Text(b.folder.isEmpty ? "\(desk) desk" : "\(desk) desk · \(b.folder)").font(Face.text(12)).foregroundStyle(Ink.muted).lineLimit(1).truncationMode(.tail)
      }
      Spacer(minLength: 4)
      if b.helpers > 0 { Text("+\(b.helpers) \(b.helpers == 1 ? "helper" : "helpers")").font(Face.text(12, .semibold)).foregroundStyle(Ink.muted).padding(.horizontal, 7).padding(.vertical, 3).overlay(Capsule().strokeBorder(Ink.lineStrong)) }
      Circle().fill(b.agent.online ? Ink.stDone : Ink.lineStrong).frame(width: 8, height: 8).accessibilityLabel(b.agent.online ? "connected" : "away")
      Image(systemName: "chevron.right").font(.system(size: 13, weight: .semibold)).foregroundStyle(Ink.faint)
    }
    .padding(.leading, 34).padding(.trailing, 16).frame(height: 60).contentShape(Rectangle())
  }
}

// ---- Security ---------------------------------------------------------------------------------------------------
// Every key that opens the account's data, each with what it can read (spec/v1.md 4, 5, 8; SECURITY.md), as the web's
// /settings/security: the account key (the room's recovery key) with the Emergency Kit, the person's devices with
// their fingerprints and the day each came, the agents, and the self test under it.

/** A day as the pages show it ("12 Sept 2026"); nil for no time. */
private func dayOf(_ ms: UInt64?) -> String? {
  guard let ms else { return nil }
  let f = DateFormatter(); f.locale = Locale(identifier: "en_GB"); f.dateFormat = "d MMM yyyy"
  return f.string(from: Date(timeIntervalSince1970: Double(ms) / 1000))
}
/** What opens the sealed copies of the account key: its ways in, in words. */
private func sealedUnder(_ a: AccountFacts?) -> String {
  guard let a else { return "a way in of your account" }
  let ways = (a.hasPassword ? ["your password"] : []) + (a.passkeys > 0 ? [a.passkeys == 1 ? "your passkey" : "a passkey"] : []) + (a.hasRecovery ? ["your Emergency Kit"] : [])
  if ways.isEmpty { return "a way in of your account" }
  return ways.count == 1 ? ways[0] : ways.dropLast().joined(separator: ", ") + " or " + ways.last!
}

/** A row of Security: an icon, a title with a second line under it, what it can read on the right. */
private struct KeyRow<Icon: View>: View {
  let title: String
  let line: String
  var reads: String
  var chevron = false
  @ViewBuilder var icon: Icon
  var body: some View {
    HStack(spacing: 14) {
      icon.frame(width: 26, height: 26)
      VStack(alignment: .leading, spacing: 2) {
        Text(title).font(Face.text(17, .medium)).foregroundStyle(Ink.fg).lineLimit(1)
        Text(line).font(Face.mono(12)).foregroundStyle(Ink.muted).fixedSize(horizontal: false, vertical: true)
      }
      Spacer(minLength: 8)
      Text(reads).font(Face.text(15)).foregroundStyle(Ink.muted).lineLimit(1)
      if chevron { Image(systemName: "chevron.right").font(.system(size: 13, weight: .semibold)).foregroundStyle(Ink.faint) }
    }
    .padding(.horizontal, 16).padding(.vertical, 8).frame(minHeight: 60)
    .contentShape(Rectangle())
  }
}

struct SecurityPage: View {
  @EnvironmentObject var model: BoardModel
  @StateObject private var account = AccountState()
  var body: some View {
    let _ = model.version
    let board = model.board
    let devices = (board?.members.values.filter { $0.isActive && $0.deviceRole == "human" } ?? []).sorted { ($0.addedEntryNumber, $0.deviceId) < ($1.addedEntryNumber, $1.deviceId) }
    let agents = concreteAgents(model.desk?.agents ?? [], members: board?.members ?? [:])
    let sessions = agents.reduce(0) { $0 + $1.sessions.count }
    let helpers: Int = {
      var ids = Set<String>()
      for s in board.map({ Array($0.sessions.values) }) ?? [] where s.isActive && s.parentSessionId != nil {
        for d in s.agentDeviceIds where d != s.creatorDeviceId && board?.members[d]?.isActive == true { ids.insert(d) }
      }
      return ids.count
    }()
    let key = board?.recoveryKey
    let facts = account.facts(model)
    SettingsPage(title: "Security") {
      Text("Every key that opens your data, and the self test.").font(Face.text(15)).foregroundStyle(Ink.muted).padding(.horizontal, 16)
      SettingsGroup(header: "Account Key", footer: "Opens everything; it is also your recovery key. The hub keeps it only sealed: \(sealedUnder(facts)) opens it on a new device.") {
        KeyRow(title: "Account Key", line: "\(key?.fingerprint ?? "unknown") · recovery key, " + (dayOf(key?.since).map { "since \($0)" } ?? "since: unknown"), reads: "everything") { Sketch("key") }
        RowRule()
        Button { model.path.append(.settings("account/kit")) } label: {
          SettingsRow(title: "Emergency Kit", detail: facts.map { $0.hasRecovery ? "made" : "not made yet" } ?? (account.loaded ? nil : "…"), chevron: true) { Sketch("page") }
        }.buttonStyle(.plain)
      }
      SettingsGroup(header: "Authorized Devices", footer: "Each of your devices reads everything, also what came before it was added.") {
        ForEach(Array(devices.enumerated()), id: \.element.deviceId) { i, d in
          if i > 0 { RowRule() }
          Button { model.path.append(.settings("devices/\(d.deviceId)")) } label: {
            KeyRow(title: deviceName(d) + (d.isMe ? " · this device" : ""), line: "\(d.fingerprint) · " + (dayOf(d.addedAt).map { "added \($0)" } ?? "added: unknown"), reads: "everything", chevron: true) {
              Group { if deviceIcon(d) == "phone" { PenMark("draw:phone", color: Ink.fg) } else { Sketch(deviceIcon(d)) } }
            }
          }.buttonStyle(.plain)
        }
      }
      SettingsGroup(header: "Agents", footer: "An agent reads only its own session and the helper sessions it starts; a helper reads only its own.") {
        Button { model.path.append(.settings("sessions")) } label: {
          KeyRow(title: "Agents", line: (agents.isEmpty ? "none" : "\(agents.count) \(agents.count == 1 ? "agent" : "agents") · \(sessions) \(sessions == 1 ? "session" : "sessions")") + (helpers > 0 ? " · \(helpers) \(helpers == 1 ? "helper" : "helpers")" : "") + ": each with its own key", reads: "own session", chevron: true) { Sketch("terminal") }
        }.buttonStyle(.plain)
      }
      Text("Nobody else, not the hub either: it sees which device is in which group, when, and how much.").font(Face.text(13, .semibold)).foregroundStyle(Ink.muted).padding(.horizontal, 16).fixedSize(horizontal: false, vertical: true)
      SettingsGroup(header: "Check") {
        Button { model.path.append(.settings("mls-proof")) } label: {
          SettingsRow(title: "Self Test", detail: "runs on this device", chevron: true) { Sketch("tick") }
        }.buttonStyle(.plain)
      }
    }
    .task { await account.load(model) }
  }
}

// ---- Devices ----------------------------------------------------------------------------------------------------
// The devices and agents as rows of the list (each opens a page of its own: DevicePage), Push on this phone as a
// tick list (as Theme), Invite Agent… under the agents; the removed ones on a page of their own.

/** What a row says of a device: its drawing, its name, its fingerprint, whether it is here. */
private func deviceName(_ d: RoomMember) -> String { d.deviceName.isEmpty ? (d.deviceRole == "human" ? "Device" : "Agent") : d.deviceName }
private func deviceState(_ d: RoomMember) -> String { !d.isActive ? "removed" : d.isMe ? "this device" : d.isOnline ? "online" : "away" }
private func deviceIcon(_ d: RoomMember) -> String {
  d.deviceRole != "human" ? "terminal" : (d.deviceName + " " + (d.platform ?? "")).range(of: "phone|ipad|ios|android", options: [.regularExpression, .caseInsensitive]) != nil ? "phone" : "laptop"
}
struct DeviceRow: View {
  let d: RoomMember
  var chevron = true
  var body: some View {
    HStack(spacing: 14) {
      Group { if deviceIcon(d) == "phone" { PenMark("draw:phone", color: Ink.fg) } else { Sketch(deviceIcon(d)) } }.frame(width: 26, height: 26)
      VStack(alignment: .leading, spacing: 2) {
        Text(deviceName(d)).font(Face.text(17, .medium)).foregroundStyle(d.isActive ? Ink.fg : Ink.muted).lineLimit(1)
        Text(d.fingerprint).font(Face.mono(12)).foregroundStyle(Ink.muted).lineLimit(1)
      }
      Spacer(minLength: 8)
      if d.isActive { Circle().fill(d.isOnline || d.isMe ? Ink.stDone : Ink.lineStrong).frame(width: 8, height: 8) }
      Text(deviceState(d)).font(Face.text(15)).foregroundStyle(Ink.muted).lineLimit(1)
      if chevron { Image(systemName: "chevron.right").font(.system(size: 13, weight: .semibold)).foregroundStyle(Ink.faint) }
    }
    .padding(.horizontal, 16).frame(minHeight: 60)
    .contentShape(Rectangle())
  }
}

struct DevicesPage: View {
  @EnvironmentObject var model: BoardModel
  @State private var pushLevel = Push.level
  var body: some View {
    let _ = model.version
    // (only the person's devices: phones, tablets, browsers; the agents are Settings · Agents)
    let members = (model.board?.members.values.filter { $0.deviceRole == "human" } ?? []).sorted { $0.addedEntryNumber < $1.addedEntryNumber }
    let people = members.filter { $0.isActive }
    let gone = members.filter { !$0.isActive }
    SettingsPage(title: "Devices") {
      SettingsGroup(header: "Your Devices") {
        ForEach(Array(people.enumerated()), id: \.element.deviceId) { i, d in
          if i > 0 { RowRule() }
          Button { model.path.append(.settings("devices/\(d.deviceId)")) } label: { DeviceRow(d: d) }.buttonStyle(.plain)
        }
      }
      SettingsGroup(header: "Push on This \(UIDeviceName.model)", footer: pushLevel == "knocking" ? "Only urgent questions and a session that lost its link ring here." : pushLevel == "off" ? "Nothing rings here; the board still shows everything." : "Every new question rings here.") {
        ForEach(Array([("all", "bell", "Yes"), ("knocking", "knock", "Only Knocking"), ("off", "snooze", "No")].enumerated()), id: \.offset) { i, l in
          if i > 0 { RowRule() }
          Button { pushLevel = l.0; Task { await Push.setLevel(l.0) } } label: {
            SettingsRow(title: l.2) { Sketch(l.1) }
              .overlay(alignment: .trailing) { if pushLevel == l.0 { Sketch("tick", color: Ink.accent).frame(width: 20, height: 20).padding(.trailing, 16) } }
          }.buttonStyle(.plain).disabled(model.demo)
          .accessibilityAddTraits(pushLevel == l.0 ? .isSelected : [])
        }
      }
      if !gone.isEmpty {
        SettingsGroup {
          Button { model.path.append(.settings("devices/removed")) } label: { SettingsRow(title: "Removed", detail: "\(gone.count)", chevron: true) { Sketch("archive") } }.buttonStyle(.plain)
        }
      }
    }
  }
}

/** One device: what it is, whether it is here, its fingerprint, its Push; this phone's name; Remove for another one. */
struct DevicePage: View {
  @EnvironmentObject var model: BoardModel
  let deviceId: String
  @State private var removing = false
  @State private var renaming = false
  @State private var newName = ""
  @State private var push: String?
  var body: some View {
    let _ = model.version
    let members = (model.board?.members.values.map { $0 } ?? [])
    if deviceId == "removed" {
      SettingsPage(title: "Removed") {
        SettingsGroup(footer: "They open nothing new; what they had before stays with them.") {
          ForEach(Array(members.filter { !$0.isActive && $0.deviceRole == "human" }.enumerated()), id: \.element.deviceId) { i, d in
            if i > 0 { RowRule() }
            DeviceRow(d: d, chevron: false)
          }
        }
      }
    } else if let d = members.first(where: { $0.deviceId == deviceId }) {
      SettingsPage(title: deviceName(d)) {
        SettingsGroup(footer: "The fingerprint comes from the device's own key, as the room's group names it: it must look the same on every device.") {
          SettingsRow(title: "Kind", detail: d.deviceRole == "human" ? "Person" : "Agent") { Group { if deviceIcon(d) == "phone" { PenMark("draw:phone", color: Ink.fg) } else { Sketch(deviceIcon(d)) } } }
          RowRule()
          SettingsRow(title: "Status", detail: deviceState(d)) { Circle().fill(d.isActive && (d.isOnline || d.isMe) ? Ink.stDone : Ink.lineStrong).frame(width: 10, height: 10) }
          RowRule()
          VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 14) { Sketch("key").frame(width: 26, height: 26); Text("Key Fingerprint").font(Face.text(17, .medium)) }
            Text(d.fingerprint).font(Face.mono(14)).foregroundStyle(Ink.muted).textSelection(.enabled).padding(.leading, 40)
          }.padding(.horizontal, 16).padding(.vertical, 12).frame(maxWidth: .infinity, alignment: .leading)
          if d.deviceRole == "human" && d.isActive && !d.isMe {
            RowRule()
            SettingsRow(title: "Push", detail: push == nil ? "No" : push == "knocking" ? "Only Knocking" : "Yes") { Sketch("bell") }
          }
        }
        if d.isActive && d.isMe && !model.demo {
          SettingsGroup(footer: "This name is what your other devices see.") {
            Button { newName = d.deviceName; renaming = true } label: { SettingsRow(title: "Name", detail: d.deviceName, chevron: true) { Sketch("pen") } }.buttonStyle(.plain)
          }
        }
        if d.isActive && !d.isMe {
          SettingsGroup(footer: d.deviceRole == "human" ? "\(deviceName(d)) can open nothing new after this. Everyone else gets a new key; that takes a moment." : "\(deviceName(d)) can read nothing new after this. The others get a new key; the session’s history stays.") {
            Button { removing = true } label: { SettingsRow(title: "Remove \(deviceName(d))…", tint: Ink.urgCritical) { Sketch("logout", color: Ink.urgCritical) } }.buttonStyle(.plain)
          }
        }
      }
      .confirmationDialog("Remove \(deviceName(d))?", isPresented: $removing, titleVisibility: .visible) {
        Button("Remove", role: .destructive) { model.removeDevice(d.deviceId, name: d.deviceName); model.path.removeLast() }
        // (spec/v1.md 8.6: a device that is not in his hands any more may have learned the recovery code; the kit's page makes a new one)
        if d.deviceRole == "human" {
          Button("Remove and Make New Recovery Code…", role: .destructive) { model.removeDevice(d.deviceId, name: d.deviceName); model.path.removeLast(); model.path.append(.settings("account/kit")) }
        }
      } message: {
        Text(d.deviceRole == "human" ? "It can open nothing new after this. Everyone else gets a new key; that takes a moment. If it is lost or no longer yours, also make a new recovery code." : "It can read nothing new after this. The others get a new key; the session’s history stays.")
      }
      .alert("Rename This Device", isPresented: $renaming) {
        TextField("Name", text: $newName)
        Button("Cancel", role: .cancel) {}
        Button("Save") {
          let n = String(newName.trimmingCharacters(in: .whitespaces).prefix(40))
          if !n.isEmpty, let room = model.acting() { model.act("Not renamed") { try await room.renameDevice(name: n, platform: "ios") } }
        }
      }
      .task { if let p = try? await model.room?.hub.pushStates() { push = p[deviceId]?.level } }
    } else {
      SettingsPage(title: "Device") { Text("This device is not in the account.").font(Face.text(15)).foregroundStyle(Ink.muted) }
    }
  }
}

// ---- Account ----------------------------------------------------------------------------------------------------
// An overview: who, the ways in (Password, Passkeys, Emergency Kit: each a page of its own, AccountWayPage), this
// device, Log Out, Delete Account. A change of a way in asks "Confirm it is you" (ProofBox): the way in of now, which
// the hub checks with the change. The demo shows a made-up account and changes nothing.

/** What the account pages show of the account: the hub's status, or the demo's made-up one. */
struct AccountFacts {
  var email: String
  var accountId: String
  var hasPassword: Bool
  var hasRecovery: Bool
  var passkeys: Int
  var confirmsEmail = false
  var emailVerified = true
  static let demo = AccountFacts(email: "ada@example.org", accountId: "a7q2k9xd-4mwp-t3rc", hasPassword: true, hasRecovery: true, passkeys: 1)
  init(email: String, accountId: String, hasPassword: Bool, hasRecovery: Bool, passkeys: Int) { self.email = email; self.accountId = accountId; self.hasPassword = hasPassword; self.hasRecovery = hasRecovery; self.passkeys = passkeys }
  init(_ st: AccountStatus) {
    email = st.email; accountId = st.accountId; hasPassword = st.hasPassword; hasRecovery = st.hasRecovery; passkeys = st.passkeys.count
    confirmsEmail = st.confirmsEmail; emailVerified = st.emailVerifiedAt != nil
  }
}

/** "Confirm it is you": the way in of now, asked again for a change of how the account opens. With a password its
 *  field; without one, the passkey is asked for when the change is sent. */
struct ProofBox: View {
  let hasPassword: Bool
  @Binding var password: String
  var label = "Current Password"
  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      HStack(alignment: .top, spacing: 12) {
        Sketch("lock").frame(width: 26, height: 26)
        VStack(alignment: .leading, spacing: 3) {
          Text("Confirm It Is You").font(Face.text(16, .semibold))
          Text(hasPassword ? confirmPassword : confirmPasskey).font(Face.text(14)).foregroundStyle(Ink.muted).fixedSize(horizontal: false, vertical: true)
        }
      }
      if hasPassword {
        SecureField(label, text: $password).textContentType(.password).font(Face.text(16)).padding(10)
          .background(RoundedRectangle(cornerRadius: 10).fill(Ink.surface)).overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Ink.lineStrong))
      }
    }
    .padding(16).frame(maxWidth: .infinity, alignment: .leading)
    .background(Ink.yellow.opacity(0.2))
  }
}

/** The account's status, loaded once per page; what a change said or what went wrong. */
@MainActor final class AccountState: ObservableObject {
  @Published var status: AccountStatus?
  @Published var loaded = false
  @Published var said = ""
  @Published var error = ""
  func load(_ model: BoardModel) async { if !model.demo { status = try? await model.room?.accountStatus() }; loaded = true }
  func facts(_ model: BoardModel) -> AccountFacts? { model.demo ? .demo : status.map(AccountFacts.init) }
  /** `passkey`: what went wrong is said in the passkey's words, which the sign-in screen shares. */
  func run(_ model: BoardModel, passkey: Bool = false, _ op: @escaping () async throws -> Void) {
    error = ""; said = ""
    if model.demo { error = "The demo changes nothing here."; return }
    Task {
      do { try await op() }
      catch {
        let code = (error as? TrommiError)?.code ?? (error as? HubError)?.code ?? ""
        if passkey && code != "wrong-login" { self.error = model.accountError(error); return }
        // (a wait the hub names is said as it is)
        if code == "rate-limited", let wait = retryWait(of: error) { self.error = "Too many tries. Please wait \(waitText(seconds: wait))."; return }
        self.error = ["wrong-login": "That password is not right.", "weak-password": "The password needs at least 12 characters.", "wrong-code": "Wrong or expired code.",
                      "account-changed": "Changed on another device meanwhile. Please try again.",
                      "pending": "Trommi has not answered yet. Check again later; if no new kit shows then, make a new Emergency Kit.", "rate-limited": "Too many tries. Please wait a few minutes."][code] ?? model.describe(error)
      }
    }
  }
}

private struct SaidLines: View {
  @ObservedObject var state: AccountState
  var body: some View {
    if !state.said.isEmpty { Text(state.said).font(Face.text(15, .medium)).foregroundStyle(Ink.accent) }
    if !state.error.isEmpty { Text(state.error).font(Face.text(15)).foregroundStyle(Ink.urgCritical) }
  }
}

struct AccountPage: View {
  @EnvironmentObject var model: BoardModel
  @StateObject private var state = AccountState()
  @State private var leave = false
  @State private var code = ""
  @State private var deleteAsk = false
  @State private var deletePasswordAsk = false
  @State private var deletePassword = ""
  var body: some View {
    SettingsPage(title: "Account") {
      SaidLines(state: state)
      if !state.loaded {
        ProgressView().frame(maxWidth: .infinity)
      } else if let f = state.facts(model) {
        SettingsGroup {
          HStack(spacing: 14) {
            Sketch(f.email.isEmpty ? "key" : "letter").frame(width: 26, height: 26)
            VStack(alignment: .leading, spacing: 2) {
              Text(f.email.isEmpty ? "Logged In with a Passkey" : f.email).font(Face.text(17, .semibold)).lineLimit(1)
              Text("Account ID \(f.accountId)").font(Face.mono(12)).foregroundStyle(Ink.muted).lineLimit(1).textSelection(.enabled)
            }
            Spacer(minLength: 0)
          }.padding(.horizontal, 16).frame(minHeight: 62)
          // (a hub that confirms addresses: the code for it; the hub does not yet)
          if f.confirmsEmail && !f.emailVerified {
            RowRule()
            VStack(alignment: .leading, spacing: 10) {
              Text("We send a six-digit code to \(f.email).").font(Face.text(14)).foregroundStyle(Ink.muted)
              HStack {
                TextField("Code", text: $code).keyboardType(.numberPad).textContentType(.oneTimeCode).font(Face.mono(18)).padding(10).background(RoundedRectangle(cornerRadius: 10).strokeBorder(Ink.lineStrong))
                Button("Confirm") { state.run(model) { try await model.room?.verifyEmail(code: code); state.said = "Email confirmed."; await state.load(model) } }.buttonStyle(QuietWay())
              }
              Button("Send Code") { state.run(model) { try await model.room?.resendEmailCode(); state.said = "Code sent." } }.font(Face.text(15, .semibold)).foregroundStyle(Ink.accent)
            }.padding(16)
          }
        }
        SettingsGroup(header: "Ways In", footer: "Each opens your account on a new device. Changing one asks for one of them again: being logged in here is not enough.") {
          way("password", "Password", f.hasPassword ? "Set" : "None", "lock")
          if model.passkeysOn || f.passkeys > 0 { RowRule(); way("passkeys", "Passkeys", f.passkeys == 0 ? "None" : "\(f.passkeys)", "key") }
          RowRule()
          way("kit", "Emergency Kit", f.hasRecovery ? "Made" : "Not Made Yet", "page")
        }
      } else {
        SettingsGroup {
          SettingsRow(title: "No Login Yet") { Sketch("letter") }
        }
      }
      if let r = model.room {
        SettingsGroup(header: "This Device") {
          SettingsRow(title: "Room", detail: "\(String(r.record.roomId.prefix(12)))…") { Sketch("key") }
          RowRule()
          SettingsRow(title: "Hub", detail: r.record.hubURL.replacingOccurrences(of: "https://", with: "")) { Sketch("link") }
          RowRule()
          SettingsRow(title: "Version", detail: "Trommi \(HubClient.appVersion) · key epoch \(r.state.epoch)") { PenMark("ui:BELL", color: Ink.fg) }
        }
      }
      if model.room != nil || model.demo {
        SettingsGroup(footer: "This device forgets the room and its keys. Your other devices and your agents carry on.") {
          Button { leave = true } label: { SettingsRow(title: "Log Out…", detail: "of this \(UIDeviceName.model)", tint: Ink.urgCritical) { Sketch("logout", color: Ink.urgCritical) } }.buttonStyle(.plain)
        }
        .confirmationDialog("Log Out of This Device?", isPresented: $leave, titleVisibility: .visible) {
          Button("Log Out", role: .destructive) { model.logOut() }
        } message: { Text("This phone logs in again with email and password or by scanning the code of a signed-in device.") }
      }
      // (App Store 5.1.1(v): an account made in the app is deleted in the app; the demo only says it is the demo)
      if model.demo || (model.room != nil && state.status != nil) {
        SettingsGroup(footer: "Deletes your account and everything Trommi keeps for it. This cannot be undone.") {
          Button { deleteAsk = true } label: { SettingsRow(title: "Delete Account…", tint: Ink.urgCritical) { Sketch("bin", color: Ink.urgCritical) } }.buttonStyle(.plain)
        }
        .confirmationDialog("Delete Your Account?", isPresented: $deleteAsk, titleVisibility: .visible) {
          Button("Delete Account", role: .destructive) {
            if model.demo { Task { try? await model.deleteAccount(password: nil) } }
            else if state.status?.hasPassword == true { deletePasswordAsk = true }
            // (an account without a password: one of its passkeys confirms it)
            else { state.run(model, passkey: true) { try await model.deleteAccount(password: nil) } }
          }
        } message: { Text("Your account and everything Trommi keeps for it on the hub are deleted: every desk, card, note, file and agent link. Your agents lose their connection. This cannot be undone.") }
        .alert("Delete Account", isPresented: $deletePasswordAsk) {
          SecureField("Your Password", text: $deletePassword)
          Button("Delete", role: .destructive) {
            let p = deletePassword
            deletePassword = ""
            state.run(model) { try await model.deleteAccount(password: p) }
          }
          Button("Cancel", role: .cancel) { deletePassword = "" }
        } message: { Text("Your password confirms that it is you. Then the account is deleted.") }
      }
    }
    .task { await state.load(model) }
  }
  private func way(_ key: String, _ title: String, _ detail: String, _ icon: String) -> some View {
    Button { model.path.append(.settings("account/\(key)")) } label: { SettingsRow(title: title, detail: detail, chevron: true) { Sketch(icon) } }.buttonStyle(.plain)
  }
}

/** One way in of the account, on a page of its own: the password, the passkeys, the Emergency Kit. */
struct AccountWayPage: View {
  @EnvironmentObject var model: BoardModel
  let way: String
  @StateObject private var state = AccountState()
  @State private var proof = ""
  @State private var next = ""
  @State private var kit: String?
  @State private var codeAsk = false
  var body: some View {
    SettingsPage(title: way == "password" ? "Password" : way == "passkeys" ? "Passkeys" : "Emergency Kit") {
      SaidLines(state: state)
      if !state.loaded { ProgressView().frame(maxWidth: .infinity) }
      else if let f = state.facts(model) {
        switch way {
        case "password": password(f)
        case "passkeys": passkeys(f)
        default: kitPart(f)
        }
      }
    }
    .task { await state.load(model) }
  }

  @ViewBuilder private func password(_ f: AccountFacts) -> some View {
    if f.hasPassword {
      SettingsGroup { ProofBox(hasPassword: true, password: $proof) }
      SettingsGroup(header: "New Password", footer: "If you lose your password and your Emergency Kit, nobody (not even Trommi) can recover your data.") {
        VStack(alignment: .leading, spacing: 8) {
          HStack {
            SecureField("New Password (at least 12 characters)", text: $next).textContentType(.newPassword).font(Face.text(16)).padding(10).background(RoundedRectangle(cornerRadius: 10).strokeBorder(Ink.lineStrong))
            Button { next = generatePassword() } label: { Image(systemName: "dice") }.accessibilityLabel("Suggest a Password")
          }
          if !next.isEmpty && next.count >= 12 { Text(next).font(Face.mono(13)).foregroundStyle(Ink.muted).textSelection(.enabled) }
        }.padding(16)
      }
      Button("Change Password") { let c = proof, n = next; state.run(model) { try await model.room?.changePassword(current: c, next: n); proof = ""; next = ""; state.said = "Password changed." } }
        .buttonStyle(QuietWay()).disabled(passwordProblem(next) != nil || proof.isEmpty)
    } else {
      SettingsGroup(footer: "If you lose your passkeys and your Emergency Kit, nobody (not even Trommi) can recover your data.") {
        SettingsRow(title: "None", detail: "a passkey opens it") { Sketch("lock") }
      }
    }
  }

  @ViewBuilder private func passkeys(_ f: AccountFacts) -> some View {
    SettingsGroup(footer: f.hasPassword && f.passkeys > 0 ? "Your password still opens this account." : nil) {
      if f.passkeys == 0 { Text("None yet. A passkey logs you in without a password.").font(Face.text(15)).foregroundStyle(Ink.muted).padding(16).frame(maxWidth: .infinity, alignment: .leading) }
      ForEach(0..<f.passkeys, id: \.self) { i in
        if i > 0 { RowRule() }
        SettingsRow(title: f.passkeys > 1 ? "Passkey \(i + 1)" : "Passkey") { Sketch("key") }
      }
    }
    if model.passkeysOn || f.passkeys > 0 {
      SettingsGroup(header: "Add a Passkey") { ProofBox(hasPassword: f.hasPassword, password: $proof, label: "Your Password") }
      Button("Add Passkey") {
        let p = proof
        proof = ""
        state.run(model, passkey: true) {
          guard let r = model.room else { return }
          if f.hasPassword { try await r.addPasskey(password: p) { try await Passkeys.make($0) } }
          // (a passkey of the account opens its key, which the new passkey then holds too)
          else { try await r.addPasskey(way: try await model.wayIn(r, password: nil)) { try await Passkeys.make($0) } }
          state.said = "Passkey added."; await state.load(model)
        }
      }.buttonStyle(QuietWay()).disabled(f.hasPassword && proof.isEmpty)
    }
  }

  @ViewBuilder private func kitPart(_ f: AccountFacts) -> some View {
    if let k = kit, let st = state.status {
      SettingsGroup(footer: "Save or print it. It is shown only now; the old kit no longer works.") {
        VStack(alignment: .leading, spacing: 10) {
          // (the same sheet and the same file as the kit's page: KitScreen.swift)
          KitSheet(email: st.email, words: k, accountId: st.accountId, qr: model.kitQR(st.accountId))
          HStack(spacing: 12) {
            ShareLink(item: KitFile(text: emergencyKitText(EmergencyKit(words: k, email: st.email, accountId: st.accountId))), preview: SharePreview(KIT_FILE_NAME)) { Text("Save") }.buttonStyle(QuietWay())
            Button("Print") { printKit(EmergencyKit(words: k, email: st.email, accountId: st.accountId), qr: model.kitQR(st.accountId)) }.buttonStyle(QuietWay())
          }
        }.padding(16)
      }
    } else {
      SettingsGroup(footer: (f.hasPassword ? "With it you can set a new password if you forget yours." : "It opens your account if you lose your passkey.") + (f.hasRecovery ? " A new kit makes the old one stop working." : "")) {
        SettingsRow(title: "Your Kit", detail: f.hasRecovery ? "Made" : "Not Made Yet") { Sketch("page") }
      }
      SettingsGroup(header: f.hasRecovery ? "Make a New Kit" : "Make the Emergency Kit") { ProofBox(hasPassword: f.hasPassword, password: $proof, label: "Your Password") }
      Button(f.hasPassword ? "Make the Kit" : "Make with Passkey") {
        let p = proof
        proof = ""
        state.run(model, passkey: !f.hasPassword) {
          guard let r = model.room else { return }
          kit = f.hasPassword ? try await r.makeEmergencyKit(password: p).words : try await r.makeEmergencyKit(way: try await model.wayIn(r, password: nil)).words
          await state.load(model)
        }
      }.buttonStyle(QuietWay()).disabled(f.hasPassword && proof.isEmpty)
      // (spec/v1.md 8.6: the room gets a new recovery code; the new kit shows here)
      SettingsGroup(footer: "For after you removed a device that is lost or no longer yours. You get a new Emergency Kit; the old kit stops working, and passkeys have to be added again.") {
        Button { codeAsk = true } label: { SettingsRow(title: "New Recovery Code…") { Sketch("key") } }.buttonStyle(.plain)
      }
      .alert("New Recovery Code", isPresented: $codeAsk) {
        if f.hasPassword { SecureField("Your Password", text: $proof) }
        Button(f.hasPassword ? "Make New Code" : "Use Passkey") {
          let p = proof
          proof = ""
          state.run(model, passkey: !f.hasPassword) {
            guard let r = model.room else { return }
            kit = f.hasPassword ? try await r.replaceRecoveryCode(way: .password(p)).words : try await r.replaceRecoveryCode(way: try await model.wayIn(r, password: nil)).words
            state.said = "New recovery code made. Save your new Emergency Kit."; await state.load(model)
          }
        }
        Button("Cancel", role: .cancel) { proof = "" }
      } message: { Text(f.hasPassword ? "Your password opens the account. Then save or print the new Emergency Kit." : "Your passkey opens the account. Then save or print the new Emergency Kit.") }
    }
  }
}

// ---- Theme -----------------------------------------------------------------------------------------------------

struct ThemePage: View {
  @EnvironmentObject var model: BoardModel
  var body: some View {
    SettingsPage(title: "Theme") {
      SettingsGroup(footer: "System follows the iPhone’s appearance.") {
        ForEach(Array(ThemeMode.allCases.enumerated()), id: \.element) { i, t in
          if i > 0 { RowRule() }
          Button { model.theme = t } label: {
            SettingsRow(title: t.word) {
              Group { if t == .dark { Sketch("moon") } else if t == .light { Sketch("sun") } else { Sketch("frame") } }
            }
            .overlay(alignment: .trailing) { if model.theme == t { Sketch("tick", color: Ink.accent).frame(width: 20, height: 20).padding(.trailing, 16) } }
          }.buttonStyle(.plain)
          .accessibilityAddTraits(model.theme == t ? .isSelected : [])
        }
      }
    }
  }
}

// ---- inviting a device ------------------------------------------------------------------------------------------

/** Invite a Device (his decision, 8 October): the QR code blurred until he taps Show Code (the code is a key to the room
 *  while it is open, so the invite is made only then); the new device scans it; both show six emoji; They Match. */
struct InviteDeviceSheet: View {
  @EnvironmentObject var model: BoardModel
  @Environment(\.dismiss) private var dismiss
  @StateObject private var pairing = PairingModel()
  @State private var revealed = false
  var body: some View {
    NavigationStack {
      ScrollView {
        VStack(spacing: 18) {
          Text("On the new device open Trommi (the app or app.trommi.com) and choose Scan QR Code. Both devices then show six emoji: if they are the same, tap They Match.")
            .font(Face.text(16)).foregroundStyle(Ink.muted).multilineTextAlignment(.center)
          switch pairing.state {
          case .open(let link, let until) where revealed:
            QRCode(text: link).frame(width: 240, height: 240).padding(12).background(RoundedRectangle(cornerRadius: 16).fill(.white))
            HStack(spacing: 18) {
              ShareLink(item: link) { Label("Send Link", systemImage: "square.and.arrow.up").font(Face.text(15, .medium)) }
              Button { revealed = false; pairing.cancel(); pairing.state = .making } label: { Label("Hide Code", systemImage: "eye.slash").font(Face.text(15, .medium)) }
            }
            TimelineView(.periodic(from: .now, by: 30)) { _ in
              Text("Waiting for the new device… The code works once, \(validFor(until)).").font(Face.text(14)).foregroundStyle(Ink.muted)
            }
          case .confirm(let code):
            Text("A device wants to join. Does it show these six emoji, in this order?").font(Face.text(17, .medium)).multilineTextAlignment(.center)
            EmojiGrid(code: code)
            HStack(spacing: 12) {
              Button { pairing.confirm(false) } label: { Text("They Don’t Match").font(Face.text(16, .semibold)).frame(maxWidth: .infinity, minHeight: 52) }.buttonStyle(TileStyle(lead: false, hue: 162))
              Button { pairing.confirm(true) } label: { Text("They Match").font(Face.text(16, .semibold)).frame(maxWidth: .infinity, minHeight: 52) }.buttonStyle(TileStyle(lead: true, hue: 162))
            }
            Text("They Don’t Match burns the invite: nobody is added.").font(Face.text(13)).foregroundStyle(Ink.muted)
          case .adding: ProgressView("Adding the device…").padding(40)
          case .joined(let name):
            Text("✓ \(name.isEmpty ? "The new device" : name) is in now.").font(Face.display(22, .bold)).foregroundStyle(Ink.accent)
            Button("Invite Another") { revealed = false; pairing.state = .making }.buttonStyle(QuietWay())
          case .failed(let why):
            Text(why).font(Face.text(16)).foregroundStyle(Ink.urgCritical).multilineTextAlignment(.center)
            Button("Try Again") { reveal() }.buttonStyle(QuietWay())
          default:
            // the code, blurred: a stand-in until he reveals the real one (nothing is made before)
            Button(action: reveal) {
              ZStack {
                QRCode(text: "https://app.trommi.com/join#v1.stand-in-for-the-blur").frame(width: 240, height: 240).padding(12)
                  .background(RoundedRectangle(cornerRadius: 16).fill(.white)).blur(radius: 10).opacity(0.75)
                if revealed { ProgressView() } else {
                  Label("Show Code", systemImage: "eye").font(Face.text(16, .semibold)).foregroundStyle(Ink.fg)
                    .padding(.horizontal, 16).padding(.vertical, 10).glass(Capsule(), interactive: true)
                }
              }
            }.buttonStyle(.plain).accessibilityLabel("Show Code")
          }
        }
        .padding(24).frame(maxWidth: .infinity)
      }
      .background(Ink.bg.ignoresSafeArea())
      .navigationTitle("Invite a Device").navigationBarTitleDisplayMode(.inline)
      .toolbar { ToolbarItem(placement: .topBarTrailing) { Button("Done") { pairing.cancel(); dismiss() } } }
    }
    .onDisappear { pairing.cancel() }
  }
  private func reveal() {
    revealed = true
    if model.demo { pairing.state = .failed("The demo invites nobody: it is a made-up room on this phone."); return }
    pairing.start(model.room)
  }
}

struct EmojiGrid: View {
  let code: String
  var body: some View {
    let e = checkEmoji(code)
    LazyVGrid(columns: Array(repeating: GridItem(.flexible()), count: 3), spacing: 14) {
      ForEach(Array(e.enumerated()), id: \.offset) { _, x in
        VStack(spacing: 2) { Text(x.emoji).font(.system(size: 46)); Text(x.word).font(Face.text(13)).foregroundStyle(Ink.muted) }
      }
    }
    .accessibilityLabel("Check code: \(e.map { $0.word }.joined(separator: ", "))")
  }
}

/** A QR code, drawn crisp (CoreImage, error correction M as the web's). */
struct QRCode: View {
  let text: String
  /** What VoiceOver calls it: the code to pair a device, or (the Emergency Kit's sheet) the one with the account id. */
  var label = "QR code to pair"
  var body: some View {
    #if canImport(UIKit)
    if let img = qrImage(text) { Image(uiImage: img).interpolation(.none).resizable().scaledToFit().accessibilityLabel(label) }
    #endif
  }
}
#if canImport(UIKit)
/** A QR code as a picture: for `QRCode`, and for the printed Emergency Kit (KitScreen.swift). */
func qrImage(_ text: String) -> UIImage? {
  let f = CIFilter.qrCodeGenerator()
  f.message = Data(text.utf8)
  f.correctionLevel = "M"
  guard let out = f.outputImage?.transformed(by: CGAffineTransform(scaleX: 10, y: 10)), let cg = CIContext().createCGImage(out, from: out.extent) else { return nil }
  return UIImage(cgImage: cg)
}
#endif

/** The clipboard: kraft board, a sheet of paper, the metal clamp on top. */
struct Clipboard: View {
  var body: some View {
    ZStack(alignment: .top) {
      RoundedRectangle(cornerRadius: 14, style: .continuous).fill(Color.dyn(0x8a6d45, 0x6b5638)).padding(-8)
      RoundedRectangle(cornerRadius: 8, style: .continuous).fill(Ink.surface)
      PenMark("auth:CLAMP", color: Ink.fg).frame(width: 110, height: 40).offset(y: -34)
    }
  }
}

/** The pairing in progress on this phone: make the link, wait for the newcomer, confirm. */
@MainActor final class PairingModel: ObservableObject {
  enum State { case making, open(String, UInt64), confirm(String), adding, joined(String), failed(String) }
  @Published var state: State = .making
  private var pairing: Room.Pairing?
  private var room: Room?
  private var watch: Task<Void, Never>?
  func start(_ room: Room?) {
    guard let room = room else { state = .failed("Not signed in."); return }
    self.room = room
    state = .making
    watch?.cancel()
    watch = Task {
      do {
        var p = try await room.createPairing()
        pairing = p
        state = .open(p.link, p.expiresAt)
        while !Task.isCancelled {
          if try await room.checkPairing(&p) { pairing = p; state = .confirm(p.code ?? ""); return }
          try await Task.sleep(nanoseconds: 1_000_000_000)
        }
      } catch is CancellationError {
      } catch { state = .failed(why(error)) }
    }
  }
  func confirm(_ yes: Bool) {
    guard let room = room, let p = pairing else { return }
    state = .adding
    Task {
      do {
        try await room.confirmPairing(p, matches: yes)
        let name = p.newcomerId.flatMap { room.board.members[$0]?.deviceName } ?? ""
        state = .joined(name)
      } catch { state = .failed(why(error)) }
    }
  }
  func cancel() { watch?.cancel() }
  private func why(_ e: Error) -> String {
    switch (e as? TrommiError)?.code {
    case "code-mismatch": return "They did not match. Nobody was added; the invite is used up."
    case "invite-expired": return "The invite has expired."
    default: return "That did not work: \((e as? TrommiError)?.message ?? (e as? HubError)?.message ?? "\(e)")"
    }
  }
}


/** Inviting an agent: 1 the installer, once per machine, 2 in the project folder inside claude (or codex) the connect command with the link, 3 compare the six emoji. */
struct AgentInviteSheet: View {
  @EnvironmentObject var model: BoardModel
  @Environment(\.dismiss) private var dismiss
  let label: String
  @State private var state = "making"     // making | open | confirm | adding | joined | failed
  @State private var inv: Room.AgentInvite?
  @State private var code = ""
  @State private var error = ""
  @State private var task: Task<Void, Never>?
  @State private var demoLink: String?
  var body: some View {
    NavigationStack {
      ScrollView {
        VStack(alignment: .leading, spacing: 18) {
          Text(label.isEmpty ? "Invite Agent" : "Invite \(label)").font(Face.display(26, .heavy))
          let steps = agentConnectSteps(link: inv?.pairing.link ?? demoLink ?? "")
          let past = state != "making" && state != "open"
          step(1, done: past, steps[0].title) {
            CodeChip(text: steps[0].command)
            Text(steps[0].note).font(Face.text(13)).foregroundStyle(Ink.muted)
          }
          step(2, done: past, steps[1].title) {
            if state == "open", inv != nil || demoLink != nil { CodeChip(text: steps[1].command) }
            Text(steps[1].note).font(Face.text(13)).foregroundStyle(Ink.muted)
          }
          step(3, done: state == "joined", state == "confirm" ? "An agent wants to join. Its terminal shows six emoji, each with a word. Are they these, in this order?" : "Compare the six emoji Claude shows with the ones here") {
            if state == "confirm" {
              EmojiGrid(code: code)
              HStack(spacing: 12) {
                Button { confirm(false) } label: { Text("They Don’t Match").font(Face.text(16, .semibold)).frame(maxWidth: .infinity, minHeight: 50) }.buttonStyle(TileStyle(lead: false, hue: 162))
                Button { confirm(true) } label: { Text("They Match").font(Face.text(16, .semibold)).frame(maxWidth: .infinity, minHeight: 50) }.buttonStyle(TileStyle(lead: true, hue: 162))
              }
            } else if state == "adding" { ProgressView("Adding the agent…") }
            else if state == "joined" { Text("✓ The agent is in.").font(Face.text(17, .semibold)).foregroundStyle(Ink.accent) }
            else if state == "failed" { Text(error).font(Face.text(15)).foregroundStyle(Ink.urgCritical) }
          }
          if state == "open", let i = inv { TimelineView(.periodic(from: .now, by: 30)) { _ in Text("The link works once · \(validFor(i.pairing.expiresAt)).").font(Face.text(13)).foregroundStyle(Ink.muted) } }
        }.padding(22)
      }
      .background(Clipboard().padding(10).ignoresSafeArea(edges: .bottom))
      .toolbar { ToolbarItem(placement: .topBarTrailing) { Button("Done") { task?.cancel(); dismiss() } } }
    }
    .onAppear(perform: start)
  }
  @ViewBuilder private func step<C: View>(_ n: Int, done: Bool, _ title: String, @ViewBuilder _ content: () -> C) -> some View {
    HStack(alignment: .top, spacing: 12) {
      PenMark(done ? "desk:BOX_TICK" : "desk:BOX", color: Ink.fg).frame(width: 26, height: 26)
      VStack(alignment: .leading, spacing: 10) { Text(title).font(Face.text(17, .semibold)).foregroundStyle(Ink.fg); content() }
    }
  }
  private func start() {
    // (the demo shows the three lines with a made-up link: nothing is invited)
    guard let room = model.room else { if model.demo { demoLink = "https://app.trommi.com/join#v1.ZGVtbw.ZGVtbw.ZGVtbw.ZGVtbw"; state = "open" }; return }
    task = Task {
      do {
        var i = try await room.createAgentInvite(label: label.isEmpty ? nil : label, desk: model.view?.all == true ? nil : model.view?.deskId)
        inv = i; state = "open"
        while !Task.isCancelled {
          if try await room.checkPairing(&i.pairing) { inv = i; code = i.pairing.code ?? ""; state = "confirm"; return }
          try await Task.sleep(nanoseconds: 1_000_000_000)
        }
      } catch is CancellationError {} catch { self.error = model.describe(error); state = "failed" }
    }
  }
  private func confirm(_ yes: Bool) {
    guard let room = model.room, let i = inv else { return }
    state = "adding"
    Task {
      do { try await room.confirmAgent(i, matches: yes); state = "joined" }
      catch { self.error = (error as? TrommiError)?.code == "code-mismatch" ? "They did not match: nobody was added." : model.describe(error); state = "failed" }
    }
  }
}

