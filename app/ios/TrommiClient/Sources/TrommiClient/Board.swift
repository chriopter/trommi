// Board.swift: the board as the views read it. The model and its rules are the client core's (client/src/model, the
// binding's `ClientModel`, the same for the web app); this is its copy in Swift classes, filled from the core's patch
// once per batch (BoardMirror.swift). The names follow the model's (snake_case fields become camelCase).
import Foundation
import TrommiCoreRust

public let ALERTS_MAX = 200
public let LAMPORT_MAX = 1 << 48
public let LAMPORT_STEP = 1 << 24
public let ASLEEP_MS: UInt64 = 10 * 60_000

public let OBJECT_STATE_NAME = [1: "open", 2: "answered", 3: "closed"]
public let OBJECT_STATE_CODE = ["open": 1, "answered": 2, "closed": 3]
public let URGENCY_NAME = [0: "low", 1: "normal", 2: "high", 3: "critical"]
public let URGENCY_CODE = ["low": 0, "normal": 1, "high": 2, "critical": 3]
public let TIMELINE_KIND_NAME = [1: "chat", 2: "scribble"]
public let CARD_CONTENT_FIELDS = ["card_type", "title", "teaser", "body", "options", "sections", "html", "allows_multiple", "recommended", "urgency_reason",
                                  "attachments", "change_note", "close_summary", "withdraw_reason", "merged_into_object_id", "merged_from_object_ids", "working"]

// ---- the record: one verified envelope, as the reducer sees it ----------------------------------------

public struct Causal: Equatable, Codable {
  public var senderDeviceId: String
  public var senderSequence: UInt64
  public var sentAt: UInt64
  public var lamport: Int
  public var noBody = false
}

// ---- the parts of the board ------------------------------------------------------------------------------

public struct AgentLink: Equatable {
  public var hears: String
  public var attached: Bool
  public var lastCallAt: UInt64?
  public var working: Bool
  public var since: UInt64?
  public var cutSince: UInt64?
  public var exitReason: String?
  public var exitClaude: String?
}
func stampOf(_ v: JV) -> UInt64? { if let i = v.int, i > 0 { return UInt64(i) }; return nil }
public func cleanLink(_ l: JV) -> AgentLink? {
  guard l.object != nil, let h = l["hears"].string, ["live", "oncall"].contains(h) else { return nil }
  let ex = l["exit"]
  let claude = ex.object != nil ? (["alive", "gone", "checking"].contains(ex["claude"].string ?? "") ? ex["claude"].string! : "gone") : nil
  return AgentLink(hears: h, attached: l["attached"] != .bool(false), lastCallAt: stampOf(l["last_call_at"]), working: l["working"] == .bool(true), since: stampOf(l["since"]),
              cutSince: stampOf(l["cut_since"]), exitReason: ex.object != nil ? String((ex["reason"].string ?? "").prefix(40)) : nil, exitClaude: claude)
}

public final class RoomMember {
  public let deviceId: String
  public var deviceRole: String
  public var fingerprint: String
  public var deviceName = ""
  public var platform: String?
  public var folder: String?
  public var host: String?
  public var isActive = true
  public var addedEntryNumber = 0
  public var removedEntryNumber: Int?
  /** When it was added: the time of the Commit that added it (its committer's clock, ms), as this device processed
   *  it; nil when this device did not see that Commit (a device older than this one). */
  public var addedAt: UInt64?
  public var isMe = false
  public var isOnline = false
  public var offlineSince: UInt64?
  public var link: AgentLink?
  public var agentSessionId: String?
  init(deviceId: String, role: String) {
    self.deviceId = deviceId; deviceRole = role
    let h = Array(deviceId.prefix(16))
    fingerprint = stride(from: 0, to: h.count, by: 4).map { String(h[$0..<min($0 + 4, h.count)]) }.joined(separator: " ")
  }
}

public struct StatusLine: Equatable {
  public var id: String
  public var label: String
  public var state: String?
  public var detail: String?
  public var objectId: String?
  public var envelopeNumber: Int
  public var updatedAt: UInt64
}

public struct RegisterValue {
  public var value: JV
  public var envelopeNumber: Int
  public var senderSequence: UInt64?
  public var byDeviceId: String?
  public var pending: Bool = false
  public var causal: Causal?
}

public final class Session {
  public let sessionId: String
  public var agentDeviceIds: [String] = []
  public var everAgentIds: [String] = []
  public var epochAgentIds: [Int: [String]] = [:]
  public var agentDeviceId: String?
  public var agentSessionId: String?
  public var deviceName = ""
  public var isActive = true
  public var isOnline = false
  public var offlineSince: UInt64?
  public var link: AgentLink?
  public var heardUpTo: Int?
  public var heardAt: UInt64?
  public var sessionKeyEpoch = 0
  public var withHistory = false
  public var createdByAgent = false
  public var creatorDeviceId: String?
  /** A helper session's main session, as its group names it. */
  public var parentSessionId: String?
  public var profile: JV = .null
  public var statusLines: [StatusLine] = []
  public var agentAlerts: [(key: String, value: JV, envelopeNumber: Int)] = []
  public var registers: [String: RegisterValue] = [:]
  public var settings: JV = .null
  public var cardIds: [String] = []
  public var openCardIds: [String] = []
  public var timelineKey: String
  public var lastActivityAt: UInt64 = 0
  init(_ id: String) { sessionId = id; timelineKey = timelineKeyOf("chat", "session/\(id)") }
}

public struct Option: Equatable {
  public var key: String
  public var label: String
  public var detail: String?
  public var final: Bool
  public var raw: JV
  public init(_ v: JV) {
    raw = v
    if let s = v.string { key = s; label = s; detail = nil; final = false; return }
    key = v["key"].string ?? ""
    label = v["label"].string ?? key
    detail = v["detail"].string
    final = v["final"] == .bool(true)
  }
}

public struct Answer {
  public var answerAction: String
  public var choices: [String]
  public var note: String?
  public var optionNotes: [String: String]
  public var attachments: [JV]
  public var marks: [JV]
  public var trusted: Bool
  public var boundObjectVersion: Int
  public var envelopeNumber: Int?
  public var envelopeHash: String?
  public var byDeviceId: String
  public var answeredAt: UInt64
  public var takenBackAt: Int?
  public var takenBackSentAt: UInt64?
  public var pending = false
}

public struct CardVersion {
  public var objectVersion: Int
  public var versionHash: String
  public var previousVersionHash: String?
  public var envelopeNumber: Int
  public var sentAt: UInt64
  public var objectState: String
  public var urgency: String
  /** Its body; of a lean card's version (`owner`), read from the client core when first asked. */
  public var content: JV? {
    get { if let c = ownContent, !c.isNull { return c }; return owner?.versionContent(objectVersion) ?? ownContent }
    set { ownContent = newValue }
  }
  var ownContent: JV?
  weak var owner: Card?
  public init(objectVersion: Int, versionHash: String, previousVersionHash: String?, envelopeNumber: Int, sentAt: UInt64, objectState: String, urgency: String, content: JV?, owner: Card? = nil) {
    self.objectVersion = objectVersion; self.versionHash = versionHash; self.previousVersionHash = previousVersionHash; self.envelopeNumber = envelopeNumber
    self.sentAt = sentAt; self.objectState = objectState; self.urgency = urgency; self.ownContent = content; self.owner = owner
  }
}

/** One entry of a card's history (client/src/model/types.rs `Revision`): a version a human reads as the card, the
 *  oldest first, with what its agent said changed. The client core keeps every one: a newer never replaces them. */
public struct Revision: Equatable {
  public var objectVersion: Int
  public var envelopeNumber: Int
  public var sentAt: UInt64
  public var note: String
  public var title: String
  public init(objectVersion: Int, envelopeNumber: Int, sentAt: UInt64, note: String, title: String) {
    self.objectVersion = objectVersion; self.envelopeNumber = envelopeNumber; self.sentAt = sentAt; self.note = note; self.title = title
  }
}

public struct InRevision: Equatable { public var by: String; public var envelopeNumber: Int }

public final class Card {
  public let objectId: String
  public var agentDeviceId: String
  public var objectState = "open"
  public var urgency = "normal"
  /** The card's content fields (CARD_CONTENT_FIELDS), as the agent sent them: unknown ones are kept too. Of a lean card
   *  (the client core's patch leaves lists and objects out, `lazy`) the rest is read from the core when first asked. */
  public var fields: [String: JV] {
    get { if !lazyFields.isEmpty { fill() }; return ownFields }
    set { ownFields = newValue }
  }
  var ownFields: [String: JV] = [:]
  /** The fields a lean card left out; the versions' bodies and the answers before the one in force are left out too. */
  var lazyFields: Set<String> = []
  var lean = false
  /** Where a lean card reads the rest: the client core's whole card (`card_json`). */
  var whole: (() -> JV?)?
  /** One field, read from the core only when the lean card left it out. */
  func field(_ name: String) -> JV? { lazyFields.contains(name) ? fields[name] : ownFields[name] }
  /** A lean card made whole: its fields, versions with their bodies and answers, from the client core. */
  func fill() {
    guard lean, let v = whole?() else { lazyFields = []; lean = false; return }
    lean = false; lazyFields = []
    for k in CARD_CONTENT_FIELDS { let x = v[k]; if !x.isNull { ownFields[k] = x } else { ownFields.removeValue(forKey: k) } }
    ownVersions = (v["versions"].array ?? []).map {
      CardVersion(objectVersion: $0["object_version"].int ?? 0, versionHash: $0["version_hash"].string ?? "", previousVersionHash: $0["previous_version_hash"].string,
                  envelopeNumber: $0["envelope_number"].int ?? 0, sentAt: $0["sent_at"].double.map { UInt64(max(0, $0)) } ?? 0, objectState: $0["object_state"].string ?? "",
                  urgency: $0["urgency"].string ?? "", content: $0["content"].isNull ? nil : $0["content"])
    }
    ownAnswers = (v["answers"].array ?? []).map(answerOf)
  }
  /** A version's body (a lean card's are read from the core when first asked). */
  func versionContent(_ objectVersion: Int) -> JV? {
    if lean { fill() }
    return ownVersions.first { $0.objectVersion == objectVersion }?.ownContent
  }
  public var objectVersion = 0
  public var versionHash: String?
  public var envelopeNumber: Int
  public var firstEnvelopeNumber: Int
  public var createdAt: UInt64
  public var sessionId: String?
  public var updatedAt: UInt64
  /** Its versions; of a lean card their headers, each body read when first asked. */
  public var versions: [CardVersion] {
    get { ownVersions }
    set { ownVersions = newValue }
  }
  var ownVersions: [CardVersion] = []
  /** Its history from the client core, the oldest first (lean cards carry it whole). */
  public var history: [Revision] = []
  public var answer: Answer?
  /** Every answer it was given (the history); of a lean card read from the core when first asked. */
  public var answers: [Answer] {
    get { if lean { fill() }; return ownAnswers }
    set { ownAnswers = newValue }
  }
  var ownAnswers: [Answer] = []
  public var closedHow: String?
  public var inRevision: InRevision?
  public var timelineKey: String
  public var contentState = "ok"
  public var refusedHead: Int?
  /** A card of a newer Trommi (an unknown card_type, a newer schema): a placeholder, never answered from here. */
  public var unsupported = false
  /** Settled at the hub after this device's cursor, not taken yet: off the Desk (the client core's Desk). */
  public var catchingUp = false
  /** His "Later" as the client core holds it (client/src/desk.rs `snooze_holds`: until its time, only for the version he
   *  put off): until when, and when he put it off; `unsnoozed`: an open card whose snooze ended. */
  public var snoozedUntil: UInt64?
  public var snoozedAt: UInt64?
  public var unsnoozed: UInt64?
  /** A card of the copy (BoardMirror.swift `cardOf`): every field is set from the core's record. */
  init(snapshot id: String, agent: String) {
    objectId = id; agentDeviceId = agent; envelopeNumber = 0; firstEnvelopeNumber = 0; createdAt = 0; sessionId = nil; updatedAt = 0
    timelineKey = timelineKeyOf("chat", "card/\(id)")
  }
  public var cardType: String { field("card_type")?.string ?? "decision" }
  public var title: String { field("title")?.string ?? "" }
  public var teaser: String? { field("teaser")?.string }
  public var body: String? { field("body")?.string }
  public var html: String? { field("html")?.string }
  public var options: [Option] { (field("options")?.array ?? []).map(Option.init) }
  public var sections: [JV]? { field("sections")?.array }
  public var allowsMultiple: Bool { field("allows_multiple") == .bool(true) }
  public var recommended: [String] {
    guard let r = field("recommended") else { return [] }
    if let s = r.string { return [s] }
    return (r.array ?? []).compactMap { $0.string }
  }
  public var urgencyReason: String? { field("urgency_reason")?.string }
  public var attachments: [JV] { field("attachments")?.array ?? [] }
  public var changeNote: String? { field("change_note")?.string }
  public var closeSummary: String? { field("close_summary")?.string }
  public var withdrawReason: String? { field("withdraw_reason")?.string }
  public var mergedIntoObjectId: String? { field("merged_into_object_id")?.string }
  public var mergedFromObjectIds: [String] { (field("merged_from_object_ids")?.array ?? []).compactMap { $0.string } }
  /** An info its agent is still working on (spec 9.1.3): off the Desk, under Off your mind, until a version without it. */
  public var working: Bool { field("working") == .bool(true) }
}

public final class Permission {
  public let objectId: String
  public var agentDeviceId: String
  public var sessionId: String?
  public var toolName: String
  public var description: String
  public var inputPreview: String
  public var expiresAt: UInt64
  public var versionHash: String
  public var envelopeNumber: Int
  public var sentAt: UInt64
  /** pending | allowed | denied | withdrawn | expired (set by the projection once `endsAt` has passed). */
  public var permissionState = "pending"
  public var verdict: (allow: Bool, byDeviceId: String, envelopeNumber: Int)?
  public var withdrawReason: String?
  /** `ok` once its body was read here (its tool, words, deadline); else what is known of it. */
  public var contentState = "ok"
  /** As a card's: settled at the hub after this device's cursor, not taken yet. */
  public var catchingUp = false
  init(objectId: String, agentDeviceId: String, sessionId: String?, toolName: String, description: String, inputPreview: String, expiresAt: UInt64, versionHash: String, envelopeNumber: Int, sentAt: UInt64) {
    self.objectId = objectId; self.agentDeviceId = agentDeviceId; self.sessionId = sessionId; self.toolName = toolName; self.description = description
    self.inputPreview = inputPreview; self.expiresAt = expiresAt; self.versionHash = versionHash; self.envelopeNumber = envelopeNumber; self.sentAt = sentAt
  }
  /** The longest a permission request waits for its verdict: a connector's hook waits at most an hour
   *  (TROMMI_PERMISSION_MS), and two minutes more for the clocks (web model.ts REQUEST_LIFE_MAX_MS). */
  public static let lifeMaxMs: UInt64 = 62 * 60_000
  /** When the request runs out: its bound deadline; one whose body was not read here (no bind, so no deadline known)
   *  past the longest wait a connector gives one, whatever it said (the hub cannot tell, so its Desk lists such a
   *  request as open for ever). The same rule as the web's `project()`. */
  public var endsAt: UInt64 { expiresAt > 0 ? expiresAt : sentAt + Permission.lifeMaxMs }
}

public struct Note {
  public var objectId: String
  public var byDeviceId: String
  public var text: String
  /** Every field of the note's body (place, session, to, …: the app defines them). */
  public var extra: [String: JV]
  public var objectVersion: Int
  public var versionHash: String
  public var versionHashes: [String]
  public var causal: Causal?
  public var envelopeNumber: Int
  public var objectState: String
  public var pending = false
  public var localId: String?
  public var base: Box<Note>? = nil
}
/** A reference box (a note keeps the confirmed version under its own pending echo). */
public final class Box<T> { public var value: T; public init(_ v: T) { value = v } }

public struct PublishedObject {
  public var objectId: String
  public var agentDeviceId: String
  public var sessionId: String?
  public var attachments: [JV]
  public var title: String
  public var note: String?
  public var releasedUntil: JV
  public var objectVersion: Int
  public var versionHash: String
  public var envelopeNumber: Int
  public var objectState: String
  public var sentAt: UInt64
}

public struct TimelineItem {
  public var envelopeNumber: Int?
  public var localId: String?
  public var pending: Bool
  public var failed = false
  public var envelopeHash: String?
  public var senderDeviceId: String
  public var senderSequence: UInt64?
  public var recipientDeviceId: String?
  public var sentAt: UInt64
  /** loaded | header | pruned | undecryptable | newer_schema */
  public var itemState: String
  public var contentType: String?
  public var content: JV?
}

public final class Timeline {
  public let key: String
  public let kind: String
  public let timelineId: String
  public let objectId: String
  public var itemCount = 0
  public var newestEnvelopeNumber = 0
  public var newestHumanEnvelopeNumber = 0
  public var newestAgentEnvelopeNumber = 0
  /** The window: the items in memory, by envelope number (or a negative number for an own echo still in flight). */
  public var items: [Int: TimelineItem] = [:] { didSet { revision &+= 1 } }
  /** Own echoes still on their way, by local id. */
  public var echoes: [String: TimelineItem] = [:] { didSet { revision &+= 1 } }
  /** Bumped by every change of the items or the echoes, wherever it is made (a didSet changes them in place): what a
   *  conversation built from them is cached by (DeskModel.itemsOf). */
  public private(set) var revision = 0
  /** The envelope numbers of the items the hub holds (header-only items included), oldest first. */
  public var numbers: [Int] = []
  public var loadedDownTo = Int.max
  public var hasMore = false
  // (this app keeps every item header of a timeline: the window pages bodies in from GET threads)
  public var windowOpen = true
  init(_ key: String) {
    self.key = key
    let p = parseTimelineKey(key)
    kind = p.kind; timelineId = p.timelineId; objectId = p.scopeId
  }
  /** The items of the window, oldest first, the own echoes last. */
  public var ordered: [TimelineItem] { items.keys.sorted().compactMap { items[$0] } + echoes.values.sorted { $0.sentAt < $1.sentAt } }
}

public struct BoardAlert: Equatable {
  public var alertId: String
  public var code: String
  public var message: String
  public var envelopeNumber: Int?
  public var senderDeviceId: String?
  public var at: UInt64
  public var source: String
}

public struct HumanRegisters {
  public var drafts: [String: JV] = [:]
  public var snoozes: [String: JV] = [:]
  public var ducks: [String: JV] = [:]
  /** Cards he starred (`star/<card>` = `{ at }`): his own mark, on all his devices. */
  public var stars: [String: JV] = [:]
  public var crown: JV = .null
  public var desks: [String: JV] = [:]
  public var sessionSettings: [String: JV] = [:]
  public var scribbleSnapshots: [String: JV] = [:]
  public var raw: [String: RegisterValue] = [:]
}

/** What a batch touched (the views refresh only that). */
public struct Change {
  public var cards = Set<String>(), sessions = Set<String>(), permissions = Set<String>(), notes = Set<String>(), published = Set<String>()
  public var timelines = Set<String>(), registers = Set<String>(), invites = Set<String>()
  public var members = false, alerts = false, outbox = false, stack = false, room = false
  /** The session list was made again (a link, a red hand, a quiet hint changed by the clock). */
  public var status = false
  public init() {}
  public var isEmpty: Bool { cards.isEmpty && sessions.isEmpty && permissions.isEmpty && notes.isEmpty && published.isEmpty && timelines.isEmpty && registers.isEmpty && invites.isEmpty && !members && !alerts && !outbox && !stack && !room && !status }
  public mutating func merge(_ o: Change) {
    cards.formUnion(o.cards); sessions.formUnion(o.sessions); permissions.formUnion(o.permissions); notes.formUnion(o.notes); published.formUnion(o.published)
    timelines.formUnion(o.timelines); registers.formUnion(o.registers); invites.formUnion(o.invites)
    members = members || o.members; alerts = alerts || o.alerts; outbox = outbox || o.outbox; stack = stack || o.stack; room = room || o.room; status = status || o.status
  }
}

public func timelineKeyOf(_ kind: String, _ id: String) -> String { "\(kind):\(id)" }
public func parseTimelineKey(_ key: String) -> (kind: String, timelineId: String, scope: String, scopeId: String) {
  let at = key.firstIndex(of: ":") ?? key.endIndex
  let kind = String(key[..<at]), id = at < key.endIndex ? String(key[key.index(after: at)...]) : ""
  let slash = id.firstIndex(of: "/") ?? id.endIndex
  return (kind, id, String(id[..<slash]), slash < id.endIndex ? String(id[id.index(after: slash)...]) : "")
}

/** R2: the one order of writes to a register or note: (lamport, sender_device_id, sender_sequence). */
public func compareWrites(_ x: Causal, _ y: Causal) -> Int {
  if x.lamport != y.lamport { return x.lamport < y.lamport ? -1 : 1 }
  if x.senderDeviceId != y.senderDeviceId { return x.senderDeviceId < y.senderDeviceId ? -1 : 1 }
  return x.senderSequence == y.senderSequence ? 0 : (x.senderSequence < y.senderSequence ? -1 : 1)
}
public func causallyAfter(_ x: Causal?, _ y: Causal?) -> Bool {
  guard let y = y else { return true }
  guard let x = x else { return false }
  return compareWrites(x, y) > 0
}
public func lamportOf(_ c: JV?) -> Int { if let l = c?["lamport"].int, l > 0, l <= LAMPORT_MAX { return l }; return 0 }
public func lamportAccepted(_ lamport: Int, _ seen: Int) -> Bool { lamport > 0 && lamport <= seen + LAMPORT_STEP }

/** Every choice is an option the agent marked final (and there is at least one). */
public func choicesFinal(_ card: Card?, _ choices: [String]) -> Bool {
  let final = Set((card?.options ?? []).filter { $0.final }.map { $0.key })
  return !choices.isEmpty && choices.allSatisfy { final.contains($0) }
}

// ---- the board ---------------------------------------------------------------------------------------------

/** The room's recovery key as Settings → Security shows it. */
public struct RecoveryKey: Equatable {
  public var fingerprint: String
  public var since: UInt64?
  public init(fingerprint: String, since: UInt64?) { self.fingerprint = fingerprint; self.since = since }
}
/** A key's fingerprint as the app shows it: the first 8 bytes of its hex, in groups of four (the web's keyFingerprint). */
public func keyFingerprint(_ key: Bytes) -> String {
  let h = Array(key.prefix(8).map { String(format: "%02x", $0) }.joined())
  return stride(from: 0, to: h.count, by: 4).map { String(h[$0..<min($0 + 4, h.count)]) }.joined(separator: " ")
}

/**
 * The board the views read. It is a copy: the model is the client core's (client/src/model, `ClientModel`), which
 * takes what the protocol core accepted and hands back, once per batch, what changed (`project`, BoardMirror.swift).
 * Nothing here decides a rule of the model.
 */
public final class Board {
  public var roomId: String?
  public var hubURL: String?
  public var myDeviceId: String? { didSet { if let me = myDeviceId, me != oldValue { try? engine.setMe(myDeviceId: me) } } }
  public var myRole: String? = "human"
  public var keyEpoch = 0
  /** A preview built from the hub's Desk alone (Room.deskPreview): its records read out of order. Never set on the room's own board. */
  public var preview = false
  public var lastEnvelopeNumber = 0
  public var connection = "offline"
  /** The room's recovery key (8.1): the fingerprint of its HPKE public key, and the time of the one room Commit that
   *  put it in force (nil when this device did not see that Commit alone). */
  public var recoveryKey: TrommiClient.RecoveryKey?
  public var members: [String: RoomMember] = [:]
  public var sessions: [String: Session] = [:]
  public var cards: [String: Card] = [:]
  public var permissions: [String: Permission] = [:]
  public var notes: [String: Note] = [:]
  public var published: [String: PublishedObject] = [:]
  public var timelines: [String: Timeline] = [:]
  public var human = HumanRegisters()
  public var alerts: [BoardAlert] = []
  public var stack: [String] = []
  /** When the core's model changes by the clock alone (a snooze ends, a request runs out, a session's status): the
   *  app projects again then (Room `wakeLater`); nil: never. */
  public internal(set) var wakeAt: UInt64?
  public var openPermissionIds: [String] = []
  /** The session list every device of the person shows (client/src/sessions.rs, one entry per session, in order):
   *  which sessions, their names, online or gone, archived, their parents. DeskModel draws it as it comes. */
  public internal(set) var sessionList: [JV] = []
  /** Open here, settled at the hub (or held while the app waits for the hub's first list): off the Desk. */
  public internal(set) var held: Set<String> = []
  /** What a newer client wrote that this version cannot show: how many, what, the newest. */
  public var newerCount = 0
  public var newerWhat: [String] = []
  public var newerEnvelope = 0
  /** Each device's own register `device/<id>` (its name before it is a member here). */
  var deviceRegisters: [String: JV] = [:]
  /** The client core's model behind this copy. */
  private(set) var engine = ClientModel(myDeviceId: "")
  /** What the views were not told yet of what came straight into the copy (none: everything goes through the core). */

  public init() {
    try? engine.setWindowsOpen(open: true)
    try? engine.setLeanCards(lean: true)
  }

  /**
   * A copy of what this board shows, for the views while the room builds its board anew (Room.readBackView): the
   * fields the views read, with the stack and what is held back as they stand. The copy has an engine of its own
   * that is never fed: nothing of it changes until it is dropped.
   */
  public func heldCopy() -> Board {
    let b = Board()
    b.roomId = roomId; b.hubURL = hubURL; b.myDeviceId = myDeviceId; b.myRole = myRole; b.keyEpoch = keyEpoch
    b.lastEnvelopeNumber = lastEnvelopeNumber; b.connection = connection; b.recoveryKey = recoveryKey
    b.members = members; b.sessions = sessions; b.cards = cards; b.permissions = permissions; b.notes = notes
    b.published = published; b.timelines = timelines; b.human = human; b.alerts = alerts
    b.stack = stack; b.openPermissionIds = openPermissionIds; b.held = held; b.sessionList = sessionList
    b.newerCount = newerCount; b.newerWhat = newerWhat; b.newerEnvelope = newerEnvelope; b.deviceRegisters = deviceRegisters
    b.deskWaiting = deskWaiting
    return b
  }

  /** Open here, but settled at the hub: off its Desk. */
  public func heldBack(_ id: String, open: Bool) -> Bool { open && held.contains(id) }
  /**
   * A launch from the cache waits for the hub's Desk of this run (Room.deskFirst): meanwhile no open card or request
   * stands (the Desk says "Catching up…"), as the cache may hold ones settled elsewhere since.
   */
  public var deskWaiting = false { didSet { if deskWaiting != oldValue { try? engine.setDeskHold(hold: deskWaiting) } } }
  /** The hub's Desk as read at change `at`: what it lists as open, by id (hex) with the change of its version; nil forgets it. */
  public func setDeskListing(_ open: [String: Int]?, at: Int) {
    if let open { try? engine.setDeskListing(at: UInt64(max(0, at)), listed: open.map { DeskListed(objectId: $0.key, versionChange: UInt64(max(0, $0.value))) }) }
    else { try? engine.forgetDeskListing() }
  }

  /** Nothing of this device in flight on the board: no own echo, no pending answer, note or register (a snapshot may be taken). */
  public var settled: Bool {
    timelines.values.allSatisfy { $0.echoes.isEmpty && !$0.items.values.contains { $0.pending && $0.localId != nil } }
      && !cards.values.contains { $0.answer?.pending == true }
      && !notes.values.contains { $0.pending } && !human.raw.values.contains { $0.pending }
  }

  /** Forget everything the records built (a store that could not be read to its end, a read back from the start). */
  public func reset() {
    // (who is connected is not in the history: presence is not replayed. What this board knows of it goes to the new
    // model, which holds each report until the groups name its device again)
    let presence: [JV] = members.values.filter { $0.isOnline || $0.link != nil }.map { m in
      var o: [String: JV] = ["device_id": .str(m.deviceId), "is_online": .bool(m.isOnline)]
      if let t = m.offlineSince { o["offline_since"] = .num(Double(t)) }
      if let l = m.link {
        var lj: [String: JV] = ["hears": .str(l.hears), "attached": .bool(l.attached), "working": .bool(l.working)]
        if let t = l.lastCallAt { lj["last_call_at"] = .num(Double(t)) }
        if let t = l.since { lj["since"] = .num(Double(t)) }
        if let t = l.cutSince { lj["cut_since"] = .num(Double(t)) }
        if l.exitReason != nil || l.exitClaude != nil { lj["exit"] = .obj(["reason": .str(l.exitReason ?? ""), "claude": .str(l.exitClaude ?? "gone")]) }
        o["link"] = .obj(lj)
      }
      return .obj(o)
    }
    lastEnvelopeNumber = 0
    sessions = [:]; cards = [:]; permissions = [:]; notes = [:]; published = [:]; timelines = [:]; members = [:]
    human = HumanRegisters(); alerts = []; stack = []; openPermissionIds = []; held = []; sessionList = []
    newerCount = 0; newerWhat = []; newerEnvelope = 0; deviceRegisters = [:]
    engine = ClientModel(myDeviceId: myDeviceId ?? "")
    if !presence.isEmpty { try? engine.applyPresence(reports: String(decoding: JV.arr(presence).encoded(), as: UTF8.self), nowMs: nowMs()) }
    try? engine.setWindowsOpen(open: true)
    try? engine.setLeanCards(lean: true)
    if deskWaiting { try? engine.setDeskHold(hold: true) }
  }

  /**
   * A cheap fingerprint of everything a screen shows: equal before and after a batch means the batch changed nothing
   * visible (a refresh with no news), and the app skips the render.
   */
  public var fingerprint: Int {
    var h = Hasher()
    h.combine(lastEnvelopeNumber); h.combine(newerCount); h.combine(alerts.count); h.combine(connection); h.combine(keyEpoch); h.combine(recoveryKey?.fingerprint); h.combine(recoveryKey?.since)
    for m in members.values.sorted(by: { $0.deviceId < $1.deviceId }) {
      h.combine(m.deviceId); h.combine(m.isActive); h.combine(m.isOnline); h.combine(m.deviceName); h.combine(m.addedAt); h.combine(m.link?.hears); h.combine(m.link?.working); h.combine(m.link?.cutSince)
    }
    for s in sessions.values.sorted(by: { $0.sessionId < $1.sessionId }) {
      h.combine(s.sessionId); h.combine(s.isOnline); h.combine(s.isActive); h.combine(s.link?.hears); h.combine(s.link?.working); h.combine(s.link?.attached)
      h.combine(s.statusLines.count); h.combine(s.statusLines.map { $0.updatedAt }.max() ?? 0); h.combine(s.lastActivityAt); h.combine(s.heardUpTo)
      h.combine(s.agentDeviceIds.count)
    }
    for t in timelines.values { h.combine(t.echoes.count); h.combine(t.items.count) }
    for c in cards.values where c.answer?.pending == true || c.inRevision != nil { h.combine(c.objectId); h.combine(c.objectState) }
    for n in notes.values where n.pending { h.combine(n.objectId); h.combine(n.text) }
    h.combine(human.raw.values.filter { $0.pending }.count)
    // (what stands on the Desk: a card held back or let stand by the hub's Desk changes nothing above)
    h.combine(stack); h.combine(openPermissionIds); h.combine(deskWaiting)
    return h.finalize()
  }

  // ---- what the views read ------------------------------------------------------------------------

  /** A session's settings (register `session/<id>`): by its session id; for a main session also by its agent device's id. */
  public func settingsOf(_ s: Session) -> JV {
    if let v = human.sessionSettings[s.sessionId] { return v }
    if s.parentSessionId == nil, let d = s.agentDeviceId, let v = human.sessionSettings[d] { return v }
    return s.settings
  }
  /** A session of the copy, made if it is new (the demo builds its board by hand). */
  public func sessionOf(_ id: String) -> Session {
    if let s = sessions[id] { return s }
    let s = Session(id)
    sessions[id] = s
    return s
  }
  /** The name a device gave itself (its register `device/<id>`): `device_name`, else the last part of its folder. */
  public func deviceNameOf(_ id: String?) -> String {
    guard let id = id else { return "" }
    let m = members[id], reg = deviceRegisters[id]
    if let n = m?.deviceName, !n.isEmpty { return n }
    if let n = reg?["device_name"].string, !n.isEmpty { return n }
    let folder = m?.folder ?? reg?["folder"].string ?? ""
    return folder.split(whereSeparator: { $0 == "/" || $0 == "\\" }).last.map(String.init) ?? ""
  }
  /** The parent a session names: its group's (5.2.5), else its profile's. */
  public func parentSessionOf(_ s: Session) -> String? {
    if let p = s.parentSessionId { return p }
    guard let want = s.profile["parent_session"].string, !want.isEmpty else { return nil }
    let parent = sessions[want]
    if s.createdByAgent { return parent.flatMap { $0 !== s && childOf($0, s) ? $0.sessionId : nil } }
    return parent?.sessionId ?? want
  }
  func childOf(_ parent: Session, _ s: Session) -> Bool { parent.agentDeviceIds.contains { $0 == s.creatorDeviceId || s.agentDeviceIds.contains($0) } }
  /** Who holds a card now (whom a human addresses): its owner while it is a leaf of the session, else the session's
   *  agent device (a helper session: its opener), spec 9.2. */
  public func holderOf(_ c: Card) -> String {
    guard let s = c.sessionId.flatMap({ sessions[$0] }), !s.agentDeviceIds.isEmpty, !s.agentDeviceIds.contains(c.agentDeviceId) else { return c.agentDeviceId }
    return (s.parentSessionId != nil ? s.creatorDeviceId : s.agentDeviceId) ?? c.agentDeviceId
  }
  public func timelineOf(_ key: String) -> Timeline {
    if let t = timelines[key] { return t }
    let t = Timeline(key)
    timelines[key] = t
    return t
  }
  /** A key only an agent writes in its session group. */
  public static func isAgentKey(_ k: String) -> Bool { k == "profile" || k == "heard" || k.hasPrefix("status_line/") || k.hasPrefix("alert/") }
}
