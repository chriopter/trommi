// The pages of Settings that mirror the web's one for one: Security (the core's SecurityView), one device
// (/settings/devices/<id>), the self test (/settings/proof), Log out (/logout) and a team's delete confirm
// (/settings/teams). Every word that depends on the account is the core's; the app only draws it.
import SwiftUI
import TrommiClient

/** A drawing of a row by the core's icon name. */
struct RowIcon: View {
  let icon: String
  var body: some View {
    if icon == "phone" { PenMark("draw:phone", color: Ink.fg) } else { Sketch(icon.isEmpty ? "laptop" : icon) }
  }
}

// ---- Security --------------------------------------------------------------------------------------------------

/** Settings → Security: the core's groups (Account key, Authorized devices, Agents, Check), each row opening its page. */
struct SecurityPage: View {
  @EnvironmentObject var shell: AppShell
  let security: CoreView.SecurityView
  var body: some View {
    VStack(alignment: .leading, spacing: 26) {
      if !security.lead.en.isEmpty { Text(security.lead.en).font(Face.text(15)).foregroundStyle(Ink.muted).padding(.horizontal, 16) }
      ForEach(security.sections, id: \.id) { section in
        VStack(alignment: .leading, spacing: 7) {
          SetGroup(header: section.label.en) {
            if section.rows.isEmpty {
              Text("—").font(Face.text(15)).foregroundStyle(Ink.muted).padding(16).frame(maxWidth: .infinity, alignment: .leading)
            }
            ForEach(Array(section.rows.enumerated()), id: \.element.id) { i, row in
              if i > 0 { SetRule() }
              line(row)
            }
          }
          ForEach(Array(section.notes.enumerated()), id: \.offset) { _, note in
            Text(note.en).font(Face.text(13)).foregroundStyle(Ink.muted).padding(.horizontal, 16).fixedSize(horizontal: false, vertical: true)
          }
          if let strong = section.strong {
            Text(strong.en).font(Face.text(13, .semibold)).foregroundStyle(Ink.fg).padding(.horizontal, 16).padding(.top, 4).fixedSize(horizontal: false, vertical: true)
          }
        }
        .accessibilityIdentifier("security-" + section.id)
      }
      DictationFacts()
    }
  }
  @ViewBuilder private func line(_ row: CoreView.SecurityRow) -> some View {
    let target = destination(row)
    let content = HStack(spacing: 14) {
      RowIcon(icon: row.icon).frame(width: 26, height: 26)
      VStack(alignment: .leading, spacing: 2) {
        HStack(spacing: 4) {
          Text(row.title.en).font(Face.text(17, .medium)).foregroundStyle(Ink.fg).lineLimit(1)
          if let mark = row.mark { Text("· \(mark.en)").font(Face.text(13)).foregroundStyle(Ink.muted).lineLimit(1) }
        }
        if !row.fingerprint.isEmpty || row.detail != nil {
          (Text(row.fingerprint).font(Face.mono(12)) + Text(!row.fingerprint.isEmpty && row.detail != nil ? " · " : "") + Text(row.detail?.en ?? ""))
            .font(Face.text(12)).foregroundStyle(Ink.muted).lineLimit(2).textSelection(.enabled)
        }
      }
      Spacer(minLength: 8)
      if let says = row.says { Text(says.en).font(Face.text(14)).foregroundStyle(Ink.muted).lineLimit(1) }
      if target != nil { Image(systemName: "chevron.right").font(.system(size: 13, weight: .semibold)).foregroundStyle(Ink.faint) }
    }
    .padding(.horizontal, 16).padding(.vertical, 8).frame(minHeight: 56).contentShape(Rectangle())
    if let target {
      Button { shell.focus(.settings(page: target)) } label: { content }.buttonStyle(.plain).accessibilityIdentifier("security-row-" + row.id)
    } else {
      content.accessibilityElement(children: .combine).accessibilityIdentifier("security-row-" + row.id)
    }
  }
  /** The page a row opens, by the core's `open`. */
  private func destination(_ row: CoreView.SecurityRow) -> String? {
    switch row.open {
    case "kit": "account"
    case "agents": "agents"
    case "proof": "proof"
    case "device": "device:" + row.id
    default: nil
    }
  }
}

// ---- One device --------------------------------------------------------------------------------------------------

/** One of the person's devices: its kind, its state, its key fingerprint; this device renames itself, another is removed. */
struct DevicePage: View {
  @EnvironmentObject var shell: AppShell
  let device: CoreView.DeviceRow?
  @State private var name = ""
  var body: some View {
    if let d = device {
      VStack(alignment: .leading, spacing: 26) {
        SetGroup(footer: "The fingerprint comes from the signed member list: it must look the same on every device.") {
          SetRow(title: "Kind", detail: d.role == "human" ? "Person" : "Agent") { RowIcon(icon: d.icon) }
          SetRule()
          SetRow(title: "Status", detail: d.says.en) { Circle().fill(d.active && d.online ? Ink.stDone : Ink.lineStrong).frame(width: 9, height: 9) }
          SetRule()
          HStack(spacing: 14) {
            Sketch("key").frame(width: 26, height: 26)
            Text("Key fingerprint").font(Face.text(17, .medium)).foregroundStyle(Ink.fg).lineLimit(1)
            Spacer(minLength: 8)
            Text(d.fingerprint).font(Face.mono(13)).foregroundStyle(Ink.muted).lineLimit(1).minimumScaleFactor(0.7).textSelection(.enabled)
          }.padding(.horizontal, 16).frame(minHeight: 52).accessibilityIdentifier("device-fp")
        }
        if d.thisDevice && d.active {
          SetGroup(header: "Name", footer: "A device names itself; this name is what your other devices see.") {
            SetField(title: "Name of this device", text: $name)
            SetRule(inset: 16)
            Button { let n = String(name.trimmingCharacters(in: .whitespaces).prefix(40)); if !n.isEmpty { shell.act(["do": "rename_device", "name": n]) } } label: {
              SetRow(title: "Rename", tint: Ink.accent) { Sketch("pen", color: Ink.accent) }
            }.buttonStyle(.plain)
          }
          .onAppear { name = d.name }
        }
        if d.active && !d.thisDevice {
          SetGroup(footer: "\(d.name) can open nothing new after this. Everyone else gets a new key; that takes a moment.") {
            Button { shell.account(["step": "remove_devices", "device_ids": [d.id]]); shell.focus(.settings(page: "devices")) } label: {
              SetRow(title: "Remove \(d.name)", tint: Ink.urgCritical) { Sketch("logout", color: Ink.urgCritical) }
            }.buttonStyle(.plain)
          }
        }
      }
    } else {
      Text("This device is no longer listed.").font(Face.text(15)).foregroundStyle(Ink.muted).padding(.horizontal, 16)
    }
  }
}

// ---- Self test ---------------------------------------------------------------------------------------------------

/** The Rust core tests itself on this device, off the main thread: the outcome, every step with its time, what ran.
 *  Nothing is sent, nothing is stored (the web's proof.mjs). */
struct SelfTestPage: View {
  @State private var report: SelfTestReport?
  @State private var running = false
  @State private var copied = false
  var body: some View {
    VStack(alignment: .leading, spacing: 22) {
      Text("The encryption core tests itself on this device. Nothing is sent, nothing is stored.").font(Face.text(15)).foregroundStyle(Ink.muted).padding(.horizontal, 16)
      Text(headline).font(Face.text(17, .semibold)).foregroundStyle(report == nil ? Ink.muted : report!.ok ? Ink.stDone : Ink.urgCritical)
        .padding(.horizontal, 16).accessibilityIdentifier("proof-line")
      if let r = report, !r.steps.isEmpty {
        SetGroup(header: "Steps") {
          ForEach(Array(r.steps.enumerated()), id: \.offset) { i, step in
            if i > 0 { SetRule(inset: 16) }
            HStack(alignment: .firstTextBaseline, spacing: 10) {
              Image(systemName: step.ok ? "checkmark" : "xmark").font(.system(size: 13, weight: .bold)).foregroundStyle(step.ok ? Ink.stDone : Ink.urgCritical)
              VStack(alignment: .leading, spacing: 2) {
                Text(step.name).font(Face.text(15)).foregroundStyle(Ink.fg)
                if !step.detail.isEmpty { Text(step.detail).font(Face.text(12)).foregroundStyle(Ink.muted) }
              }
              Spacer(minLength: 8)
              Text(Self.ms(step.micros)).font(Face.mono(12)).foregroundStyle(Ink.muted)
            }.padding(.horizontal, 16).padding(.vertical, 10)
          }
        }
      }
      if let r = report { facts("Versions", versions(r)) }
      facts("This device", here)
      HStack(spacing: 10) {
        Button("Run again") { run() }.disabled(running)
        Button(copied ? "Copied" : "Copy result") { if let r = report { copyText(text(r)); copied = true } }.disabled(report == nil)
      }
      .buttonStyle(.bordered).tint(Ink.accent).padding(.horizontal, 16)
    }
    .task { if report == nil { run() } }
  }
  private var headline: String {
    guard let r = report else { return "The Rust core is testing itself…" }
    if r.ok { return "OK: the Rust core ran \(r.steps.count) steps in \(Int((Double(r.micros) / 1000).rounded())) ms" }
    return "FAIL: at \"\(r.steps.first(where: { !$0.ok })?.name ?? "an unnamed step")\""
  }
  private func run() {
    running = true; copied = false; report = nil
    Task {
      let now = UInt64(Date().timeIntervalSince1970 * 1000)
      let r = await Task.detached(priority: .userInitiated) { selfTest(nowMs: now) }.value
      report = r; running = false
    }
  }
  private static func ms(_ micros: UInt64) -> String { String(format: micros < 100_000 ? "%.1f ms" : "%.0f ms", Double(micros) / 1000) }
  private func versions(_ r: SelfTestReport) -> [(String, String)] {
    let info = Bundle.main.infoDictionary
    let app = "Trommi \(info?["CFBundleShortVersionString"] as? String ?? "?"), build \(info?["CFBundleVersion"] as? String ?? "?")"
    return [("Core", r.versions.core), ("OpenMLS", r.versions.openmls), ("Provider", r.versions.provider), ("Binding", r.versions.binding), ("App", app)]
  }
  private var here: [(String, String)] {
    [("System", ProcessInfo.processInfo.operatingSystemVersionString), ("Cores", String(ProcessInfo.processInfo.activeProcessorCount)), ("Ran in", "a task off the main thread")]
  }
  private func facts(_ title: String, _ list: [(String, String)]) -> some View {
    SetGroup(header: title) {
      ForEach(Array(list.enumerated()), id: \.offset) { i, f in
        if i > 0 { SetRule(inset: 16) }
        HStack(alignment: .firstTextBaseline) {
          Text(f.0).font(Face.text(15)).foregroundStyle(Ink.fg)
          Spacer(minLength: 12)
          Text(f.1).font(Face.mono(12)).foregroundStyle(Ink.muted).multilineTextAlignment(.trailing).textSelection(.enabled)
        }.padding(.horizontal, 16).padding(.vertical, 10)
      }
    }
  }
  private func text(_ r: SelfTestReport) -> String {
    var lines = [headline]
    for s in r.steps { lines.append("\(s.ok ? "OK" : "FAIL"): \(s.name), \(Self.ms(s.micros))"); if !s.detail.isEmpty { lines.append("  \(s.detail)") } }
    lines += (versions(r) + here).map { "\($0.0): \($0.1)" }
    return lines.joined(separator: "\n")
  }
}

// ---- Log out ---------------------------------------------------------------------------------------------------

/** Log out of this device, asked once (the web's /logout): the core's words, then SignOut (in the demo: leave it). */
struct LogoutPage: View {
  @EnvironmentObject var shell: AppShell
  var body: some View {
    let l = shell.view.settings.logout
    VStack(alignment: .leading, spacing: 18) {
      Text(l.ask.en).font(Face.text(17)).foregroundStyle(Ink.fg).fixedSize(horizontal: false, vertical: true).accessibilityIdentifier("logout-ask")
      if let note = l.note { Text(note.en).font(Face.text(17, .semibold)).foregroundStyle(Ink.fg).fixedSize(horizontal: false, vertical: true).accessibilityIdentifier("logout-last") }
      Text(l.meta.en).font(Face.text(14)).foregroundStyle(Ink.muted).fixedSize(horizontal: false, vertical: true)
      SetGroup {
        Button { if shell.isDemo { shell.leaveDemo() } else { shell.account(["step": "sign_out"]) } } label: {
          SetRow(title: l.go.en, tint: Ink.urgCritical) { Sketch("logout", color: Ink.urgCritical) }
        }.buttonStyle(.plain).accessibilityIdentifier("logout-go")
        SetRule()
        Button { shell.focus(.settings(page: "")) } label: { SetRow(title: l.cancel.en) { EmptyView() } }.buttonStyle(.plain)
      }
    }
    .padding(.horizontal, 16)
  }
}

// ---- A team's delete --------------------------------------------------------------------------------------------

/** What a team's delete takes, counted by the core, held to delete; while it runs, how far it is. */
struct TeamDeleteSheet: View {
  @EnvironmentObject var shell: AppShell
  let teamId: String
  let name: String
  @Environment(\.dismiss) private var dismiss
  @State private var holding = false
  var body: some View {
    let s = shell.view.settings, w = s.teamDelete
    let counts = s.teamCounts.first { $0.teamId == teamId }
    let going = s.deleting.first { $0.teamId == teamId }
    VStack(alignment: .leading, spacing: 16) {
      Text(counts?.title.en.isEmpty == false ? counts!.title.en : name).font(Face.display(24, .bold)).foregroundStyle(Ink.fg)
      if let p = going {
        ProgressView(value: Double(p.done), total: Double(max(p.total, 1)))
        Text("\(w.deleting.en) \(p.done) / \(p.total)").font(Face.text(14)).foregroundStyle(Ink.muted)
      } else {
        Text(w.lead.en).font(Face.text(15)).foregroundStyle(Ink.fg)
        let lines = counts?.lines ?? []
        if lines.isEmpty {
          Text(w.empty.en).font(Face.text(15)).foregroundStyle(Ink.muted)
        } else {
          VStack(alignment: .leading, spacing: 6) {
            ForEach(lines, id: \.key) { l in
              HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text("\(l.n)").font(Face.text(15, .bold)).foregroundStyle(Ink.fg).frame(minWidth: 28, alignment: .trailing)
                Text(l.word.en).font(Face.text(15)).foregroundStyle(Ink.fg)
                if let rest = l.rest { Text(rest.en).font(Face.text(13)).foregroundStyle(Ink.muted) }
              }
            }
          }
        }
        Text(w.small.en).font(Face.text(13)).foregroundStyle(Ink.muted).fixedSize(horizontal: false, vertical: true)
        HStack(spacing: 12) {
          Button(w.cancel.en) { dismiss() }.buttonStyle(.bordered)
          Spacer()
          Text(w.hold.en).font(Face.text(16, .semibold)).foregroundStyle(.white)
            .padding(.horizontal, 18).padding(.vertical, 11)
            .background(Capsule().fill(Ink.urgCritical.opacity(holding ? 0.7 : 1)))
            .onLongPressGesture(minimumDuration: 1.6, pressing: { holding = $0 }) { shell.act(["do": "remove_team", "team_id": teamId]) }
            .accessibilityAddTraits(.isButton)
            .accessibilityAction { shell.act(["do": "remove_team", "team_id": teamId]) }
            .accessibilityIdentifier("team-delete-hold")
        }
      }
      Spacer(minLength: 0)
    }
    .padding(22)
    .presentationDetents([.medium, .large])
    .onChange(of: shell.view.sidebar.teams.contains { $0.id == teamId }) { _, still in if !still { dismiss() } }
  }
}
