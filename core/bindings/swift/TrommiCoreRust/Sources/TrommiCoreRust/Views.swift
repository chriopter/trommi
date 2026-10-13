// Written by tests/bindings/views.mjs from core/client/src/views/mod.rs: do not edit. The view model the core sends (Effect::Render)
// and the parts of it a shell sends back (Event::Focus), as Codable types: a field the core may leave out decodes to
// its default. Every type is inside `CoreView` (`CoreView.DeskView`), so none meets a name of SwiftUI or of an app.
import Foundation

public enum CoreView {
  /// A sentence of the core: `key` names it, `args` fill it, `en` is it in English.
  public struct Text: Codable, Hashable, Sendable {
    public var key: String
    public var args: [String: String]
    public var en: String

    public init(key: String = "", args: [String: String] = [:], en: String = "") {
      self.key = key
      self.args = args
      self.en = en
    }

    enum CodingKeys: String, CodingKey {
      case key = "key"
      case args = "args"
      case en = "en"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.key = try c.decode(String.self, forKey: .key)
      self.args = try c.decodeIfPresent([String: String].self, forKey: .args) ?? [:]
      self.en = try c.decode(String.self, forKey: .en)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(key, forKey: .key)
      if !args.isEmpty { try c.encode(args, forKey: .args) }
      try c.encode(en, forKey: .en)
    }
  }

  /// Where the app stands, for the whole window.
  public enum Phase: String, Codable, Hashable, Sendable {
    /// Before `Start`, or while the stored room is read.
    case starting = "starting"
    /// No room on this device: the sign-in and join screens.
    case signedOut = "signed_out"
    /// A room is open; the Desk is being read for the first time on this device.
    case opening = "opening"
    /// Open and current, the live stream runs.
    case live = "live"
    /// Open, showing what it has; the hub cannot be reached right now.
    case offline = "offline"
    /// The device was removed, or another owner took its state. Nothing more is done.
    case stopped = "stopped"

    public init() { self = .starting }
  }

  /// What the person looks at. Sent by the shell as `Event::Focus`.
  public struct Focus: Codable, Hashable, Sendable {
    public var screen: Screen
    /// The window (or app) is in front and visible.
    public var visible: Bool
    /// The desk in view: a desk's id, "all" for every desk at once, none for the first desk.
    public var desk: String?

    public init(screen: Screen = .desk, visible: Bool = true, desk: String? = nil) {
      self.screen = screen
      self.visible = visible
      self.desk = desk
    }

    enum CodingKeys: String, CodingKey {
      case screen = "screen"
      case visible = "visible"
      case desk = "desk"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.screen = try c.decode(Screen.self, forKey: .screen)
      self.visible = try c.decodeIfPresent(Bool.self, forKey: .visible) ?? true
      self.desk = try c.decodeIfPresent(String.self, forKey: .desk)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(screen, forKey: .screen)
      try c.encode(visible, forKey: .visible)
      try c.encodeIfPresent(desk, forKey: .desk)
    }
  }

  public enum Screen: Codable, Hashable, Sendable {
    case desk
    case session(sessionId: String)
    case card(cardId: String)
    case notes
    /// The page Off your mind, the complete list, with the search's words (empty: all).
    case off(query: String)
    case scribble(boardId: String)
    case settings

    public init() { self = .desk }

    enum CodingKeys: String, CodingKey {
      case screen = "screen"
      case sessionId = "session_id"
      case cardId = "card_id"
      case query = "query"
      case boardId = "board_id"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      switch try c.decode(String.self, forKey: .screen) {
      case "desk": self = .desk
      case "session": self = .session(sessionId: try c.decode(String.self, forKey: .sessionId))
      case "card": self = .card(cardId: try c.decode(String.self, forKey: .cardId))
      case "notes": self = .notes
      case "off": self = .off(query: try c.decodeIfPresent(String.self, forKey: .query) ?? "")
      case "scribble": self = .scribble(boardId: try c.decode(String.self, forKey: .boardId))
      case "settings": self = .settings
      case let other: throw DecodingError.dataCorruptedError(forKey: .screen, in: c, debugDescription: "Screen: no variant \(other)")
      }
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      switch self {
      case .desk:
        try c.encode("desk", forKey: .screen)
      case .session(let sessionId):
        try c.encode("session", forKey: .screen)
        try c.encode(sessionId, forKey: .sessionId)
      case .card(let cardId):
        try c.encode("card", forKey: .screen)
        try c.encode(cardId, forKey: .cardId)
      case .notes:
        try c.encode("notes", forKey: .screen)
      case .off(let query):
        try c.encode("off", forKey: .screen)
        try c.encode(query, forKey: .query)
      case .scribble(let boardId):
        try c.encode("scribble", forKey: .screen)
        try c.encode(boardId, forKey: .boardId)
      case .settings:
        try c.encode("settings", forKey: .screen)
      }
    }
  }

  /// Everything the shell draws. Serialised, this is what the conformance scripts compare.
  public struct ViewModel: Codable, Hashable, Sendable {
    /// Counts the deltas folded in; a delta whose `base` is not this is out of order (a shell bug).
    public var rev: UInt64
    public var phase: Phase
    /// One line on where the app stands when it is not plainly live ("Catching up", "Offline: retry in 30 s").
    public var status: Text?
    public var sidebar: SidebarView
    public var desk: DeskView
    public var sessions: [String: SessionView]
    public var cards: [String: CardView]
    /// The page Off your mind, while it is in view (`Screen::Off`).
    public var off: OffListView?
    public var notes: NotesView
    public var settings: SettingsView
    /// Signing in, joining, the emoji to compare, the Emergency Kit to keep, self-join asks.
    public var account: AccountView
    /// Files the views name (attachments, pictures), by id: uploading, ready, failed. Their bytes, when loaded,
    /// came as `Effect::FileReady` under the same id.
    public var files: [String: FileView]
    public var toasts: [Toast]

    public init(rev: UInt64 = 0, phase: Phase = .starting, status: Text? = nil, sidebar: SidebarView = SidebarView(), desk: DeskView = DeskView(), sessions: [String: SessionView] = [:], cards: [String: CardView] = [:], off: OffListView? = nil, notes: NotesView = NotesView(), settings: SettingsView = SettingsView(), account: AccountView = AccountView(), files: [String: FileView] = [:], toasts: [Toast] = []) {
      self.rev = rev
      self.phase = phase
      self.status = status
      self.sidebar = sidebar
      self.desk = desk
      self.sessions = sessions
      self.cards = cards
      self.off = off
      self.notes = notes
      self.settings = settings
      self.account = account
      self.files = files
      self.toasts = toasts
    }

    enum CodingKeys: String, CodingKey {
      case rev = "rev"
      case phase = "phase"
      case status = "status"
      case sidebar = "sidebar"
      case desk = "desk"
      case sessions = "sessions"
      case cards = "cards"
      case off = "off"
      case notes = "notes"
      case settings = "settings"
      case account = "account"
      case files = "files"
      case toasts = "toasts"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.rev = try c.decode(UInt64.self, forKey: .rev)
      self.phase = try c.decode(Phase.self, forKey: .phase)
      self.status = try c.decodeIfPresent(Text.self, forKey: .status)
      self.sidebar = try c.decode(SidebarView.self, forKey: .sidebar)
      self.desk = try c.decode(DeskView.self, forKey: .desk)
      self.sessions = try c.decodeIfPresent([String: SessionView].self, forKey: .sessions) ?? [:]
      self.cards = try c.decodeIfPresent([String: CardView].self, forKey: .cards) ?? [:]
      self.off = try c.decodeIfPresent(OffListView.self, forKey: .off)
      self.notes = try c.decode(NotesView.self, forKey: .notes)
      self.settings = try c.decode(SettingsView.self, forKey: .settings)
      self.account = try c.decode(AccountView.self, forKey: .account)
      self.files = try c.decodeIfPresent([String: FileView].self, forKey: .files) ?? [:]
      self.toasts = try c.decodeIfPresent([Toast].self, forKey: .toasts) ?? []
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(rev, forKey: .rev)
      try c.encode(phase, forKey: .phase)
      try c.encodeIfPresent(status, forKey: .status)
      try c.encode(sidebar, forKey: .sidebar)
      try c.encode(desk, forKey: .desk)
      if !sessions.isEmpty { try c.encode(sessions, forKey: .sessions) }
      if !cards.isEmpty { try c.encode(cards, forKey: .cards) }
      try c.encodeIfPresent(off, forKey: .off)
      try c.encode(notes, forKey: .notes)
      try c.encode(settings, forKey: .settings)
      try c.encode(account, forKey: .account)
      if !files.isEmpty { try c.encode(files, forKey: .files) }
      if !toasts.isEmpty { try c.encode(toasts, forKey: .toasts) }
    }
  }

  /// One change of the view model, from `base` to `base + 1`.
  public struct ViewDelta: Codable, Hashable, Sendable {
    public var base: UInt64
    public var parts: [ViewPart]

    public init(base: UInt64, parts: [ViewPart]) {
      self.base = base
      self.parts = parts
    }

    enum CodingKeys: String, CodingKey {
      case base = "base"
      case parts = "parts"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.base = try c.decode(UInt64.self, forKey: .base)
      self.parts = try c.decode([ViewPart].self, forKey: .parts)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(base, forKey: .base)
      try c.encode(parts, forKey: .parts)
    }
  }

  public enum ViewPart: Codable, Hashable, Sendable {
    /// Everything, replacing what the shell had (the first render, after a restart of the core).
    case whole(model: ViewModel)
    case phase(phase: Phase, status: Text?)
    case sidebar(view: SidebarView)
    case desk(view: DeskView)
    case session(id: String, view: SessionView?)
    case card(id: String, view: CardView?)
    /// The page Off your mind; none when it is left.
    case off(view: OffListView?)
    case notes(view: NotesView)
    case settings(view: SettingsView)
    case account(view: AccountView)
    case file(id: String, view: FileView?)
    case toasts(toasts: [Toast])

    enum CodingKeys: String, CodingKey {
      case part = "part"
      case model = "model"
      case phase = "phase"
      case status = "status"
      case view = "view"
      case id = "id"
      case toasts = "toasts"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      switch try c.decode(String.self, forKey: .part) {
      case "whole": self = .whole(model: try c.decode(ViewModel.self, forKey: .model))
      case "phase": self = .phase(phase: try c.decode(Phase.self, forKey: .phase), status: try c.decodeIfPresent(Text.self, forKey: .status))
      case "sidebar": self = .sidebar(view: try c.decode(SidebarView.self, forKey: .view))
      case "desk": self = .desk(view: try c.decode(DeskView.self, forKey: .view))
      case "session": self = .session(id: try c.decode(String.self, forKey: .id), view: try c.decodeIfPresent(SessionView.self, forKey: .view))
      case "card": self = .card(id: try c.decode(String.self, forKey: .id), view: try c.decodeIfPresent(CardView.self, forKey: .view))
      case "off": self = .off(view: try c.decodeIfPresent(OffListView.self, forKey: .view))
      case "notes": self = .notes(view: try c.decode(NotesView.self, forKey: .view))
      case "settings": self = .settings(view: try c.decode(SettingsView.self, forKey: .view))
      case "account": self = .account(view: try c.decode(AccountView.self, forKey: .view))
      case "file": self = .file(id: try c.decode(String.self, forKey: .id), view: try c.decodeIfPresent(FileView.self, forKey: .view))
      case "toasts": self = .toasts(toasts: try c.decode([Toast].self, forKey: .toasts))
      case let other: throw DecodingError.dataCorruptedError(forKey: .part, in: c, debugDescription: "ViewPart: no variant \(other)")
      }
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      switch self {
      case .whole(let model):
        try c.encode("whole", forKey: .part)
        try c.encode(model, forKey: .model)
      case .phase(let phase, let status):
        try c.encode("phase", forKey: .part)
        try c.encode(phase, forKey: .phase)
        try c.encode(status, forKey: .status)
      case .sidebar(let view):
        try c.encode("sidebar", forKey: .part)
        try c.encode(view, forKey: .view)
      case .desk(let view):
        try c.encode("desk", forKey: .part)
        try c.encode(view, forKey: .view)
      case .session(let id, let view):
        try c.encode("session", forKey: .part)
        try c.encode(id, forKey: .id)
        try c.encode(view, forKey: .view)
      case .card(let id, let view):
        try c.encode("card", forKey: .part)
        try c.encode(id, forKey: .id)
        try c.encode(view, forKey: .view)
      case .off(let view):
        try c.encode("off", forKey: .part)
        try c.encode(view, forKey: .view)
      case .notes(let view):
        try c.encode("notes", forKey: .part)
        try c.encode(view, forKey: .view)
      case .settings(let view):
        try c.encode("settings", forKey: .part)
        try c.encode(view, forKey: .view)
      case .account(let view):
        try c.encode("account", forKey: .part)
        try c.encode(view, forKey: .view)
      case .file(let id, let view):
        try c.encode("file", forKey: .part)
        try c.encode(id, forKey: .id)
        try c.encode(view, forKey: .view)
      case .toasts(let toasts):
        try c.encode("toasts", forKey: .part)
        try c.encode(toasts, forKey: .toasts)
      }
    }
  }

  /// Edits of a long list, by position: at `at`, `gone` rows leave and `rows` come (the Desk's stack ops today).
  public struct RowEdit<T: Codable & Hashable & Sendable>: Codable, Hashable, Sendable {
    public var at: UInt32
    public var gone: UInt32
    public var rows: [T]

    public init(at: UInt32, gone: UInt32, rows: [T]) {
      self.at = at
      self.gone = gone
      self.rows = rows
    }

    enum CodingKeys: String, CodingKey {
      case at = "at"
      case gone = "gone"
      case rows = "rows"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.at = try c.decode(UInt32.self, forKey: .at)
      self.gone = try c.decode(UInt32.self, forKey: .gone)
      self.rows = try c.decode([T].self, forKey: .rows)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(at, forKey: .at)
      try c.encode(gone, forKey: .gone)
      try c.encode(rows, forKey: .rows)
    }
  }

  public enum Rows<T: Codable & Hashable & Sendable>: Codable, Hashable, Sendable {
    case whole(items: [T])
    case edits(edits: [RowEdit<T>])

    enum CodingKeys: String, CodingKey {
      case rows = "rows"
      case items = "items"
      case edits = "edits"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      switch try c.decode(String.self, forKey: .rows) {
      case "whole": self = .whole(items: try c.decode([T].self, forKey: .items))
      case "edits": self = .edits(edits: try c.decode([RowEdit<T>].self, forKey: .edits))
      case let other: throw DecodingError.dataCorruptedError(forKey: .rows, in: c, debugDescription: "Rows: no variant \(other)")
      }
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      switch self {
      case .whole(let items):
        try c.encode("whole", forKey: .rows)
        try c.encode(items, forKey: .items)
      case .edits(let edits):
        try c.encode("edits", forKey: .rows)
        try c.encode(edits, forKey: .edits)
      }
    }
  }

  /// The sidebar (the phone: the menu's desks, the list of chats, the tab pill's numbers): the desks, and one row per
  /// session of the desk in view, in the order they are drawn.
  public struct SidebarView: Codable, Hashable, Sendable {
    /// The desks, in the menu's order. A room without desks has one: "Personal".
    public var desks: [DeskTab]
    /// The sessions of the desk in view, one row each, in drawing order: a main, then its helpers.
    public var sessions: [SessionRowView]
    /// The number on the app icon and the tab: the open questions of the desk in view (no infos, nothing that is
    /// with its agent). Never drawn red.
    public var badge: UInt32
    /// The desk in view: its id, or "all".
    public var desk: String
    /// "All Desks", the row above the desks: there with two desks or more.
    public var all: DeskTab?
    /// Where the person is, for the head: the desk's name, or "All desks".
    public var place: Text
    /// The open questions of the desk in view (its lamp is lit while there are some).
    public var waiting: UInt32
    /// How many of them knock.
    public var knocking: UInt32
    /// How many rows of the desk in view are stopped (the red hand).
    public var stopped: UInt32
    /// How many of them are at work.
    public var working: UInt32
    /// How many of his answers the sessions of the desk in view have not picked up.
    public var unheard: UInt32

    public init(desks: [DeskTab] = [], sessions: [SessionRowView] = [], badge: UInt32 = 0, desk: String = "", all: DeskTab? = nil, place: Text = Text(), waiting: UInt32 = 0, knocking: UInt32 = 0, stopped: UInt32 = 0, working: UInt32 = 0, unheard: UInt32 = 0) {
      self.desks = desks
      self.sessions = sessions
      self.badge = badge
      self.desk = desk
      self.all = all
      self.place = place
      self.waiting = waiting
      self.knocking = knocking
      self.stopped = stopped
      self.working = working
      self.unheard = unheard
    }

    enum CodingKeys: String, CodingKey {
      case desks = "desks"
      case sessions = "sessions"
      case badge = "badge"
      case desk = "desk"
      case all = "all"
      case place = "place"
      case waiting = "waiting"
      case knocking = "knocking"
      case stopped = "stopped"
      case working = "working"
      case unheard = "unheard"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.desks = try c.decode([DeskTab].self, forKey: .desks)
      self.sessions = try c.decode([SessionRowView].self, forKey: .sessions)
      self.badge = try c.decode(UInt32.self, forKey: .badge)
      self.desk = try c.decodeIfPresent(String.self, forKey: .desk) ?? ""
      self.all = try c.decodeIfPresent(DeskTab.self, forKey: .all)
      self.place = try c.decodeIfPresent(Text.self, forKey: .place) ?? Text()
      self.waiting = try c.decodeIfPresent(UInt32.self, forKey: .waiting) ?? 0
      self.knocking = try c.decodeIfPresent(UInt32.self, forKey: .knocking) ?? 0
      self.stopped = try c.decodeIfPresent(UInt32.self, forKey: .stopped) ?? 0
      self.working = try c.decodeIfPresent(UInt32.self, forKey: .working) ?? 0
      self.unheard = try c.decodeIfPresent(UInt32.self, forKey: .unheard) ?? 0
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(desks, forKey: .desks)
      try c.encode(sessions, forKey: .sessions)
      try c.encode(badge, forKey: .badge)
      try c.encode(desk, forKey: .desk)
      try c.encodeIfPresent(all, forKey: .all)
      try c.encode(place, forKey: .place)
      try c.encode(waiting, forKey: .waiting)
      try c.encode(knocking, forKey: .knocking)
      try c.encode(stopped, forKey: .stopped)
      try c.encode(working, forKey: .working)
      try c.encode(unheard, forKey: .unheard)
    }
  }

  public struct DeskTab: Codable, Hashable, Sendable {
    public var id: String
    public var name: Text
    /// The open questions of the desk's sessions.
    public var waiting: UInt32
    public var selected: Bool
    /// One of them knocks.
    public var knocks: Bool

    public init(id: String = "", name: Text = Text(), waiting: UInt32 = 0, selected: Bool = false, knocks: Bool = false) {
      self.id = id
      self.name = name
      self.waiting = waiting
      self.selected = selected
      self.knocks = knocks
    }

    enum CodingKeys: String, CodingKey {
      case id = "id"
      case name = "name"
      case waiting = "waiting"
      case selected = "selected"
      case knocks = "knocks"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.id = try c.decode(String.self, forKey: .id)
      self.name = try c.decode(Text.self, forKey: .name)
      self.waiting = try c.decode(UInt32.self, forKey: .waiting)
      self.selected = try c.decode(Bool.self, forKey: .selected)
      self.knocks = try c.decodeIfPresent(Bool.self, forKey: .knocks) ?? false
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(id, forKey: .id)
      try c.encode(name, forKey: .name)
      try c.encode(waiting, forKey: .waiting)
      try c.encode(selected, forKey: .selected)
      try c.encode(knocks, forKey: .knocks)
    }
  }

  /// One session in the sidebar.
  public struct SessionRowView: Codable, Hashable, Sendable {
    /// The session's key in the model (`Screen::Session`, the actions' `session_id`, `ViewModel.sessions`).
    public var id: String
    public var name: Text
    /// "live", "oncall", "asleep", "cut", "gone" (core/client/src/status.rs); "" for a helper under its main (it
    /// runs inside its main's process: the main's link says it for both).
    public var link: String
    /// The red hand.
    public var blocked: Bool
    /// How many of his answers it has not picked up for two minutes; a main counts its helpers' too.
    public var unheard: UInt32
    /// 0 for a row of its own, 1 for a helper under its main.
    public var depth: UInt32
    /// Its short id on the board: its address (`/chat/<slug>`).
    public var slug: String
    /// What draws it: the icon the person or the agent chose ("draw:leaf"), else its slug.
    public var mark: String
    /// The desk it stands on.
    public var deskId: String
    public var online: Bool
    /// Connected and at work (a status line of it shows "working").
    public var working: Bool
    /// A main session: one that says so, or one with helpers.
    public var main: Bool
    /// The crowned session of its desk (the one a note goes to).
    public var crowned: Bool
    /// Its main's id, for a helper drawn under it.
    public var parent: String?
    /// Its helpers' ids, in drawing order.
    public var subs: [String]
    /// Its open questions: what waits for the person on the Desk (no infos, nothing that is with its agent).
    public var open: UInt32
    /// One of them knocks.
    public var knocks: Bool
    /// Why it is stopped, in words.
    public var hand: HandView?
    /// The small line under its name: its link when it does not hear at once, else the answers it has not picked up.
    public var cap: CapView?
    /// A helper whose main is not listening, cut off or gone: drawn hushed.
    public var hushed: Bool
    /// Not connected, and none of its helpers is: it stands under "Disconnected".
    public var away: Bool
    /// The line drawn above this row: "Disconnected" over the first row that is away, a desk's name over its first
    /// row on All desks.
    public var heading: HeadingView?
    /// The part of the list a main row may be dragged within (its desk, connected or not); "" for a helper.
    public var group: String
    /// What its badge says to a screen reader ("Working, 2 questions open"); none without a badge.
    public var says: Text?
    /// A main with helpers, drawn folded: what the row then shows for all of them together.
    public var folded: FoldView?

    public init(id: String = "", name: Text = Text(), link: String = "", blocked: Bool = false, unheard: UInt32 = 0, depth: UInt32 = 0, slug: String = "", mark: String = "", deskId: String = "", online: Bool = false, working: Bool = false, main: Bool = false, crowned: Bool = false, parent: String? = nil, subs: [String] = [], open: UInt32 = 0, knocks: Bool = false, hand: HandView? = nil, cap: CapView? = nil, hushed: Bool = false, away: Bool = false, heading: HeadingView? = nil, group: String = "", says: Text? = nil, folded: FoldView? = nil) {
      self.id = id
      self.name = name
      self.link = link
      self.blocked = blocked
      self.unheard = unheard
      self.depth = depth
      self.slug = slug
      self.mark = mark
      self.deskId = deskId
      self.online = online
      self.working = working
      self.main = main
      self.crowned = crowned
      self.parent = parent
      self.subs = subs
      self.open = open
      self.knocks = knocks
      self.hand = hand
      self.cap = cap
      self.hushed = hushed
      self.away = away
      self.heading = heading
      self.group = group
      self.says = says
      self.folded = folded
    }

    enum CodingKeys: String, CodingKey {
      case id = "id"
      case name = "name"
      case link = "link"
      case blocked = "blocked"
      case unheard = "unheard"
      case depth = "depth"
      case slug = "slug"
      case mark = "mark"
      case deskId = "desk_id"
      case online = "online"
      case working = "working"
      case main = "main"
      case crowned = "crowned"
      case parent = "parent"
      case subs = "subs"
      case open = "open"
      case knocks = "knocks"
      case hand = "hand"
      case cap = "cap"
      case hushed = "hushed"
      case away = "away"
      case heading = "heading"
      case group = "group"
      case says = "says"
      case folded = "folded"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.id = try c.decode(String.self, forKey: .id)
      self.name = try c.decode(Text.self, forKey: .name)
      self.link = try c.decode(String.self, forKey: .link)
      self.blocked = try c.decode(Bool.self, forKey: .blocked)
      self.unheard = try c.decode(UInt32.self, forKey: .unheard)
      self.depth = try c.decode(UInt32.self, forKey: .depth)
      self.slug = try c.decodeIfPresent(String.self, forKey: .slug) ?? ""
      self.mark = try c.decodeIfPresent(String.self, forKey: .mark) ?? ""
      self.deskId = try c.decodeIfPresent(String.self, forKey: .deskId) ?? ""
      self.online = try c.decodeIfPresent(Bool.self, forKey: .online) ?? false
      self.working = try c.decodeIfPresent(Bool.self, forKey: .working) ?? false
      self.main = try c.decodeIfPresent(Bool.self, forKey: .main) ?? false
      self.crowned = try c.decodeIfPresent(Bool.self, forKey: .crowned) ?? false
      self.parent = try c.decodeIfPresent(String.self, forKey: .parent)
      self.subs = try c.decodeIfPresent([String].self, forKey: .subs) ?? []
      self.open = try c.decodeIfPresent(UInt32.self, forKey: .open) ?? 0
      self.knocks = try c.decodeIfPresent(Bool.self, forKey: .knocks) ?? false
      self.hand = try c.decodeIfPresent(HandView.self, forKey: .hand)
      self.cap = try c.decodeIfPresent(CapView.self, forKey: .cap)
      self.hushed = try c.decodeIfPresent(Bool.self, forKey: .hushed) ?? false
      self.away = try c.decodeIfPresent(Bool.self, forKey: .away) ?? false
      self.heading = try c.decodeIfPresent(HeadingView.self, forKey: .heading)
      self.group = try c.decodeIfPresent(String.self, forKey: .group) ?? ""
      self.says = try c.decodeIfPresent(Text.self, forKey: .says)
      self.folded = try c.decodeIfPresent(FoldView.self, forKey: .folded)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(id, forKey: .id)
      try c.encode(name, forKey: .name)
      try c.encode(link, forKey: .link)
      try c.encode(blocked, forKey: .blocked)
      try c.encode(unheard, forKey: .unheard)
      try c.encode(depth, forKey: .depth)
      try c.encode(slug, forKey: .slug)
      try c.encode(mark, forKey: .mark)
      try c.encode(deskId, forKey: .deskId)
      try c.encode(online, forKey: .online)
      try c.encode(working, forKey: .working)
      try c.encode(main, forKey: .main)
      try c.encode(crowned, forKey: .crowned)
      try c.encodeIfPresent(parent, forKey: .parent)
      if !subs.isEmpty { try c.encode(subs, forKey: .subs) }
      try c.encode(open, forKey: .open)
      try c.encode(knocks, forKey: .knocks)
      try c.encodeIfPresent(hand, forKey: .hand)
      try c.encodeIfPresent(cap, forKey: .cap)
      try c.encode(hushed, forKey: .hushed)
      try c.encode(away, forKey: .away)
      try c.encodeIfPresent(heading, forKey: .heading)
      try c.encode(group, forKey: .group)
      try c.encodeIfPresent(says, forKey: .says)
      try c.encodeIfPresent(folded, forKey: .folded)
    }
  }

  /// A main and its helpers as one folded row.
  public struct FoldView: Codable, Hashable, Sendable {
    public var open: UInt32
    public var knocks: Bool
    /// One of them is connected.
    public var online: Bool
    public var working: Bool
    /// The first of them that is stopped.
    public var hand: HandView?
    public var says: Text?
    /// The helpers that are stopped: their edges show it while the row is folded.
    public var stopped: [String]

    public init(open: UInt32 = 0, knocks: Bool = false, online: Bool = false, working: Bool = false, hand: HandView? = nil, says: Text? = nil, stopped: [String] = []) {
      self.open = open
      self.knocks = knocks
      self.online = online
      self.working = working
      self.hand = hand
      self.says = says
      self.stopped = stopped
    }

    enum CodingKeys: String, CodingKey {
      case open = "open"
      case knocks = "knocks"
      case online = "online"
      case working = "working"
      case hand = "hand"
      case says = "says"
      case stopped = "stopped"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.open = try c.decode(UInt32.self, forKey: .open)
      self.knocks = try c.decode(Bool.self, forKey: .knocks)
      self.online = try c.decode(Bool.self, forKey: .online)
      self.working = try c.decode(Bool.self, forKey: .working)
      self.hand = try c.decodeIfPresent(HandView.self, forKey: .hand)
      self.says = try c.decodeIfPresent(Text.self, forKey: .says)
      self.stopped = try c.decodeIfPresent([String].self, forKey: .stopped) ?? []
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(open, forKey: .open)
      try c.encode(knocks, forKey: .knocks)
      try c.encode(online, forKey: .online)
      try c.encode(working, forKey: .working)
      try c.encodeIfPresent(hand, forKey: .hand)
      try c.encodeIfPresent(says, forKey: .says)
      if !stopped.isEmpty { try c.encode(stopped, forKey: .stopped) }
    }
  }

  /// Why a session is stopped: the red hand.
  public struct HandView: Codable, Hashable, Sendable {
    /// "cut", "offline", "permission", "blocking".
    public var why: String
    public var text: Text
    public var since: UInt64?

    public init(why: String = "", text: Text = Text(), since: UInt64? = nil) {
      self.why = why
      self.text = text
      self.since = since
    }

    enum CodingKeys: String, CodingKey {
      case why = "why"
      case text = "text"
      case since = "since"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.why = try c.decode(String.self, forKey: .why)
      self.text = try c.decode(Text.self, forKey: .text)
      self.since = try c.decodeIfPresent(UInt64.self, forKey: .since)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(why, forKey: .why)
      try c.encode(text, forKey: .text)
      try c.encodeIfPresent(since, forKey: .since)
    }
  }

  /// The small line under a session's name.
  public struct CapView: Codable, Hashable, Sendable {
    /// "oncall", "asleep", "cut", "gone", or "unheard" (it hears at once and has answers to pick up).
    public var kind: String
    /// Two or three words: "cut off · 14:05", "1 answer waits".
    public var word: Text
    /// The tooltip: "Cut off since 14:05".
    public var short: Text
    /// The whole sentence about the session, for a screen reader; none for "unheard".
    public var line: Text?

    public init(kind: String = "", word: Text = Text(), short: Text = Text(), line: Text? = nil) {
      self.kind = kind
      self.word = word
      self.short = short
      self.line = line
    }

    enum CodingKeys: String, CodingKey {
      case kind = "kind"
      case word = "word"
      case short = "short"
      case line = "line"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.kind = try c.decode(String.self, forKey: .kind)
      self.word = try c.decode(Text.self, forKey: .word)
      self.short = try c.decode(Text.self, forKey: .short)
      self.line = try c.decodeIfPresent(Text.self, forKey: .line)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(kind, forKey: .kind)
      try c.encode(word, forKey: .word)
      try c.encode(short, forKey: .short)
      try c.encodeIfPresent(line, forKey: .line)
    }
  }

  /// A line above a row of the sidebar.
  public struct HeadingView: Codable, Hashable, Sendable {
    /// "away" ("Disconnected") or "desk" (a desk's name, on All desks).
    public var kind: String
    public var text: Text
    /// For "desk": the desk, and its open questions.
    public var deskId: String?
    public var waiting: UInt32

    public init(kind: String = "", text: Text = Text(), deskId: String? = nil, waiting: UInt32 = 0) {
      self.kind = kind
      self.text = text
      self.deskId = deskId
      self.waiting = waiting
    }

    enum CodingKeys: String, CodingKey {
      case kind = "kind"
      case text = "text"
      case deskId = "desk_id"
      case waiting = "waiting"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.kind = try c.decode(String.self, forKey: .kind)
      self.text = try c.decode(Text.self, forKey: .text)
      self.deskId = try c.decodeIfPresent(String.self, forKey: .deskId)
      self.waiting = try c.decodeIfPresent(UInt32.self, forKey: .waiting) ?? 0
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(kind, forKey: .kind)
      try c.encode(text, forKey: .text)
      try c.encodeIfPresent(deskId, forKey: .deskId)
      try c.encode(waiting, forKey: .waiting)
    }
  }

  /// A session's link in words: whether it hears the person, and what to do when it does not.
  public struct LinkWords: Codable, Hashable, Sendable {
    /// "live", "oncall", "asleep", "cut", "gone".
    public var state: String
    /// Since when it is cut off or gone; for "oncall" and "asleep" its last step.
    public var since: UInt64?
    /// Two or three words under its name ("not listening · 12 min"); none while it hears at once.
    public var word: Text?
    /// The same as a short sentence ("Not listening for 12 min").
    public var short: Text?
    /// The whole sentence about the session.
    public var line: Text
    /// The way out.
    public var fix: FixView?

    public init(state: String = "", since: UInt64? = nil, word: Text? = nil, short: Text? = nil, line: Text = Text(), fix: FixView? = nil) {
      self.state = state
      self.since = since
      self.word = word
      self.short = short
      self.line = line
      self.fix = fix
    }

    enum CodingKeys: String, CodingKey {
      case state = "state"
      case since = "since"
      case word = "word"
      case short = "short"
      case line = "line"
      case fix = "fix"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.state = try c.decode(String.self, forKey: .state)
      self.since = try c.decodeIfPresent(UInt64.self, forKey: .since)
      self.word = try c.decodeIfPresent(Text.self, forKey: .word)
      self.short = try c.decodeIfPresent(Text.self, forKey: .short)
      self.line = try c.decode(Text.self, forKey: .line)
      self.fix = try c.decodeIfPresent(FixView.self, forKey: .fix)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(state, forKey: .state)
      try c.encodeIfPresent(since, forKey: .since)
      try c.encodeIfPresent(word, forKey: .word)
      try c.encodeIfPresent(short, forKey: .short)
      try c.encode(line, forKey: .line)
      try c.encodeIfPresent(fix, forKey: .fix)
    }
  }

  /// What to do about a link: a sentence, and a line to type (empty when there is none).
  public struct FixView: Codable, Hashable, Sendable {
    public var say: Text
    public var code: String

    public init(say: Text = Text(), code: String = "") {
      self.say = say
      self.code = code
    }

    enum CodingKeys: String, CodingKey {
      case say = "say"
      case code = "code"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.say = try c.decode(Text.self, forKey: .say)
      self.code = try c.decode(String.self, forKey: .code)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(say, forKey: .say)
      try c.encode(code, forKey: .code)
    }
  }

  /// The Desk of the desk in view: its head, and the stack of what waits for the person as rows.
  public struct DeskView: Codable, Hashable, Sendable {
    /// Card and request ids, in the Desk's order: the ids of `rows`.
    public var stack: [String]
    /// Off your mind, of the desk in view (`off.rs`, from the core's `off_mind`): the cards with their agents,
    /// starred ones too, the one that moved last first (the complete list's top part).
    public var offYourMind: [String]
    /// The put-off cards, the last put off first.
    public var later: [String]
    /// The cards closed last (done, withdrawn, shredded), the newest first: the core's window of them
    /// (`off_mind::RECENT` for the whole room), not every closed card; the complete list has them all.
    public var done: [String]
    /// The two stacks docked at the Desk's foot; none while a stack is empty. A card has one home: a starred card
    /// is in `starred` only, also while it is being worked on.
    public var starred: DockView?
    public var running: DockView?
    /// The section Off your mind under the Desk: its first rows and how many; none while nothing is put off or
    /// closed.
    public var off: OffHintView?
    /// What stands in the list's place when nothing is open at all ("Questions land here.").
    public var empty: Text?
    /// "invite" (no session stands here yet: invite the first agent), "reading" (none yet, and the room is still
    /// being read), "desk".
    public var mode: String
    /// The one big line at the top; none for "invite".
    public var hello: Text?
    /// How far the reading of the room's past is ("Reading your history: 12,400 of 27,000"), while it is read.
    public var reading: Text?
    /// The desk's goals; none on All desks.
    public var goals: GoalsView?
    /// What waits, in the Desk's order: the questions and what is only to read.
    public var rows: [DeskRowView]
    /// The open questions (no infos): the number in the window's title.
    public var waiting: UInt32
    /// What Blitz walks through and the duck closes ("Blitz: 3 open cards"); none when nothing waits.
    public var blitz: Text?
    /// The sessions that are cut off, above the questions: one line each, with the step in its terminal.
    public var slip: [SlipView]

    public init(stack: [String] = [], offYourMind: [String] = [], later: [String] = [], done: [String] = [], starred: DockView? = nil, running: DockView? = nil, off: OffHintView? = nil, empty: Text? = nil, mode: String = "", hello: Text? = nil, reading: Text? = nil, goals: GoalsView? = nil, rows: [DeskRowView] = [], waiting: UInt32 = 0, blitz: Text? = nil, slip: [SlipView] = []) {
      self.stack = stack
      self.offYourMind = offYourMind
      self.later = later
      self.done = done
      self.starred = starred
      self.running = running
      self.off = off
      self.empty = empty
      self.mode = mode
      self.hello = hello
      self.reading = reading
      self.goals = goals
      self.rows = rows
      self.waiting = waiting
      self.blitz = blitz
      self.slip = slip
    }

    enum CodingKeys: String, CodingKey {
      case stack = "stack"
      case offYourMind = "off_your_mind"
      case later = "later"
      case done = "done"
      case starred = "starred"
      case running = "running"
      case off = "off"
      case empty = "empty"
      case mode = "mode"
      case hello = "hello"
      case reading = "reading"
      case goals = "goals"
      case rows = "rows"
      case waiting = "waiting"
      case blitz = "blitz"
      case slip = "slip"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.stack = try c.decode([String].self, forKey: .stack)
      self.offYourMind = try c.decode([String].self, forKey: .offYourMind)
      self.later = try c.decode([String].self, forKey: .later)
      self.done = try c.decode([String].self, forKey: .done)
      self.starred = try c.decodeIfPresent(DockView.self, forKey: .starred)
      self.running = try c.decodeIfPresent(DockView.self, forKey: .running)
      self.off = try c.decodeIfPresent(OffHintView.self, forKey: .off)
      self.empty = try c.decodeIfPresent(Text.self, forKey: .empty)
      self.mode = try c.decodeIfPresent(String.self, forKey: .mode) ?? ""
      self.hello = try c.decodeIfPresent(Text.self, forKey: .hello)
      self.reading = try c.decodeIfPresent(Text.self, forKey: .reading)
      self.goals = try c.decodeIfPresent(GoalsView.self, forKey: .goals)
      self.rows = try c.decodeIfPresent([DeskRowView].self, forKey: .rows) ?? []
      self.waiting = try c.decodeIfPresent(UInt32.self, forKey: .waiting) ?? 0
      self.blitz = try c.decodeIfPresent(Text.self, forKey: .blitz)
      self.slip = try c.decodeIfPresent([SlipView].self, forKey: .slip) ?? []
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(stack, forKey: .stack)
      try c.encode(offYourMind, forKey: .offYourMind)
      try c.encode(later, forKey: .later)
      try c.encode(done, forKey: .done)
      try c.encodeIfPresent(starred, forKey: .starred)
      try c.encodeIfPresent(running, forKey: .running)
      try c.encodeIfPresent(off, forKey: .off)
      try c.encodeIfPresent(empty, forKey: .empty)
      try c.encode(mode, forKey: .mode)
      try c.encodeIfPresent(hello, forKey: .hello)
      try c.encodeIfPresent(reading, forKey: .reading)
      try c.encodeIfPresent(goals, forKey: .goals)
      if !rows.isEmpty { try c.encode(rows, forKey: .rows) }
      try c.encode(waiting, forKey: .waiting)
      try c.encodeIfPresent(blitz, forKey: .blitz)
      if !slip.isEmpty { try c.encode(slip, forKey: .slip) }
    }
  }

  /// One of the two stacks docked at the Desk's foot (the owner's pick of 13 October 2026: two small tabs, the
  /// titles shown only when a stack is fanned out).
  public struct DockView: Codable, Hashable, Sendable {
    /// "star" or "run".
    public var key: String
    /// How many cards it holds: the number on its tab (`rows.len()`).
    public var count: UInt32
    /// What the tab says to a screen reader ("3 starred", "1 running").
    public var says: Text
    /// Its cards in order, fanned out: the first is the top card, the one the tab shows.
    public var rows: [DockRowView]
    /// The way on to the complete list at the end of the fan ("All running and done"); the running stack only.
    public var all: Text?

    public init(key: String = "", count: UInt32 = 0, says: Text = Text(), rows: [DockRowView] = [], all: Text? = nil) {
      self.key = key
      self.count = count
      self.says = says
      self.rows = rows
      self.all = all
    }

    enum CodingKeys: String, CodingKey {
      case key = "key"
      case count = "count"
      case says = "says"
      case rows = "rows"
      case all = "all"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.key = try c.decode(String.self, forKey: .key)
      self.count = try c.decode(UInt32.self, forKey: .count)
      self.says = try c.decode(Text.self, forKey: .says)
      self.rows = try c.decode([DockRowView].self, forKey: .rows)
      self.all = try c.decodeIfPresent(Text.self, forKey: .all)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(key, forKey: .key)
      try c.encode(count, forKey: .count)
      try c.encode(says, forKey: .says)
      try c.encode(rows, forKey: .rows)
      try c.encodeIfPresent(all, forKey: .all)
    }
  }

  /// A small card of a docked stack.
  public struct DockRowView: Codable, Hashable, Sendable {
    /// The card (`Screen::Card`).
    public var id: String
    public var title: String
    public var from: FromView
    /// The quiet line under the title: "Fernly · running" (a starred card being worked on), "Fernly", "Fernly ·
    /// Reworking the copy" (its status line), "Fernly · on it".
    public var line: Text
    /// It is being worked on (the drawing's ring turns).
    public var running: Bool
    /// When it last moved (its status line's update, else when it went to its agent); when it was starred, for a
    /// starred card.
    public var atMs: UInt64

    public init(id: String = "", title: String = "", from: FromView = FromView(), line: Text = Text(), running: Bool = false, atMs: UInt64 = 0) {
      self.id = id
      self.title = title
      self.from = from
      self.line = line
      self.running = running
      self.atMs = atMs
    }

    enum CodingKeys: String, CodingKey {
      case id = "id"
      case title = "title"
      case from = "from"
      case line = "line"
      case running = "running"
      case atMs = "at_ms"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.id = try c.decode(String.self, forKey: .id)
      self.title = try c.decode(String.self, forKey: .title)
      self.from = try c.decode(FromView.self, forKey: .from)
      self.line = try c.decode(Text.self, forKey: .line)
      self.running = try c.decode(Bool.self, forKey: .running)
      self.atMs = try c.decode(UInt64.self, forKey: .atMs)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(id, forKey: .id)
      try c.encode(title, forKey: .title)
      try c.encode(from, forKey: .from)
      try c.encode(line, forKey: .line)
      try c.encode(running, forKey: .running)
      try c.encode(atMs, forKey: .atMs)
    }
  }

  /// The section Off your mind under the Desk: a short hinted list that opens the complete list (`Screen::Off`).
  public struct OffHintView: Codable, Hashable, Sendable {
    /// The first rows, put off first, then the closed ones, the newest first (at most three; the shell fades them).
    public var rows: [OffRowView]
    /// "2 put off · 14 done".
    public var says: Text
    public var putOff: UInt32
    /// Every closed card of the desk in view, counted (not only the ones named).
    public var closed: UInt32

    public init(rows: [OffRowView] = [], says: Text = Text(), putOff: UInt32 = 0, closed: UInt32 = 0) {
      self.rows = rows
      self.says = says
      self.putOff = putOff
      self.closed = closed
    }

    enum CodingKeys: String, CodingKey {
      case rows = "rows"
      case says = "says"
      case putOff = "put_off"
      case closed = "closed"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.rows = try c.decode([OffRowView].self, forKey: .rows)
      self.says = try c.decode(Text.self, forKey: .says)
      self.putOff = try c.decode(UInt32.self, forKey: .putOff)
      self.closed = try c.decode(UInt32.self, forKey: .closed)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(rows, forKey: .rows)
      try c.encode(says, forKey: .says)
      try c.encode(putOff, forKey: .putOff)
      try c.encode(closed, forKey: .closed)
    }
  }

  /// A put-off or closed card, one small quiet row.
  public struct OffRowView: Codable, Hashable, Sendable {
    public var id: String
    /// "later", "done" or "trash".
    public var place: String
    /// Its mark: "snooze" (put off), "shred" (shredded: the title struck through), "withdraw" (withdrawn by its
    /// agent), "tick" (done).
    public var mark: String
    public var title: String
    public var starred: Bool
    /// When it closed (ms); 0 for a put-off card and when no time is known (show none, never the epoch).
    public var atMs: UInt64
    /// A put-off card: until when ("until Tue 07:00", the person's zone).
    public var until: Text?
    /// What it is, to a screen reader: "Later", "Done", "Shredded", "Withdrawn".
    public var says: Text

    public init(id: String = "", place: String = "", mark: String = "", title: String = "", starred: Bool = false, atMs: UInt64 = 0, until: Text? = nil, says: Text = Text()) {
      self.id = id
      self.place = place
      self.mark = mark
      self.title = title
      self.starred = starred
      self.atMs = atMs
      self.until = until
      self.says = says
    }

    enum CodingKeys: String, CodingKey {
      case id = "id"
      case place = "place"
      case mark = "mark"
      case title = "title"
      case starred = "starred"
      case atMs = "at_ms"
      case until = "until"
      case says = "says"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.id = try c.decode(String.self, forKey: .id)
      self.place = try c.decode(String.self, forKey: .place)
      self.mark = try c.decode(String.self, forKey: .mark)
      self.title = try c.decode(String.self, forKey: .title)
      self.starred = try c.decodeIfPresent(Bool.self, forKey: .starred) ?? false
      self.atMs = try c.decodeIfPresent(UInt64.self, forKey: .atMs) ?? 0
      self.until = try c.decodeIfPresent(Text.self, forKey: .until)
      self.says = try c.decode(Text.self, forKey: .says)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(id, forKey: .id)
      try c.encode(place, forKey: .place)
      try c.encode(mark, forKey: .mark)
      try c.encode(title, forKey: .title)
      try c.encode(starred, forKey: .starred)
      try c.encode(atMs, forKey: .atMs)
      try c.encodeIfPresent(until, forKey: .until)
      try c.encode(says, forKey: .says)
    }
  }

  /// The page Off your mind: every card off the Desk of the desk in view. What runs as live cards, the one that moved
  /// last first; then what is put off, the last put off first; then what is closed, the newest first. Each card in its
  /// place by time, none lifted. Kept to the search's words.
  public struct OffListView: Codable, Hashable, Sendable {
    /// The search's words as given (`Screen::Off`).
    public var query: String
    /// The cards with their agents.
    public var works: [OffWorkView]
    /// The put-off cards, then the closed ones.
    public var rest: [OffRowView]
    /// In the list's place when nothing is shown: "Nothing yet.", "Nothing here has these words.".
    public var none: Text?
    /// Older closed cards are on the hub and not on this device yet (history on demand): the line under the list
    /// that fetches the next page. Always none until the engine pages the Done list.
    public var older: Text?
    /// A page of older cards is being fetched: the shell shows its quiet "Loading …" line at the end.
    public var loading: Bool

    public init(query: String = "", works: [OffWorkView] = [], rest: [OffRowView] = [], none: Text? = nil, older: Text? = nil, loading: Bool = false) {
      self.query = query
      self.works = works
      self.rest = rest
      self.none = none
      self.older = older
      self.loading = loading
    }

    enum CodingKeys: String, CodingKey {
      case query = "query"
      case works = "works"
      case rest = "rest"
      case none = "none"
      case older = "older"
      case loading = "loading"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.query = try c.decodeIfPresent(String.self, forKey: .query) ?? ""
      self.works = try c.decodeIfPresent([OffWorkView].self, forKey: .works) ?? []
      self.rest = try c.decodeIfPresent([OffRowView].self, forKey: .rest) ?? []
      self.none = try c.decodeIfPresent(Text.self, forKey: .none)
      self.older = try c.decodeIfPresent(Text.self, forKey: .older)
      self.loading = try c.decodeIfPresent(Bool.self, forKey: .loading) ?? false
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(query, forKey: .query)
      if !works.isEmpty { try c.encode(works, forKey: .works) }
      if !rest.isEmpty { try c.encode(rest, forKey: .rest) }
      try c.encodeIfPresent(none, forKey: .none)
      try c.encodeIfPresent(older, forKey: .older)
      try c.encode(loading, forKey: .loading)
    }
  }

  /// A card with its agent, on the page Off your mind.
  public struct OffWorkView: Codable, Hashable, Sendable {
    public var id: String
    public var title: String
    public var starred: Bool
    public var from: FromView
    /// Its status line's words, else "on it".
    public var label: Text
    /// When it last moved (ms): the shell's "5 min".
    public var atMs: UInt64
    /// The whole row to a screen reader: "Fix the login, Fernly is on it".
    public var says: Text

    public init(id: String = "", title: String = "", starred: Bool = false, from: FromView = FromView(), label: Text = Text(), atMs: UInt64 = 0, says: Text = Text()) {
      self.id = id
      self.title = title
      self.starred = starred
      self.from = from
      self.label = label
      self.atMs = atMs
      self.says = says
    }

    enum CodingKeys: String, CodingKey {
      case id = "id"
      case title = "title"
      case starred = "starred"
      case from = "from"
      case label = "label"
      case atMs = "at_ms"
      case says = "says"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.id = try c.decode(String.self, forKey: .id)
      self.title = try c.decode(String.self, forKey: .title)
      self.starred = try c.decodeIfPresent(Bool.self, forKey: .starred) ?? false
      self.from = try c.decode(FromView.self, forKey: .from)
      self.label = try c.decode(Text.self, forKey: .label)
      self.atMs = try c.decode(UInt64.self, forKey: .atMs)
      self.says = try c.decode(Text.self, forKey: .says)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(id, forKey: .id)
      try c.encode(title, forKey: .title)
      try c.encode(starred, forKey: .starred)
      try c.encode(from, forKey: .from)
      try c.encode(label, forKey: .label)
      try c.encode(atMs, forKey: .atMs)
      try c.encode(says, forKey: .says)
    }
  }

  /// A desk's goals: a note of the person's, at most twenty lines.
  public struct GoalsView: Codable, Hashable, Sendable {
    public var deskId: String
    /// The lines; empty when there are none (the shell shows its faint "Goals…").
    public var lines: [String]
    /// Two lines or more: one toggle folds them to the first line. Whether they are folded is this device's.
    public var foldable: Bool
    /// What the toggle says while they are folded ("Show all 4 goals").
    public var showAll: Text?

    public init(deskId: String = "", lines: [String] = [], foldable: Bool = false, showAll: Text? = nil) {
      self.deskId = deskId
      self.lines = lines
      self.foldable = foldable
      self.showAll = showAll
    }

    enum CodingKeys: String, CodingKey {
      case deskId = "desk_id"
      case lines = "lines"
      case foldable = "foldable"
      case showAll = "show_all"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.deskId = try c.decode(String.self, forKey: .deskId)
      self.lines = try c.decode([String].self, forKey: .lines)
      self.foldable = try c.decode(Bool.self, forKey: .foldable)
      self.showAll = try c.decodeIfPresent(Text.self, forKey: .showAll)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(deskId, forKey: .deskId)
      try c.encode(lines, forKey: .lines)
      try c.encode(foldable, forKey: .foldable)
      try c.encodeIfPresent(showAll, forKey: .showAll)
    }
  }

  /// A session that is cut off, as the Desk says it.
  public struct SlipView: Codable, Hashable, Sendable {
    public var sessionId: String
    public var text: Text
    public var fix: FixView

    public init(sessionId: String = "", text: Text = Text(), fix: FixView = FixView()) {
      self.sessionId = sessionId
      self.text = text
      self.fix = fix
    }

    enum CodingKeys: String, CodingKey {
      case sessionId = "session_id"
      case text = "text"
      case fix = "fix"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.sessionId = try c.decode(String.self, forKey: .sessionId)
      self.text = try c.decode(Text.self, forKey: .text)
      self.fix = try c.decode(FixView.self, forKey: .fix)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(sessionId, forKey: .sessionId)
      try c.encode(text, forKey: .text)
      try c.encode(fix, forKey: .fix)
    }
  }

  /// One row of the Desk: a question, a request for approval, or something to read.
  public struct DeskRowView: Codable, Hashable, Sendable {
    public var id: String
    /// "decision", "info" (to read: also a card not read here yet, and one of a newer Trommi), "request".
    public var kind: String
    /// Its body is not read on this device yet: a quiet placeholder.
    public var loading: Bool
    /// A card of a newer Trommi: its title where readable, nothing to answer.
    public var newer: Bool
    public var title: String
    /// One or two lines under the title: its teaser, else its body as plain words.
    public var words: String
    /// Small words before them: "replaces 2 questions", "revised", "Back from Later", why it is urgent.
    public var notes: [Text]
    /// "low", "normal", "high", "critical".
    public var urgency: String
    /// The sign before the title: "block" (blocking, or an approval), "knock" (urgent), "whenever" (low), "".
    public var sign: String
    /// What the sign says ("Knock! Blocking").
    public var signSays: Text?
    public var starred: Bool
    /// When it was asked.
    public var createdMs: UInt64
    /// Who asked; none for a session the list does not show.
    public var from: FromView?
    /// On the first row of a run (rows of one session that follow each other): how many rows it has. Else 0.
    public var run: UInt32
    /// Its pictures and videos, for the small fan beside the title: the first three.
    public var media: [MediaView]
    /// What the fan says ("Look at 4 pictures and videos").
    public var mediaSays: Text?
    /// The answers the row itself offers.
    public var tiles: TilesView
    /// The ways to put it aside that apply, in order: "later", "reverse", "duck", "shred".
    public var ways: [String]

    public init(id: String = "", kind: String = "", loading: Bool = false, newer: Bool = false, title: String = "", words: String = "", notes: [Text] = [], urgency: String = "", sign: String = "", signSays: Text? = nil, starred: Bool = false, createdMs: UInt64 = 0, from: FromView? = nil, run: UInt32 = 0, media: [MediaView] = [], mediaSays: Text? = nil, tiles: TilesView = TilesView(), ways: [String] = []) {
      self.id = id
      self.kind = kind
      self.loading = loading
      self.newer = newer
      self.title = title
      self.words = words
      self.notes = notes
      self.urgency = urgency
      self.sign = sign
      self.signSays = signSays
      self.starred = starred
      self.createdMs = createdMs
      self.from = from
      self.run = run
      self.media = media
      self.mediaSays = mediaSays
      self.tiles = tiles
      self.ways = ways
    }

    enum CodingKeys: String, CodingKey {
      case id = "id"
      case kind = "kind"
      case loading = "loading"
      case newer = "newer"
      case title = "title"
      case words = "words"
      case notes = "notes"
      case urgency = "urgency"
      case sign = "sign"
      case signSays = "sign_says"
      case starred = "starred"
      case createdMs = "created_ms"
      case from = "from"
      case run = "run"
      case media = "media"
      case mediaSays = "media_says"
      case tiles = "tiles"
      case ways = "ways"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.id = try c.decode(String.self, forKey: .id)
      self.kind = try c.decode(String.self, forKey: .kind)
      self.loading = try c.decodeIfPresent(Bool.self, forKey: .loading) ?? false
      self.newer = try c.decodeIfPresent(Bool.self, forKey: .newer) ?? false
      self.title = try c.decode(String.self, forKey: .title)
      self.words = try c.decodeIfPresent(String.self, forKey: .words) ?? ""
      self.notes = try c.decodeIfPresent([Text].self, forKey: .notes) ?? []
      self.urgency = try c.decode(String.self, forKey: .urgency)
      self.sign = try c.decodeIfPresent(String.self, forKey: .sign) ?? ""
      self.signSays = try c.decodeIfPresent(Text.self, forKey: .signSays)
      self.starred = try c.decodeIfPresent(Bool.self, forKey: .starred) ?? false
      self.createdMs = try c.decode(UInt64.self, forKey: .createdMs)
      self.from = try c.decodeIfPresent(FromView.self, forKey: .from)
      self.run = try c.decodeIfPresent(UInt32.self, forKey: .run) ?? 0
      self.media = try c.decodeIfPresent([MediaView].self, forKey: .media) ?? []
      self.mediaSays = try c.decodeIfPresent(Text.self, forKey: .mediaSays)
      self.tiles = try c.decode(TilesView.self, forKey: .tiles)
      self.ways = try c.decode([String].self, forKey: .ways)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(id, forKey: .id)
      try c.encode(kind, forKey: .kind)
      try c.encode(loading, forKey: .loading)
      try c.encode(newer, forKey: .newer)
      try c.encode(title, forKey: .title)
      try c.encode(words, forKey: .words)
      if !notes.isEmpty { try c.encode(notes, forKey: .notes) }
      try c.encode(urgency, forKey: .urgency)
      try c.encode(sign, forKey: .sign)
      try c.encodeIfPresent(signSays, forKey: .signSays)
      try c.encode(starred, forKey: .starred)
      try c.encode(createdMs, forKey: .createdMs)
      try c.encodeIfPresent(from, forKey: .from)
      try c.encode(run, forKey: .run)
      if !media.isEmpty { try c.encode(media, forKey: .media) }
      try c.encodeIfPresent(mediaSays, forKey: .mediaSays)
      try c.encode(tiles, forKey: .tiles)
      try c.encode(ways, forKey: .ways)
    }
  }

  /// Who a card comes from.
  public struct FromView: Codable, Hashable, Sendable {
    public var sessionId: String
    public var name: String
    /// What draws the session (`SessionRowView::mark`).
    public var mark: String
    /// Where it stands, for the line above a card's title ("Desk › Main › Helper"): its desk while there are two
    /// or more, its main for a helper, itself.
    public var path: [String]
    /// Its desk's name, on All desks only: the small chip before the row's words.
    public var desk: String?

    public init(sessionId: String = "", name: String = "", mark: String = "", path: [String] = [], desk: String? = nil) {
      self.sessionId = sessionId
      self.name = name
      self.mark = mark
      self.path = path
      self.desk = desk
    }

    enum CodingKeys: String, CodingKey {
      case sessionId = "session_id"
      case name = "name"
      case mark = "mark"
      case path = "path"
      case desk = "desk"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.sessionId = try c.decode(String.self, forKey: .sessionId)
      self.name = try c.decode(String.self, forKey: .name)
      self.mark = try c.decode(String.self, forKey: .mark)
      self.path = try c.decode([String].self, forKey: .path)
      self.desk = try c.decodeIfPresent(String.self, forKey: .desk)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(sessionId, forKey: .sessionId)
      try c.encode(name, forKey: .name)
      try c.encode(mark, forKey: .mark)
      try c.encode(path, forKey: .path)
      try c.encodeIfPresent(desk, forKey: .desk)
    }
  }

  public struct MediaView: Codable, Hashable, Sendable {
    /// The file (`ViewModel.files`, `Action::OpenFile`).
    public var fileId: String
    /// "image" or "video".
    public var kind: String

    public init(fileId: String = "", kind: String = "") {
      self.fileId = fileId
      self.kind = kind
    }

    enum CodingKeys: String, CodingKey {
      case fileId = "file_id"
      case kind = "kind"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.fileId = try c.decode(String.self, forKey: .fileId)
      self.kind = try c.decode(String.self, forKey: .kind)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(fileId, forKey: .fileId)
      try c.encode(kind, forKey: .kind)
    }
  }

  /// The answers a row offers without opening the card.
  public struct TilesView: Codable, Hashable, Sendable {
    /// "none" (not read on this device yet: nothing to answer), "read" (What?? and Got it), "pair" (two answers, the lead last), "advised" (the advised answer, and
    /// "+N other ways" that opens the card), "choose" (one tile that opens the card).
    public var kind: String
    /// The answers of "pair" and "advised", in drawing order.
    public var tiles: [TileView]
    /// "advised": how many other ways there are.
    public var others: UInt32
    /// "advised": "3 other ways: open the card"; "choose": "Choose: 4 options, several".
    public var says: Text?

    public init(kind: String = "", tiles: [TileView] = [], others: UInt32 = 0, says: Text? = nil) {
      self.kind = kind
      self.tiles = tiles
      self.others = others
      self.says = says
    }

    enum CodingKeys: String, CodingKey {
      case kind = "kind"
      case tiles = "tiles"
      case others = "others"
      case says = "says"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.kind = try c.decode(String.self, forKey: .kind)
      self.tiles = try c.decodeIfPresent([TileView].self, forKey: .tiles) ?? []
      self.others = try c.decodeIfPresent(UInt32.self, forKey: .others) ?? 0
      self.says = try c.decodeIfPresent(Text.self, forKey: .says)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(kind, forKey: .kind)
      if !tiles.isEmpty { try c.encode(tiles, forKey: .tiles) }
      try c.encode(others, forKey: .others)
      try c.encodeIfPresent(says, forKey: .says)
    }
  }

  public struct TileView: Codable, Hashable, Sendable {
    /// The option's key (`Action::Answer`; "allow" and "deny" of a request: `Action::Verdict`).
    public var key: String
    /// The words on the tile; empty for a tile that shows its drawing alone.
    public var label: String
    /// Its drawing: "yes" (the lead: thumb up, or the filled tile) or "no".
    public var thumb: String
    /// A real yes or no (thumbs); else two named tiles.
    public var thumbs: Bool
    public var lead: Bool
    /// Its agent advises it.
    public var advised: Bool
    /// Its words need the small type.
    public var small: Bool
    /// `label` is the option's short name (its full one fits no tile).
    public var short: Bool
    /// Choosing it settles the card (its agent marked it final).
    public var settles: Bool
    /// The tooltip ("Recommended", "Settles it", or the option's full name); none without one.
    public var tip: Text?
    /// What it says to a screen reader ("Ship it (settles it)").
    public var says: Text

    public init(key: String = "", label: String = "", thumb: String = "", thumbs: Bool = false, lead: Bool = false, advised: Bool = false, small: Bool = false, short: Bool = false, settles: Bool = false, tip: Text? = nil, says: Text = Text()) {
      self.key = key
      self.label = label
      self.thumb = thumb
      self.thumbs = thumbs
      self.lead = lead
      self.advised = advised
      self.small = small
      self.short = short
      self.settles = settles
      self.tip = tip
      self.says = says
    }

    enum CodingKeys: String, CodingKey {
      case key = "key"
      case label = "label"
      case thumb = "thumb"
      case thumbs = "thumbs"
      case lead = "lead"
      case advised = "advised"
      case small = "small"
      case short = "short"
      case settles = "settles"
      case tip = "tip"
      case says = "says"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.key = try c.decode(String.self, forKey: .key)
      self.label = try c.decode(String.self, forKey: .label)
      self.thumb = try c.decode(String.self, forKey: .thumb)
      self.thumbs = try c.decode(Bool.self, forKey: .thumbs)
      self.lead = try c.decode(Bool.self, forKey: .lead)
      self.advised = try c.decode(Bool.self, forKey: .advised)
      self.small = try c.decode(Bool.self, forKey: .small)
      self.short = try c.decode(Bool.self, forKey: .short)
      self.settles = try c.decode(Bool.self, forKey: .settles)
      self.tip = try c.decodeIfPresent(Text.self, forKey: .tip)
      self.says = try c.decode(Text.self, forKey: .says)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(key, forKey: .key)
      try c.encode(label, forKey: .label)
      try c.encode(thumb, forKey: .thumb)
      try c.encode(thumbs, forKey: .thumbs)
      try c.encode(lead, forKey: .lead)
      try c.encode(advised, forKey: .advised)
      try c.encode(small, forKey: .small)
      try c.encode(short, forKey: .short)
      try c.encode(settles, forKey: .settles)
      try c.encodeIfPresent(tip, forKey: .tip)
      try c.encode(says, forKey: .says)
    }
  }

  /// One session's page: its heading, its conversation, the status lines of its running work, and what stands above
  /// the field to write in. Made for the session in view (`Lens::session`).
  public struct SessionView: Codable, Hashable, Sendable {
    public var id: String
    public var name: Text
    /// "live", "oncall", "asleep", "cut", "gone"; "" for an archived or removed session.
    public var link: String
    public var blocked: Bool
    /// Talk rows, newest last.
    public var talk: [TalkRow]
    public var statusLines: [StatusLineView]
    /// The hub holds older messages than `talk` has.
    public var hasMore: Bool
    /// Its short id on the board (`/chat/<slug>`).
    public var slug: String
    /// What draws it (`SessionRowView::mark`).
    public var mark: String
    public var deskId: String
    public var main: Bool
    public var crowned: Bool
    public var online: Bool
    /// Where it stands: its desk (while there are two or more), its main (for a helper), itself.
    public var path: [String]
    /// The model it runs on, as the heading says it ("Opus 5.5"); "" when it named none.
    public var modelLabel: String
    /// The model's id, for the tooltip.
    public var model: String
    /// The dot beside the name.
    public var dot: DotView
    /// Why it is stopped (the red hand).
    public var hand: HandView?
    /// The quiet hint ("quiet for 24 min"): connected, working, and nothing from it for a while. None while stopped.
    public var quiet: Text?
    /// Above the field: why it does not hear and the step in its terminal, or that his last message waits.
    public var note: LinkNoteView?
    /// What the field says while it is empty.
    public var compose: Text
    /// "<name> is working": his word was the last, a short while ago.
    public var answering: Text?
    /// Its questions that wait for the person (its infos too), and the way to the first of them.
    public var open: UInt32
    /// "3 open".
    public var openWord: Text?
    /// "3 open questions: go to the first one".
    public var openSays: Text?
    /// The talk row of the first of them, where it was asked.
    public var firstOpen: String?
    /// An empty conversation: the card of the agent that joined, and three ways to start.
    public var empty: EmptyTalkView?

    public init(id: String = "", name: Text = Text(), link: String = "", blocked: Bool = false, talk: [TalkRow] = [], statusLines: [StatusLineView] = [], hasMore: Bool = false, slug: String = "", mark: String = "", deskId: String = "", main: Bool = false, crowned: Bool = false, online: Bool = false, path: [String] = [], modelLabel: String = "", model: String = "", dot: DotView = DotView(), hand: HandView? = nil, quiet: Text? = nil, note: LinkNoteView? = nil, compose: Text = Text(), answering: Text? = nil, open: UInt32 = 0, openWord: Text? = nil, openSays: Text? = nil, firstOpen: String? = nil, empty: EmptyTalkView? = nil) {
      self.id = id
      self.name = name
      self.link = link
      self.blocked = blocked
      self.talk = talk
      self.statusLines = statusLines
      self.hasMore = hasMore
      self.slug = slug
      self.mark = mark
      self.deskId = deskId
      self.main = main
      self.crowned = crowned
      self.online = online
      self.path = path
      self.modelLabel = modelLabel
      self.model = model
      self.dot = dot
      self.hand = hand
      self.quiet = quiet
      self.note = note
      self.compose = compose
      self.answering = answering
      self.open = open
      self.openWord = openWord
      self.openSays = openSays
      self.firstOpen = firstOpen
      self.empty = empty
    }

    enum CodingKeys: String, CodingKey {
      case id = "id"
      case name = "name"
      case link = "link"
      case blocked = "blocked"
      case talk = "talk"
      case statusLines = "status_lines"
      case hasMore = "has_more"
      case slug = "slug"
      case mark = "mark"
      case deskId = "desk_id"
      case main = "main"
      case crowned = "crowned"
      case online = "online"
      case path = "path"
      case modelLabel = "model_label"
      case model = "model"
      case dot = "dot"
      case hand = "hand"
      case quiet = "quiet"
      case note = "note"
      case compose = "compose"
      case answering = "answering"
      case open = "open"
      case openWord = "open_word"
      case openSays = "open_says"
      case firstOpen = "first_open"
      case empty = "empty"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.id = try c.decode(String.self, forKey: .id)
      self.name = try c.decode(Text.self, forKey: .name)
      self.link = try c.decode(String.self, forKey: .link)
      self.blocked = try c.decode(Bool.self, forKey: .blocked)
      self.talk = try c.decode([TalkRow].self, forKey: .talk)
      self.statusLines = try c.decode([StatusLineView].self, forKey: .statusLines)
      self.hasMore = try c.decode(Bool.self, forKey: .hasMore)
      self.slug = try c.decodeIfPresent(String.self, forKey: .slug) ?? ""
      self.mark = try c.decodeIfPresent(String.self, forKey: .mark) ?? ""
      self.deskId = try c.decodeIfPresent(String.self, forKey: .deskId) ?? ""
      self.main = try c.decodeIfPresent(Bool.self, forKey: .main) ?? false
      self.crowned = try c.decodeIfPresent(Bool.self, forKey: .crowned) ?? false
      self.online = try c.decodeIfPresent(Bool.self, forKey: .online) ?? false
      self.path = try c.decodeIfPresent([String].self, forKey: .path) ?? []
      self.modelLabel = try c.decodeIfPresent(String.self, forKey: .modelLabel) ?? ""
      self.model = try c.decodeIfPresent(String.self, forKey: .model) ?? ""
      self.dot = try c.decodeIfPresent(DotView.self, forKey: .dot) ?? DotView()
      self.hand = try c.decodeIfPresent(HandView.self, forKey: .hand)
      self.quiet = try c.decodeIfPresent(Text.self, forKey: .quiet)
      self.note = try c.decodeIfPresent(LinkNoteView.self, forKey: .note)
      self.compose = try c.decodeIfPresent(Text.self, forKey: .compose) ?? Text()
      self.answering = try c.decodeIfPresent(Text.self, forKey: .answering)
      self.open = try c.decodeIfPresent(UInt32.self, forKey: .open) ?? 0
      self.openWord = try c.decodeIfPresent(Text.self, forKey: .openWord)
      self.openSays = try c.decodeIfPresent(Text.self, forKey: .openSays)
      self.firstOpen = try c.decodeIfPresent(String.self, forKey: .firstOpen)
      self.empty = try c.decodeIfPresent(EmptyTalkView.self, forKey: .empty)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(id, forKey: .id)
      try c.encode(name, forKey: .name)
      try c.encode(link, forKey: .link)
      try c.encode(blocked, forKey: .blocked)
      try c.encode(talk, forKey: .talk)
      try c.encode(statusLines, forKey: .statusLines)
      try c.encode(hasMore, forKey: .hasMore)
      try c.encode(slug, forKey: .slug)
      try c.encode(mark, forKey: .mark)
      try c.encode(deskId, forKey: .deskId)
      try c.encode(main, forKey: .main)
      try c.encode(crowned, forKey: .crowned)
      try c.encode(online, forKey: .online)
      if !path.isEmpty { try c.encode(path, forKey: .path) }
      try c.encode(modelLabel, forKey: .modelLabel)
      try c.encode(model, forKey: .model)
      try c.encode(dot, forKey: .dot)
      try c.encodeIfPresent(hand, forKey: .hand)
      try c.encodeIfPresent(quiet, forKey: .quiet)
      try c.encodeIfPresent(note, forKey: .note)
      try c.encode(compose, forKey: .compose)
      try c.encodeIfPresent(answering, forKey: .answering)
      try c.encode(open, forKey: .open)
      try c.encodeIfPresent(openWord, forKey: .openWord)
      try c.encodeIfPresent(openSays, forKey: .openSays)
      try c.encodeIfPresent(firstOpen, forKey: .firstOpen)
      try c.encodeIfPresent(empty, forKey: .empty)
    }
  }

  /// The dot beside a session's name.
  public struct DotView: Codable, Hashable, Sendable {
    /// Connected (green).
    public var on: Bool
    /// How it hears when it does not hear at once: "oncall", "asleep", "cut", "gone"; else "".
    public var link: String
    /// The tooltip: "connected", "disconnected", or the link's words.
    public var word: Text
    /// For a screen reader: the word, and what it is at.
    public var says: Text

    public init(on: Bool = false, link: String = "", word: Text = Text(), says: Text = Text()) {
      self.on = on
      self.link = link
      self.word = word
      self.says = says
    }

    enum CodingKeys: String, CodingKey {
      case on = "on"
      case link = "link"
      case word = "word"
      case says = "says"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.on = try c.decode(Bool.self, forKey: .on)
      self.link = try c.decode(String.self, forKey: .link)
      self.word = try c.decode(Text.self, forKey: .word)
      self.says = try c.decode(Text.self, forKey: .says)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(on, forKey: .on)
      try c.encode(link, forKey: .link)
      try c.encode(word, forKey: .word)
      try c.encode(says, forKey: .says)
    }
  }

  /// The note above a field: a receipt, the sentence about the session, the way out.
  public struct LinkNoteView: Codable, Hashable, Sendable {
    /// "cut", "gone", "asleep", "oncall" (the link's state), or "unheard", "sent", "heard" (a receipt alone).
    public var tone: String
    public var receipt: Text?
    public var line: Text?
    public var fix: FixView?

    public init(tone: String = "", receipt: Text? = nil, line: Text? = nil, fix: FixView? = nil) {
      self.tone = tone
      self.receipt = receipt
      self.line = line
      self.fix = fix
    }

    enum CodingKeys: String, CodingKey {
      case tone = "tone"
      case receipt = "receipt"
      case line = "line"
      case fix = "fix"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.tone = try c.decode(String.self, forKey: .tone)
      self.receipt = try c.decodeIfPresent(Text.self, forKey: .receipt)
      self.line = try c.decodeIfPresent(Text.self, forKey: .line)
      self.fix = try c.decodeIfPresent(FixView.self, forKey: .fix)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(tone, forKey: .tone)
      try c.encodeIfPresent(receipt, forKey: .receipt)
      try c.encodeIfPresent(line, forKey: .line)
      try c.encodeIfPresent(fix, forKey: .fix)
    }
  }

  public struct EmptyTalkView: Codable, Hashable, Sendable {
    /// Where it runs, quietly under its name ("Opus 5.5"); "" when it said nothing.
    public var meta: String
    public var text: Text
    /// A tap sends one.
    public var starters: [Text]

    public init(meta: String = "", text: Text = Text(), starters: [Text] = []) {
      self.meta = meta
      self.text = text
      self.starters = starters
    }

    enum CodingKeys: String, CodingKey {
      case meta = "meta"
      case text = "text"
      case starters = "starters"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.meta = try c.decode(String.self, forKey: .meta)
      self.text = try c.decode(Text.self, forKey: .text)
      self.starters = try c.decode([Text].self, forKey: .starters)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(meta, forKey: .meta)
      try c.encode(text, forKey: .text)
      try c.encode(starters, forKey: .starters)
    }
  }

  /// One row of a conversation. `kind` says what it is; the fields it needs are filled.
  public struct TalkRow: Codable, Hashable, Sendable {
    public var id: String
    public var fromMe: Bool
    public var text: String
    public var timeMs: UInt64
    /// His message: its session picked it up (false also when its connector writes no receipts).
    public var heard: Bool
    /// Not confirmed by the hub yet (an echo).
    public var pending: Bool
    /// File ids (`ViewModel.files`).
    public var files: [String]
    /// "user" (his bubble), "agent" (the agent's words, light markdown), "note" (his note, taped on), "event" (one
    /// quiet line: `event`), "ask" (an open question where it was asked: `row`), "work" (a turn's trail: `work`),
    /// "artifact" (something the session published: `published`), "newer" (written by a newer Trommi).
    public var kind: String
    /// Its place in the hub's order (an envelope number); 0 for what has none yet.
    public var number: UInt64
    /// The line above it when a new day begins there ("Today", "Yesterday", "Tuesday 13 October").
    public var day: Text?
    /// It goes on a run of the same side: no head, close to the row before.
    public var cont: Bool
    /// Typed into the session's terminal (his words, sent for him by the agent's connector), or the agent's answer there.
    public var terminal: Bool
    /// The card it is about.
    public var card: CardRefView?
    public var event: EventView?
    /// What he did with a card along with these words (handed back, took back, What??): a quiet line above them.
    public var deed: EventView?
    public var row: DeskRowView?
    public var work: WorkView?
    /// The Artifact it announces (its object id).
    public var published: String?
    /// The agent's details, folded under its words.
    public var details: String
    /// The agent's layout under its words (drawn in the sandboxed frame).
    public var html: String
    /// The cards it carries, a chip each.
    public var chips: [CardRefView]
    /// A selection of the Scribble Board (its one file is the picture).
    public var scribble: Bool
    /// A note: when it was written.
    public var writtenMs: UInt64?

    public init(id: String = "", fromMe: Bool = false, text: String = "", timeMs: UInt64 = 0, heard: Bool = false, pending: Bool = false, files: [String] = [], kind: String = "", number: UInt64 = 0, day: Text? = nil, cont: Bool = false, terminal: Bool = false, card: CardRefView? = nil, event: EventView? = nil, deed: EventView? = nil, row: DeskRowView? = nil, work: WorkView? = nil, published: String? = nil, details: String = "", html: String = "", chips: [CardRefView] = [], scribble: Bool = false, writtenMs: UInt64? = nil) {
      self.id = id
      self.fromMe = fromMe
      self.text = text
      self.timeMs = timeMs
      self.heard = heard
      self.pending = pending
      self.files = files
      self.kind = kind
      self.number = number
      self.day = day
      self.cont = cont
      self.terminal = terminal
      self.card = card
      self.event = event
      self.deed = deed
      self.row = row
      self.work = work
      self.published = published
      self.details = details
      self.html = html
      self.chips = chips
      self.scribble = scribble
      self.writtenMs = writtenMs
    }

    enum CodingKeys: String, CodingKey {
      case id = "id"
      case fromMe = "from_me"
      case text = "text"
      case timeMs = "time_ms"
      case heard = "heard"
      case pending = "pending"
      case files = "files"
      case kind = "kind"
      case number = "number"
      case day = "day"
      case cont = "cont"
      case terminal = "terminal"
      case card = "card"
      case event = "event"
      case deed = "deed"
      case row = "row"
      case work = "work"
      case published = "published"
      case details = "details"
      case html = "html"
      case chips = "chips"
      case scribble = "scribble"
      case writtenMs = "written_ms"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.id = try c.decode(String.self, forKey: .id)
      self.fromMe = try c.decode(Bool.self, forKey: .fromMe)
      self.text = try c.decode(String.self, forKey: .text)
      self.timeMs = try c.decode(UInt64.self, forKey: .timeMs)
      self.heard = try c.decode(Bool.self, forKey: .heard)
      self.pending = try c.decode(Bool.self, forKey: .pending)
      self.files = try c.decodeIfPresent([String].self, forKey: .files) ?? []
      self.kind = try c.decodeIfPresent(String.self, forKey: .kind) ?? ""
      self.number = try c.decodeIfPresent(UInt64.self, forKey: .number) ?? 0
      self.day = try c.decodeIfPresent(Text.self, forKey: .day)
      self.cont = try c.decodeIfPresent(Bool.self, forKey: .cont) ?? false
      self.terminal = try c.decodeIfPresent(Bool.self, forKey: .terminal) ?? false
      self.card = try c.decodeIfPresent(CardRefView.self, forKey: .card)
      self.event = try c.decodeIfPresent(EventView.self, forKey: .event)
      self.deed = try c.decodeIfPresent(EventView.self, forKey: .deed)
      self.row = try c.decodeIfPresent(DeskRowView.self, forKey: .row)
      self.work = try c.decodeIfPresent(WorkView.self, forKey: .work)
      self.published = try c.decodeIfPresent(String.self, forKey: .published)
      self.details = try c.decodeIfPresent(String.self, forKey: .details) ?? ""
      self.html = try c.decodeIfPresent(String.self, forKey: .html) ?? ""
      self.chips = try c.decodeIfPresent([CardRefView].self, forKey: .chips) ?? []
      self.scribble = try c.decodeIfPresent(Bool.self, forKey: .scribble) ?? false
      self.writtenMs = try c.decodeIfPresent(UInt64.self, forKey: .writtenMs)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(id, forKey: .id)
      try c.encode(fromMe, forKey: .fromMe)
      try c.encode(text, forKey: .text)
      try c.encode(timeMs, forKey: .timeMs)
      try c.encode(heard, forKey: .heard)
      try c.encode(pending, forKey: .pending)
      if !files.isEmpty { try c.encode(files, forKey: .files) }
      try c.encode(kind, forKey: .kind)
      try c.encode(number, forKey: .number)
      try c.encodeIfPresent(day, forKey: .day)
      try c.encode(cont, forKey: .cont)
      try c.encode(terminal, forKey: .terminal)
      try c.encodeIfPresent(card, forKey: .card)
      try c.encodeIfPresent(event, forKey: .event)
      try c.encodeIfPresent(deed, forKey: .deed)
      try c.encodeIfPresent(row, forKey: .row)
      try c.encodeIfPresent(work, forKey: .work)
      try c.encodeIfPresent(published, forKey: .published)
      if !details.isEmpty { try c.encode(details, forKey: .details) }
      if !html.isEmpty { try c.encode(html, forKey: .html) }
      if !chips.isEmpty { try c.encode(chips, forKey: .chips) }
      try c.encode(scribble, forKey: .scribble)
      try c.encodeIfPresent(writtenMs, forKey: .writtenMs)
    }
  }

  /// A card as a row of a conversation names it.
  public struct CardRefView: Codable, Hashable, Sendable {
    public var id: String
    /// Its number ("Nr. 12"); 0 when this device does not hold it.
    public var number: UInt32
    public var title: String

    public init(id: String = "", number: UInt32 = 0, title: String = "") {
      self.id = id
      self.number = number
      self.title = title
    }

    enum CodingKeys: String, CodingKey {
      case id = "id"
      case number = "number"
      case title = "title"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.id = try c.decode(String.self, forKey: .id)
      self.number = try c.decode(UInt32.self, forKey: .number)
      self.title = try c.decode(String.self, forKey: .title)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(id, forKey: .id)
      try c.encode(number, forKey: .number)
      try c.encode(title, forKey: .title)
    }
  }

  /// One quiet line for something that happened to a card.
  public struct EventView: Codable, Hashable, Sendable {
    /// "asked", "info", "revised", "decided", "trusted", "read", "shredded", "reopened", "done", "handed", "snoozed",
    /// "took_back", "handed_back", "explain".
    public var kind: String
    /// "New question", "Answered", …
    public var label: Text
    /// The card's title, when the words do not say it already.
    public var about: String
    public var text: String
    /// What became of the question is said where it was asked: this line only repeats it.
    public var echo: Bool

    public init(kind: String = "", label: Text = Text(), about: String = "", text: String = "", echo: Bool = false) {
      self.kind = kind
      self.label = label
      self.about = about
      self.text = text
      self.echo = echo
    }

    enum CodingKeys: String, CodingKey {
      case kind = "kind"
      case label = "label"
      case about = "about"
      case text = "text"
      case echo = "echo"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.kind = try c.decode(String.self, forKey: .kind)
      self.label = try c.decode(Text.self, forKey: .label)
      self.about = try c.decodeIfPresent(String.self, forKey: .about) ?? ""
      self.text = try c.decodeIfPresent(String.self, forKey: .text) ?? ""
      self.echo = try c.decodeIfPresent(Bool.self, forKey: .echo) ?? false
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(kind, forKey: .kind)
      try c.encode(label, forKey: .label)
      if !about.isEmpty { try c.encode(about, forKey: .about) }
      if !text.isEmpty { try c.encode(text, forKey: .text) }
      try c.encode(echo, forKey: .echo)
    }
  }

  /// A turn's trail: what the agent did between his prompt and its answer, as one block.
  public struct WorkView: Codable, Hashable, Sendable {
    public var turn: String
    /// "running", "done", "interrupted", "failed".
    public var state: String
    /// "Working", "Worked 2 min", "Interrupted after 12 s", "Stopped by an error after 1 h 5 min".
    public var head: Text
    /// "7 steps · 1 helper · 2 with errors"; none without any.
    public var sum: Text?
    /// While it runs: the step it is at ("Bash · run the tests").
    public var now: String
    public var lines: [WorkLineView]
    /// "and 12 more steps, not listed".
    public var rest: Text?

    public init(turn: String = "", state: String = "", head: Text = Text(), sum: Text? = nil, now: String = "", lines: [WorkLineView] = [], rest: Text? = nil) {
      self.turn = turn
      self.state = state
      self.head = head
      self.sum = sum
      self.now = now
      self.lines = lines
      self.rest = rest
    }

    enum CodingKeys: String, CodingKey {
      case turn = "turn"
      case state = "state"
      case head = "head"
      case sum = "sum"
      case now = "now"
      case lines = "lines"
      case rest = "rest"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.turn = try c.decode(String.self, forKey: .turn)
      self.state = try c.decode(String.self, forKey: .state)
      self.head = try c.decode(Text.self, forKey: .head)
      self.sum = try c.decodeIfPresent(Text.self, forKey: .sum)
      self.now = try c.decodeIfPresent(String.self, forKey: .now) ?? ""
      self.lines = try c.decode([WorkLineView].self, forKey: .lines)
      self.rest = try c.decodeIfPresent(Text.self, forKey: .rest)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(turn, forKey: .turn)
      try c.encode(state, forKey: .state)
      try c.encode(head, forKey: .head)
      try c.encodeIfPresent(sum, forKey: .sum)
      if !now.isEmpty { try c.encode(now, forKey: .now) }
      try c.encode(lines, forKey: .lines)
      try c.encodeIfPresent(rest, forKey: .rest)
    }
  }

  public struct WorkLineView: Codable, Hashable, Sendable {
    public var id: String
    /// "step", "helper", or "text" (the agent's words between two steps: `text`).
    public var kind: String
    /// "running", "ok", "failed", "interrupted"; "" for words.
    public var state: String
    /// What its sign says ("done", "exit code 2").
    public var says: Text?
    /// The bold word: the tool, "Helper <name>", or "Step".
    public var tool: String
    /// What it is about ("title · subject").
    public var what: String
    /// "3 steps" (a helper), "exit code 2" (a failed command).
    public var count: Text?
    /// How long it took ("0.3 s", "2 min").
    public var took: String
    public var text: String
    public var input: String
    public var output: String

    public init(id: String = "", kind: String = "", state: String = "", says: Text? = nil, tool: String = "", what: String = "", count: Text? = nil, took: String = "", text: String = "", input: String = "", output: String = "") {
      self.id = id
      self.kind = kind
      self.state = state
      self.says = says
      self.tool = tool
      self.what = what
      self.count = count
      self.took = took
      self.text = text
      self.input = input
      self.output = output
    }

    enum CodingKeys: String, CodingKey {
      case id = "id"
      case kind = "kind"
      case state = "state"
      case says = "says"
      case tool = "tool"
      case what = "what"
      case count = "count"
      case took = "took"
      case text = "text"
      case input = "input"
      case output = "output"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.id = try c.decode(String.self, forKey: .id)
      self.kind = try c.decode(String.self, forKey: .kind)
      self.state = try c.decodeIfPresent(String.self, forKey: .state) ?? ""
      self.says = try c.decodeIfPresent(Text.self, forKey: .says)
      self.tool = try c.decodeIfPresent(String.self, forKey: .tool) ?? ""
      self.what = try c.decodeIfPresent(String.self, forKey: .what) ?? ""
      self.count = try c.decodeIfPresent(Text.self, forKey: .count)
      self.took = try c.decodeIfPresent(String.self, forKey: .took) ?? ""
      self.text = try c.decodeIfPresent(String.self, forKey: .text) ?? ""
      self.input = try c.decodeIfPresent(String.self, forKey: .input) ?? ""
      self.output = try c.decodeIfPresent(String.self, forKey: .output) ?? ""
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(id, forKey: .id)
      try c.encode(kind, forKey: .kind)
      try c.encode(state, forKey: .state)
      try c.encodeIfPresent(says, forKey: .says)
      if !tool.isEmpty { try c.encode(tool, forKey: .tool) }
      if !what.isEmpty { try c.encode(what, forKey: .what) }
      try c.encodeIfPresent(count, forKey: .count)
      if !took.isEmpty { try c.encode(took, forKey: .took) }
      if !text.isEmpty { try c.encode(text, forKey: .text) }
      if !input.isEmpty { try c.encode(input, forKey: .input) }
      if !output.isEmpty { try c.encode(output, forKey: .output) }
    }
  }

  /// A status line of a session's running work, as its page shows it (core/client/README.md "Status lines"): only
  /// the lines that show are here.
  public struct StatusLineView: Codable, Hashable, Sendable {
    public var id: String
    public var label: String
    public var detail: String
    /// "working", "decision", "later", "done" (for ten minutes), "stale" (greyed and still, for a day).
    public var state: String
    /// "working", "waits on you", "put off", "done", "no news for a while".
    public var says: Text
    /// When its agent last set it.
    public var updatedMs: UInt64
    /// The card it stands for: the one it names, else the card with its session whose line it is.
    public var card: CardRefView?

    public init(id: String = "", label: String = "", detail: String = "", state: String = "", says: Text = Text(), updatedMs: UInt64 = 0, card: CardRefView? = nil) {
      self.id = id
      self.label = label
      self.detail = detail
      self.state = state
      self.says = says
      self.updatedMs = updatedMs
      self.card = card
    }

    enum CodingKeys: String, CodingKey {
      case id = "id"
      case label = "label"
      case detail = "detail"
      case state = "state"
      case says = "says"
      case updatedMs = "updated_ms"
      case card = "card"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.id = try c.decode(String.self, forKey: .id)
      self.label = try c.decode(String.self, forKey: .label)
      self.detail = try c.decode(String.self, forKey: .detail)
      self.state = try c.decode(String.self, forKey: .state)
      self.says = try c.decodeIfPresent(Text.self, forKey: .says) ?? Text()
      self.updatedMs = try c.decodeIfPresent(UInt64.self, forKey: .updatedMs) ?? 0
      self.card = try c.decodeIfPresent(CardRefView.self, forKey: .card)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(id, forKey: .id)
      try c.encode(label, forKey: .label)
      try c.encode(detail, forKey: .detail)
      try c.encode(state, forKey: .state)
      try c.encode(says, forKey: .says)
      try c.encode(updatedMs, forKey: .updatedMs)
      try c.encodeIfPresent(card, forKey: .card)
    }
  }

  /// One card's page (a question, something to read, a request for approval): who it comes from, its words, its
  /// answers or what was said, its history, and the comments under it. Made for the card in view (`Lens::card`).
  public struct CardView: Codable, Hashable, Sendable {
    public var id: String
    public var sessionId: String?
    /// "decision", "info", "request", "note".
    public var kind: String
    /// "open", "answered", "closed".
    public var state: String
    /// 0 low, 1 normal, 2 high, 3 critical.
    public var urgency: UInt8
    public var title: String
    /// Its whole text (markdown).
    public var body: String
    /// Its options, in the tiles' order (the advised last).
    public var options: [OptionView]
    /// "You chose: Ship it", when he answered (for a screen reader; drawn from `answer`).
    public var youChose: Text?
    /// How many versions stood before this one.
    public var history: UInt32
    public var starred: Bool
    public var snoozedUntil: UInt64?
    /// File ids (`ViewModel.files`): every attachment that is no page of a picture.
    public var files: [String]
    /// Its number, for its address (`/card/<number>`); 0 when unknown.
    public var number: UInt32
    /// "Nr. 12".
    public var nr: Text
    /// Where it stands: "catching" (settled elsewhere, its outcome on the way), "open", "decided", "shredded", "done".
    public var status: String
    /// Who it comes from, above its title: "Desk › Main › Helper" (`FromView::path`).
    public var from: FromView?
    /// "Knock! Blocking", while it is open and knocks.
    public var knock: Text?
    /// The strip above the title while its session works on it.
    public var work: CardWorkView?
    /// When it was asked, or last revised.
    public var askedMs: UInt64
    /// Its place among what waits on the Desk in view ("3 of 7"), with its neighbours.
    public var place: PlaceView?
    /// Small words of the quiet line: "replaces 2 questions", "revised", "3 messages below".
    public var notes: [Text]
    /// The text that stands on the card (markdown): its paragraphs; code and tables stand in the comments.
    public var text: String
    /// The comments hold more of the text ("More in the comments ↓").
    public var more: Bool
    /// The agent's layout under the text (drawn in the sandboxed frame).
    public var html: String
    /// The options' own paragraphs, in the tiles' order; empty when none says more than its label.
    public var sections: [SectionView]
    /// Several options may be ticked.
    public var multiple: Bool
    /// The answers' column: what can be done, or what was said.
    public var answer: AnswerView
    /// Under the answers: whether its session hears, and whether it has his answer.
    public var note: LinkNoteView?
    /// What the field under the card says while it is empty.
    public var ask: Text
    /// His unsent words (the draft kept with the card).
    public var draft: String
    /// The versions that stood before this one, oldest first.
    public var versions: [VersionView]
    /// What the star says ("Star", "Starred").
    public var starSays: Text
    /// The comments and what happened to the card, oldest first.
    public var talk: [TalkRow]
    /// The hub holds older comments than `talk` has.
    public var hasMore: Bool
    /// The cards it replaces.
    public var mergedFrom: [CardRefView]
    /// The card that replaced it.
    public var mergedInto: CardRefView?

    public init(id: String = "", sessionId: String? = nil, kind: String = "", state: String = "", urgency: UInt8 = 0, title: String = "", body: String = "", options: [OptionView] = [], youChose: Text? = nil, history: UInt32 = 0, starred: Bool = false, snoozedUntil: UInt64? = nil, files: [String] = [], number: UInt32 = 0, nr: Text = Text(), status: String = "", from: FromView? = nil, knock: Text? = nil, work: CardWorkView? = nil, askedMs: UInt64 = 0, place: PlaceView? = nil, notes: [Text] = [], text: String = "", more: Bool = false, html: String = "", sections: [SectionView] = [], multiple: Bool = false, answer: AnswerView = AnswerView(), note: LinkNoteView? = nil, ask: Text = Text(), draft: String = "", versions: [VersionView] = [], starSays: Text = Text(), talk: [TalkRow] = [], hasMore: Bool = false, mergedFrom: [CardRefView] = [], mergedInto: CardRefView? = nil) {
      self.id = id
      self.sessionId = sessionId
      self.kind = kind
      self.state = state
      self.urgency = urgency
      self.title = title
      self.body = body
      self.options = options
      self.youChose = youChose
      self.history = history
      self.starred = starred
      self.snoozedUntil = snoozedUntil
      self.files = files
      self.number = number
      self.nr = nr
      self.status = status
      self.from = from
      self.knock = knock
      self.work = work
      self.askedMs = askedMs
      self.place = place
      self.notes = notes
      self.text = text
      self.more = more
      self.html = html
      self.sections = sections
      self.multiple = multiple
      self.answer = answer
      self.note = note
      self.ask = ask
      self.draft = draft
      self.versions = versions
      self.starSays = starSays
      self.talk = talk
      self.hasMore = hasMore
      self.mergedFrom = mergedFrom
      self.mergedInto = mergedInto
    }

    enum CodingKeys: String, CodingKey {
      case id = "id"
      case sessionId = "session_id"
      case kind = "kind"
      case state = "state"
      case urgency = "urgency"
      case title = "title"
      case body = "body"
      case options = "options"
      case youChose = "you_chose"
      case history = "history"
      case starred = "starred"
      case snoozedUntil = "snoozed_until"
      case files = "files"
      case number = "number"
      case nr = "nr"
      case status = "status"
      case from = "from"
      case knock = "knock"
      case work = "work"
      case askedMs = "asked_ms"
      case place = "place"
      case notes = "notes"
      case text = "text"
      case more = "more"
      case html = "html"
      case sections = "sections"
      case multiple = "multiple"
      case answer = "answer"
      case note = "note"
      case ask = "ask"
      case draft = "draft"
      case versions = "versions"
      case starSays = "star_says"
      case talk = "talk"
      case hasMore = "has_more"
      case mergedFrom = "merged_from"
      case mergedInto = "merged_into"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.id = try c.decode(String.self, forKey: .id)
      self.sessionId = try c.decodeIfPresent(String.self, forKey: .sessionId)
      self.kind = try c.decode(String.self, forKey: .kind)
      self.state = try c.decode(String.self, forKey: .state)
      self.urgency = try c.decode(UInt8.self, forKey: .urgency)
      self.title = try c.decode(String.self, forKey: .title)
      self.body = try c.decode(String.self, forKey: .body)
      self.options = try c.decode([OptionView].self, forKey: .options)
      self.youChose = try c.decodeIfPresent(Text.self, forKey: .youChose)
      self.history = try c.decode(UInt32.self, forKey: .history)
      self.starred = try c.decode(Bool.self, forKey: .starred)
      self.snoozedUntil = try c.decodeIfPresent(UInt64.self, forKey: .snoozedUntil)
      self.files = try c.decodeIfPresent([String].self, forKey: .files) ?? []
      self.number = try c.decodeIfPresent(UInt32.self, forKey: .number) ?? 0
      self.nr = try c.decodeIfPresent(Text.self, forKey: .nr) ?? Text()
      self.status = try c.decodeIfPresent(String.self, forKey: .status) ?? ""
      self.from = try c.decodeIfPresent(FromView.self, forKey: .from)
      self.knock = try c.decodeIfPresent(Text.self, forKey: .knock)
      self.work = try c.decodeIfPresent(CardWorkView.self, forKey: .work)
      self.askedMs = try c.decodeIfPresent(UInt64.self, forKey: .askedMs) ?? 0
      self.place = try c.decodeIfPresent(PlaceView.self, forKey: .place)
      self.notes = try c.decodeIfPresent([Text].self, forKey: .notes) ?? []
      self.text = try c.decodeIfPresent(String.self, forKey: .text) ?? ""
      self.more = try c.decodeIfPresent(Bool.self, forKey: .more) ?? false
      self.html = try c.decodeIfPresent(String.self, forKey: .html) ?? ""
      self.sections = try c.decodeIfPresent([SectionView].self, forKey: .sections) ?? []
      self.multiple = try c.decodeIfPresent(Bool.self, forKey: .multiple) ?? false
      self.answer = try c.decodeIfPresent(AnswerView.self, forKey: .answer) ?? AnswerView()
      self.note = try c.decodeIfPresent(LinkNoteView.self, forKey: .note)
      self.ask = try c.decodeIfPresent(Text.self, forKey: .ask) ?? Text()
      self.draft = try c.decodeIfPresent(String.self, forKey: .draft) ?? ""
      self.versions = try c.decodeIfPresent([VersionView].self, forKey: .versions) ?? []
      self.starSays = try c.decodeIfPresent(Text.self, forKey: .starSays) ?? Text()
      self.talk = try c.decodeIfPresent([TalkRow].self, forKey: .talk) ?? []
      self.hasMore = try c.decodeIfPresent(Bool.self, forKey: .hasMore) ?? false
      self.mergedFrom = try c.decodeIfPresent([CardRefView].self, forKey: .mergedFrom) ?? []
      self.mergedInto = try c.decodeIfPresent(CardRefView.self, forKey: .mergedInto)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(id, forKey: .id)
      try c.encode(sessionId, forKey: .sessionId)
      try c.encode(kind, forKey: .kind)
      try c.encode(state, forKey: .state)
      try c.encode(urgency, forKey: .urgency)
      try c.encode(title, forKey: .title)
      try c.encode(body, forKey: .body)
      try c.encode(options, forKey: .options)
      try c.encodeIfPresent(youChose, forKey: .youChose)
      try c.encode(history, forKey: .history)
      try c.encode(starred, forKey: .starred)
      try c.encodeIfPresent(snoozedUntil, forKey: .snoozedUntil)
      if !files.isEmpty { try c.encode(files, forKey: .files) }
      try c.encode(number, forKey: .number)
      try c.encode(nr, forKey: .nr)
      try c.encode(status, forKey: .status)
      try c.encodeIfPresent(from, forKey: .from)
      try c.encodeIfPresent(knock, forKey: .knock)
      try c.encodeIfPresent(work, forKey: .work)
      try c.encode(askedMs, forKey: .askedMs)
      try c.encodeIfPresent(place, forKey: .place)
      if !notes.isEmpty { try c.encode(notes, forKey: .notes) }
      try c.encode(text, forKey: .text)
      try c.encode(more, forKey: .more)
      if !html.isEmpty { try c.encode(html, forKey: .html) }
      if !sections.isEmpty { try c.encode(sections, forKey: .sections) }
      try c.encode(multiple, forKey: .multiple)
      try c.encode(answer, forKey: .answer)
      try c.encodeIfPresent(note, forKey: .note)
      try c.encode(ask, forKey: .ask)
      if !draft.isEmpty { try c.encode(draft, forKey: .draft) }
      if !versions.isEmpty { try c.encode(versions, forKey: .versions) }
      try c.encode(starSays, forKey: .starSays)
      if !talk.isEmpty { try c.encode(talk, forKey: .talk) }
      try c.encode(hasMore, forKey: .hasMore)
      if !mergedFrom.isEmpty { try c.encode(mergedFrom, forKey: .mergedFrom) }
      try c.encodeIfPresent(mergedInto, forKey: .mergedInto)
    }
  }

  public struct OptionView: Codable, Hashable, Sendable {
    public var key: String
    public var label: String
    public var recommended: Bool
    /// He chose it (the answer that stands), or ticked it (his draft, while several may be ticked).
    public var chosen: Bool
    /// Its line of detail, as plain words.
    public var detail: String
    /// Choosing it settles the card (its agent marked it final).
    public var settles: Bool
    /// His note on it: unsent (his draft) while the card is open, else as he sent it.
    public var note: String
    /// The tooltip ("Recommended · Settles it").
    public var tip: Text?

    public init(key: String = "", label: String = "", recommended: Bool = false, chosen: Bool = false, detail: String = "", settles: Bool = false, note: String = "", tip: Text? = nil) {
      self.key = key
      self.label = label
      self.recommended = recommended
      self.chosen = chosen
      self.detail = detail
      self.settles = settles
      self.note = note
      self.tip = tip
    }

    enum CodingKeys: String, CodingKey {
      case key = "key"
      case label = "label"
      case recommended = "recommended"
      case chosen = "chosen"
      case detail = "detail"
      case settles = "settles"
      case note = "note"
      case tip = "tip"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.key = try c.decode(String.self, forKey: .key)
      self.label = try c.decode(String.self, forKey: .label)
      self.recommended = try c.decode(Bool.self, forKey: .recommended)
      self.chosen = try c.decode(Bool.self, forKey: .chosen)
      self.detail = try c.decodeIfPresent(String.self, forKey: .detail) ?? ""
      self.settles = try c.decodeIfPresent(Bool.self, forKey: .settles) ?? false
      self.note = try c.decodeIfPresent(String.self, forKey: .note) ?? ""
      self.tip = try c.decodeIfPresent(Text.self, forKey: .tip)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(key, forKey: .key)
      try c.encode(label, forKey: .label)
      try c.encode(recommended, forKey: .recommended)
      try c.encode(chosen, forKey: .chosen)
      if !detail.isEmpty { try c.encode(detail, forKey: .detail) }
      try c.encode(settles, forKey: .settles)
      if !note.isEmpty { try c.encode(note, forKey: .note) }
      try c.encodeIfPresent(tip, forKey: .tip)
    }
  }

  /// An option's own paragraph (a card written as sections).
  public struct SectionView: Codable, Hashable, Sendable {
    public var key: String
    public var label: String
    public var recommended: Bool
    /// What it says beyond its label (markdown); "" when nothing.
    public var text: String
    public var html: String

    public init(key: String = "", label: String = "", recommended: Bool = false, text: String = "", html: String = "") {
      self.key = key
      self.label = label
      self.recommended = recommended
      self.text = text
      self.html = html
    }

    enum CodingKeys: String, CodingKey {
      case key = "key"
      case label = "label"
      case recommended = "recommended"
      case text = "text"
      case html = "html"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.key = try c.decode(String.self, forKey: .key)
      self.label = try c.decode(String.self, forKey: .label)
      self.recommended = try c.decode(Bool.self, forKey: .recommended)
      self.text = try c.decode(String.self, forKey: .text)
      self.html = try c.decodeIfPresent(String.self, forKey: .html) ?? ""
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(key, forKey: .key)
      try c.encode(label, forKey: .label)
      try c.encode(recommended, forKey: .recommended)
      try c.encode(text, forKey: .text)
      if !html.isEmpty { try c.encode(html, forKey: .html) }
    }
  }

  /// The strip above a card's title while its session works on it.
  public struct CardWorkView: Codable, Hashable, Sendable {
    public var sessionId: String
    /// What draws the session.
    public var mark: String
    /// "Fernly Website is working on it".
    public var says: Text
    /// Its status line ("Launch checklist · 14 of 19 done"), else "Reworking the card".
    public var words: Text
    /// When that was said.
    public var atMs: UInt64

    public init(sessionId: String = "", mark: String = "", says: Text = Text(), words: Text = Text(), atMs: UInt64 = 0) {
      self.sessionId = sessionId
      self.mark = mark
      self.says = says
      self.words = words
      self.atMs = atMs
    }

    enum CodingKeys: String, CodingKey {
      case sessionId = "session_id"
      case mark = "mark"
      case says = "says"
      case words = "words"
      case atMs = "at_ms"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.sessionId = try c.decode(String.self, forKey: .sessionId)
      self.mark = try c.decode(String.self, forKey: .mark)
      self.says = try c.decode(Text.self, forKey: .says)
      self.words = try c.decode(Text.self, forKey: .words)
      self.atMs = try c.decode(UInt64.self, forKey: .atMs)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(sessionId, forKey: .sessionId)
      try c.encode(mark, forKey: .mark)
      try c.encode(says, forKey: .says)
      try c.encode(words, forKey: .words)
      try c.encode(atMs, forKey: .atMs)
    }
  }

  /// A card's place among what waits on the Desk in view.
  public struct PlaceView: Codable, Hashable, Sendable {
    public var at: UInt32
    public var of: UInt32
    /// "3 of 7".
    public var says: Text
    public var prev: String?
    public var next: String?

    public init(at: UInt32 = 0, of: UInt32 = 0, says: Text = Text(), prev: String? = nil, next: String? = nil) {
      self.at = at
      self.of = of
      self.says = says
      self.prev = prev
      self.next = next
    }

    enum CodingKeys: String, CodingKey {
      case at = "at"
      case of = "of"
      case says = "says"
      case prev = "prev"
      case next = "next"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.at = try c.decode(UInt32.self, forKey: .at)
      self.of = try c.decode(UInt32.self, forKey: .of)
      self.says = try c.decode(Text.self, forKey: .says)
      self.prev = try c.decodeIfPresent(String.self, forKey: .prev)
      self.next = try c.decodeIfPresent(String.self, forKey: .next)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(at, forKey: .at)
      try c.encode(of, forKey: .of)
      try c.encode(says, forKey: .says)
      try c.encodeIfPresent(prev, forKey: .prev)
      try c.encodeIfPresent(next, forKey: .next)
    }
  }

  /// The answers' column of a card's page.
  public struct AnswerView: Codable, Hashable, Sendable {
    /// "options" (the card's options answer; a decision also offers the duck, What?? and the reverse card), "read"
    /// (Got it, What??, the reverse card), "request" (Allow, Deny), "said" (what was said, and the way back),
    /// "revising" (handed back), "working" (an info still worked on), "catching", "loading", "newer".
    public var mode: String
    /// The one still tile: what was said ("Ship it", "Read", "Shredded"), "In revision", "Being worked on".
    public var label: Text?
    /// Under it: "Your note: …", or what the mode means.
    public var detail: Text?
    /// The foot: "You chose", "You handed it back".
    public var did: Text?
    public var atMs: UInt64?
    /// The way back, when there is one: "reopen" (his answer is taken back: `Action::DecideAgain`), "takeback" (his hand-back is).
    public var back: String
    /// His notes on single options, as sent.
    public var optionNotes: [NotedView]
    /// "<name> marked this answer as final: nothing follows from it.", when his answer settled the card.
    public var settled: Text?
    /// What its agent said when it closed the card ("Done by the agent").
    public var summary: String
    /// Its agent closed it after his answer: What?? asks about what it did.
    public var finished: Bool
    /// What the options say to a screen reader ("Your answer. One tap answers."), or the mode's one quiet sentence.
    public var says: Text?

    public init(mode: String = "", label: Text? = nil, detail: Text? = nil, did: Text? = nil, atMs: UInt64? = nil, back: String = "", optionNotes: [NotedView] = [], settled: Text? = nil, summary: String = "", finished: Bool = false, says: Text? = nil) {
      self.mode = mode
      self.label = label
      self.detail = detail
      self.did = did
      self.atMs = atMs
      self.back = back
      self.optionNotes = optionNotes
      self.settled = settled
      self.summary = summary
      self.finished = finished
      self.says = says
    }

    enum CodingKeys: String, CodingKey {
      case mode = "mode"
      case label = "label"
      case detail = "detail"
      case did = "did"
      case atMs = "at_ms"
      case back = "back"
      case optionNotes = "option_notes"
      case settled = "settled"
      case summary = "summary"
      case finished = "finished"
      case says = "says"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.mode = try c.decode(String.self, forKey: .mode)
      self.label = try c.decodeIfPresent(Text.self, forKey: .label)
      self.detail = try c.decodeIfPresent(Text.self, forKey: .detail)
      self.did = try c.decodeIfPresent(Text.self, forKey: .did)
      self.atMs = try c.decodeIfPresent(UInt64.self, forKey: .atMs)
      self.back = try c.decodeIfPresent(String.self, forKey: .back) ?? ""
      self.optionNotes = try c.decodeIfPresent([NotedView].self, forKey: .optionNotes) ?? []
      self.settled = try c.decodeIfPresent(Text.self, forKey: .settled)
      self.summary = try c.decodeIfPresent(String.self, forKey: .summary) ?? ""
      self.finished = try c.decodeIfPresent(Bool.self, forKey: .finished) ?? false
      self.says = try c.decodeIfPresent(Text.self, forKey: .says)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(mode, forKey: .mode)
      try c.encodeIfPresent(label, forKey: .label)
      try c.encodeIfPresent(detail, forKey: .detail)
      try c.encodeIfPresent(did, forKey: .did)
      try c.encodeIfPresent(atMs, forKey: .atMs)
      if !back.isEmpty { try c.encode(back, forKey: .back) }
      if !optionNotes.isEmpty { try c.encode(optionNotes, forKey: .optionNotes) }
      try c.encodeIfPresent(settled, forKey: .settled)
      if !summary.isEmpty { try c.encode(summary, forKey: .summary) }
      try c.encode(finished, forKey: .finished)
      try c.encodeIfPresent(says, forKey: .says)
    }
  }

  public struct NotedView: Codable, Hashable, Sendable {
    public var label: String
    public var note: Text

    public init(label: String = "", note: Text = Text()) {
      self.label = label
      self.note = note
    }

    enum CodingKeys: String, CodingKey {
      case label = "label"
      case note = "note"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.label = try c.decode(String.self, forKey: .label)
      self.note = try c.decode(Text.self, forKey: .note)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(label, forKey: .label)
      try c.encode(note, forKey: .note)
    }
  }

  /// A version of a card that stood before.
  public struct VersionView: Codable, Hashable, Sendable {
    public var n: UInt32
    public var atMs: UInt64
    public var title: String
    /// What its agent said changed with the version after it; "" without a word.
    public var note: String

    public init(n: UInt32 = 0, atMs: UInt64 = 0, title: String = "", note: String = "") {
      self.n = n
      self.atMs = atMs
      self.title = title
      self.note = note
    }

    enum CodingKeys: String, CodingKey {
      case n = "n"
      case atMs = "at_ms"
      case title = "title"
      case note = "note"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.n = try c.decode(UInt32.self, forKey: .n)
      self.atMs = try c.decode(UInt64.self, forKey: .atMs)
      self.title = try c.decode(String.self, forKey: .title)
      self.note = try c.decodeIfPresent(String.self, forKey: .note) ?? ""
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(n, forKey: .n)
      try c.encode(atMs, forKey: .atMs)
      try c.encode(title, forKey: .title)
      if !note.isEmpty { try c.encode(note, forKey: .note) }
    }
  }

  public struct NotesView: Codable, Hashable, Sendable {
    public var notes: [String]

    public init(notes: [String] = []) {
      self.notes = notes
    }

    enum CodingKeys: String, CodingKey {
      case notes = "notes"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.notes = try c.decode([String].self, forKey: .notes)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(notes, forKey: .notes)
    }
  }

  public struct SettingsView: Codable, Hashable, Sendable {
    public var account: String?
    public var devices: [DeviceRow]

    public init(account: String? = nil, devices: [DeviceRow] = []) {
      self.account = account
      self.devices = devices
    }

    enum CodingKeys: String, CodingKey {
      case account = "account"
      case devices = "devices"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.account = try c.decodeIfPresent(String.self, forKey: .account)
      self.devices = try c.decode([DeviceRow].self, forKey: .devices)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(account, forKey: .account)
      try c.encode(devices, forKey: .devices)
    }
  }

  public struct DeviceRow: Codable, Hashable, Sendable {
    public var id: String
    public var name: String
    public var thisDevice: Bool

    public init(id: String = "", name: String = "", thisDevice: Bool = false) {
      self.id = id
      self.name = name
      self.thisDevice = thisDevice
    }

    enum CodingKeys: String, CodingKey {
      case id = "id"
      case name = "name"
      case thisDevice = "this_device"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.id = try c.decode(String.self, forKey: .id)
      self.name = try c.decode(String.self, forKey: .name)
      self.thisDevice = try c.decode(Bool.self, forKey: .thisDevice)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(id, forKey: .id)
      try c.encode(name, forKey: .name)
      try c.encode(thisDevice, forKey: .thisDevice)
    }
  }

  /// The account's flows as a screen shows them. `step` names the screen; the fields it needs are filled.
  /// `Debug` leaves out the invite link and the Kit words: they open the account.
  public struct AccountView: Codable, Hashable, Sendable {
    /// "none", "sign_in", "found", "join", "compare_emoji", "keep_kit", "invite", "recover", "working".
    public var step: String
    /// The invite in progress (for `ConfirmEmoji`).
    public var inviteId: String?
    /// An invite link to show as text or QR code.
    public var link: String?
    /// The six emoji to compare, with their words.
    public var emoji: [EmojiView]
    /// The Emergency Kit words to keep, shown once.
    public var kit: [String]
    /// Self-join asks waiting for "Allow" or "Deny" (for `AnswerAsk`).
    public var asks: [AskView]
    public var problem: Text?

    public init(step: String = "", inviteId: String? = nil, link: String? = nil, emoji: [EmojiView] = [], kit: [String] = [], asks: [AskView] = [], problem: Text? = nil) {
      self.step = step
      self.inviteId = inviteId
      self.link = link
      self.emoji = emoji
      self.kit = kit
      self.asks = asks
      self.problem = problem
    }

    enum CodingKeys: String, CodingKey {
      case step = "step"
      case inviteId = "invite_id"
      case link = "link"
      case emoji = "emoji"
      case kit = "kit"
      case asks = "asks"
      case problem = "problem"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.step = try c.decode(String.self, forKey: .step)
      self.inviteId = try c.decodeIfPresent(String.self, forKey: .inviteId)
      self.link = try c.decodeIfPresent(String.self, forKey: .link)
      self.emoji = try c.decodeIfPresent([EmojiView].self, forKey: .emoji) ?? []
      self.kit = try c.decodeIfPresent([String].self, forKey: .kit) ?? []
      self.asks = try c.decodeIfPresent([AskView].self, forKey: .asks) ?? []
      self.problem = try c.decodeIfPresent(Text.self, forKey: .problem)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(step, forKey: .step)
      try c.encodeIfPresent(inviteId, forKey: .inviteId)
      try c.encodeIfPresent(link, forKey: .link)
      if !emoji.isEmpty { try c.encode(emoji, forKey: .emoji) }
      if !kit.isEmpty { try c.encode(kit, forKey: .kit) }
      if !asks.isEmpty { try c.encode(asks, forKey: .asks) }
      try c.encodeIfPresent(problem, forKey: .problem)
    }
  }

  public struct EmojiView: Codable, Hashable, Sendable {
    public var emoji: String
    public var word: String

    public init(emoji: String = "", word: String = "") {
      self.emoji = emoji
      self.word = word
    }

    enum CodingKeys: String, CodingKey {
      case emoji = "emoji"
      case word = "word"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.emoji = try c.decode(String.self, forKey: .emoji)
      self.word = try c.decode(String.self, forKey: .word)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(emoji, forKey: .emoji)
      try c.encode(word, forKey: .word)
    }
  }

  public struct AskView: Codable, Hashable, Sendable {
    public var nonce: String
    /// What the vouching agent says it is: account, name, host (its word, shown as such).
    public var says: Text

    public init(nonce: String = "", says: Text = Text()) {
      self.nonce = nonce
      self.says = says
    }

    enum CodingKeys: String, CodingKey {
      case nonce = "nonce"
      case says = "says"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.nonce = try c.decode(String.self, forKey: .nonce)
      self.says = try c.decode(Text.self, forKey: .says)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(nonce, forKey: .nonce)
      try c.encode(says, forKey: .says)
    }
  }

  public struct FileView: Codable, Hashable, Sendable {
    public var name: String
    public var mediaType: String
    public var size: UInt64
    /// "uploading", "ready", "failed", "missing".
    public var state: String
    /// The attachment's id once the hub has it (an upload's local id maps to it here).
    public var attachmentId: String?
    /// 0..=100 while it moves.
    public var percent: UInt8?
    /// Its bytes were handed over (`Effect::FileReady`).
    public var loaded: Bool

    public init(name: String = "", mediaType: String = "", size: UInt64 = 0, state: String = "", attachmentId: String? = nil, percent: UInt8? = nil, loaded: Bool = false) {
      self.name = name
      self.mediaType = mediaType
      self.size = size
      self.state = state
      self.attachmentId = attachmentId
      self.percent = percent
      self.loaded = loaded
    }

    enum CodingKeys: String, CodingKey {
      case name = "name"
      case mediaType = "media_type"
      case size = "size"
      case state = "state"
      case attachmentId = "attachment_id"
      case percent = "percent"
      case loaded = "loaded"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.name = try c.decode(String.self, forKey: .name)
      self.mediaType = try c.decode(String.self, forKey: .mediaType)
      self.size = try c.decode(UInt64.self, forKey: .size)
      self.state = try c.decode(String.self, forKey: .state)
      self.attachmentId = try c.decodeIfPresent(String.self, forKey: .attachmentId)
      self.percent = try c.decodeIfPresent(UInt8.self, forKey: .percent)
      self.loaded = try c.decode(Bool.self, forKey: .loaded)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(name, forKey: .name)
      try c.encode(mediaType, forKey: .mediaType)
      try c.encode(size, forKey: .size)
      try c.encode(state, forKey: .state)
      try c.encodeIfPresent(attachmentId, forKey: .attachmentId)
      try c.encodeIfPresent(percent, forKey: .percent)
      try c.encode(loaded, forKey: .loaded)
    }
  }

  public struct Toast: Codable, Hashable, Sendable {
    public var id: String
    public var text: Text
    /// An action the toast offers ("Undo", "Retry"), sent back as that action.
    public var action: CoreJSON?

    public init(id: String, text: Text, action: CoreJSON?) {
      self.id = id
      self.text = text
      self.action = action
    }

    enum CodingKeys: String, CodingKey {
      case id = "id"
      case text = "text"
      case action = "action"
    }

    public init(from decoder: any Decoder) throws {
      let c = try decoder.container(keyedBy: CodingKeys.self)
      self.id = try c.decode(String.self, forKey: .id)
      self.text = try c.decode(Text.self, forKey: .text)
      self.action = try c.decodeIfPresent(CoreJSON.self, forKey: .action)
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.container(keyedBy: CodingKeys.self)
      try c.encode(id, forKey: .id)
      try c.encode(text, forKey: .text)
      try c.encodeIfPresent(action, forKey: .action)
    }
  }

  /// A value the views carry as plain JSON, to be sent back as it came (a toast's action).
  public enum CoreJSON: Codable, Hashable, Sendable {
    case null
    case bool(Bool)
    case number(Double)
    case string(String)
    case array([CoreJSON])
    case object([String: CoreJSON])

    public init(from decoder: any Decoder) throws {
      let c = try decoder.singleValueContainer()
      if c.decodeNil() { self = .null }
      else if let b = try? c.decode(Bool.self) { self = .bool(b) }
      else if let n = try? c.decode(Double.self) { self = .number(n) }
      else if let s = try? c.decode(String.self) { self = .string(s) }
      else if let a = try? c.decode([CoreJSON].self) { self = .array(a) }
      else { self = .object(try c.decode([String: CoreJSON].self)) }
    }

    public func encode(to encoder: any Encoder) throws {
      var c = encoder.singleValueContainer()
      switch self {
      case .null: try c.encodeNil()
      case .bool(let b): try c.encode(b)
      case .number(let n): try c.encode(n)
      case .string(let s): try c.encode(s)
      case .array(let a): try c.encode(a)
      case .object(let o): try c.encode(o)
      }
    }
  }
}
