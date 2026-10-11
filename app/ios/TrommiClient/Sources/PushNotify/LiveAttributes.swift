// The Live Activity: the bell, the count of questions waiting and, set by the app on this phone, the most urgent
// card with its two answers (README "Live Activity"). The hub starts it with a push-to-start push
// (`attributes-type: TrommiActivityAttributes`, `attributes: { tag }`), updates and ends it with two marks only, each
// 0 or 1 (anyone working, anything waiting: Apple never learns how many, spec/v1.md 15.3);
// the type's name and its JSON keys are the wire format, so they are fixed here for the app and its widget alike.
import Foundation

/**
 * What the Live Activity shows. The hub pushes `{ working, waiting }`, each 0 or 1, and nothing else (its pushes
 * decode with the rest nil); `card`, `done`, `needsApp` and `exact` are set by the app on this phone (LiveState),
 * after it opened the cards, and never leave it except to ActivityKit.
 */
public struct LiveCounts: Codable, Hashable {
  /** The hub's mark: an agent is at work (1) or not (0). */
  public var working: Int
  /** How many questions wait: the exact count when `exact`, else the hub's mark (1: some). */
  public var waiting: Int
  /** Set by the app, which counted the waiting cards itself: `waiting` is a count, not a mark. */
  public var exact: Bool?
  /** The most urgent waiting card (LiveState.pick), or nil: the widget shows the count alone. */
  public var card: LiveCard?
  /** Just answered from the island: shown for a moment before the next card. */
  public var done: LiveDone?
  /** The answer from the island did not go out (the app could not open the room or the card): open the card. */
  public var needsApp: Bool?
  public init(working: Int, waiting: Int, card: LiveCard? = nil, done: LiveDone? = nil, needsApp: Bool? = nil, exact: Bool? = nil) {
    self.working = working; self.waiting = waiting; self.card = card; self.done = done; self.needsApp = needsApp; self.exact = exact
  }
  /** Whether `waiting` counts the questions (the app counted them), not only marks that some wait. */
  public var counted: Bool { exact == true }
  /** "3 questions waiting" when counted; from the hub's mark alone "Questions waiting". */
  public var waitingText: String {
    waiting == 0 ? "Nothing waiting" : !counted ? "Questions waiting" : waiting == 1 ? "1 question waiting" : "\(waiting) questions waiting"
  }
  /** "Agents at work" or "No agent at work": the hub says whether, never how many. */
  public var workingText: String { working > 0 ? "Agents at work" : "No agent at work" }
  /** The island's badge: the count when counted, a dot for the hub's mark. */
  public var badge: String { counted || waiting == 0 ? "\(waiting)" : "•" }
  /** How many wait beside the one shown: only when counted. */
  public var others: Int { counted ? max(0, waiting - 1) : 0 }
  /** "Agents at work · 3 questions waiting". */
  public var line: String { "\(workingText) · \(waitingText)" }
}

/** A waiting card as the island shows it: already cut to length (LiveState.card). */
public struct LiveCard: Codable, Hashable {
  public var id: String
  public var title: String
  /** The session's name. */
  public var agent: String
  /** low, normal, high, critical. */
  public var urgency: String
  /** decision, info, permission. */
  public var kind: String
  public var since: Date
  /** The two answers of a two-option decision; empty: the card opens instead. */
  public var options: [LiveOption]
  public init(id: String, title: String, agent: String, urgency: String, kind: String, since: Date, options: [LiveOption]) {
    self.id = id; self.title = title; self.agent = agent; self.urgency = urgency; self.kind = kind; self.since = since; self.options = options
  }
  /** A knock: a permission request or an urgent question (DeskCard.isKnock). */
  public var knocks: Bool { kind == "permission" || urgency == "high" || urgency == "critical" }
  public var critical: Bool { kind == "permission" || urgency == "critical" }
}

public struct LiveOption: Codable, Hashable {
  public var key: String
  public var label: String
  public init(key: String, label: String) { self.key = key; self.label = label }
}

/** "Answered · Ship it": the card's title and the answer's label. */
public struct LiveDone: Codable, Hashable {
  public var title: String
  public var label: String
  public init(title: String, label: String) { self.title = title; self.label = label }
}

#if canImport(ActivityKit) && os(iOS)   // (the framework exists on macOS, where the type is unavailable)
import ActivityKit

public struct TrommiActivityAttributes: ActivityAttributes {
  public typealias ContentState = LiveCounts
  /** A random word of this phone per room (LiveActivities in the app): which room's hub started it. */
  public var tag: String
  public init(tag: String) { self.tag = tag }
}
#endif
