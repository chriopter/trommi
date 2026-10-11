// DataScreen: Settings → Data. What this device sent and received (counted here since install or the last reset,
// TrommiClient Traffic.swift) and what the account keeps on the hub (counts and sizes only, GET /v1/account/usage).
import SwiftUI
import TrommiClient

func megabytes(_ b: Int64) -> String {
  let f = ByteCountFormatter(); f.allowedUnits = [.useKB, .useMB, .useGB]; f.countStyle = .file
  return f.string(fromByteCount: b)
}
private func number(_ n: Int) -> String { NumberFormatter.localizedString(from: NSNumber(value: n), number: .decimal) }

extension AccountUsage {
  /** The demo's: made up. */
  static var demo: AccountUsage {
    var u = AccountUsage()
    u.rooms = 1; u.groups = 41; u.liveGroups = 38; u.devices = 9; u.messages = 7_215; u.envelopes = 7_215; u.cards = 570; u.openCards = 31
    u.files = 214; u.fileBytes = 182_400_000; u.totalBytes = 214_900_000
    return u
  }
}

/** Settings → Data. */
struct DataPage: View {
  @EnvironmentObject var model: BoardModel
  @State private var traffic = Traffic.totals
  @State private var usage: AccountUsage?
  @State private var failed = false
  @State private var askReset = false
  var body: some View {
    SettingsPage(title: "Data") {
      SettingsGroup(header: "This \(UIDeviceName.model)", footer: "Counted on this \(UIDeviceName.model) since \(traffic.since.formatted(date: .abbreviated, time: .omitted)). Pictures and files you open count too.") {
        SettingsRow(title: "Downloaded", detail: megabytes(model.demo ? 48_300_000 : traffic.down)) { icon("arrow.down.circle") }
        RowRule()
        SettingsRow(title: "Uploaded", detail: megabytes(model.demo ? 3_120_000 : traffic.up)) { icon("arrow.up.circle") }
        RowRule()
        Button { askReset = true } label: {
          SettingsRow(title: "Reset Count", tint: Ink.accent) { Image(systemName: "arrow.counterclockwise").font(.system(size: 18)).foregroundStyle(Ink.accent) }
        }.buttonStyle(.plain).accessibilityIdentifier("data-reset")
      }
      SettingsGroup(header: "Stored on the hub", footer: failed ? "Not reachable now. Pull to try again." : "All of it encrypted: the hub knows how much, never what.") {
        let u = usage ?? AccountUsage()
        let shown = usage != nil
        SettingsRow(title: "Total", detail: shown ? megabytes(u.totalBytes) : "…") { icon("externaldrive") }
        RowRule()
        SettingsRow(title: "Messages", detail: shown ? number(u.envelopes) : "…") { icon("bubble.left") }
        RowRule()
        SettingsRow(title: "Cards", detail: shown ? "\(number(u.cards)) · \(number(u.openCards)) open" : "…") { Sketch("desk") }
        RowRule()
        SettingsRow(title: "Files", detail: shown ? "\(number(u.files)) · \(megabytes(u.fileBytes))" : "…") { icon("doc") }
        RowRule()
        SettingsRow(title: "Sessions · Devices", detail: shown ? "\(number(u.liveGroups)) · \(number(u.devices))" : "…") { icon("square.stack.3d.up") }
      }
    }
    .refreshable { await load() }
    .task { await load() }
    .confirmationDialog("Reset the count of this \(UIDeviceName.model)?", isPresented: $askReset, titleVisibility: .visible) {
      Button("Reset Count", role: .destructive) { if !model.demo { Traffic.reset() }; traffic = Traffic.totals }
    }
  }
  private func icon(_ name: String) -> some View { Image(systemName: name).font(.system(size: 19)).foregroundStyle(Ink.fg) }
  private func load() async {
    traffic = Traffic.totals
    if model.demo { usage = .demo; return }
    guard let r = model.room else { return }
    do { usage = try await r.accountUsage() ?? AccountUsage(); failed = false } catch { failed = true }
  }
}
