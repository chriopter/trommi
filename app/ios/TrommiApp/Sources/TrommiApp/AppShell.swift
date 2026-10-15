import SwiftUI
import TrommiClient
import UniformTypeIdentifiers
#if canImport(UIKit)
import UIKit
#else
import AppKit
#endif

enum NativeTab: Equatable { case chat, desk }

// Published state is the generated view fold plus drawing/platform state. There is no second board model.
@MainActor final class AppShell: ObservableObject {
  static let shared = AppShell()
  @Published var view = CoreView.ViewModel()
  @Published var screen = CoreView.Screen.desk
  @Published var path: [CoreView.Screen] = []
  @Published var tab = NativeTab.desk
  @Published var files: [String: Data] = [:]
  @Published var saved: SavedFile?
  @Published var isDemo = false
  @Published var platformError: String?
  private var session: CoreSession?
  private var tail: Task<Void, Never>?
  private var started = false
  private var inboxEvents: [NativePacket] = []
  var snapshot: [String: Any] = [:]
  func start(demo: Bool = false) async {
    guard !started else { return }; started = true
    if NativeDemo.measuring { Spent.add("ui.before_start", since: NativeDemo.launchTicks) }
    isDemo = demo || NativeDemo.screen != nil
    view = CoreView.ViewModel(); files = [:]; saved = nil; snapshot = [:]
    do {
      #if os(macOS)
      LocalKey.systemKeychain = true
      #endif
      let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("Trommi/AppCore/default")
      let preview = isDemo
      session = try Spent.time("session.init") { try CoreSession(directory: preview ? nil : dir, network: NativeNetworkHooks(cached: { request in if preview { return NativeDemo.file(request) }; return await NativeServices.shared.prefetched(request) }, before: { _, url in preview && !url.hasPrefix("/demo/files/") ? "denied" : nil })) { [weak self] packet in await self?.effect(packet) } }
      var config: [String: Any] = ["platform": Self.platform, "version": Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "1", "slot": "default", "seed": UInt64.random(in: 0..<(1 << 53)), "locale": Locale.current.identifier, "utc_offset_min": TimeZone.current.secondsFromGMT() / 60, "can": ["notify": true, "secrets": true, "snapshots": true, "push": "apns", "passkeys": false]]
      #if targetEnvironment(simulator)
      // UI automation uses the real executor and store against its own loopback hub.
      if let hub = ProcessInfo.processInfo.environment["TROMMI_TEST_HUB"],
         let url = URL(string: hub), url.scheme == "http", url.host == "127.0.0.1" { config["hub"] = hub }
      #endif
      if preview { let fixture: Any = try Spent.time("fixture") { try NativeDemo.fixture() }; config["demo"] = fixture; config["can"] = [:] }
      await session?.send(NativePacket(["type": "start", "config": config]))
      if !preview {
        for id in UserDefaults.standard.stringArray(forKey: "trommi-goals-folded") ?? [] {
          await session?.send(NativePacket(["type": "action", "action": ["do": "fold_goals", "desk_id": id, "folded": true]]))
        }
      }
      for event in inboxEvents { await session?.send(event) }; inboxEvents = []
      if preview { NativeDemo.route(NativeDemo.screen ?? "desk", shell: self) } else { NativeServices.shared.attach(self) }
    } catch { platformError = "The encrypted device store could not be opened." }
  }
  func enterDemo() {
    Task { await tail?.value; await session?.close(); session = nil; started = false; screen = .desk; path = []; tab = .desk; await start(demo: true) }
  }
  func leaveDemo() {
    Task { await tail?.value; await session?.close(); session = nil; started = false; screen = .desk; path = []; tab = .desk; await start() }
  }
  static var platform: String {
    #if os(macOS)
    "mac"
    #else
    "ios"
    #endif
  }
  func send(_ json: [String: Any], blobs: [Data] = []) {
    let event = NativePacket(json, blobs: blobs)
    guard let session else { inboxEvents.append(event); return }
    let before = tail
    tail = Task { await before?.value; await session.send(event) }
  }
  func act(_ action: [String: Any], blobs: [Data] = []) {
    if !isDemo, action["do"] as? String == "fold_goals", let id = action["desk_id"] as? String {
      var folded = UserDefaults.standard.stringArray(forKey: "trommi-goals-folded") ?? []
      folded.removeAll { $0 == id }; if action["folded"] as? Bool == true { folded.append(id) }
      UserDefaults.standard.set(folded, forKey: "trommi-goals-folded")
    }
    send(["type": "action", "action": action], blobs: blobs)
  }
  func account(_ action: [String: Any]) { act(["do": "account", "action": action]) }
  func focus(_ screen: CoreView.Screen, desk: String? = nil) {
    if screen == .desk { path = []; tab = .desk }
    else if path.last != screen { path.append(screen) }
    show(screen, desk: desk)
  }
  func show(_ screen: CoreView.Screen, desk: String? = nil) {
    self.screen = screen
    let focus = CoreView.Focus(screen: screen, visible: true, desk: desk ?? view.sidebar.desk)
    if let data = try? JSONEncoder().encode(focus), let json = try? JSONSerialization.jsonObject(with: data) { send(["type": "focus", "focus": json]) }
  }
  func scene(_ active: Bool) { send(["type": active ? "foreground" : "background"]); if !isDemo { NativeServices.shared.foreground(active) } }
  @discardableResult func attach(_ url: URL) -> String? {
    let access = url.startAccessingSecurityScopedResource(); defer { if access { url.stopAccessingSecurityScopedResource() } }
    guard let data = try? Data(contentsOf: url) else { return nil }
    let id = UUID().uuidString
    act(["do": "attach", "local_id": id, "name": url.lastPathComponent, "media_type": UTType(filenameExtension: url.pathExtension)?.preferredMIMEType ?? "application/octet-stream", "bytes": 0], blobs: [data])
    return id
  }
  private func effect(_ packet: NativePacket) async {
    let e = packet.json
    switch e["type"] as? String {
    case "render":
      do {
        let delta = try Spent.time("view.decode") { try JSONDecoder().decode(CoreView.ViewDelta.self, from: JSONSerialization.data(withJSONObject: e["delta"]!)) }
        try Spent.time("view.fold") { try view.apply(delta) }
        if NativeDemo.measuring { NativeDemo.foldedAt = (ProcessInfo.processInfo.systemUptime - NativeDemo.launchStart) * 1000 }
        if view.phase == .signedOut { files = [:]; saved = nil; screen = .desk; path = []; tab = .desk }
        if !isDemo { NativeServices.shared.draw(view) }
      } catch { platformError = "The view update was out of order." }
    case "file_ready": if let id = e["file_id"] as? String { files[id] = try? packet.bytes(e["bytes"]) }
    case "file_gone": if let id = e["file_id"] as? String { files[id] = nil }
    case "save_file": if let data = try? packet.bytes(e["bytes"]) { saved = SavedFile(name: e["name"] as? String ?? "Trommi", data: data) }
    case "open_url": if let text = e["url"] as? String, let url = URL(string: text) {
      #if canImport(UIKit)
      _ = await UIApplication.shared.open(url)
      #else
      NSWorkspace.shared.open(url)
      #endif
    }
    case "platform": Task { await NativeServices.shared.platform(packet, shell: self) }
    case "notify": NativeServices.shared.notify(e["notice"] as? [String: Any] ?? [:])
    case "forget_shared": NativeShare.shared.forget()
    case "snapshot": snapshot = e["snapshot"] as? [String: Any] ?? [:]; NativeServices.shared.snapshot(snapshot)
    case "share_inbox": if let id = e["id"] as? String { NativeShare.shared.completed(id, consumed: e["consumed"] as? Bool == true) }
    case "notify_context": NativeServices.shared.context(packet)
    case "log": break // The core's diagnostics contain no user content; no credentials are logged by the shell.
    default: break
    }
  }
}
struct SavedFile: Identifiable { let id = UUID(); let name: String; let data: Data }
