// LiveActivities: the bell and the questions waiting in the Dynamic Island and on the lock screen (spec/v1.md 15.3;
// the widget is Sources/TrommiLive). The hub starts, updates and ends it by push, with two counts and nothing else;
// the app hands it the tokens (POST /v1/live-activity): the push-to-start token once per room (with a random tag of
// this phone for that room, which the activity's attributes carry back), and each running activity's own token to
// the hub whose tag it carries. iOS wakes the app in the background to deliver an activity's token when the hub
// started it. The tokens go through the room the app has open (Push.model), as in Push.swift.
//
// The card is the app's: after a change of the board (at most every second), and after each push of the hub (which
// carries the counts alone), the app puts the most urgent waiting card into the running activities, opened here on
// the phone (PushNotify LiveState): its title, its session's name, its urgency and its two answers. The two buttons
// answer it through the room in this process (AnswerCardIntent, LiveAnswer.handler), as a tap on the card would; the
// activity says "Answered" for a moment (2.5 s), then shows the next card. An answer that cannot go out from here offers
// to open the card. While the phone cannot read the room (no room, protected data locked), no card goes in.
//
// Off when Push is off (Settings · Devices) and while Live Activities are off in the system's settings. The hub
// has no route that takes a token back, so with Push off no token is handed over any more, and an activity the hub
// still starts with a token it got earlier is ended here at once.
#if canImport(ActivityKit) && os(iOS)
import ActivityKit
import Foundation
import PushNotify
import TrommiClient
import UIKit
import os

private let log = Logger(subsystem: "com.trommi.ios", category: "live")

@MainActor
enum LiveActivities {
  private static var watching = false
  private static var observing = false
  private static var seen = Set<String>()
  /** The tokens this run got (kind, token, tag), to hand over again when Push is turned off or on. */
  private static var tokens: [(kind: String, token: String, tag: String?)] = []

  /** The tag of a room on this phone (8 random bytes, hex), made once. */
  static func tag(room: String) -> String {
    let k = "trommi-live-tag.\(room)"
    if let t = UserDefaults.standard.string(forKey: k) { return t }
    let t = (0..<8).map { _ in String(format: "%02x", UInt8.random(in: 0...255)) }.joined()
    UserDefaults.standard.set(t, forKey: k)
    return t
  }

  /** Start listening for tokens (at launch, also a launch in the background). */
  static func watch() {
    // turned on later in the system's settings: the tokens then
    if !observing {
      observing = true
      Task { @MainActor in for await on in ActivityAuthorizationInfo().activityEnablementUpdates where on { watch() } }
    }
    guard !watching, ActivityAuthorizationInfo().areActivitiesEnabled else { return }
    watching = true
    Task { @MainActor in
      for await data in Activity<TrommiActivityAttributes>.pushToStartTokenUpdates {
        await register(token: hex(data), kind: "start", tag: nil)
      }
    }
    Task { @MainActor in
      for a in Activity<TrommiActivityAttributes>.activities { follow(a) }
      for await a in Activity<TrommiActivityAttributes>.activityUpdates { follow(a) }
    }
  }

  private static func follow(_ a: Activity<TrommiActivityAttributes>) {
    // Push is off, but the hub still holds an earlier token and started this one: it ends at once
    if Push.level == "off" { Task { await a.end(nil, dismissalPolicy: .immediate) }; return }
    guard seen.insert(a.id).inserted else { return }
    Task { @MainActor in
      for await data in a.pushTokenUpdates {
        await register(token: hex(data), kind: "activity", tag: a.attributes.tag)
      }
    }
    // a push of the hub replaces the content with its two counts: the card again from this phone, at once (a
    // second of the count alone between made the island change its look on every push of the hub)
    Task { @MainActor in
      for await c in a.contentUpdates where c.state.card == nil && c.state.done == nil { await applyCards() }
    }
    cardsChanged()
  }

  /** start: to the room's hub with this phone's tag for it; activity: only if the activity carries that tag. */
  private static func register(token: String, kind: String, tag: String?) async {
    if !tokens.contains(where: { $0.kind == kind && $0.token == token }) { tokens.append((kind, token, tag)) }
    guard Push.level != "off", let room = Push.model?.room, Push.model?.demo != true else { return }
    let id = room.roomIdHex
    let mine = Self.tag(room: id)
    if kind == "activity" && tag != mine { return }
    let mark = "live.\(id).\(kind).\(token)"
    if UserDefaults.standard.bool(forKey: mark) { return }
    do {
      let body: [String: Any] = ["kind": kind, "token": token, "tag": mine, "environment": Push.environment, "topic": Bundle.main.bundleIdentifier ?? "com.trommi.ios"]
      _ = try await room.hub.request("POST", "/live-activity", body: body)
      UserDefaults.standard.set(true, forKey: mark)
    } catch { log.error("trommi live: \(kind, privacy: .public) token for room \(String(id.prefix(8)), privacy: .public) not registered: \(loggable(error), privacy: .public)") }
  }

  /** Push turned off or on: on hands the tokens over again; off ends what runs (the hub is not told, see above). */
  static func levelChanged() {
    for k in UserDefaults.standard.dictionaryRepresentation().keys where k.hasPrefix("live.") { UserDefaults.standard.removeObject(forKey: k) }
    if Push.level == "off" { endAll(); return }
    again()
    watch()
  }

  /**
   * The tokens of this run once more (each is sent once only): a token that arrived before the room was open, at
   * launch, is handed over when Push registers its own (Push.register).
   */
  static func again() {
    let list = tokens
    Task { @MainActor in for t in list { await register(token: t.token, kind: t.kind, tag: t.tag) } }
  }

  /** Signed out: every activity ends at once. */
  static func endAll() {
    Task { for a in Activity<TrommiActivityAttributes>.activities { await a.end(nil, dismissalPolicy: .immediate) } }
  }

  private static func hex(_ d: Data) -> String { d.map { String(format: "%02x", $0) }.joined() }

  // ---- the card (PushNotify LiveState) ----

  private static var cardsDue = false
  /** Answered from the island: not shown again, whatever the board says meanwhile (in the demo it never changes). */
  private static var answered = Set<String>()

  /** After a change of the board: the running activities' card within a second; while the room catches up (page
   *  after page), once it is through. Only a change of what the island shows is sent (`applyCards`). */
  static func cardsChanged() {
    guard !cardsDue else { return }
    cardsDue = true
    Task { @MainActor in
      try? await Task.sleep(nanoseconds: 1_000_000_000)
      while BoardModel.shared.room?.behind == true { try? await Task.sleep(nanoseconds: 1_000_000_000) }
      cardsDue = false
      await applyCards()
    }
  }

  /** Whether the phone reads the room now: a room (or the demo) and its protected data unlocked. */
  private static func readable(_ m: BoardModel) -> Bool {
    m.demo || (m.room != nil && UIApplication.shared.isProtectedDataAvailable)
  }

  /** The cards that wait for him, on every desk: open decisions and permission requests, not snoozed or shelved. */
  static func candidates(_ m: BoardModel) -> [LiveCandidate] {
    guard let d = m.desk else { return [] }
    return d.cards.filter { c in
      c.status == "open" && c.kind != "info" && c.withAgent == nil && c.snoozedUntil == nil && !answered.contains(c.id)
        && d.byAgent[c.agent]?.archived != true
    }.map { c in
      LiveCandidate(id: c.id, title: c.title, agent: d.byAgent[c.agent]?.name ?? "Agent", kind: c.kind, urgency: c.urgency, created: c.created,
                    options: c.options.map { LiveOption(key: $0.key, label: $0.label) }, multiple: c.multiple)
    }
  }

  private static func applyCards() async {
    let m = BoardModel.shared
    let list = candidates(m), canRead = readable(m)
    for a in Activity<TrommiActivityAttributes>.activities {
      let s = a.content.state
      if s.done != nil { continue }   // the moment after an answer: answer() goes on from there
      var next = LiveState.state(working: s.working, waiting: list, readable: canRead)
      if !canRead { next.waiting = s.waiting }   // the hub's count stands; nothing of a card
      if next.card?.id == s.card?.id, s.needsApp == true { next.needsApp = true }
      if next == s { continue }
      await a.update(ActivityContent(state: next, staleDate: a.content.staleDate, relevanceScore: a.content.relevanceScore))
    }
  }

  /**
   * An answer from the island (LiveAnswer.handler, set at launch): card `id`, option `key`, through the room as a tap
   * on the card would send it. A launch in the background first waits for the board (up to 10 s).
   */
  static func answer(card id: String, key: String) async -> LiveAnswerOutcome {
    let m = BoardModel.shared
    for _ in 0..<40 where m.card(id) == nil { try? await Task.sleep(nanoseconds: 250_000_000) }
    guard let c = m.card(id), c.status == "open", c.kind == "decision", let option = c.options.first(where: { $0.key == key }) else {
      return await notSent(id, "the card is not open here")
    }
    // the demo sends nothing: the island goes on as if it had
    if !m.demo {
      guard let room = m.room else { return await notSent(id, "no room") }
      do { try await room.answer(cardId: id, choices: [key]) } catch { return await notSent(id, loggable(error)) }
    }
    answered.insert(id)
    let done = LiveDone(title: LiveState.clip(c.title, LiveState.titleMax), label: LiveState.clip(option.label, LiveState.labelMax))
    let left = candidates(m).count
    for a in Activity<TrommiActivityAttributes>.activities {
      var s = a.content.state
      s.done = done; s.card = nil; s.needsApp = nil; s.waiting = left
      await a.update(ActivityContent(state: s, staleDate: a.content.staleDate, relevanceScore: a.content.relevanceScore))
    }
    try? await Task.sleep(nanoseconds: 2_500_000_000)
    // the next card in one step (no moment of the count alone in between)
    let list = candidates(m), canRead = readable(m)
    for a in Activity<TrommiActivityAttributes>.activities where a.content.state.done != nil {
      let next = LiveState.state(working: a.content.state.working, waiting: list, readable: canRead)
      await a.update(ActivityContent(state: next, staleDate: a.content.staleDate, relevanceScore: a.content.relevanceScore))
    }
    return .answered
  }

  /** Not sent: the card stays, and its button opens it. */
  private static func notSent(_ id: String, _ why: String) async -> LiveAnswerOutcome {
    log.error("trommi live: answer from the island not sent: \(why, privacy: .public)")
    for a in Activity<TrommiActivityAttributes>.activities where a.content.state.card?.id == id {
      var s = a.content.state
      s.needsApp = true
      await a.update(ActivityContent(state: s, staleDate: a.content.staleDate, relevanceScore: a.content.relevanceScore))
    }
    return .openCard
  }

  /**
   * The screenshot hook TROMMI_LIVE=1 beside TROMMI_SCREEN (the demo): an activity started here, without a hub, with
   * the demo's cards, to see and try the island in the simulator.
   */
  static func startForDemo() {
    Task { @MainActor in
      for a in Activity<TrommiActivityAttributes>.activities { await a.end(nil, dismissalPolicy: .immediate) }
      let m = BoardModel.shared
      let s = LiveState.state(working: 2, waiting: candidates(m), readable: true)
      do { _ = try Activity.request(attributes: TrommiActivityAttributes(tag: "demo"), content: ActivityContent(state: s, staleDate: nil), pushType: nil) }
      catch { log.error("trommi live: the demo's activity did not start: \(String(describing: error), privacy: .public)") }
    }
  }
}
#endif
