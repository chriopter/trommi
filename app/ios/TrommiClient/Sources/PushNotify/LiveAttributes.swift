// The Live Activity: the bell, the count of questions waiting and, set by the app on this phone, the most urgent
// card with its two answers (README "Live Activity"). The hub starts it with a push-to-start push
// (`attributes-type: TrommiActivityAttributes`, `attributes: { tag }`), updates and ends it with the two counts only;
// the type's name and its JSON keys are the wire format, so they are fixed here for the app and its widget alike.
import Foundation

/**
 * What the Live Activity shows. The hub pushes `{ working, waiting }` and nothing else (its pushes decode with the
 * rest nil); `card`, `done` and `needsApp` are set by the app on this phone (LiveState), after it opened the card,
 * and never leave it except to ActivityKit.
 */
public struct LiveCounts: Codable, Hashable {
  public var working: Int
  public var waiting: Int
  /** The most urgent waiting card (LiveState.pick), or nil: the widget shows the count alone. */
  public var card: LiveCard?
  /** Just answered from the island: shown for a moment before the next card. */
  public var done: LiveDone?
  /** The answer from the island did not go out (the app could not open the room or the card): open the card. */
  public var needsApp: Bool?
  public init(working: Int, waiting: Int, card: LiveCard? = nil, done: LiveDone? = nil, needsApp: Bool? = nil) {
    self.working = working; self.waiting = waiting; self.card = card; self.done = done; self.needsApp = needsApp
  }
  /** "2 agents working · 3 questions waiting". */
  public var line: String {
    let a = working == 1 ? "1 agent working" : "\(working) agents working"
    let q = waiting == 1 ? "1 question waiting" : "\(waiting) questions waiting"
    return "\(a) · \(q)"
  }
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
