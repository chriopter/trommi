// NotifyPrefetch: reading ahead for the app. With the ticket of a push the extension reads from the hub what the app
// would catch up on when it opens (GET /v1/push-changes, spec/v1.md 15.2: the room's changes as GET /v1/changes
// gives them to this device, up to the change that rang) and keeps those pages in the App Group's container. The app
// takes a page whose start is its own cursor instead of asking the hub for it (Room's `takeAhead`), and the core
// checks it like any page of the hub: the pages are the hub's answer, ciphertext, which only the app opens.
//
// Bounded: a few pages per push, at most `maxBytes` kept per room; a page that is not the hub's shape is not kept.
// Signed out, or the room left: the folder goes.
import Foundation

/** The pages read ahead for one room, as files `<after>-<change>.json` in one folder. */
public struct PrefetchStore {
  public let dir: URL
  public static let maxBytes = 8 << 20
  public static let maxPages = 40
  public init(dir: URL) { self.dir = dir }

  /** The folder of a room in the App Group's container; nil without an App Group. */
  public static func of(room hex: String, container: URL?) -> PrefetchStore? {
    container.map { PrefetchStore(dir: $0.appendingPathComponent("ahead", isDirectory: true).appendingPathComponent(hex, isDirectory: true)) }
  }

  struct Entry { let after: UInt64; let change: UInt64; let url: URL; let size: Int }
  func entries() -> [Entry] {
    let fm = FileManager.default
    guard let names = try? fm.contentsOfDirectory(atPath: dir.path) else { return [] }
    return names.compactMap { name -> Entry? in
      guard name.hasSuffix(".json") else { return nil }
      let parts = name.dropLast(5).split(separator: "-")
      guard parts.count == 2, let a = UInt64(parts[0]), let c = UInt64(parts[1]), c > a else { return nil }
      let url = dir.appendingPathComponent(name)
      let size = (try? fm.attributesOfItem(atPath: url.path)[.size] as? Int) ?? 0
      return Entry(after: a, change: c, url: url, size: size)
    }.sorted { $0.after < $1.after }
  }

  /** Where the pages kept so far end: the start of the next one; nil when none is kept. */
  public func end() -> UInt64? { entries().map(\.change).max() }
  /** How many bytes are kept. */
  public func bytes() -> Int { entries().reduce(0) { $0 + $1.size } }

  /** Keeps a page that starts at `after` and ends at `change`; false when it is full or the page is not one. */
  @discardableResult
  public func put(after: UInt64, change: UInt64, page: Data) -> Bool {
    guard change > after else { return false }
    let kept = entries()
    guard kept.count < Self.maxPages, kept.reduce(0, { $0 + $1.size }) + page.count <= Self.maxBytes else { return false }
    do {
      try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
      try page.write(to: dir.appendingPathComponent("\(after)-\(change).json"), options: .atomic)
      return true
    } catch { return false }
  }

  /** The page that starts at `after`, taken out of the store; pages that end at or before `after` go too. */
  public func take(after: UInt64) -> Data? {
    var found: Data?
    for e in entries() {
      if e.after == after, found == nil { found = try? Data(contentsOf: e.url); try? FileManager.default.removeItem(at: e.url) }
      else if e.change <= after { try? FileManager.default.removeItem(at: e.url) }
    }
    return found
  }

  /** The kept page that holds `change` (it starts before it and ends at or after it), read and left in place. */
  public func page(holding change: UInt64) -> Data? {
    entries().first { $0.after < change && change <= $0.change }.flatMap { try? Data(contentsOf: $0.url) }
  }

  /** The pages that end at or before `after` go: the app has read them. The page that starts there stays. */
  public func drop(upTo after: UInt64) {
    for e in entries() where e.change <= after { try? FileManager.default.removeItem(at: e.url) }
  }

  /** Everything goes (signed out, or the app read past it all). */
  public func clear() { try? FileManager.default.removeItem(at: dir) }
}

/** The extension's reading ahead for one push. `get` fetches a URL with the given headers (URLSession on the phone). */
public enum NotifyPrefetch {
  /** Pages read for one push at most, and the items of one page. */
  public static let pagesPerPush = 4
  public static let pageLimit = 100

  /**
   * Reads the pages after what is kept (or after the app's cursor) up to the change that rang, as far as `pagesPerPush`
   * and the store take them. Returns how many pages were kept. Nothing to do without a ticket, a hub or a cursor.
   */
  public static func run(_ push: ApnsPush, context: NotifyContext, store: PrefetchStore,
                         get: (URL, [String: String]) async -> Data?) async -> Int {
    guard !push.ticket.isEmpty, let hub = context.hub, let cursor = context.cursor, push.change > cursor,
          hub.hasPrefix("https://") || hub.hasPrefix("http://127.0.0.1") || hub.hasPrefix("http://localhost") else { return 0 }
    // what the app has read already is not kept; the page it reads next stays
    store.drop(upTo: cursor)
    var after = max(cursor, store.end() ?? 0)
    var kept = 0
    for _ in 0..<pagesPerPush {
      guard after < push.change, var comps = URLComponents(string: "\(hub)/v1/push-changes") else { break }
      // the ticket goes in a header, never in the URL (which proxies log)
      // (as the app reads: the live sessions alone, spec/hub-api.md point 46)
      comps.queryItems = [URLQueryItem(name: "after", value: String(after)), URLQueryItem(name: "limit", value: String(pageLimit)), URLQueryItem(name: "active", value: "1")]
      guard let url = comps.url,
            let data = await get(url, ["trommi-client": context.client ?? "ios/0.0.0", "trommi-ticket": NotifyText.b64u(push.ticket)]),
            let page = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
            page["items"] is [Any], let change = (page["change"] as? NSNumber)?.uint64Value, change > after,
            store.put(after: after, change: change, page: data) else { break }
      kept += 1
      after = change
      if (page["more"] as? Bool) != true { break }
    }
    return kept
  }

  /**
   * The one change that rang, read straight from the hub (`after` = change − 1, one item) and not kept: for when the
   * pages kept do not hold it (the store full, the app's cursor already past it). nil without a ticket or a hub.
   */
  public static func one(_ push: ApnsPush, context: NotifyContext, get: (URL, [String: String]) async -> Data?) async -> Data? {
    guard !push.ticket.isEmpty, push.change > 0, let hub = context.hub,
          hub.hasPrefix("https://") || hub.hasPrefix("http://127.0.0.1") || hub.hasPrefix("http://localhost"),
          var comps = URLComponents(string: "\(hub)/v1/push-changes") else { return nil }
    comps.queryItems = [URLQueryItem(name: "after", value: String(push.change - 1)), URLQueryItem(name: "limit", value: "1")]
    guard let url = comps.url else { return nil }
    return await get(url, ["trommi-client": context.client ?? "ios/0.0.0", "trommi-ticket": NotifyText.b64u(push.ticket)])
  }
}
