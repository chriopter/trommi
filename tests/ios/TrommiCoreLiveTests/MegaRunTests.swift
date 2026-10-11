// The iOS mega run, headless: the client's engine (`Room`) with the real core against a hub that holds a BIG
// account (seeded by tests/tools/seed_account.rs --bulk N --serve DIR, which stays online as the person's other
// device: it follows the room, answers key requests and writes "while the phone is away" on request). Measures what
// the person waits for and whether the Desk ever shows a card as open that the hub has closed (a stale card):
//
//   login     password sign-in → the room is in (the app's button), then the first catch-up, emit by emit
//   warm      the room closed and opened again: the cache, then the catch-up from the cache's cursor
//   away      the room closed; the other device answers and closes cards and writes new ones; opened again
//   logout    leaveRoom
//
// Runs only when TROMMI_MEGA_HUB names the hub, with TROMMI_MEGA_ENV (the seeder's --env-out file) and
// TROMMI_MEGA_SERVE (its --serve folder); the numbers go to stdout and to TROMMI_MEGA_OUT (JSON) if set.
// tests/ios/mega/run.sh starts all of it.
#if !os(iOS)
import Foundation
import XCTest
@testable import TrommiClient
@testable import TrommiCoreLive

/** A line on stderr at once (stdout is held back until the test ends). */
func say(_ line: String) { FileHandle.standardError.write(Data((line + "\n").utf8)) }

@MainActor
final class MegaRunTests: XCTestCase {
  struct Env { let hub: String; let email: String; let password: String; let serve: URL; let out: String? }
  private var transport: [AnyClass] = []

  private func env() throws -> Env {
    let e = ProcessInfo.processInfo.environment
    guard let hub = e["TROMMI_MEGA_HUB"], !hub.isEmpty else { throw XCTSkip("TROMMI_MEGA_HUB names no hub") }
    let file = try String(contentsOfFile: try XCTUnwrap(e["TROMMI_MEGA_ENV"]), encoding: .utf8)
    var kv = [String: String]()
    for line in file.split(separator: "\n") where line.contains("=") && !line.hasPrefix("#") {
      let p = line.split(separator: "=", maxSplits: 1).map(String.init)
      kv[p[0]] = p[1].trimmingCharacters(in: CharacterSet(charactersIn: "\""))
    }
    return Env(hub: hub, email: try XCTUnwrap(kv["APPSTORE_DEMO_USER"]), password: try XCTUnwrap(kv["APPSTORE_DEMO_PASSWORD"]),
               serve: URL(fileURLWithPath: try XCTUnwrap(e["TROMMI_MEGA_SERVE"])), out: e["TROMMI_MEGA_OUT"])
  }

  override func setUp() async throws {
    Core.tools = LiveCore()
    transport = HubClient.transportForTests
    HubClient.transportForTests = []
  }
  override func tearDown() async throws { HubClient.transportForTests = transport }

  /** What one phase saw. */
  final class Watch {
    let t0 = Date()
    var truth = Set<String>()
    var firstShown: Double?      // ms to the first emit with a card on the Desk
    var matched: Double?         // ms to the first emit whose open set is the hub's (the Desk is right)
    var staleMax = 0             // the most cards shown open that the hub has closed, in one emit
    var staleEmits = 0
    var emits = 0
    var ms: Double { Date().timeIntervalSince(t0) * 1000 }
    func see(_ b: Board) {
      emits += 1
      // (what the Desk shows: the app's own model of it)
      let open = Set(DeskModel(board: b).cards.filter { $0.status == "open" && $0.kind != "permission" }.map { $0.id })
      let stale = open.subtracting(truth).count
      if stale > 0 { staleEmits += 1; staleMax = max(staleMax, stale) }
      if firstShown == nil && !open.isEmpty { firstShown = ms }
      if matched == nil && open == truth { matched = ms }
    }
    func line() -> String {
      "first card shown \(firstShown.map { "\(Int($0)) ms" } ?? "-"), Desk right \(matched.map { "\(Int($0)) ms" } ?? "never"), stale max \(staleMax) in \(staleEmits) of \(emits) emits"
    }
  }

  /** The hub's open cards (hex ids), as the Desk route lists them. */
  private func truth(_ room: Room) async throws -> Set<String> {
    let desk = try await room.hub.desk()
    return Set((desk["cards"] as? [JSON] ?? []).compactMap { ($0["object_id"] as? String).flatMap { try? unb64u($0) }.map(hex) })
  }

  private func waitFor(_ file: URL, seconds: Double) async throws -> String {
    let until = Date().addingTimeInterval(seconds)
    while Date() < until {
      if let s = try? String(contentsOf: file, encoding: .utf8) { return s }
      try await Task.sleep(nanoseconds: 300_000_000)
    }
    throw TrommiError("test", "no \(file.lastPathComponent) within \(Int(seconds)) s")
  }

  func testMegaRun() async throws {
    let e = try env()
    let base = try scratchFolder(self)
    var results = [String: Any]()
    func note(_ phase: String, _ values: [String: Any]) {
      results[phase] = values
      say("MEGA \(phase): " + values.sorted { $0.key < $1.key }.map { "\($0.key)=\($0.value)" }.joined(separator: " "))
    }
    _ = Spent.take()

    // ---- login: as the app (BoardModel.login → entered → refresh)
    var t = Date()
    guard case .joined(let room) = try await Room.signInWithPassword(hubURL: e.hub, account: e.email, password: e.password, base: base) else { return XCTFail("not joined") }
    let signedIn = Date().timeIntervalSince(t) * 1000
    let signInSpent = Spent.take()
    var w = Watch()
    w.truth = try await truth(room)
    room.onChange = { [w] _ in w.see(room.preview ?? room.board) }
    try? await room.sendDeviceRegister(name: "iPhone", platform: "ios")
    let registered = w.ms
    let report = try await room.sync()
    w.see(room.board)
    note("login", ["sign_in_ms": Int(signedIn), "register_ms": Int(registered), "first_sync_ms": Int(w.ms), "desk": w.line(), "open_at_hub": w.truth.count,
                   "envelopes": report.envelopes, "spent_sign_in": signInSpent, "spent": Spent.take()])
    // the rest of the first sign-in (past, keys) as a second sync, as the app's next refresh would
    t = Date()
    _ = try await room.sync()
    w.see(room.board)
    note("login_second_sync", ["ms": Int(Date().timeIntervalSince(t) * 1000), "desk": w.line(), "spent": Spent.take()])
    await room.saveCacheAndWait(snapshot: true)
    let roomId = room.roomIdHex
    await room.shutdown()

    // ---- warm: opened again, from the cache
    t = Date()
    var r = try Room.open(base: base, roomId: roomId)
    w = Watch(); w.truth = try await truth(r)
    r.onChange = { [w, r] _ in w.see(r.board) }
    _ = await r.restore()
    let restored = Date().timeIntervalSince(t) * 1000
    w.see(r.board)
    _ = try await r.sync()
    w.see(r.board)
    note("warm", ["restore_ms": Int(restored), "sync_ms": Int(Date().timeIntervalSince(t) * 1000), "desk": w.line(), "spent": Spent.take()])
    await r.saveCacheAndWait(snapshot: true)
    await r.shutdown()

    // ---- read back: keys came (a handover) for items that took their place without a body; the room is read again
    for fromCache in [false, true] {
      Room.readBackFromCache = fromCache
      r = try Room.open(base: base, roomId: roomId)
      _ = await r.restore()
      _ = try await r.sync()
      _ = Spent.take()
      r.notePast { $0.readBack = true }
      w = Watch(); w.truth = try await truth(r)
      r.onChange = { [w, r] _ in w.see(r.board) }
      t = Date()
      _ = try await r.sync()
      w.see(r.board)
      note(fromCache ? "read_back_new" : "read_back_old", ["ms": Int(Date().timeIntervalSince(t) * 1000), "desk": w.line(), "spent": Spent.take()])
      await r.saveCacheAndWait(snapshot: true)
      await r.shutdown()
    }
    Room.readBackFromCache = true

    // ---- away: the other device writes meanwhile
    let rounds = ProcessInfo.processInfo.environment["TROMMI_MEGA_AWAY"] ?? "60 80 3"
    let cmd = e.serve.appendingPathComponent("away-\(Int(Date().timeIntervalSince1970)).cmd")
    try rounds.write(to: cmd, atomically: true, encoding: .utf8)
    let done = try await waitFor(cmd.deletingPathExtension().appendingPathExtension("done"), seconds: 1800)
    say("MEGA away round: \(done.prefix(400))")
    t = Date()
    r = try Room.open(base: base, roomId: roomId)
    w = Watch(); w.truth = try await truth(r)
    r.onChange = { [w, r] _ in w.see(r.board) }
    _ = await r.restore()
    let restoredAway = Date().timeIntervalSince(t) * 1000
    w.see(r.board)
    let staleOnRestore = w.staleMax
    _ = try await r.sync()
    w.see(r.board)
    note("away", ["restore_ms": Int(restoredAway), "sync_ms": Int(Date().timeIntervalSince(t) * 1000), "stale_from_cache": staleOnRestore, "desk": w.line(), "spent": Spent.take()])

    // ---- logout
    t = Date()
    try await r.leaveRoom()
    note("logout", ["ms": Int(Date().timeIntervalSince(t) * 1000)])
    if let out = e.out { try JSONSerialization.data(withJSONObject: results, options: [.prettyPrinted, .sortedKeys]).write(to: URL(fileURLWithPath: out)) }
  }
}
#endif
