// LiveState: what the app puts into the Live Activity after it opened the cards on this phone (LiveActivities.swift
// in the app). Without UIKit or ActivityKit, so it is tested on Linux (tests/ios/PushNotifyTests/LiveStateTests).
// The most urgent card goes first: a blocking question or a permission request, then a knock, then the rest; among
// equals the one that waits longest (the Desk's order). Titles and names are cut to what the island can show. When
// the phone cannot read the room (no keys, protected data locked), only the counts go in: no title, no name.
import Foundation

/** A waiting card as the app hands it over (from DeskCard). */
public struct LiveCandidate: Equatable {
  public var id: String
  public var title: String
  public var agent: String
  public var kind: String
  public var urgency: String
  /** ms since 1970. */
  public var created: UInt64
  public var options: [LiveOption]
  public var multiple: Bool
  public init(id: String, title: String, agent: String, kind: String, urgency: String, created: UInt64, options: [LiveOption], multiple: Bool = false) {
    self.id = id; self.title = title; self.agent = agent; self.kind = kind; self.urgency = urgency; self.created = created; self.options = options; self.multiple = multiple
  }
}

public enum LiveState {
  /** The island's lines: two lines of a title, a session's name, a button's label. */
  public static let titleMax = 90
  public static let agentMax = 24
  public static let labelMax = 18

  /** 2: blocking or a permission request, 1: a knock, 0: the rest. */
  static func rank(_ c: LiveCandidate) -> Int {
    c.kind == "permission" || c.urgency == "critical" ? 2 : c.urgency == "high" ? 1 : 0
  }

  /** The most urgent card, the one that waits longest among equals; nil when none waits. */
  public static func pick(_ list: [LiveCandidate]) -> LiveCandidate? {
    list.min { a, b in
      let ra = rank(a), rb = rank(b)
      if ra != rb { return ra > rb }
      if a.created != b.created { return a.created < b.created }
      return a.id < b.id
    }
  }

  /** One line, at most `max` characters, "…" where it was cut. */
  public static func clip(_ s: String, _ max: Int) -> String {
    let flat = s.split(whereSeparator: { $0.isWhitespace || $0.isNewline }).joined(separator: " ")
    guard flat.count > max else { return flat }
    let cut = flat.prefix(max - 1)
    // at a word's end when one is near
    if let sp = cut.lastIndex(of: " "), cut.distance(from: cut.startIndex, to: sp) >= max * 2 / 3 {
      return String(cut[..<sp]) + "…"
    }
    return String(cut) + "…"
  }

  /**
   * The card as the island shows it. The two answers only for a decision with exactly two options and one choice;
   * a permission request, a card to read and every other card open in the app instead.
   */
  public static func card(_ c: LiveCandidate) -> LiveCard {
    let two = c.kind == "decision" && !c.multiple && c.options.count == 2
    return LiveCard(id: c.id, title: clip(c.title, titleMax), agent: clip(c.agent, agentMax), urgency: c.urgency, kind: c.kind,
                    since: Date(timeIntervalSince1970: Double(c.created) / 1000),
                    options: two ? c.options.map { LiveOption(key: $0.key, label: clip($0.label, labelMax)) } : [])
  }

  /**
   * The content of the activity: the hub's count of agents working, the waiting cards counted, and the most urgent
   * one when the phone could read them (`readable`). Not readable: the counts alone, nothing of a card.
   */
  public static func state(working: Int, waiting list: [LiveCandidate], readable: Bool) -> LiveCounts {
    guard readable else { return LiveCounts(working: working, waiting: list.count) }
    return LiveCounts(working: working, waiting: list.count, card: pick(list).map(card))
  }
}
