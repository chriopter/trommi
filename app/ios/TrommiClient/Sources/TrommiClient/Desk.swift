// Desk.swift: what the screens show, worked out from the board (a port of the web app's app.mjs BoardState and
// boardModel, and the small rules of desk.mjs and ui.mjs): sessions as the board names them, cards as the Desk reads them
// (number, status, Later, with the agent, Done rows), the Desk's parts, a session's or a card's conversation, and the
// words of a session's link. Pure: no network, no UI, the same on every device.
import Foundation

public let SESSION_ID_LEN = 12
public let UNHEARD_MS: UInt64 = 2 * 60_000
public let ACTING_MS: UInt64 = 6 * 60 * 60 * 1000

public enum Words {
  public static let later = "Later", duck = "Duck it", wake = "Wake up", ack = "Got it", what = "What??", trust = "I don’t give a duck", revise = "Reverse"
  public static let revising = "In revision", shred = "Shred", walk = "Blitz", desk = "Desk", takeBack = "Take back"
  public static let explainText = "Explain this question in more detail and in plain words: what it is about, what each option means for me, and what you would do."
  /** What a bare hand-back and a take-back say to the session (the machine's words: the talk shows a quiet line instead). */
  public static let handBackText = "Back to you: please rework this question and present it again. Take the comments under the card into account."
  public static let takeBackText = "The human took the card back; no need to rework or explain it."
  public static let settled = "Settled by your answer"
  public static let finalTip = "Settles it: nothing follows from this answer, the card goes straight to Done"
}

// ---- a session as the board shows it ----------------------------------------------------------------

public struct Agent: Identifiable, Equatable {
  public var id: String                 // the board id (address /chat/<id>)
  public var deviceId: String           // the session's key in the model (its session_id)
  public var sessionId: String?
  public var agentDeviceId: String?
  public var given: String              // what the agent calls itself
  public var name: String               // the human's label, else given
  public var label: String
  public var icon: String
  public var mark: String               // "draw:<name>" or a seed
  public var online: Bool
  public var offlineSince: UInt64?
  public var model: String
  public var task: String
  public var starred: Bool
  public var parent: String?
  public var main: Bool
  public var desk: String?
  public var archived: Bool
  public var position: Int
  public var seen: UInt64
  public var active: UInt64
  public var deviceActive: UInt64
  public var removed: Bool
  public var own: Bool
  public var link: AgentLink?
  public var heardUpTo: Int?
  public var hue: Int = 162
  /** Whether the human gave it a place in the order (its register's position; the sidebar's drag on the web). */
  public var placed: Bool = false
  /** Where it stands, as a card's page names its sender: desk (two or more), main session (a helper), itself; the
   *  client core's (client/src/sessions.rs `path`). */
  public var path: [String] = []
  /** The model it runs on as its heading says it ("Opus 5.5"): the client core's (client/src/sessions.rs model_label). */
  public var modelLabel: String = ""
  /** Archived because a newer pairing in the same folder took its place (the client core's session list). */
  public var replaced: Bool = false
  /** At the client core's clock (client/src/status.rs): its link (nil when archived or removed), why it is stopped
   *  (the red hand: it reads the Desk), whether a status line says working, since when it is quiet. */
  public var linkState: CoreLink? = nil
  public var blocked: CoreBlocked? = nil
  public var working: Bool = false
  public var quietSince: UInt64? = nil
}

/** A session's link as the client core decides it (status.rs `LinkView`). */
public struct CoreLink: Equatable { public var state: String; public var since: UInt64?; public var reason: String }
/** Why a session is stopped, as the client core decides it (status.rs `Blocked`). */
public struct CoreBlocked: Equatable { public var why: String; public var since: UInt64? }

public struct LinkWords: Equatable {
  public var state: String              // live | oncall | asleep | cut | gone
  public var sign: String               // ear | ear-later | ear-off | plug
  public var since: UInt64?
  public var word: String
  public var short: String
  public var line: String
  public var fixSay: String?
  public var fixCode: String?
}

func minutes(_ ms: UInt64) -> Int { max(1, Int((Double(ms) / 60000).rounded())) }
/** 14:05 today, else "3 Oct 14:05". */
public func clockText(_ t: UInt64, now: UInt64 = nowMs()) -> String {
  let d = Date(timeIntervalSince1970: Double(t) / 1000), n = Date(timeIntervalSince1970: Double(now) / 1000)
  let cal = Calendar.current
  let f = DateFormatter(); f.locale = Locale(identifier: "en_GB")
  f.dateFormat = cal.isDate(d, inSameDayAs: n) ? "HH:mm" : "d MMM HH:mm"
  return f.string(from: d)
}
/** "just now", "5 min ago", "3 h ago", "4 Oct". */
public func agoText(_ ts: UInt64, now: UInt64 = nowMs()) -> String {
  let min = Int((Double(Int64(now) - Int64(ts)) / 60000).rounded())
  if min < 1 { return "just now" }
  if min < 60 { return "\(min) min ago" }
  if min < 1440 { return "\(Int((Double(min) / 60).rounded())) h ago" }
  let f = DateFormatter(); f.locale = Locale(identifier: "en_GB"); f.dateFormat = "d MMM"
  return f.string(from: Date(timeIntervalSince1970: Double(ts) / 1000))
}

let RECONNECT = "/mcp → trommi → Reconnect"
/** The session's link in words (app.mjs linkOf); its state is the client core's (Agent.linkState). */
public func linkOf(_ a: Agent?, now: UInt64 = nowMs()) -> LinkWords? {
  guard let a = a, !a.archived, !a.removed, let v = a.linkState else { return nil }
  let n = a.name, l = a.link
  let since = v.since, at = since.map { clockText($0, now: now) } ?? ""
  switch v.state {
  case "cut" where !a.online:
    return LinkWords(state: "cut", sign: "ear-off", since: since, word: "cut off\(at.isEmpty ? "" : " · \(at)")", short: at.isEmpty ? "Cut off" : "Cut off since \(at)",
                     line: "\(n) is cut off\(at.isEmpty ? "" : " since \(at)"): its Claude Code runs, its Trommi tools are gone. It cannot hear you and cannot write to you.", fixSay: "In its terminal:", fixCode: RECONNECT)
  case "gone":
    let how = l?.exitReason != nil ? "its Claude Code session ended." : "it stopped without a word: its session ended, was killed, or its machine is off the network."
    return LinkWords(state: "gone", sign: "plug", since: since, word: "gone\(at.isEmpty ? "" : " · \(at)")", short: at.isEmpty ? "Gone" : "Gone since \(at)",
                     line: "\(n) is gone\(at.isEmpty ? "" : " since \(at)"): \(how)", fixSay: "Start it again in its folder:", fixCode: "claude --continue")
  case "cut":
    return LinkWords(state: "cut", sign: "ear-off", since: since, word: "cut off\(at.isEmpty ? "" : " · \(at)")", short: at.isEmpty ? "Cut off" : "Cut off since \(at)",
                     line: "A Claude Code session in \(n)'s folder is cut off\(at.isEmpty ? "" : " since \(at)"): it runs, its Trommi tools are gone. It cannot hear you and cannot write to you.", fixSay: "In its terminal:", fixCode: RECONNECT)
  case "live":
    return LinkWords(state: "live", sign: "ear", since: nil, word: "", short: "", line: "\(n) hears you at once.", fixSay: nil, fixCode: nil)
  default: break
  }
  let m = since.map { minutes(now > $0 ? now - $0 : 0) }
  if v.state == "asleep", let m = m {
    return LinkWords(state: "asleep", sign: "ear-later", since: since, word: "not listening · \(m) min", short: "Not listening for \(m) min",
                     line: "\(n) is not listening: it hears you only on its next step, and its last one was \(m) min ago.", fixSay: "Wake it: type anything in its terminal.", fixCode: nil)
  }
  return LinkWords(state: "oncall", sign: "ear-later", since: since, word: "on its next step", short: "Hears you on its next step",
                   line: "\(n) hears you on its next step\(m.map { "; its last one was \($0) min ago" } ?? "").", fixSay: nil, fixCode: nil)
}

// ---- a card as the Desk reads it ---------------------------------------------------------------------

/**
 * The Desk's two tools, Blitz and the duck for all (his word, 9 October: with only cards to read he still wants to
 * read card by card, Blitz, or close them all, the duck). Over the open cards that wait for him: questions and infos,
 * not snoozed, not with an agent, not of an archived session.
 * Blitz walks all of them one by one; its count is `walk`, and it shows when walk > 0.
 * The duck acts on all at once: a decision gets "I don't give a duck" (`duck`), an info is marked read (`read`), a
 * permission request is never touched. It shows when one of the two lists is not empty: not with permissions alone.
 */
public struct DeskTools: Equatable {
  public var walk: Int
  public var duck: [String]
  public var read: [String]
  public var showsBlitz: Bool { walk > 0 }
  public var showsDuck: Bool { !duck.isEmpty || !read.isEmpty }
  public init(_ open: [(id: String, kind: String)]) {
    walk = open.count
    duck = open.filter { $0.kind == "decision" }.map { $0.id }
    read = open.filter { $0.kind == "info" }.map { $0.id }
  }
  /**
   * What the duck for all says, the web's words (desk.mjs duckAll): `ask` is the one short line of the confirmation,
   * `yes` its primary action beside "Cancel"; how many of which (`what`) is only said to a screen reader, and `tip`
   * names the button itself.
   */
  public var duckWords: (ask: String, yes: String, what: String, tip: String) {
    let asks = duck.count, reads = read.count, n = asks + reads
    let goes = asks == 1 ? "1 decision goes" : "\(asks) decisions go"
    let closed = reads == 1 ? "1 card to read is" : "\(reads) cards to read are"
    return (
      ask: asks > 0 ? (n == 1 ? "Duck it?" : "Duck all \(n)?") : n == 1 ? "Close it as read?" : "Close all \(n) as read?",
      yes: asks > 0 ? (n == 1 ? "Yes, duck it" : "Yes, duck them all") : n == 1 ? "Yes, read" : "Yes, all read",
      what: reads == 0 ? "\(goes) to the agents" : asks == 0 ? "\(closed) closed" : "\(goes) to the agents, \(closed) closed",
      tip: reads == 0 ? (n == 1 ? "I don’t give a duck: for the one open decision" : "I don’t give a duck: for all \(n) open decisions")
        : asks == 0 ? (n == 1 ? "I don’t give a duck: close the one card to read" : "I don’t give a duck: close all \(n) cards to read")
        : "I don’t give a duck: for all \(n) open cards")
  }
}

public struct DeskCard: Identifiable {
  public var id: String
  public var agent: String
  public var number: Int
  public var kind: String               // decision | info | permission
  public var status: String             // open | decided | done | shredded
  public var urgency: String
  public var urgencyReason: String
  public var title: String
  public var teaser: String
  public var body: String
  /** Its options, sections, attachments and recommended keys: of a card of the board read from it when first asked (a
   *  lean card reads them from the client core), so that a Desk of a thousand cards does not hold them all. */
  public var options: [Option] { get { ownOptions ?? from?.options ?? [] } set { ownOptions = newValue } }
  public var sections: [JV]? { get { ownSections ?? from?.sections } set { ownSections = newValue } }
  public var html: String?
  public var attachments: [JV] { get { ownAttachments ?? from?.attachments ?? [] } set { ownAttachments = newValue } }
  var ownOptions: [Option]?
  var ownSections: [JV]?
  var ownAttachments: [JV]?
  var ownRecommended: [String]?
  /** The board's card it was made from (its lazy parts). */
  var from: Card?
  public var version: Int
  public var multiple: Bool
  public var choices: [String]
  public var note: String
  public var optionNotes: [String: String] = [:]
  public var summary: String
  public var created: UInt64
  public var decided: UInt64?
  public var recommended: [String] { get { ownRecommended ?? from?.recommended ?? [] } set { ownRecommended = newValue } }
  public var versionHash: String?
  public var contentState: String
  public var revised: UInt64?
  public var revisionNote: String = ""
  public var versions: [CardVersion] = []
  public var trusted = false
  public var settled = false
  public var read: UInt64?
  public var shredded: UInt64?
  public var pending = false
  public var heard: Bool?
  public var withAgent: UInt64?
  /** Its agent is still working on it (an info from start_task): with the agent, no Take back. */
  public var working = false
  public var draft: JV?
  public var snoozedUntil: UInt64?
  public var snoozedAt: UInt64?
  public var unsnoozed: UInt64?
  /** His star: when he set it (`star/<card>`); nil when not starred. */
  public var starred: UInt64?
  /** Its agent closed it after his answer (close_card): What?? asks about what it did. */
  public var finished = false
  public var mergedInto: String?
  public var mergedFrom: [String] { get { ownMergedFrom ?? from?.mergedFromObjectIds ?? [] } set { ownMergedFrom = newValue } }
  var ownMergedFrom: [String]?
  public var noteAttachments: [JV] = []
  public var answeredVersion: Int?
  public var unsupported = false
  init(id: String, agent: String, number: Int, kind: String, status: String, urgency: String, urgencyReason: String, title: String, teaser: String, body: String,
       options: [Option]?, sections: [JV]?, html: String?, attachments: [JV]?, version: Int, multiple: Bool, choices: [String], note: String, summary: String,
       created: UInt64, decided: UInt64?, recommended: [String]?, versionHash: String?, contentState: String, from: Card? = nil) {
    self.id = id; self.agent = agent; self.number = number; self.kind = kind; self.status = status; self.urgency = urgency; self.urgencyReason = urgencyReason
    self.title = title; self.teaser = teaser; self.body = body; self.ownOptions = options; self.ownSections = sections; self.html = html; self.ownAttachments = attachments
    self.version = version; self.multiple = multiple; self.choices = choices; self.note = note; self.summary = summary; self.created = created; self.decided = decided
    self.ownRecommended = recommended; self.versionHash = versionHash; self.contentState = contentState; self.from = from
  }
  public var isKnock: Bool { kind == "permission" || urgency == "high" || urgency == "critical" }
  public var knockWord: String? { kind == "permission" ? "Knock! Permission" : urgency == "critical" ? "Knock! Blocking" : urgency == "high" ? "Knock" : nil }
  public var nr: String { "Nr. \(number)" }
  public var choice: String? { choices.first }
  public var advisedLabels: String { options.filter { recommended.contains($0.key) }.map { $0.label }.joined(separator: ", ") }
  /** Answered by him (a choice, a duck, a read info). */
  public var answeredBy: Bool { (kind == "decision" && (!choices.isEmpty || trusted)) || (kind == "info" && read != nil) }
}

// ---- one line of a conversation ---------------------------------------------------------------------

public struct Message: Identifiable {
  public var id: String
  public var seq: Double
  public var agent: String
  public var from: String               // user | agent | event
  public var kind: String? = nil        // events: asked, info, revised, decided, read, shredded, reopened, done
  public var text: String
  public var attachments: [JV] = []
  public var ts: UInt64
  public var cardId: String? = nil
  public var details: String? = nil
  public var html: String? = nil
  public var published: String? = nil
  public var noteWritten: UInt64?? = nil
  public var handback = false
  public var explain = false
  public var present = false
  public var marks: [JV] = []
  public var copiedCards: [String] = []
  public var pending = false
  public var itemState: String = "loaded"
  public var labels: [String] = []      // a decided event: each choice by its name then
  public var optionNotes: [(String, String)] = []
  public var trusted = false
  public var settled = false
  public var version: Int? = nil
  public var contentType: String = "message"
  /** The terminal mirror, from an agent only: "input" = what he typed into the session's terminal (from is "user":
   *  his message, passed on by the agent's connector, no device of his signed it), "answer" = the agent's final text
   *  of a turn there. */
  public var terminal: String? = nil
  /** A turn's trail (README "The trail", Work.swift): the envelopes of one turn folded into one block. Such a message
   *  has from "work", no text and no attachments, and stands where Work.anchor says; it is nothing
   *  the agent said: no unread mark, no preview. */
  public var work: WorkBlock? = nil
}

// ---- the Desk -----------------------------------------------------------------------------------------

public struct DeskUnit: Identifiable {
  public var id: String
  public var agent: Agent
  public var open: Int
  public var online: Bool
  public var running: Bool
  public var stuck: Bool
  public var blocked: (why: String, text: String)?
  public var link: LinkWords?
  public var unheard: Int = 0
  public var parent: String?
  public var subs: [String] = []
}

/**
 * A main session's helpers as the web's sidebar shows them (sidebar.mjs row, app.mjs summary): folded into one stack
 * under their main until he unfolds it. `lie`: the helpers whose drawings lie in the stack, in their order; more than
 * `edges` of them: the stopped ones first, then cut to `edges`. `whole`: the main with its helpers counted in (what
 * waits on him, whether one is connected, works, knocks or is stopped), which is what the main's row says.
 */
public struct UnitStack {
  public static let edges = 7
  public var lie: [DeskUnit]
  public var count: Int
  /** Helpers at work. */
  public var working: Int
  /** Helpers with something for him: stopped, an open question, or words he has not read. */
  public var waiting: Int
  public var whole: DeskUnit
  public init(main: DeskUnit, subs: [DeskUnit], unread: (Agent) -> Bool = { _ in false }) {
    lie = subs.count > UnitStack.edges ? Array((subs.filter { $0.blocked != nil } + subs.filter { $0.blocked == nil }).prefix(UnitStack.edges)) : subs
    count = subs.count
    working = subs.filter { $0.online && $0.running }.count
    waiting = subs.filter { $0.blocked != nil || $0.open > 0 || unread($0.agent) }.count
    var w = main
    for s in subs {
      w.open += s.open
      w.running = (w.online && w.running) || (s.online && s.running)
      w.online = w.online || s.online
      w.stuck = w.stuck || s.stuck
      if w.blocked == nil { w.blocked = s.blocked }
    }
    whole = w
  }
}

public struct Task1 { public var agent: String; public var id: String; public var label: String; public var state: String?; public var detail: String?; public var cardId: String?; public var updated: UInt64 }

public struct DeskDesc: Identifiable, Equatable {
  public var id: String; public var name: String; public var created: UInt64; public var order: Int?; public var crown: JV?
  /** The desk's goals (register field goals): cleaned, "" when it has none. */
  public var goals: String
  public init(id: String, name: String, created: UInt64, order: Int?, crown: JV?, goals: String = "") { self.id = id; self.name = name; self.created = created; self.order = order; self.crown = crown; self.goals = goals }
}

/** A desk's goals (app/web/public/app.mjs cleanGoals, desk.mjs deskGoals): his own short note under the Desk's greeting,
 *  kept in the desk's register desk/<id> as the field goals (a human register: end-to-end encrypted like the name, on
 *  every device of his). At most GOALS_LINES lines of at most GOALS_LINE_MAX characters (UTF-16 units, as the web
 *  counts; never half a character), no spaces at a line's end, no blank lines at either end. The caps are the same
 *  on every client and in the connector; the Desk folds them to their first line on a tap. */
public let GOALS_LINES = 20, GOALS_LINE_MAX = 200
public func cleanGoals(_ text: String) -> String {
  var lines = text.replacingOccurrences(of: "\r\n", with: "\n").replacingOccurrences(of: "\r", with: "\n").split(separator: "\n", omittingEmptySubsequences: false).map { raw -> String in
    var l = Substring(raw)
    while let c = l.last, c.isWhitespace || c == "\u{feff}" { l.removeLast() }
    var out = "", n = 0
    for c in l { n += c.utf16.count; if n > GOALS_LINE_MAX { break }; out.append(c) }
    return out
  }
  while lines.first == "" { lines.removeFirst() }
  while lines.last == "" { lines.removeLast() }
  return lines.prefix(GOALS_LINES).joined(separator: "\n")
}
/** The goals as the Desk shows them folded (desk.mjs goalsWords): the first line, and how many more lie under it
 *  (0: one line, nothing to fold, no toggle). */
public func goalsFold(_ text: String) -> (shown: String, more: Int) {
  let lines = text.split(separator: "\n", omittingEmptySubsequences: false)
  if lines.count <= 1 { return (text, 0) }
  return (String(lines[0]), lines.count - 1)
}
/** The register a desk has after its goals were written (app.mjs hub.desk({ id, goals })): every other field kept, the
 *  field gone when the text is empty; a desk that is not there yet starts as the first desk, "Personal". */
public func deskRegister(_ have: JV?, goals: String, now: UInt64 = nowMs()) -> JV {
  var v = have?.object ?? ["name": .str("Personal"), "created_at": .n(0)]
  let text = cleanGoals(goals)
  v["goals"] = text.isEmpty ? nil : .str(text)
  return .obj(v)
}

public let ALL_DESKS = "all"

/** Everything the screens read, made once per change of the board. */
public final class DeskModel {
  public let board: Board
  public private(set) var agents: [Agent] = []                  // every session (archived ones too), in their order
  public private(set) var byAgent: [String: Agent] = [:]
  public private(set) var cards: [DeskCard] = []
  public private(set) var byCard: [String: DeskCard] = [:]
  public private(set) var numberOf: [String: Int] = [:]
  public private(set) var tasks: [Task1] = []
  /** By agent id, built once per update (a big room has hundreds of sessions: no scan of every card or line per row). */
  private var workingOf: [String: [Task1]] = [:]
  public private(set) var desks: [DeskDesc] = []
  private(set) var devToAgent: [String: String] = [:]
  private(set) var agentToDev: [String: String] = [:]
  private var humans = Set<String>()

  public init(board: Board) { self.board = board; update() }

  /** After a change of the board: everything again (the board is small enough; the views diff). */
  /** What the screens asked for since the last update: computed once per change of the board, not once per view. */
  private var viewCache: [String: View] = [:]
  private var messageCache: [String: [Message]] = [:]
  private var cardMessageCache: [String: [Message]] = [:]
  /** A timeline's messages, kept ACROSS updates while nothing they were built from changed: the timeline (its revision),
   *  who is human, and the artifacts and notes its messages named. A live message elsewhere then costs the open
   *  5,000-item chat no rebuild. */
  private struct ItemsBuilt {
    // (the timeline itself, held: a board reset makes new ones, which start their revisions at 0 again)
    var timeline: Timeline; var revision: Int; var humans: Set<String>; var me: String?; var agent: String; var cardId: String?
    var published: [String: String?]; var notes: [String: JV?]
    var messages: [Message]
  }
  private var itemsCache: [String: ItemsBuilt] = [:]
  /** Bumped by every update: a view keyed on it re-reads, others keep what they built. */
  public private(set) var revision = 0
  public func update(now: UInt64 = nowMs()) {
    viewCache = [:]; messageCache = [:]; cardMessageCache = [:]; askAtCache = [:]; revision &+= 1
    let m = board
    devToAgent = [:]; agentToDev = [:]
    let sessions = m.sessions.values.sorted { $0.sessionId < $1.sessionId }
    // (two sessions may name the same agent id, e.g. one agent_session_id twice: the newest session holds the id)
    // (each session's board id as the client core's session list names it; one it does not list, by the same rule)
    var listed = [String: String]()
    for r in m.sessionList { if let k = r["key"].string, let id = r["id"].string { listed[k] = id } }
    for s in sessions {
      let id = listed[s.sessionId] ?? agentIdOf(s); devToAgent[s.sessionId] = id
      if let had = agentToDev[id], let h = m.sessions[had], h.lastActivityAt > s.lastActivityAt { continue }
      agentToDev[id] = s.sessionId
    }
    humans = Set(m.members.values.filter { $0.deviceRole == "human" }.map { $0.deviceId })
    // Card numbers: the order cards and permission requests were first filed in, from 1.
    var numbered = m.cards.values.map { ($0.firstEnvelopeNumber, $0.objectId, false) } + m.permissions.values.map { ($0.envelopeNumber, $0.objectId, true) }
    numbered.sort { ($0.0, $0.1) < ($1.0, $1.1) }
    numberOf = [:]
    for (i, x) in numbered.enumerated() { numberOf[x.1] = i + 1 }
    var out = [DeskCard]()
    for x in numbered {
      // (held back while a catch-up is behind the hub's Desk: settled elsewhere, not a question any more; Room.deskFirst)
      if x.2 { if let p = m.permissions[x.1], !m.heldBack(p.objectId, open: p.permissionState == "pending") { out.append(permissionCard(p, numberOf[x.1]!, now: now)) } }
      else if let c = m.cards[x.1], !m.heldBack(c.objectId, open: c.objectState == "open") { out.append(boardCard(c, numberOf[x.1]!, now: now)) }
    }
    cards = out
    byCard = Dictionary(out.map { ($0.id, $0) }, uniquingKeysWith: { a, b in b.created >= a.created ? b : a })
    agents = makeAgents()
    byAgent = DeskModel.newestById(agents)
    tasks = []
    // (each line's state as shown: the client core's, at its clock, in the session list (client/src/status.rs
    //  line_state: its card and session decide, so it says what Off your mind says); a session the list does not name:
    //  gone)
    var linesOf = [String: JV]()
    for r in m.sessionList { if let k = r["key"].string { linesOf[k] = r["lines"] } }
    for s in sessions { for t in s.statusLines {
      var line = Task1(agent: devToAgent[s.sessionId] ?? "", id: t.id, label: t.label, state: t.state, detail: t.detail, cardId: t.objectId, updated: t.updatedAt)
      line.state = linesOf[s.sessionId]?[t.id].string ?? "gone"
      tasks.append(line)
    } }
    workingOf = Dictionary(grouping: tasks.filter { $0.state == "working" }, by: { $0.agent })
    var d = m.human.desks.compactMap { (id, v) -> DeskDesc? in
      guard v.object != nil else { return nil }
      let name = (v["name"].string ?? "").trimmingCharacters(in: .whitespaces)
      return DeskDesc(id: id, name: name.isEmpty ? "Personal" : name, created: clampedU64(v["created_at"].double), order: v["order"].int, crown: v.has("crown") ? v["crown"] : nil, goals: cleanGoals(v["goals"].string ?? ""))
    }
    let ordered = d.contains { $0.order != nil }
    d.sort { a, b in
      if ordered { return (a.order ?? Int.max, a.created, a.id) < (b.order ?? Int.max, b.created, b.id) }
      if a.id == "main" { return b.id != "main" }
      if b.id == "main" { return false }
      return (a.created, a.id) < (b.created, b.id)
    }
    desks = d
  }

  /** Agents by id; for an id twice (no input may crash the app), the one active last. */
  static func newestById(_ list: [Agent]) -> [String: Agent] {
    Dictionary(list.map { ($0.id, $0) }, uniquingKeysWith: { a, b in b.active >= a.active ? b : a })
  }
  func agentIdOf(_ s: Session) -> String {
    if let a = s.agentSessionId, !(a.count >= 12 && a.allSatisfy { $0.isHexDigit && !$0.isUppercase }) { return a }
    return String(s.sessionId.prefix(SESSION_ID_LEN))
  }
  public func agentOf(cardId: String) -> Agent? { byCard[cardId].flatMap { byAgent[$0.agent] } }
  public func sessionKey(of agentId: String) -> String? { agentToDev[agentId] }

  /**
   * The sessions as the sidebar shows them: the client core's session list (Board.sessionList, client/src/sessions.rs),
   * the same on every device of the person. What is added here is only what the views draw beside it.
   */
  private func makeAgents() -> [Agent] {
    let m = board
    var lastOfDevice = [String: UInt64]()
    for s in m.sessions.values { if let a = s.agentDeviceId { lastOfDevice[a] = max(lastOfDevice[a] ?? 0, s.lastActivityAt) } }
    return m.sessionList.compactMap { r -> Agent? in
      let key = r["key"].string ?? ""
      guard let s = m.sessions[key] else { return nil }
      let p = s.profile
      let id = r["id"].string ?? String(key.prefix(SESSION_ID_LEN))
      let icon = r["icon"].string ?? ""
      var a = Agent(id: id, deviceId: key, sessionId: key, agentDeviceId: s.agentDeviceId, given: r["given"].string ?? id, name: r["name"].string ?? id,
                    label: r["label"].string ?? "", icon: icon, mark: icon.isEmpty ? id : icon, online: r["online"].truthy, offlineSince: r["offline_since"].double.map { UInt64($0) },
                    model: p["model"].string ?? "", task: p["task"].string ?? "", starred: r["starred"].truthy, parent: r["parent"].string, main: r["main"].truthy,
                    desk: r["desk"].string, archived: r["archived"].truthy, position: Int((r["position"].double ?? 0).rounded()), seen: s.lastActivityAt, active: s.lastActivityAt,
                    deviceActive: s.agentDeviceId.flatMap { lastOfDevice[$0] } ?? 0, removed: r["removed"].truthy, own: r["own"].truthy, link: s.link, heardUpTo: s.heardUpTo)
      a.hue = Pen.hueFor(id: a.id, mark: a.mark)
      a.placed = r["placed"].truthy
      a.path = r["path"].array?.compactMap(\.string) ?? []
      a.modelLabel = r["model_label"].string ?? ""
      a.replaced = r["replaced"].truthy
      let lv = r["link"]
      if lv.object != nil { a.linkState = CoreLink(state: lv["state"].string ?? "live", since: lv["since"].double.map { UInt64(max(0, $0)) }, reason: lv["reason"].string ?? "") }
      let bv = r["blocked"]
      if bv.object != nil { a.blocked = CoreBlocked(why: bv["why"].string ?? "", since: bv["since"].double.map { UInt64(max(0, $0)) }) }
      a.working = r["working"].truthy
      a.quietSince = r["quiet_since"].double.map { UInt64(max(0, $0)) }
      if let d = r["device_active"].double { a.deviceActive = UInt64(max(0, d)) }
      return a
    }
  }

  /** The versions a human reads as the card: those of its history (the client core's `history`, every revision). */
  static func revisionsOf(_ c: Card) -> [CardVersion] { let keep = Set(c.history.map { $0.envelopeNumber }); return c.versions.filter { keep.contains($0.envelopeNumber) } }

  private func boardCard(_ c: Card, _ number: Int, now: UInt64) -> DeskCard {
    let m = board, a = c.answer, h = m.human
    let status = c.objectState == "open" ? "open" : c.closedHow == "shredded" ? "shredded" : c.closedHow == "answered" && c.objectState == "answered" ? "decided" : "done"
    let turns = DeskModel.revisionsOf(c)
    let key = c.sessionId ?? c.agentDeviceId
    var d = DeskCard(id: c.objectId, agent: devToAgent[key] ?? String(key.prefix(SESSION_ID_LEN)), number: number, kind: c.cardType == "info" ? "info" : "decision", status: status,
                     urgency: c.urgency, urgencyReason: c.urgencyReason ?? "", title: c.title, teaser: c.teaser ?? "", body: c.body ?? "", options: nil, sections: nil,
                     html: c.html, attachments: nil, version: max(1, c.objectVersion), multiple: c.allowsMultiple, choices: a?.choices ?? [], note: a?.note ?? "",
                     summary: c.closeSummary ?? c.withdrawReason ?? "", created: c.createdAt, decided: a?.answeredAt ?? (status == "done" ? c.updatedAt : nil),
                     recommended: nil, versionHash: c.versionHash, contentState: c.contentState, from: c)
    if turns.count > 1 { d.versions = Array(turns.dropLast()); d.revised = turns.last?.sentAt ?? c.updatedAt; d.revisionNote = c.changeNote ?? "" }
    if let a = a {
      d.answeredVersion = a.boundObjectVersion
      d.optionNotes = a.optionNotes
      d.noteAttachments = a.attachments
      d.trusted = a.trusted
      if c.closedHow == "settled" { d.settled = true }
      if a.answerAction == "read" { d.read = a.answeredAt }
      if a.answerAction == "shred" { d.shredded = a.answeredAt }
      d.pending = a.pending
    }
    let said: Int? = a != nil && !(a!.pending) && status == "decided" ? a!.envelopeNumber : status == "open" ? c.inRevision?.envelopeNumber : nil
    if let said = said, let mark = m.sessions[key]?.heardUpTo { d.heard = said <= mark }
    if let r = c.inRevision, status == "open" {
      d.withAgent = m.timelines[c.timelineKey]?.items[r.envelopeNumber]?.sentAt ?? c.updatedAt
    }
    // An info its agent is still working on (start_task, spec 9.1.3): with the agent from the start, under Off your
    // mind with the working strip, never on the Desk; its "Done: …" version brings it to the Desk.
    if c.working && status == "open" && d.kind == "info" { d.working = true; if d.withAgent == nil { d.withAgent = d.created } }
    if let dr = h.drafts[c.objectId], status == "open" { d.draft = dr }
    if let st = h.stars[c.objectId], st.object != nil { d.starred = max(1, clampedU64(st["at"].double)) }
    // (his "Later" as the client core holds it: until its time and only for the version he put off, desk.rs)
    if status == "open" { if let u = c.snoozedUntil { d.snoozedUntil = u; d.snoozedAt = c.snoozedAt ?? 0 } else if let u = c.unsnoozed { d.unsnoozed = u } }
    if status == "done" && c.closedHow == "closed", let a = a, !a.pending, a.answerAction != "read", a.answerAction != "shred" { d.finished = true }
    d.mergedInto = c.mergedIntoObjectId
    if c.unsupported || c.contentState == "newer_schema" { d.unsupported = true }
    return d
  }
  private func permissionCard(_ p: Permission, _ number: Int, now: UInt64) -> DeskCard {
    let status = p.permissionState == "pending" && now <= p.endsAt ? "open" : "done"
    let key = p.sessionId ?? p.agentDeviceId
    let choice = p.verdict.map { $0.allow ? "allow" : "deny" }
    return DeskCard(id: p.objectId, agent: devToAgent[key] ?? String(key.prefix(SESSION_ID_LEN)), number: number, kind: "permission", status: status, urgency: "critical", urgencyReason: "",
                    title: "Approval: \(p.toolName)", teaser: "", body: "\(p.description)\n\n\(p.inputPreview)",
                    options: [Option(.obj(["key": "allow", "label": "Allow"])), Option(.obj(["key": "deny", "label": "Deny"]))], sections: nil, html: nil, attachments: [], version: 1,
                    multiple: false, choices: choice.map { [$0] } ?? [], note: "", summary: p.permissionState == "withdrawn" ? "Answered in the terminal" : "", created: p.sentAt,
                    decided: p.verdict != nil ? p.sentAt : nil, recommended: [], versionHash: p.versionHash, contentState: "ok")
  }

  // ---- the Desk (boardModel) ---------------------------------------------------------------------------

  public struct View {
    public var deskId: String?
    public var all: Bool
    public var deskName: String
    public var here: [Agent]                 // sessions on this desk, not archived
    public var fresh: [DeskCard]             // open questions waiting on him
    public var reads: [DeskCard]             // open infos
    public var revising: [DeskCard]          // with the agent (handed back, What??)
    public var snoozed: [DeskCard]
    public var done: [DeskCard]
    public var units: [DeskUnit]
    public var cut: [DeskUnit]
    public var unheard: Int
    public var knocking: Int
    public var working: Int
    public var allFreshCount: Int
    public var goals: String = ""            // this desk's goals; none on All desks
    /** The client core's order of every card that waits (client/src/desk.rs walk_order, the patch's desk_order). */
    public var order: [String] = []
    /** What the Desk's two tools act on: every open card that waits for him (fresh and reads). */
    public var tools: DeskTools { DeskTools((fresh + reads).map { (id: $0.id, kind: $0.kind) }) }
    /** The Scribble Board of this view: All desks' own, else the desk's (a room without desks: the board of 'main'). */
    public var board: String { all ? ALL_BOARD : deskBoard(deskId) }
    /** The Desk's rows (questions and infos) in the client core's order, the same on every device: what blocks an
     *  agent first, then what knocks, then the rest; in each the infos first, newest first, then the decisions. */
    public func deskCards() -> [DeskCard] {
      var here = Dictionary((fresh + reads).map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
      var out = [DeskCard]()
      for id in order { if let c = here.removeValue(forKey: id) { out.append(c) } }
      // (a card the core's order does not name yet, between two patches: at the end, until it does)
      return out + (fresh + reads).filter { here[$0.id] != nil }
    }
  }

  public func deskOf(_ a: Agent?) -> String? {
    guard !desks.isEmpty else { return nil }
    if let d = a?.desk, desks.contains(where: { $0.id == d }) { return d }
    return desks[0].id
  }

  /** The Desk in view (desk: its id, ALL_DESKS for all, nil: the first). */
  public func view(desk: String?) -> View {
    let k = desk ?? ""
    if let v = viewCache[k] { return v }
    let v = makeView(desk: desk, now: nowMs())
    viewCache[k] = v
    return v
  }
  public func view(desk: String?, now: UInt64) -> View { makeView(desk: desk, now: now) }
  private func makeView(desk: String?, now: UInt64) -> View {
    let hasDesks = !desks.isEmpty
    let all = hasDesks && desks.count > 1 && desk == ALL_DESKS
    let deskId: String? = all ? ALL_DESKS : hasDesks ? (desks.contains { $0.id == desk } ? desk : desks[0].id) : nil
    var everyone = agents
    if hasDesks {
      for i in everyone.indices {
        let d = desks.first { $0.id == deskOf(everyone[i]) }
        if let d = d, let crown = d.crown {
          everyone[i].starred = crown.truthy && (crown["session_id"].string.map { $0 == everyone[i].sessionId } ?? (crown["agent_device_id"].string == everyone[i].agentDeviceId && everyone[i].parent == nil))
        }
      }
    }
    let onDesk: (Agent?) -> Bool = { a in !hasDesks || all || self.deskOf(a) == deskId }
    let here = everyone.filter { !$0.archived && onDesk($0) }
    let byA = DeskModel.newestById(everyone)
    let shelved = Set(everyone.filter { $0.archived }.map { $0.id })
    let mine: (DeskCard) -> Bool = { onDesk(byA[$0.agent]) }
    // The Desk: what the client core's Desk holds (its stack and open requests: not held, not put off, not of an
    // archived session; client/src/desk.rs), with the infos their agents still work on (Off your mind), by age.
    let onCoreDesk = Set(board.stack).union(board.openPermissionIds)
    let allOpen = cards.filter { $0.status == "open" && (onCoreDesk.contains($0.id) || ($0.working && !shelved.contains($0.agent) && $0.snoozedUntil == nil)) }.sorted { ($0.created, $0.number) < ($1.created, $1.number) }
    let allFresh = allOpen.filter { $0.withAgent == nil && $0.kind != "info" }
    let open = allOpen.filter(mine)
    let reads = open.filter { $0.withAgent == nil && $0.kind == "info" }.sorted { ($1.isKnock ? 1 : 0, $1.created) < ($0.isKnock ? 1 : 0, $0.created) }
    let fresh = open.filter { $0.withAgent == nil && $0.kind != "info" }
    let revising = open.filter { $0.withAgent != nil }.sorted { ($0.withAgent ?? 0) > ($1.withAgent ?? 0) }
    var snoozed = [DeskCard](), done = [DeskCard](), unheardOf = [String: Int]()
    for c in cards where mine(c) {
      if c.status == "open" && c.snoozedUntil != nil && !shelved.contains(c.agent) { snoozed.append(c) }
      if c.status == "shredded" || (c.status != "open" && ((c.kind == "decision" && (!c.choices.isEmpty || c.trusted)) || (c.kind == "info" && c.read != nil))) { done.append(c) }
      if c.status == "open" || c.status == "decided", let h = heardOf(c, now: now), h.late { unheardOf[c.agent, default: 0] += 1 }
    }
    snoozed.sort { ($0.snoozedAt ?? 0) > ($1.snoozedAt ?? 0) }
    done.sort { ($0.status == "shredded" ? $0.shredded : $0.decided) ?? 0 > ($1.status == "shredded" ? $1.shredded : $1.decided) ?? 0 }
    // (one session's summary; the agent ids are unique, so "every agent of the set" is the one agent)
    let freshOf = Dictionary(grouping: fresh, by: { $0.agent })
    func summary(_ a: Agent) -> (open: Int, online: Bool, running: Bool, stuck: Bool, blocked: (String, String)?) {
      let mineF = freshOf[a.id] ?? []
      return (mineF.count, a.online, a.online && workingOf[a.id] != nil, mineF.contains { $0.isKnock }, blockedOf(a, now: now))
    }
    var units: [DeskUnit] = here.map { a in
      let s = summary(a)
      return DeskUnit(id: a.id, agent: a, open: s.open, online: s.online, running: s.running, stuck: s.stuck, blocked: s.blocked.map { (why: $0.0, text: $0.1) })
    }
    let ix = Dictionary(units.enumerated().map { ($1.id, $0) }, uniquingKeysWith: { a, b in units[b].agent.active >= units[a].agent.active ? b : a })
    for i in units.indices {
      if let p = units[i].agent.parent, let j = ix[p], j != i, units[j].agent.parent == nil { units[i].parent = p; units[j].subs.append(units[i].id) }
    }
    for i in units.indices { units[i].link = units[i].parent != nil ? nil : linkOf(units[i].agent, now: now); units[i].unheard = unheardOf[units[i].id] ?? 0 }
    for i in units.indices where !units[i].subs.isEmpty {
      for s in units[i].subs { if let j = ix[s] { units[i].unheard += units[j].unheard; units[j].unheard = 0 } }
    }
    let cut = units.filter { u in u.link?.state == "cut" && !(u.parent.flatMap { ix[$0] }.map { units[$0].link?.state == "cut" && units[$0].agent.agentDeviceId == u.agent.agentDeviceId } ?? false) }
    let name = all ? "All desks" : desks.first { $0.id == deskId }?.name ?? "Personal"
    return View(deskId: deskId, all: all, deskName: name, here: here, fresh: fresh, reads: reads, revising: revising, snoozed: snoozed, done: done, units: units, cut: cut,
                unheard: units.reduce(0) { $0 + $1.unheard }, knocking: fresh.filter { $0.isKnock }.count, working: units.filter { $0.online && $0.running }.count, allFreshCount: allFresh.count,
                goals: all ? "" : desks.first { $0.id == deskId }?.goals ?? "", order: board.deskOrder)
  }

  /** agent: why the session is stopped, or nil (app.mjs blockedOf): the client core's decision (Agent.blocked), in words. */
  public func blockedOf(_ a: Agent, now: UInt64 = nowMs()) -> (String, String)? {
    guard !a.archived, let b = a.blocked else { return nil }
    switch b.why {
    case "cut": return ("cut", linkOf(a, now: now)?.short ?? "Cut off")
    case "offline": return ("offline", b.since.map { "Connection lost since \(clockText($0, now: now))" } ?? "Disconnected while working")
    case "permission": return ("permission", "Waiting for permission")
    default: return ("blocking", "Waiting for you: a blocking question")
    }
  }
  /** The quiet hint: "quiet for 24 min" (the client core's quiet_since). */
  public func quietOf(_ a: Agent, now: UInt64 = nowMs()) -> String? {
    guard !a.archived, a.online, let q = a.quietSince else { return nil }
    return "quiet for \(minutes(now > q ? now - q : 0)) min"
  }
  /** Whether the card's session has his last word on it (heardOf). */
  public func heardOf(_ c: DeskCard, now: UInt64 = nowMs()) -> (heard: Bool?, waiting: UInt64, late: Bool)? {
    let at: UInt64? = c.status == "open" ? c.withAgent : c.status == "decided" && !c.settled ? c.decided : nil
    guard let at = at, !c.pending else { return nil }
    let heard = c.heard
    let waiting = heard == true ? 0 : (now > at ? now - at : 0)
    return (heard, waiting, heard == false && now - min(now, at) >= UNHEARD_MS)
  }

  // ---- the places a card lies (desk.mjs stackOf, stackCards, offSheets) ----------------------------------

  public func stackOf(_ c: DeskCard, now: UInt64 = nowMs()) -> String? {
    let online: (String) -> Bool = { id in self.byAgent[id]?.online == true || linkOf(self.byAgent[id], now: now)?.state == "cut" }
    if c.kind == "permission" { return c.status == "open" && c.snoozedUntil != nil ? "later" : nil }
    if c.status == "open" { return c.snoozedUntil != nil ? "later" : c.withAgent != nil ? "works" : nil }
    if c.status == "shredded" { return "trash" }
    if c.status == "decided" { return !c.answeredBy ? nil : now - min(now, c.decided ?? 0) < ACTING_MS && (online(c.agent) || c.heard == false) ? "works" : "done" }
    if c.status == "done" { return c.answeredBy ? "done" : "trash" }
    return nil
  }


  /** The status line a card with its session shows (the Desk's "Off your mind" rows): the working line that names it,
   *  else the session's newest working line that names no card. */
  public func workLine(_ c: DeskCard, working: [Task1]) -> Task1? {
    working.first { $0.cardId == c.id } ?? working.filter { $0.agent == c.agent && $0.cardId == nil }.max { $0.updated < $1.updated }
  }
  /** The card a session's status line stands for, by the Desk's own pairing (workLine): the card it names, else the
   *  newest of the session's cards with it whose line it is; nil for none. A tap on the line in the conversation opens
   *  the same card a tap on its Desk row opens. */
  public func lineCard(_ t: Task1) -> DeskCard? {
    if let cid = t.cardId { return byCard[cid] }
    guard t.state == "working" else { return nil }
    let working = tasks.filter { $0.agent == t.agent && $0.state == "working" }
    return cards.filter { $0.agent == t.agent && stackOf($0) == "works" && workLine($0, working: working)?.id == t.id }
      .max { (($0.status == "open" ? $0.withAgent : $0.decided) ?? 0) < (($1.status == "open" ? $1.withAgent : $1.decided) ?? 0) }
  }

  // ---- conversations (BoardState.messagesOf, messagesOfCard) ---------------------------------------------

  /** One session's conversation: its cards' events, its chat and its cards' chats, in hub order. */
  public func messagesOf(agent: String) -> [Message] {
    if let m = messageCache[agent] { return m }
    let m = makeMessages(agent: agent)
    messageCache[agent] = m
    return m
  }
  private func makeMessages(agent: String) -> [Message] {
    guard let dev = agentToDev[agent] else { return [] }
    var out = [Message]()
    for id in board.sessions[dev]?.cardIds ?? [] {
      guard let card = byCard[id], card.kind != "permission", let c = board.cards[id] else { continue }
      out += eventsOf(c, card)
      itemsOf(board.timelines[timelineKeyOf("chat", "card/\(id)")], agent: agent, cardId: id, into: &out)
    }
    itemsOf(board.timelines[timelineKeyOf("chat", "session/\(dev)")], agent: agent, cardId: nil, into: &out)
    return out.sorted { ($0.seq, $0.ts) < ($1.seq, $1.ts) }
  }
  /** One card's conversation: its events and its chat. */
  public func messagesOfCard(_ id: String) -> [Message] {
    if let m = cardMessageCache[id] { return m }
    let m = makeCardMessages(id)
    cardMessageCache[id] = m
    return m
  }
  private func makeCardMessages(_ id: String) -> [Message] {
    guard let card = byCard[id], card.kind != "permission", let c = board.cards[id] else { return [] }
    var out = eventsOf(c, card)
    itemsOf(board.timelines[timelineKeyOf("chat", "card/\(id)")], agent: card.agent, cardId: id, into: &out)
    return out.sorted { ($0.seq, $0.ts) < ($1.seq, $1.ts) }
  }
  /**
   * The newest thing a session's agents SAID in its chat, as an envelope number (0 for nothing): what the unread dot
   * goes by. The envelopes of a turn's trail (README "The trail") are not words to read: they are passed over, down
   * to the newest envelope that is none. An item whose body is not in memory counts (it cannot be told apart).
   */
  public func newestWord(sessionId: String) -> Int {
    guard let t = board.timelines[timelineKeyOf("chat", "session/\(sessionId)")] else { return 0 }
    let top = t.newestAgentEnvelopeNumber
    guard top > 0 else { return 0 }
    guard let first = t.items[top], isTrail(first) else { return top }
    for n in t.numbers.reversed() where n < top {
      guard let i = t.items[n] else { return n }   // (older than the window: not known, so it counts)
      if i.senderDeviceId == board.myDeviceId || humans.contains(i.senderDeviceId) || isTrail(i) { continue }
      return n
    }
    return 0
  }
  private func isTrail(_ i: TimelineItem) -> Bool { i.itemState == "loaded" && i.content?["terminal"].string == "work" }

  /** Whether older items of a conversation are on the hub than the window holds. */
  public func hasOlder(agent: String) -> Bool {
    guard let dev = agentToDev[agent] else { return false }
    let keys = [timelineKeyOf("chat", "session/\(dev)")] + (board.sessions[dev]?.cardIds ?? []).map { timelineKeyOf("chat", "card/\($0)") }
    // headers without bodies, or (after a snapshot boot) items the hub holds that are not here at all
    return keys.contains { k in board.timelines[k].map { t in hasHeaders(t) || (t.hasMore && t.items.count < t.itemCount) } ?? false }
  }
  /** Whether a timeline holds items without their bodies: scanned once per change of it (a chat screen asks on every run). */
  private var headersCache: [String: (timeline: Timeline, revision: Int, any: Bool)] = [:]
  private func hasHeaders(_ t: Timeline) -> Bool {
    if let c = headersCache[t.key], c.timeline === t, c.revision == t.revision { return c.any }
    let any = t.items.values.contains { $0.itemState == "header" }
    headersCache[t.key] = (t, t.revision, any)
    return any
  }
  /** A session's questions as asked in its chat: card id -> the id of its "asked" row (once per update). */
  public func askAt(agent: String) -> [String: String] {
    if let m = askAtCache[agent] { return m }
    let m = Dictionary(messagesOf(agent: agent).filter { $0.from == "event" && $0.kind == "asked" && $0.cardId != nil }.map { ($0.cardId!, $0.id) }, uniquingKeysWith: { $1 })
    askAtCache[agent] = m
    return m
  }
  private var askAtCache: [String: [String: String]] = [:]

  /** Whose message a chat item is, and its terminal mark. The terminal mirror (README, shared/types.ts `terminal`):
   *  an AGENT's device sends what the human typed into its terminal as an ordinary message with terminal "input",
   *  which stands as his message; "answer" is the agent's own. From a human device the field counts for nothing. */
  public static func speaker(human: Bool, content c: JV) -> (from: String, terminal: String?) {
    if human { return ("user", nil) }
    let t = c["terminal"].string
    if t == "input" { return ("user", t) }
    return ("agent", t == "answer" ? t : nil)
  }

  private func itemsOf(_ t: Timeline?, agent: String, cardId: String?, into out: inout [Message]) {
    guard let t = t else { return }
    if let c = itemsCache[t.key], c.timeline === t, c.revision == t.revision, c.agent == agent, c.cardId == cardId, c.me == board.myDeviceId, c.humans == humans,
       c.published.allSatisfy({ board.published[$0.key]?.objectState == $0.value }),
       c.notes.allSatisfy({ board.notes[$0.key]?.extra["attachments"] == $0.value }) {
      out += c.messages
      return
    }
    var built = [Message]()
    var published = [String: String?](), notes = [String: JV?]()
    buildItems(t, agent: agent, cardId: cardId, into: &built, published: &published, notes: &notes)
    itemsCache[t.key] = ItemsBuilt(timeline: t, revision: t.revision, humans: humans, me: board.myDeviceId, agent: agent, cardId: cardId, published: published, notes: notes, messages: built)
    out += built
  }
  private func buildItems(_ t: Timeline, agent: String, cardId: String?, into out: inout [Message], published seenPublished: inout [String: String?], notes seenNotes: inout [String: JV?]) {
    let me = board.myDeviceId
    // a turn's id -> its message's place in out, its envelopes, where each of them stands
    var trails = [String: (at: Int, envelopes: [JV], places: [(seq: Double, ts: UInt64)])]()
    var typed = [Double]()   // where the messages he typed into the terminal stand
    defer {
      for x in trails.values {
        out[x.at].work = Work.fold(x.envelopes)
        if let seq = Work.anchor(x.places.map { $0.seq }, typed: typed), let p = x.places.first(where: { $0.seq == seq }) { out[x.at].seq = p.seq; out[x.at].ts = p.ts }
      }
    }
    for i in t.ordered {
      if i.itemState == "header" { continue }
      let kind = i.contentType ?? "message"
      // a content type of a newer Trommi: a placeholder in its place; strokes and the like belong to a canvas
      let newer = !Compat.CONTENT_TYPES.contains(kind) || i.itemState == "newer_schema"
      if i.itemState == "loaded" && !newer && kind != "message" && kind != "selection_sent" { continue }
      let human = i.senderDeviceId == me || humans.contains(i.senderDeviceId)
      let c = i.content ?? .obj([:])
      let text: String
      switch i.itemState {
      case "loaded": text = c["text"].string ?? ""
      case "pruned": text = "(removed after 30 days)"
      case "newer_schema", "unsupported": text = ""
      default: text = ""
      }
      var msg = Message(id: i.envelopeNumber.map { "e\($0)" } ?? (i.localId ?? UUID().uuidString), seq: Double(i.envelopeNumber ?? Int.max), agent: agent, from: human ? "user" : "agent",
                        text: text, attachments: c["attachments"].array ?? [], ts: i.sentAt)
      // A turn's trail (README "The trail"): its envelopes are one message, the block of what the agent did, where
      // the first of them stands, or behind what he typed into the terminal while the turn ran (Work.anchor). Counts
      // only from an agent; an envelope that is not a trail's is left out. The message is named after the turn, so it
      // stays the same row while later envelopes and earlier pages come in and when the block moves.
      if i.itemState == "loaded" && !newer && c["terminal"].string == "work" {
        if human || !Work.isWork(c) { continue }
        let w = c["work"], turn = w["turn"].string ?? ""
        if trails[turn] != nil { trails[turn]!.envelopes.append(w); trails[turn]!.places.append((msg.seq, msg.ts)); continue }
        msg.id = "work-\(cardId ?? "")-\(turn)"; msg.from = "work"; msg.text = ""; msg.attachments = []
        msg.contentType = kind; msg.cardId = cardId
        trails[turn] = (out.count, [w], [(msg.seq, msg.ts)])
        out.append(msg)
        continue
      }
      let said = DeskModel.speaker(human: human, content: c)
      msg.from = said.from; msg.terminal = said.terminal
      if said.terminal == "input" && i.itemState == "loaded" { typed.append(msg.seq) }
      msg.itemState = newer ? "unsupported" : i.itemState
      msg.contentType = kind
      msg.cardId = cardId
      msg.details = c["details"].string
      msg.html = c["html"].string
      if let p = c["published_object_id"].string { seenPublished.updateValue(board.published[p]?.objectState, forKey: p); if board.published[p]?.objectState == "closed" { continue }; msg.published = p }
      if c["note"].object != nil {
        msg.noteWritten = .some(c["note"]["written_at"].int.map { UInt64($0) })
        // a note's pictures and files: those the message names, else the note object's own (the web shows these too)
        if msg.attachments.isEmpty, let id = c["note"]["object_id"].string {
          seenNotes.updateValue(board.notes[id]?.extra["attachments"], forKey: id)
          if let n = board.notes[id] { msg.attachments = n.extra["attachments"]?.array ?? [] }
        }
      }
      msg.handback = c["hand_back"].truthy
      msg.explain = c["explain"].truthy
      msg.present = c["present_card"].truthy
      msg.marks = c["marks"].array ?? []
      msg.copiedCards = (c["copied_cards"].array ?? []).compactMap { $0.string }
      msg.pending = i.pending
      out.append(msg)
    }
  }

  private func eventsOf(_ c: Card, _ card: DeskCard) -> [Message] {
    func ev(_ n: Double, _ kind: String, _ text: String, _ ts: UInt64) -> Message {
      var m = Message(id: "v\(n)\(kind.prefix(1))", seq: n, agent: card.agent, from: "event", text: text, ts: ts)
      m.kind = kind; m.cardId = card.id
      return m
    }
    var out = [Message]()
    // (the card's history from the client core: every revision, each with what its agent said changed)
    for v in c.history {
      let title = v.title.isEmpty ? card.title : v.title
      if v.objectVersion == 1 { out.append(ev(Double(v.envelopeNumber), card.kind == "info" ? "info" : "asked", title, v.sentAt)) }
      else { var e = ev(Double(v.envelopeNumber), "revised", v.note.isEmpty ? title : v.note, v.sentAt); e.version = v.objectVersion; out.append(e) }
    }
    func label(_ keys: [String]) -> String { keys.map { k in card.options.first { $0.key == k }?.label ?? k }.joined(separator: ", ") }
    for a in c.answers {
      let what = a.answerAction == "read" ? "read" : a.answerAction == "shred" ? "shredded" : "decided"
      let n = Double(a.envelopeNumber ?? Int.max)
      if what == "read" { out.append(ev(n, "read", card.title, a.answeredAt)) }
      else if what == "shredded" { out.append(ev(n, "shredded", [card.title, a.note ?? ""].filter { !$0.isEmpty }.joined(separator: " · "), a.answeredAt)) }
      else {
        let then = c.versions.first { $0.objectVersion == a.boundObjectVersion }?.content?["options"].array?.map(Option.init) ?? []
        let name: (String) -> String = { k in then.first { $0.key == k }?.label ?? label([k]) }
        var e = ev(n, "decided", a.trusted ? "Duck: your call\(a.choices.isEmpty ? "" : " · \(label(a.choices))")" : label(a.choices), a.answeredAt)
        e.labels = a.choices.map(name); e.optionNotes = a.optionNotes.filter { !$0.value.isEmpty }.map { (name($0.key), $0.value) }.sorted { $0.0 < $1.0 }
        e.trusted = a.trusted; e.settled = a.envelopeHash == c.answer?.envelopeHash && c.closedHow == "settled"; e.version = a.boundObjectVersion
        if let note = a.note, !note.isEmpty { e.details = note }
        e.attachments = a.attachments
        out.append(e)
      }
      if let tb = a.takenBackAt { out.append(ev(Double(tb), "reopened", card.title, a.takenBackSentAt ?? a.answeredAt + 1)) }
    }
    if c.objectState == "closed", c.closeSummary != nil || c.withdrawReason != nil {
      out.append(ev(Double(c.envelopeNumber) + 0.5, "done", c.withdrawReason.map { "Withdrawn: \($0)" } ?? c.closeSummary ?? "", c.updatedAt))
    }
    return out
  }

  // ---- notes ---------------------------------------------------------------------------------------------

  public struct NoteVM: Identifiable { public var id: String; public var text: String; public var attachments: [JV]; public var held: JV; public var created: UInt64; public var updated: UInt64; public var pending: Bool }
  public var notes: [NoteVM] {
    board.notes.values.filter { $0.objectState != "closed" && !($0.extra["removed"]?.truthy ?? false) }.map {
      NoteVM(id: $0.objectId, text: $0.text, attachments: $0.extra["attachments"]?.array ?? [], held: $0.extra["held"] ?? .null,
             created: clampedU64($0.extra["created_at"]?.double), updated: clampedU64($0.extra["updated_at"]?.double), pending: $0.pending)
    }.sorted { $0.created < $1.created }
  }

  /** The crowned session of a desk: who receives a note. */
  public func crownOf(desk: String?) -> Agent? {
    let v = view(desk: desk)
    let home = v.all ? (desks.first?.id) : v.deskId
    if let home = home { return v.here.first { $0.starred && !$0.archived && deskOf($0) == home } ?? agents.first { $0.starred && !$0.archived && deskOf($0) == home } }
    return v.here.first { $0.starred }
  }
}
