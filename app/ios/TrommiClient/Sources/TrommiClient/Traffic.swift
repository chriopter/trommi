// Traffic.swift: what this device sent to the hub and received from it, counted here (Settings → Data): the bodies
// of every request and answer and the live stream's bytes, since the app was installed or the count was reset.
// Kept in the app's own settings; it says how much, never what. And what the account keeps on the hub, as the hub
// counts it (GET /v1/account/usage, spec/v1.md 8.9).
import Foundation

public enum Traffic {
  private static let lock = NSLock()
  private static let key = "trommi-traffic"
  /** Where the totals are kept (tests use a suite of their own). */
  nonisolated(unsafe) public static var store: UserDefaults = .standard
  nonisolated(unsafe) private static var held: (down: Int64, up: Int64, since: Date)?

  private static func load() -> (down: Int64, up: Int64, since: Date) {
    if let h = held { return h }
    let d = store.dictionary(forKey: key) ?? [:]
    let since = (d["since"] as? Double).map { Date(timeIntervalSince1970: $0) } ?? Date()
    let h = ((d["down"] as? NSNumber)?.int64Value ?? 0, (d["up"] as? NSNumber)?.int64Value ?? 0, since)
    held = h
    if d["since"] == nil { write(h) }
    return h
  }
  private static func write(_ h: (down: Int64, up: Int64, since: Date)) {
    store.set(["down": NSNumber(value: h.down), "up": NSNumber(value: h.up), "since": h.since.timeIntervalSince1970], forKey: key)
  }
  /** Bytes received and sent. */
  public static func add(down: Int = 0, up: Int = 0) {
    guard down > 0 || up > 0 else { return }
    lock.withLock {
      var h = load()
      h.down += Int64(down); h.up += Int64(up)
      held = h
      write(h)
    }
  }
  /** The totals and since when they are counted. */
  public static var totals: (down: Int64, up: Int64, since: Date) { lock.withLock { load() } }
  /** Counting starts again from now. */
  public static func reset(now: Date = Date()) {
    lock.withLock { let h: (down: Int64, up: Int64, since: Date) = (0, 0, now); held = h; write(h) }
  }
  /** Forgets what is held in memory (tests that change `store`). */
  static func forget() { lock.withLock { held = nil } }
}

/** What the account keeps on the hub (GET /v1/account/usage): counts and sizes only. */
public struct AccountUsage: Equatable, Sendable {
  public var rooms = 0, groups = 0, liveGroups = 0, devices = 0, commits = 0, messages = 0, envelopes = 0
  public var cards = 0, openCards = 0, files = 0
  public var fileBytes: Int64 = 0, logBytes: Int64 = 0, envelopeBytes: Int64 = 0, totalBytes: Int64 = 0
  public init() {}
  public init(_ j: JSON) {
    func n(_ k: String) -> Int { Wire.int(j[k]) ?? 0 }
    func b(_ k: String) -> Int64 { Int64(Wire.uint(j[k]) ?? 0) }
    rooms = n("rooms"); groups = n("groups"); liveGroups = n("live_groups"); devices = n("devices"); commits = n("commits")
    messages = n("messages"); envelopes = n("envelopes"); cards = n("cards"); openCards = n("open_cards"); files = n("files")
    fileBytes = b("file_bytes"); logBytes = b("log_bytes"); envelopeBytes = b("envelope_bytes"); totalBytes = b("total_bytes")
  }
}

extension Room {
  /** What the account keeps on the hub; nil for a room without an account (`not-found`). */
  public func accountUsage() async throws -> AccountUsage? {
    do { return AccountUsage(try await noted { try await hub.request("GET", "/account/usage") }) }
    catch let e as HubError where e.code == "not-found" { return nil }
  }
}
