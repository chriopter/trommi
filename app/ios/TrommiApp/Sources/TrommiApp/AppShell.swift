import SwiftUI
import TrommiClient
import UniformTypeIdentifiers
#if canImport(UIKit)
import UIKit
#else
import AppKit
#endif

enum NativeTab: Hashable { case chat, desk, search }

// Published state is the generated view fold plus drawing/platform state. There is no second board model.
@MainActor final class AppShell: ObservableObject {
  static let shared = AppShell()
  @Published var view = CoreView.ViewModel()
  @Published var screen = CoreView.Screen.desk
  @Published var path: [CoreView.Screen] = []
  @Published var tab = NativeTab.desk
  /** The words in the iPhone's search (sent as `Focus.chats_query`; the core filters the chats). */
  @Published var chatsQuery = ""
  /** The search over every team's chats, opened from the top right of a main screen (a sheet). */
  @Published var searchOpen = false
  private var sentTeams = false
  /** The note lies over the page he came from (the iPhone's yellow page, a sheet on a wide window). */
  @Published var noteOpen = false
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
      var config: [String: Any] = ["platform": Self.platform, "version": Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "1", "slot": "default", "seed": UInt64.random(in: 0..<(1 << 53)), "locale": Locale.current.identifier, "utc_offset_min": TimeZone.current.secondsFromGMT() / 60, "can": ["notify": true, "secrets": true, "snapshots": true, "push": "apns", "passkeys": false, "dictate": NativeDictation.can]]
      #if targetEnvironment(simulator)
      // UI automation uses the real executor and store against its own loopback hub.
      if let hub = ProcessInfo.processInfo.environment["TROMMI_TEST_HUB"],
         let url = URL(string: hub), url.scheme == "http", url.host == "127.0.0.1" { config["hub"] = hub }
      #endif
      if preview { let fixture: Any = try Spent.time("fixture") { try NativeDemo.fixture() }; config["demo"] = fixture; config["can"] = [:] }
      await session?.send(NativePacket(["type": "start", "config": config]))
      if !preview {
        for id in UserDefaults.standard.stringArray(forKey: "trommi-goals-folded") ?? [] {
          await session?.send(NativePacket(["type": "action", "action": ["do": "fold_goals", "team_id": id, "folded": true]]))
        }
      }
      // (the team chosen last on this device; launched in the back by a push, the Desk is not in view)
      deskAll = preview ? true : Self.storedDeskAll
      if !preview, let data = try? JSONEncoder().encode(CoreView.Focus(screen: .desk, visible: Self.launchedInFront, team: Self.lastTeam, deskAll: deskAll, openTeams: Self.storedOpenTeams ?? [], calm: Self.phone)),
         let focus = try? JSONSerialization.jsonObject(with: data) {
        await session?.send(NativePacket(["type": "focus", "focus": focus]))
      }
      for event in inboxEvents { await session?.send(event) }; inboxEvents = []
      if preview { NativeDemo.route(NativeDemo.screen ?? "desk", shell: self) } else { NativeServices.shared.attach(self) }
    } catch { platformError = "The encrypted device store could not be opened." }
  }
  func enterDemo() {
    Task { await tail?.value; await session?.close(); session = nil; started = false; screen = .desk; path = []; tab = .desk; noteOpen = false; await start(demo: true) }
  }
  func leaveDemo() {
    Task { await tail?.value; await session?.close(); session = nil; started = false; screen = .desk; path = []; tab = .desk; noteOpen = false; await start() }
  }
  /** The teams whose chats stand open under their title (`Focus.open_teams`), kept on this device; until he folds
   *  one, every team is open (the list of all chats). */
  static let openTeamsKey = "trommi-open-teams"
  static var storedOpenTeams: [String]? { UserDefaults.standard.stringArray(forKey: openTeamsKey) }
  var demoOpenTeams: [String]?
  var openTeams: [String] { (isDemo ? demoOpenTeams : Self.storedOpenTeams) ?? view.sidebar.teams.map(\.id) }
  /** A team's title was tapped: its chats fold to its stack, or open again. */
  func fold(team id: String) {
    var open = openTeams
    if open.contains(id) { open.removeAll { $0 == id } } else { open.append(id) }
    if !isDemo { UserDefaults.standard.set(open, forKey: Self.openTeamsKey) } else { demoOpenTeams = open }
    show(screen)
  }
  /** A tab of the iPhone's bar was chosen: its root, the note put away, the search's words left with its tab. */
  func pick(_ next: NativeTab) {
    closeNote(); path = []
    if next != .search { chatsQuery = "" }
    tab = next; show(.desk)
  }
  /** An iPhone: sends `Focus.calm` (the core makes the calm chat); its drawing leaves out faces and times. */
  static let phone: Bool = {
    #if os(iOS)
    UIDevice.current.userInterfaceIdiom == .phone
    #else
    false
    #endif
  }()
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
  /** Home: the Desk of every team (sent as `Focus.desk_all`), kept on this device; until he picks a team, home. */
  static let deskAllKey = "trommi-desk-all"
  static var storedDeskAll: Bool { UserDefaults.standard.object(forKey: deskAllKey) as? Bool ?? true }
  private(set) var deskAll = true
  /** Home was chosen (the menu's "All teams"): the Desk of every team. */
  func home() {
    deskAll = true
    if !isDemo { UserDefaults.standard.set(true, forKey: Self.deskAllKey) }
    focus(.desk)
  }
  /** The team last chosen on this device (sent as `Focus.team` at the start; the core falls back when it is gone). */
  static let teamKey = "trommi-team"
  static var lastTeam: String? { UserDefaults.standard.string(forKey: teamKey) }
  private static var launchedInFront: Bool {
    #if canImport(UIKit)
    UIApplication.shared.applicationState != .background
    #else
    true
    #endif
  }
  func act(_ action: [String: Any], blobs: [Data] = []) {
    // (what this device folded is kept here and sent again after a start)
    let kept = ["fold_goals": "trommi-goals-folded"]
    if !isDemo, let what = action["do"] as? String, let key = kept[what], let id = action["team_id"] as? String {
      var folded = UserDefaults.standard.stringArray(forKey: key) ?? []
      folded.removeAll { $0 == id }; if action["folded"] as? Bool == true { folded.append(id) }
      UserDefaults.standard.set(folded, forKey: key)
    }
    send(["type": "action", "action": action], blobs: blobs)
  }
  func account(_ action: [String: Any]) { act(["do": "account", "action": action]) }
  func focus(_ screen: CoreView.Screen, team: String? = nil) {
    if screen != .desk { searchOpen = false }
    if screen == .notes { path = []; noteOpen = true; show(screen, team: team); return }
    noteOpen = false
    if screen == .desk { path = []; tab = .desk }
    else if path.last != screen { path.append(screen) }
    show(screen, team: team)
  }
  /** The chat whose Blitz the card in view is a step of (`Focus.walk_chat`); none: the Desk's. */
  private(set) var walkChat: String?
  /** A chat's stack tapped: its first card, and on through that chat's open cards only, then back to the chat. */
  func walk(chat: String, from cardId: String) { walkChat = chat; focus(.card(cardId: cardId)) }
  func closeNote() { guard noteOpen else { return }; noteOpen = false; show(path.last ?? .desk) }
  func show(_ screen: CoreView.Screen, team: String? = nil) {
    self.screen = screen
    if case .card = screen {} else { walkChat = nil }
    // (a team chosen: its Desk, no longer home)
    if let team { deskAll = false; if !isDemo { UserDefaults.standard.set(team, forKey: Self.teamKey); UserDefaults.standard.set(false, forKey: Self.deskAllKey) } }
    let inView = view.sidebar.team.isEmpty ? (isDemo ? nil : Self.lastTeam) : view.sidebar.team
    let query = chatsQuery.trimmingCharacters(in: .whitespaces)
    let focus = CoreView.Focus(screen: screen, visible: true, team: team ?? inView, deskAll: deskAll, openTeams: openTeams, chatsQuery: query.isEmpty ? nil : query, calm: Self.phone, walkChat: walkChat)
    if let data = try? JSONEncoder().encode(focus), let json = try? JSONSerialization.jsonObject(with: data) { send(["type": "focus", "focus": json]) }
  }
  func scene(_ active: Bool) { send(["type": active ? "foreground" : "background"]); if !isDemo { NativeServices.shared.foreground(active) } }
  @discardableResult func attach(_ url: URL) -> String? {
    let access = url.startAccessingSecurityScopedResource(); defer { if access { url.stopAccessingSecurityScopedResource() } }
    guard let data = try? Data(contentsOf: url) else { return nil }
    return attach(data, name: url.lastPathComponent, mediaType: UTType(filenameExtension: url.pathExtension)?.preferredMIMEType ?? "application/octet-stream")
  }
  /** Bytes picked elsewhere (a photo, the camera) to attach: the core's local id, shown in `view.files` as it uploads. */
  func attach(_ data: Data, name: String, mediaType: String) -> String {
    let id = UUID().uuidString
    // (a picture this device can draw is kept for its thumbnail in the composer until the core has the file)
    #if canImport(UIKit)
    if UIImage(data: data) != nil { files[id] = data }
    #endif
    act(["do": "attach", "local_id": id, "name": name, "media_type": mediaType, "bytes": 0], blobs: [data])
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
        if view.phase == .signedOut { files = [:]; saved = nil; screen = .desk; path = []; tab = .desk; noteOpen = false }
        if !isDemo { NativeServices.shared.draw(view) }
        // (the first picture names the teams: every one is open until he folds one)
        if !sentTeams, !view.sidebar.teams.isEmpty { sentTeams = true; show(screen) }
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
