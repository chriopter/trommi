// The Notification Service Extension's logic with a fake core and a fake Keychain: the context's round trip, a push
// that opens, and every push that must leave the hub's fixed text (nil here). The extension shows no content: the
// context holds no content key, and nothing is fetched.
import XCTest
@testable import PushNotify

/** The Keychain item of the tests: bytes in memory. */
final class MemoryStorage: NotifyStorage {
  var bytes: [UInt8]?
  func read() -> [UInt8]? { bytes }
  func write(_ b: [UInt8]) throws { bytes = b }
  func clear() { bytes = nil }
}

/** A core that seals nothing: a "sealed" push is its key followed by a JSON text. It refuses, like the real one, a wrong key. */
struct FakeCore: NotifyCore {
  struct Refused: Error {}
  var broken = false

  static func sealPush(key: [UInt8], _ p: ApnsPush) -> [UInt8] {
    key + Array(try! JSONSerialization.data(withJSONObject: ["room": p.roomId, "change": p.change, "urgency": p.urgency, "ticket": p.ticket]))
  }
  func openPush(key: [UInt8], sealed: [UInt8]) throws -> ApnsPush {
    guard !broken, sealed.count > 32, Array(sealed[0..<32]) == key,
          let o = try JSONSerialization.jsonObject(with: Data(sealed[32...])) as? [String: Any] else { throw Refused() }
    return ApnsPush(roomId: bytes(o["room"]), change: (o["change"] as! NSNumber).uint64Value, urgency: (o["urgency"] as! NSNumber).uint8Value, ticket: bytes(o["ticket"]))
  }
  private func bytes(_ v: Any?) -> [UInt8] { ((v as? [NSNumber]) ?? []).map(\.uint8Value) }

  /** A "sealed" Live Activity block: the key, then the two counts as text. */
  static func sealLive(key: [UInt8], _ working: UInt64, _ waiting: UInt64) -> [UInt8] { key + Array("\(working),\(waiting)".utf8) }
  func openLive(key: [UInt8], sealed: [UInt8]) throws -> (working: UInt64, waiting: UInt64) {
    guard !broken, sealed.count > 32, Array(sealed[0..<32]) == key else { throw Refused() }
    let parts = String(decoding: sealed[32...], as: UTF8.self).split(separator: ",").compactMap { UInt64($0) }
    guard parts.count == 2 else { throw Refused() }
    return (parts[0], parts[1])
  }
}

final class PushNotifyTests: XCTestCase {
  func random(_ n: Int) -> [UInt8] { (0..<n).map { _ in UInt8.random(in: 0...255) } }
  lazy var roomId = random(32)
  lazy var pushKey = random(32)
  let ticket: [UInt8] = Array("room-device-change-expiry-mac".utf8)

  func context() -> NotifyContext { NotifyContext(pushKey: pushKey, rooms: [NotifyText.hex(roomId)]) }

  func e(ticket t: [UInt8]? = nil, key: [UInt8]? = nil, room: [UInt8]? = nil, change: UInt64 = 42, urgency: UInt8 = 1) -> String {
    NotifyText.b64u(FakeCore.sealPush(key: key ?? pushKey, ApnsPush(roomId: room ?? roomId, change: change, urgency: urgency, ticket: t ?? ticket)))
  }

  // ---- the context ----

  func testContextRoundTripThroughTheStorage() throws {
    let storage = MemoryStorage()
    let store = NotifyStore(storage: storage)
    XCTAssertNil(store.read())
    let c = context()
    try store.write(c)
    XCTAssertEqual(store.read(), c)
    XCTAssertEqual(store.read()?.holds(room: roomId), true)
    XCTAssertEqual(store.read()?.holds(room: random(32)), false)
    try store.write(c)
    store.clear()
    XCTAssertNil(storage.bytes)
    XCTAssertNil(store.read())
  }

  /// The context is the push key and the rooms' ids, and nothing else: no content key, no session, no name.
  func testTheContextHoldsNoContentKey() throws {
    let storage = MemoryStorage()
    try NotifyStore(storage: storage).write(context())
    let stored = try XCTUnwrap(try JSONSerialization.jsonObject(with: Data(try XCTUnwrap(storage.bytes))) as? [String: Any])
    XCTAssertEqual(Set(stored.keys), ["version", "pushKey", "rooms"])
    XCTAssertEqual(stored["version"] as? Int, 4)
    // with where to read ahead: the hub, the client name, the cursor; still no key of any content
    var ahead = context(); ahead.hub = "https://hub.example"; ahead.client = "ios/1"; ahead.cursor = 9
    try NotifyStore(storage: storage).write(ahead)
    let full = try XCTUnwrap(try JSONSerialization.jsonObject(with: Data(try XCTUnwrap(storage.bytes))) as? [String: Any])
    XCTAssertEqual(Set(full.keys), ["version", "pushKey", "rooms", "hub", "client", "cursor"])
    XCTAssertEqual(stored["rooms"] as? [String], [NotifyText.hex(roomId)])
  }

  /// What an older build stored (version 2, with the sessions' content keys) is not read: the extension then has no
  /// context and shows the fixed text, until the app wrote today's.
  func testAContextOfAnotherVersionIsNotRead() {
    let storage = MemoryStorage()
    let store = NotifyStore(storage: storage)
    storage.bytes = Array(#"{"version":2,"pushKey":"","rooms":[{"roomId":"00","hub":"https://hub.example","me":"00","keys":[],"sessions":{}}]}"#.utf8)
    XCTAssertNil(store.read())
    storage.bytes = Array(#"{"version":1,"pushKey":"","rooms":[]}"#.utf8)
    XCTAssertNil(store.read())
    storage.bytes = Array(#"{"version":2,"pushKey":"","rooms":[]}"#.utf8)
    XCTAssertNil(store.read())
    storage.bytes = Array(#"{"version":3,"pushKey":"","rooms":[]}"#.utf8)
    XCTAssertNil(store.read())
  }

  func testText() {
    let b = random(41)
    XCTAssertEqual(NotifyText.unb64u(NotifyText.b64u(b)), b)
    XCTAssertEqual(NotifyText.hex([0, 0x0f, 0xa0, 0xff]), "000fa0ff")
    XCTAssertEqual(NotifyText.b64u([0xfb, 0xff]), "-_8")
    XCTAssertNil(NotifyText.unb64u("+/8="))
    XCTAssertNil(NotifyText.unb64u("not base64!"))
  }

  // ---- a push ----

  /// A push sealed under this phone's key for a room it is in opens: the room, the change, the urgency. No content.
  func testAPushOpensToRoomChangeAndUrgency() {
    let push = NotifyOpen.push(e(urgency: 2), context: context(), core: FakeCore())
    XCTAssertEqual(push, ApnsPush(roomId: roomId, change: 42, urgency: 2, ticket: ticket))
    XCTAssertEqual(push.map(NotifyOpen.thread), String(NotifyText.hex(roomId).prefix(16)))
    // A push without a ticket opens the same: the ticket is not used.
    XCTAssertEqual(NotifyOpen.push(e(ticket: []), context: context(), core: FakeCore())?.change, 42)
  }

  /// Apple sees one neutral text; the phone picks the real one from what was sealed.
  func testTheTextComesFromTheSealedPart() {
    XCTAssertEqual(NotifyOpen.neutral, "Something new.")
    let open = { (u: UInt8, t: [UInt8]) in NotifyOpen.push(self.e(ticket: t, urgency: u), context: self.context(), core: FakeCore()).map(NotifyOpen.text) }
    XCTAssertEqual(open(1, ticket), "A new question.")
    XCTAssertEqual(open(0, ticket), "A new question.")
    XCTAssertEqual(open(2, ticket), "Urgent: a new question.")
    XCTAssertEqual(open(3, ticket), "Urgent: a new question.")
    XCTAssertEqual(open(2, []), "An agent lost its connection.")
  }

  /// Everything else leaves the notification as it came (nil): another key, a room this phone is not in, bytes that
  /// are no sealed push, one that is too long, a core that refuses, a context without a usable key.
  func testAPushThatDoesNotOpenGivesNothing() {
    for bad in [e(key: random(32)), e(room: random(32)), "not base64!", "", NotifyText.b64u(random(600))] {
      XCTAssertNil(NotifyOpen.push(bad, context: context(), core: FakeCore()), bad)
    }
    XCTAssertNil(NotifyOpen.push(e(), context: context(), core: FakeCore(broken: true)))
    XCTAssertNil(NotifyOpen.push(e(), context: NotifyContext(pushKey: pushKey, rooms: []), core: FakeCore()))
    XCTAssertNil(NotifyOpen.push(e(key: [1, 2, 3]), context: NotifyContext(pushKey: [1, 2, 3], rooms: [NotifyText.hex(roomId)]), core: FakeCore()))
  }

  func testGroupCandidates() {
    XCTAssertEqual(NotifyGroup.candidates(bundleID: "XTL-70CB783D.com.trommi.ios.notify", infoGroup: nil), ["group.XTL-70CB783D.com.trommi.ios", "group.com.trommi.ios"])
    XCTAssertEqual(NotifyGroup.candidates(bundleID: "com.trommi.ios.live", infoGroup: "group.x"), ["group.x", "group.com.trommi.ios"])
  }

  func testCountsLine() {
    // the hub's marks: whether, never how many
    XCTAssertEqual(LiveCounts(working: 1, waiting: 1).line, "Agents at work · Questions waiting")
    XCTAssertEqual(LiveCounts(working: 0, waiting: 0).line, "No agent at work · Nothing waiting")
    XCTAssertEqual(LiveCounts(working: 1, waiting: 1).badge, "•")
    XCTAssertEqual(LiveCounts(working: 1, waiting: 1).others, 0)
    // the app's own count
    XCTAssertEqual(LiveCounts(working: 1, waiting: 3, exact: true).line, "Agents at work · 3 questions waiting")
    XCTAssertEqual(LiveCounts(working: 0, waiting: 1, exact: true).line, "No agent at work · 1 question waiting")
    XCTAssertEqual(LiveCounts(working: 0, waiting: 3, exact: true).badge, "3")
    XCTAssertEqual(LiveCounts(working: 0, waiting: 3, exact: true).others, 2)
  }

  // ---- the Live Activity's sealed counts ----

  func testTheWidgetOpensTheSealedCounts() {
    var hub = LiveCounts(working: 1, waiting: 1)
    hub.s = NotifyText.b64u(FakeCore.sealLive(key: pushKey, 2, 5))
    let shown = hub.opened(key: pushKey, core: FakeCore())
    XCTAssertEqual(shown.working, 2); XCTAssertEqual(shown.waiting, 5); XCTAssertEqual(shown.exact, true)
    XCTAssertEqual(shown.line, "Agents at work · 5 questions waiting")
    // another key, a broken core, no key: the marks stay
    for (k, core) in [(random(32), FakeCore()), (pushKey, FakeCore(broken: true))] {
      XCTAssertEqual(hub.opened(key: k, core: core), hub)
    }
    XCTAssertEqual(hub.opened(key: nil, core: FakeCore()), hub)
    // the app's own count stands above the hub's
    var mine = LiveCounts(working: 1, waiting: 3, exact: true)
    mine.s = hub.s
    XCTAssertEqual(mine.opened(key: pushKey, core: FakeCore()).waiting, 3)
    // the hub's content-state decodes with `s`
    let decoded = try! JSONDecoder().decode(LiveCounts.self, from: Data(#"{"working":1,"waiting":1,"s":"AQID"}"#.utf8))
    XCTAssertEqual(decoded.s, "AQID")
  }

  // ---- reading ahead ----

  func tempStore() -> PrefetchStore {
    PrefetchStore(dir: FileManager.default.temporaryDirectory.appendingPathComponent("ahead-\(UUID().uuidString)", isDirectory: true))
  }

  func page(_ after: UInt64, _ change: UInt64, more: Bool) -> Data {
    let items = (after + 1...change).map { ["kind": "envelope", "change": $0] as [String: Any] }
    return try! JSONSerialization.data(withJSONObject: ["items": items, "change": change, "more": more], options: .sortedKeys)
  }

  func testTheStoreKeepsPagesByTheirStart() {
    let store = tempStore()
    defer { store.clear() }
    XCTAssertNil(store.end())
    XCTAssertTrue(store.put(after: 10, change: 20, page: page(10, 20, more: true)))
    XCTAssertTrue(store.put(after: 20, change: 25, page: page(20, 25, more: false)))
    XCTAssertFalse(store.put(after: 30, change: 30, page: Data()))
    XCTAssertEqual(store.end(), 25)
    XCTAssertNil(store.take(after: 15))
    XCTAssertEqual(store.take(after: 20), page(20, 25, more: false))
    // the page before what was taken went with it
    XCTAssertNil(store.take(after: 10))
    XCTAssertNil(store.end())
    // full: no more pages than the bound
    XCTAssertFalse(store.put(after: 0, change: 1, page: Data(count: PrefetchStore.maxBytes + 1)))
  }

  func testTheExtensionReadsAheadWithTheTicketUpToTheChangeThatRang() async {
    let store = tempStore()
    defer { store.clear() }
    let push = ApnsPush(roomId: roomId, change: 30, urgency: 1, ticket: ticket)
    var c = context(); c.hub = "https://hub.example"; c.client = "ios/9.9.9"; c.cursor = 10
    var asked = [(URL, [String: String])]()
    var top: UInt64 = 30
    let get: (URL, [String: String]) async -> Data? = { url, headers in
      asked.append((url, headers))
      let q = URLComponents(url: url, resolvingAgainstBaseURL: false)!.queryItems!
      let after = UInt64(q.first { $0.name == "after" }!.value!)!
      let to = min(after + 10, top)
      return self.page(after, to, more: to < top)
    }
    let kept = await NotifyPrefetch.run(push, context: c, store: store, get: get)
    XCTAssertEqual(kept, 2)
    XCTAssertEqual(store.end(), 30)
    XCTAssertEqual(asked.first?.0.path, "/v1/push-changes")
    XCTAssertEqual(asked.first?.1["trommi-client"], "ios/9.9.9")
    XCTAssertEqual(asked.first?.1["trommi-ticket"], NotifyText.b64u(ticket))
    XCTAssertFalse(asked.first!.0.query!.contains("ticket"))
    XCTAssertTrue(asked.first!.0.query!.contains("after=10"))
    // a second push before the app opened: the pages kept stay, reading goes on where they end
    asked = []; top = 35
    let later = await NotifyPrefetch.run(ApnsPush(roomId: roomId, change: 35, urgency: 1, ticket: ticket), context: c, store: store, get: get)
    XCTAssertEqual(later, 1)
    XCTAssertTrue(asked.first!.0.query!.contains("after=30"))
    XCTAssertEqual(store.end(), 35)
    XCTAssertNotNil(store.take(after: 10))
    // nothing more to read: the app is at the change that rang, or there is no ticket, hub or cursor
    asked = []
    var at = c; at.cursor = 30
    let none1 = await NotifyPrefetch.run(push, context: at, store: store, get: get)
    let none2 = await NotifyPrefetch.run(ApnsPush(roomId: roomId, change: 30, urgency: 1, ticket: []), context: c, store: store, get: get)
    var noHub = c; noHub.hub = nil
    let none3 = await NotifyPrefetch.run(push, context: noHub, store: store, get: get)
    var plain = c; plain.hub = "http://hub.example"
    let none4 = await NotifyPrefetch.run(push, context: plain, store: store, get: get)
    XCTAssertEqual([none1, none2, none3, none4], [0, 0, 0, 0])
    XCTAssertTrue(asked.isEmpty)
    // a page that is not the hub's shape is not kept
    let junk = tempStore()
    defer { junk.clear() }
    let bad = await NotifyPrefetch.run(push, context: c, store: junk, get: { _, _ in Data("[]".utf8) })
    XCTAssertEqual(bad, 0)
    XCTAssertNil(junk.end())
  }

  func testAContextOfVersionFourCarriesWhereToReadAhead() throws {
    let s = NotifyStore(storage: MemoryStorage())
    try s.write(NotifyContext(pushKey: pushKey, rooms: ["ab"], hub: "https://hub.trommi.com", client: "ios/1", cursor: 7))
    XCTAssertEqual(s.read()?.cursor, 7)
    XCTAssertEqual(s.read()?.hub, "https://hub.trommi.com")
  }
}
