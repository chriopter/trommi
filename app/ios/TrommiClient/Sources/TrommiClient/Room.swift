// Room.swift: one room on this device: the device (trommi-core, through Core.swift), its store, the hub, and the
// board the views read (Board.swift). This file holds what the room is and how it keeps up with the hub:
//
//   catch up   GET /v1/changes after the cursor: Commits, messages and envelopes in the hub's order, each handed to
//              the core, which checks it and keeps the keys, chains and object state; what it took goes into the
//              client core's model (`ClientModel`, the same as the web app's), whose patch the board takes.
//   live       GET /v1/stream (server-sent events) with the same items, resumed by change number.
//   outbox     everything this device wants sent is in the core's outbox, stored with the state it implies; it is
//              posted in order and the hub's answer reported back. After a crash the same bytes go out again.
//              The hub's "taken" merges no Commit of this device: the core merges it when it comes back among the
//              changes, at its place in the hub's order, so the changes are read right after such an answer.
//
// What a human does is in RoomActions.swift, devices and invites in RoomDevices.swift, signing in and recovery in
// RoomAccount.swift, the record cache in RoomCache.swift.
//
// Threads: the room is the main actor's (`@MainActor`), and so is its board: every method of it, async ones
// included, runs there, and its operations run one after the other (`serial`). (Without it an async method of the
// room ran on any thread of the pool, and the outbox pump changed the board's members while the main thread did.)
// The core is only called inside `onCore`, which runs on one queue of its own: that keeps the order of its
// operations, and a long catch-up does not hold the main thread.
import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif

/** What the room is, kept beside the state as room.json. No secret in it. */
public struct RoomRecord: Codable {
  public var hubURL: String
  public var roomId: String        // hex
  public var myDeviceId: String    // hex
  public var role: String          // "human": the app is always a human device
  public var deviceRegisterSent: Bool
  /** What is left to do for what was written before this device came (RoomPast.swift); nil: nothing. */
  public var past: PastWork? = nil
  /** A sign-in with the recovery code has session groups left to join (RoomAccount.swift `finishCodeJoin`); nil: none. */
  public var codeJoin: Bool? = nil
  /** When this device's core confirmed that it was removed from the room (ms); nil: it is in (Room.removedAt). */
  public var removedAt: UInt64? = nil
  /** The archived sessions this device opened (hex session ids, newest last, at most 20): loaded again after a read
   *  back, which reads the live sessions alone (RoomArchived.swift); nil: none. */
  public var archivedOpened: [String]? = nil
}

/** `beforeJoining`: envelopes of an epoch before this device came into their group, which it cannot take until it learned that group's past (RoomPast.swift): passed over. */
public struct SyncReport { public var envelopes = 0, opened = 0, headerOnly = 0, undecryptable = 0, voids = 0, refused = 0, beforeJoining = 0; public var warnings: [String] = [] }

/** A mark set from the core's queue and read after it. */
final class Flag: @unchecked Sendable { var on = false }

/** The hub said this app is too old to be served. */
public struct UpgradeNotice: Equatable { public var minimumVersion: String?; public var message: String }

/**
 * A device's folder: room.json (which room, which hub; no secret), the device's state (DeviceStore), the record
 * cache. In the app's own container, left out of backups. The folder's name is random and never changes: the keys in
 * the Keychain are named by it, and a room gets its id only from its founding, after the state exists.
 */
public struct Store {
  public let dir: URL
  init(dir: URL) { self.dir = dir }
  /** The folder of the room this device is in; nil when it is in no such room. */
  public init?(base: URL, roomId: String) {
    guard let d = Store.folders(base).first(where: { (try? Store(dir: $0).load())?.roomId == roomId }) else { return nil }
    dir = d
  }
  /** A new, empty folder. */
  static func new(base: URL) throws -> Store { Store(dir: base.appendingPathComponent("d-\(hex(try LocalKey.random(8)))", isDirectory: true)) }
  public static func defaultBase() -> URL {
    if let x = ProcessInfo.processInfo.environment["TROMMI_SWIFT_HOME"] { return URL(fileURLWithPath: x, isDirectory: true) }
    #if os(Linux)
    return FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".local/share/trommi-swift", isDirectory: true)
    #else
    return FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("trommi", isDirectory: true)
    #endif
  }
  static func folders(_ base: URL) -> [URL] {
    ((try? FileManager.default.contentsOfDirectory(atPath: base.path)) ?? []).filter { $0.hasPrefix("d-") }.sorted().map { base.appendingPathComponent($0, isDirectory: true) }
  }
  /** The rooms this device is in (hex ids). */
  public static func rooms(base: URL) -> [String] { folders(base).compactMap { (try? Store(dir: $0).load())?.roomId }.sorted() }
  var stateDir: URL { dir.appendingPathComponent("state", isDirectory: true) }
  var hasRecord: Bool { FileManager.default.fileExists(atPath: dir.appendingPathComponent("room.json").path) }
  /** Forgets the device: its folder and its items in the Keychain. Only its owner calls this, with the state closed. */
  public func wipe() {
    try? FileManager.default.removeItem(at: dir)
    LocalKey.wipe(dir: dir)
  }
  public func save(_ r: RoomRecord) throws {
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
    #if os(iOS) || os(macOS)
    var values = URLResourceValues(); values.isExcludedFromBackup = true
    var d = dir; try d.setResourceValues(values)
    #endif
    try JSONEncoder().encode(r).write(to: dir.appendingPathComponent("room.json"), options: .atomic)
  }
  /**
   * A sign-in with the recovery code whose room join the hub may have taken without its answer arriving: the room
   * (hex) it was for. Such a folder is kept (never swept) until the next sign-in to that room settles it.
   */
  var unsureJoin: String? {
    (try? Data(contentsOf: dir.appendingPathComponent("join-unsure"))).map { String(decoding: $0, as: UTF8.self) }
  }
  func markUnsureJoin(_ roomId: String?) {
    let file = dir.appendingPathComponent("join-unsure")
    if let id = roomId { try? Data(id.utf8).write(to: file, options: .atomic) } else { try? FileManager.default.removeItem(at: file) }
  }
  public func load() throws -> RoomRecord { try JSONDecoder().decode(RoomRecord.self, from: Data(contentsOf: dir.appendingPathComponent("room.json"))) }
  /**
   * The device's state under its lock; throws `StoreError.failed("busy")` when it is open already. The lock is
   * taken before the key is asked for, so two openers never both make a key. `create`: a new device.
   */
  func openState(create: Bool = false) throws -> DeviceStore {
    let fresh = create && !FileManager.default.fileExists(atPath: stateDir.path)
    // A throwaway key until the lock is held; the store is then opened again with the real one.
    let probe = try DeviceStore(directory: stateDir, key: Bytes(repeating: 0, count: 32))
    let key: Bytes
    do { key = try LocalKey.get("state", dir: dir, create: fresh) } catch { probe.close(); throw error }
    return try probe.rekeyed(key, anchor: LocalAnchor(dir: dir))
  }
  /** The key of the record cache and the share links: lost, it is made again and the cache starts empty. */
  func cacheKey() throws -> Bytes { try LocalKey.get("cache", dir: dir, create: true) }

  /**
   * Folders a join or a founding left behind without a room (the app was ended in between): removed, unless
   * someone holds their lock. Called under `lifecycle`.
   */
  static func sweep(base: URL) {
    for d in folders(base) {
      let s = Store(dir: d)
      guard !s.hasRecord, s.unsureJoin == nil, let state = try? DeviceStore(directory: s.stateDir, key: Bytes(repeating: 0, count: 32)) else { continue }
      state.close()
      s.wipe()
    }
  }
  /** Creating, abandoning and removing device folders happen one at a time in this process. */
  static let lifecycle = NSLock()
}

@MainActor public final class Room {
  public let store: Store
  public var record: RoomRecord
  public let hub: HubClient
  /** The board: cards, sessions, conversations, registers, notes (Board.swift). */
  public let board = Board()
  /** The room's change number up to which the board holds everything. */
  public internal(set) var cursor: UInt64 = 0
  public internal(set) var upgrade: UpgradeNotice?
  /** Called after every batch that changed the board (on the main actor). */
  public var onChange: ((Change) -> Void)?
  /**
   * A page of GET /v1/changes read ahead by the notification extension (PushNotify NotifyPrefetch), taken by its
   * start: the hub's answer for exactly that cursor, which the core checks like any page. nil: ask the hub.
   */
  public var takeAhead: ((UInt64) -> JSON?)?
  /** Live: the stream is open. */
  public internal(set) var live = false
  /**
   * When this device learned that it was removed from the room (ms); nil: it is in. Set only on the word of the
   * room group's own Commits, which the device checked itself (`checkRemoval`), never on the hub's word alone. The
   * room then sends and reads nothing more; forgetting what is stored here is the app's act, after it said so.
   */
  public internal(set) var removedAt: UInt64?
  /** DELETE /v1/account was sent from here and its answer may have been lost (Account.swift deleteAccount). */
  var deletionSent = false
  /** A catch-up behind the hub's Desk: from which cursor to which (deskFirst); nil once it is through. */
  public internal(set) var catchingUp: (from: UInt64, to: UInt64)?
  /** How far that catch-up is, 0…1; nil when none runs. */
  public var catchUpShare: Double? {
    guard let c = catchingUp, c.to > c.from else { return nil }
    return min(1, Double(cursor > c.from ? cursor - c.from : 0) / Double(c.to - c.from))
  }
  /** How far the reading back of the room's past is (RoomPast.swift): change numbers read of all it reads; nil while
   *  none runs. A sign-in reads it behind the open cards, and the Desk says so: "Reading your history: 12,400 of 27,000". */
  public var readingProgress: (done: UInt64, total: UInt64)? {
    guard readBackView != nil, let c = catchingUp, c.to > c.from else { return nil }
    return (min(max(cursor, c.from), c.to) - c.from, c.to - c.from)
  }
  /** Not caught up yet: no catch-up went through in this run, or one is behind the hub's Desk. */
  public var behind: Bool { !synced || catchingUp != nil }
  /** Whether the board came from the cache (shown before the first catch-up). */
  public internal(set) var restored = false
  /** A launch from the cache: the app's first paint of the Desk waits for the hub's Desk (RoomCache.swift holdDesk). */
  public internal(set) var deskHolding = false
  var deskHoldTask: Task<Void, Never>?
  var deskWaiters: [CheckedContinuation<Void, Never>] = []

  public let roomId: RoomId
  public var roomIdHex: String { record.roomId }
  public var deviceIdHex: String { record.myDeviceId }
  public var hubURL: String { record.hubURL }
  /** What the views show of the room group: its epoch. */
  public internal(set) var state: (epoch: Int, humans: Int) = (0, 0)

  let device: CoreDevice
  let deviceStore: DeviceStore
  private let coreQueue = DispatchQueue(label: "trommi.core")
  /** The groups as the core last told them, by group id (hex). */
  var groups: [String: GroupSummary] = [:]
  var roomGroup: GroupId?
  /** The hub's last word on how many of this device's KeyPackages are unused; nil before it said so in this run. */
  var keyPackagesAtHub: Int?
  /**
   * The session groups a sign-in with the code joins after the device is in (`finishCodeJoin`), run behind the
   * sign-in so that a big account opens at once; the first catch-up waits for it. nil: none under way.
   */
  var codeJoinWork: Task<Void, Never>?
  /** The session groups (hex) the code's joins of a sign-in left out (`finishCodeJoin`): a Welcome of one asks for its
   *  keys at once (`takeWelcomes`). */
  var codeJoinMissed = Set<String>()
  /** 7.1.1: when the hub's list of wishes was last read, and the wishes for keys answered in this run. */
  var keyWishesReadAt: UInt64 = 0
  var keyWishesAnswered = Set<Int>()
  /** When `tendKeys` last ran from the stream's pings. */
  var keyTendedAt: UInt64 = 0
  /** Which session a message of a group belongs to: group id (hex) to session id (hex). */
  var sessionIdOfGroup: [String: String] = [:]
  /** Findings about a sender's chain that were shown in this run ("<group>/<sender>/<code>"): each is said once. */
  var findingsSaid = Set<String>()
  var synced = false
  var queueTail: Task<Void, Never>?
  var echoSeq = 0
  /** Records applied since launch, by change number: what the cache appends at its next save. */
  var log: [UInt64: Rec] = [:]
  /** The room's recovery HPKE key as the core last said it (its fingerprint stands in Settings → Security). */
  var recoveryKeyNow: Bytes?
  /** What this device sealed and has not seen back yet, by envelope hash (hex): the local id of its echo. */
  var ownEchoes: [String: String] = [:]
  /** What the hub said to an envelope this device sealed, by outbox id. */
  var outcome: [UInt64: Result<Void, HubError>] = [:]
  /** Own Commits (outbox ids) the core dropped because another device's Commit took their epoch. */
  var supersededIds = Set<UInt64>()
  /**
   * The core dropped an own Commit of this device: another device's took the epoch first (`superseded`). What it
   * was for did not happen: its caller is told so, whatever the hub answered (its answer may come later).
   */
  func lostToAnother(_ id: UInt64) {
    supersededIds.insert(id)
    outcome[id] = .failure(HubError(status: 409, code: "epoch-taken", message: "another device changed the group first: this change was not made"))
  }
  var pumping = false
  /** The invites whose steps run now (hex invite ids): `finishInvite` from "They match", or the upkeep's round
   *  (`invitesSoon`). One runner per invite: two would claim two KeyPackages and make two Commits for one step. */
  var invitesAtWork = Set<String>()
  /** The upkeep's next round of invite steps, while one is planned; its pause grows while steps stay open. */
  var invitesPlanned: Task<Void, Never>?
  var invitesPause: UInt64 = 2_000_000_000
  /** How long an agent without a session waits for its inviter before any human device founds its session (the
   *  web's heal_delay * 10). */
  var seatGraceMs: UInt64 = 30_000
  /** The changes are being read again from the start (RoomPast.swift): an envelope below the core's cursor is handed in as the next of its chain. */
  var readingBack = false
  /** The archived sessions loaded in this run (hex session ids): RoomArchived.swift. */
  var archivedLoaded = Set<String>()
  /** While reading back: what this device holds whole already, by change number; put back without the core. */
  var readBackWhole: [UInt64: Rec] = [:]
  /** Something came that the past waits for (a Welcome, keys): `tendPast` is due after what is being processed. */
  var pastDue = false
  /** A fresh sign-in with the code that took the hub's order from its own join (RoomAccount.swift settleCodeJoin):
   *  its first sync shows the open state, the stream opens beside it, and the room's past is read after it, in a
   *  job of its own (`readPastBehind`). */
  var pastBehind = false
  /** Session groups were joined while the past waits (`pastBehind`): the next Desk reading is the full one, read into
   *  the board (their open cards' versions, their names), not the brief list. */
  var deskFullDue = false
  /**
   * While the changes are read back (RoomPast.swift tendPast): the board as it stood before, which the views show
   * until the read is through, so that the Desk never stands empty meanwhile. nil: the views show the board.
   */
  public internal(set) var readBackView: Board?
  /** What the views show: the board as it stood while a read back builds it anew, a fresh sign-in's preview, or the board. */
  public var shownBoard: Board { readBackView ?? preview ?? board }
  /** The cache on disk is left as it is: a read back builds the board anew, and the cache of before stays until it
   *  is through (a launch meanwhile starts warm from it and reads back again). */
  var cacheHeld = false
  /** The bytes of waiting items kept in this pass (RoomPast.swift noteWaiting), and whether one was not. */
  var waitingBytes = 0
  var waitingDropped = false
  var waitingTail: Task<Void, Never>?
  /** The role the hub gave this run's token for (`catchUp`); nil: not signed in in this run. */
  var signedInAs: String?
  /** Keys came while the past was being read back (7.1.1): read back once more when that read ends. */
  var readBackAgain = false
  /** The chains (group/sender, hex) whose gap this run filled once already (`fillGap`). */
  var gapsFilled = Set<String>()
  /** Counts the times the board was built anew from the hub's changes (a read-back, after a Cut): a Scribble Board
   *  on screen loads itself again when it changed. */
  public internal(set) var boardsReset = 0
  /** The highest lamport this device has seen or written (9.3.2): what an echo of an own write is ordered by. */
  var lamport = 0
  // the record cache (RoomCache.swift)
  var recordStoreMemo: RecordStore?
  var dirtyRecs = Set<UInt64>()
  var cacheDirty = false
  var cacheTask: Task<Void, Never>?
  var saveTail: Task<Void, Never>?
  var restoring: Task<Bool, Never>?
  var restoredCount = 0
  var sinceSnapshot = 0
  // what a human does (RoomActions.swift)
  var noteHeads: [String: (version: Int, hash: String, content: [String: JV])] = [:]
  var attachmentCache: [String: Bytes] = [:]
  var attachmentOrder: [String] = []
  var shareStoreMemo: ShareStore?

  // ---- opening -------------------------------------------------------------------------------------------

  /** A room that is on this device already. Fails with `busy` when it is open elsewhere in this process. */
  public static func open(base: URL = Store.defaultBase(), roomId: String) throws -> Room {
    guard let store = Store(base: base, roomId: roomId) else { throw TrommiError("not-found", "this device is not in that room") }
    let record = try store.load()
    let state = try store.openState()
    do { return try Room(store: store, record: record, deviceStore: state, device: try Core.tools.openDevice(store: state)) }
    catch { state.close(); throw error }
  }

  init(store: Store, record: RoomRecord, deviceStore: DeviceStore, device: CoreDevice) throws {
    guard let room = try? unhex(record.roomId), room.count == 32 else { throw TrommiError("bad-argument", "room.json names no room") }
    guard hex(device.id) == record.myDeviceId else { throw TrommiError("bad-device", "the stored device does not belong to this room record") }
    if let r = device.room, r != room { throw TrommiError("wrong-room", "the stored device is in another room") }
    self.store = store; self.record = record; self.deviceStore = deviceStore; self.device = device
    roomId = room
    coreCursor = device.cursor
    hub = try HubClient(hubURL: record.hubURL, room: room)
    hub.signer = QueueSigner(id: device.id) { [coreQueue] room, hub, challenge in try coreQueue.sync { try device.signHubAuth(room: room, hub: hub, challenge: challenge) } }
    board.roomId = record.roomId; board.hubURL = record.hubURL; board.myDeviceId = record.myDeviceId; board.myRole = record.role
    removedAt = record.removedAt
    var ch = Change()
    recoveryKeyNow = (try? device.recoveryHpkeKey()) ?? nil
    try applyGroups(try device.groups(), &ch)
  }

  /**
   * Stops the room and gives the state's lock back (signing out, tests). First nothing new is taken (every later
   * call into the core fails with `closed`, the outbox pump and the timers end, the hub gets no more signatures),
   * then the lock is released. The room is unusable afterwards.
   */
  public func close() {
    closed = true
    liveTask?.cancel(); goalsTask?.cancel(); cacheTask?.cancel(); invitesPlanned?.cancel(); wakeTask?.cancel()
    hub.signer = nil
    hub.shutdown()
    // The core first (it wipes what it holds in memory and lets go of the store), then the store's lock.
    coreQueue.sync { device.close(); deviceStore.close() }
  }
  /** `close`, after what is running (queued operations, a cache save) has ended. */
  public func shutdown() async {
    closed = true
    // (what waits on the hub ends now, not after its own timeouts: the queued work then fails with `closed`)
    liveTask?.cancel(); invitesPlanned?.cancel(); wakeTask?.cancel()
    hub.shutdown()
    // (queued work gets five seconds to end; after that, what still runs can no longer reach the core: `closed`)
    let queued = queueTail, saving = saveTail
    await Room.waitAtMost(5) { _ = await queued?.value; await saving?.value }
    close()
  }
  public private(set) var closed = false
  /** Runs `op` and returns when it ended or after `seconds`, whichever comes first; `op` is not waited for beyond. */
  static func waitAtMost(_ seconds: Double, _ op: @escaping @MainActor () async -> Void) async {
    final class Once: @unchecked Sendable { let lock = NSLock(); var done = false; func first() -> Bool { lock.withLock { defer { done = true }; return !done } } }
    let once = Once()
    await withCheckedContinuation { (c: CheckedContinuation<Void, Never>) in
      Task { @MainActor in await op(); if once.first() { c.resume() } }
      Task { try? await Task.sleep(nanoseconds: UInt64(seconds * 1e9)); if once.first() { c.resume() } }
    }
  }
  /** The device's own cursor as last seen: what the core has processed, which the board's cache may lag behind. */
  var coreCursor: UInt64 = 0
  var liveTask: Task<Void, Never>?

  // ---- one after the other -------------------------------------------------------------------------------

  /** Runs strictly after everything queued before (catch-up, live items, sends). */
  func serial<T>(_ op: @escaping @MainActor () async throws -> T) async throws -> T {
    let prev = queueTail
    let t = Task { @MainActor () throws -> T in
      _ = await prev?.value
      return try await op()
    }
    queueTail = Task { _ = try? await t.value }
    return try await t.value
  }

  /** The one place the core is called from once the room is open. Only inside `serial`. */
  func onCore<T>(_ body: @escaping (CoreDevice) throws -> T) async throws -> T {
    guard !closed else { throw TrommiError("closed", "this room was closed") }
    let device = self.device
    return try await withCheckedThrowingContinuation { (cont: CheckedContinuation<T, Error>) in
      coreQueue.async { cont.resume(with: Result { try body(device) }) }
    }
  }

  func emit(_ c: Change) {
    // (what the client core's model changed since its last patch goes with it)
    var c = c
    board.project(&c)
    wakeLater()
    if c.isEmpty { return }
    Spent.count("emits")
    onChange?(c)
    // (a desk, a session's settings, the sessions or the stream changed: the goals handed to the agents may be behind)
    if c.room || !c.sessions.isEmpty || c.registers.contains(where: { $0.hasPrefix("desk/") || $0.hasPrefix("session/") }) { goalsSoon() }
    if !c.sessions.isEmpty || !c.registers.isEmpty { vouchSoon() }
    // (an agent came into the room or came online: one may wait for its session, `seatAgents`)
    if c.members && synced { invitesSoon() }
  }

  /** A hub error that says this app is too old: remembered (the app shows it), and rethrown. */
  func noted<T>(_ op: () async throws -> T) async throws -> T {
    do { return try await op() }
    catch let e as HubError where e.status == 426 || e.code == "client-too-old" {
      upgrade = UpgradeNotice(minimumVersion: e.extra["minimum_version"] as? String, message: e.message)
      var ch = Change(); ch.room = true; emit(ch)
      throw e
    }
  }

  // ---- desk goals for the agents (spec/v1.md 9.3.4, Goals.swift) -----------------------------------------

  var goalsTask: Task<Void, Never>?
  /** The emit at the client core's next wake (Board.wakeAt: a snooze ends, a request runs out, a session falls asleep,
   *  a red hand after its grace): the core decides it at its clock, the app only asks again then. */
  var wakeTask: Task<Void, Never>?
  var wakeTaskAt: UInt64?
  func wakeLater() {
    let at = board.wakeAt
    if at == wakeTaskAt { return }
    wakeTask?.cancel(); wakeTask = nil; wakeTaskAt = at
    guard let at = at, !closed else { return }
    let now = nowMs(), wait = at > now ? at - now : 0
    wakeTask = Task { @MainActor [weak self] in
      try? await Task.sleep(nanoseconds: (min(wait, 86_400_000) + 50) * 1_000_000)
      guard let self = self, !Task.isCancelled, !self.closed else { return }
      self.wakeTask = nil; self.wakeTaskAt = nil
      self.emit(Change())
    }
  }
  /** Self-join (spec/v1.md 12.4, SelfJoin.swift): the look at the agents' asks that is due, and the asks answered. */
  var vouchTask: Task<Void, Never>?
  var vouchAnswered = Set<String>()
  /** The asks waiting for the person's tap, as the app shows them ("Allow new Claude session …?"). */
  public internal(set) var vouchAsks: [SelfJoinPlan.Ask] = []
  /** This device's vouched links not through yet (nonce → account), counted against the cap; devices being removed. */
  var vouchPending: [String: String] = [:]
  var vouchRetiring = Set<String>()
  /** What this device last wrote to `goals` of a session, in this run. */
  private var goalsSaid: [String: JV] = [:]
  private func goalsSoon() {
    guard goalsTask == nil else { return }
    goalsTask = Task { @MainActor [weak self] in
      try? await Task.sleep(nanoseconds: 400_000_000)
      guard let self = self else { return }
      self.goalsTask = nil
      _ = await self.syncGoals()
    }
  }
  /** Whether this device is a leaf of that session's group now (it can write there). */
  func holdsSession(_ sid: String) -> Bool { sessionGroup(sid) != nil }
  /**
   * Keeps `goals` in each live main session's group equal to what its Desk says (9.3.4), writing where it differs.
   * Only while live; how many were written. Runs by itself 400 ms after a change of a desk, a session's settings or
   * the sessions (also a Commit that added an agent device).
   */
  @discardableResult public func syncGoals() async -> Int {
    guard live, upgrade == nil else { return 0 }
    var n = 0
    for w in GoalsSync.toWrite(board, holdsKey: holdsSession, said: goalsSaid) {
      do {
        try await setRegisters(["goals": w.value], sessionId: w.sessionId)
        goalsSaid[w.sessionId] = w.value
        n += 1
      } catch { continue }
    }
    return n
  }

  // ---- groups: who is in the room and which sessions there are -------------------------------------------

  /** The session group of a session (hex id), if this device is in it and it is not archived. */
  func sessionGroup(_ sid: String) -> GroupSummary? {
    groups.values.first { $0.session.map { hex($0.session) } == sid && !$0.archived }
  }
  /** The members and sessions of the board from the groups the core holds. No network: the core verified them. */
  func applyGroups(_ list: [GroupSummary], _ change: inout Change, into other: Board? = nil) throws {
    let board = other ?? self.board
    groups = Dictionary(list.map { (hex($0.group), $0) }, uniquingKeysWith: { a, _ in a })
    guard let room = list.first(where: { $0.session == nil }) else { return }   // not in a room yet (founding, joining)
    roomGroup = room.group
    let humans = room.leaves.map(hex)
    state = (Int(room.epoch), humans.count)
    for g in list { if let s = g.session { sessionIdOfGroup[hex(g.group)] = hex(s.session) } }
    // (the enrolled agent devices: those the session groups name; every other leaf that is no human device is a helper)
    var enrolled = [String]()
    for d in list.flatMap({ $0.session?.agents ?? [] }).map(hex) where !enrolled.contains(d) { enrolled.append(d) }
    board.applyGroups(list, humans: humans, agents: enrolled, epoch: room.epoch, recoveryHpkeKey: recoveryKeyNow)
    change.room = true; change.members = true
    board.project(&change)
  }
  func refreshGroups(_ change: inout Change) async throws {
    let (list, recovery): ([GroupSummary], Bytes?) = try await onCore { d in (try d.groups(), (try? d.recoveryHpkeKey()) ?? nil) }
    recoveryKeyNow = recovery
    try applyGroups(list, &change)
    await notifyKeysIfMoved(list)
  }

  /**
   * Called with the session groups' current content keys whenever one of them moved to another epoch (or came or
   * went), and once after opening: the app hands them to the notification extension (NotifyBridge). Only then are
   * the keys asked of the core.
   */
  public var notifyKeysChanged: (([CoreNotifyKey]) -> Void)?
  private var notifyEpochs: [String]?
  /** A refusal of the core here costs only the title in a notification: it never stops a catch-up. */
  private func notifyKeysIfMoved(_ list: [GroupSummary]) async {
    guard let hook = notifyKeysChanged else { return }
    let epochs = list.filter { $0.session != nil }.map { "\(hex($0.group))@\($0.epoch)" }.sorted()
    guard epochs != notifyEpochs, let keys = try? await onCore({ try $0.notifyKeys() }) else { return }
    notifyEpochs = epochs
    hook(keys)
  }

  // ---- catching up ---------------------------------------------------------------------------------------

  /** Sign in, join what this device was welcomed to, and read everything after the cursor in the hub's order. */
  @discardableResult public func sync() async throws -> SyncReport {
    try await serial { [self] in try await self.catchUp() }
  }

  func catchUp() async throws -> SyncReport {
    // the cache is read while the hub is asked
    let restoring: Task<Bool, Never>? = cursor == 0 ? Task { @MainActor in await self.restore() } : nil
    var report = SyncReport()
    // (a sync that ends before the hub's Desk answered, offline: the list kept from the last launch decides)
    defer { deskAnswered() }
    if removedAt != nil { _ = await restoring?.value; throw TrommiError("removed", "this device was removed from the room") }
    // A token is not membership: the hub names the role it gave it for. A removed device gets one that reads the
    // proof of its removal and nothing else (spec/hub-api.md point 42).
    let role: String
    // A token of this run that is good for two minutes more is used as it is (the web keeps its token too): a removed
    // device's token is answered `not-member` (spec/hub-api.md point 8), and then it signs in again at once below.
    let kept = signedInAs != nil && hub.holdsToken(validFor: 120_000)
    if kept { role = signedInAs!; Spent.count("sign-in kept") }
    else {
      do { role = try await noted { try await hub.signIn() } } catch { _ = await restoring?.value; throw error }
      signedInAs = role == "removed" ? nil : role
    }
    _ = await restoring?.value
    if role == "removed" {
      try await checkRemoval()
      throw TrommiError("removed", "this device was removed from the room")
    }
    var change = Change()
    // (a device back after a while: what was settled elsewhere leaves the Desk before the long history is read)
    let groupsAtDesk = sessionGroupIds
    // (the Desk is read alongside the Welcomes, and taken once they are in)
    let deskRead = Task { @MainActor in await self.fetchDesk(whole: false) }
    var until: UInt64?
    do { try await Spent.time("welcomes") { try await takeWelcomes(&change) } } catch {
      deskRead.cancel(); _ = await deskRead.value
      // (the kept token is not good any more: signed in again, and the hub says what this device is now)
      if kept, let h = error as? HubError, h.code == "not-member" || h.status == 401 { hub.forgetToken(); signedInAs = nil; return try await catchUp() }
      throw error
    }
    until = await Spent.time("desk first") { await applyDesk(await deskRead.value) }
    let joiningByCode = record.codeJoin == true
    if let work = codeJoinWork { await Spent.time("wait session joins") { await work.value }; codeJoinWork = nil }
    await Spent.time("finish code join") { await finishCodeJoin() }
    // A Welcome that came while the code's joins ran (another human device added this one first, and the join of that
    // session was refused as `busy` or lost its epoch) is taken now, not at the next sync; then the joins once more.
    if joiningByCode && (record.codeJoin == true || !codeJoinMissed.isEmpty) {
      let before = sessionGroupIds
      try? await takeWelcomes(&change)
      if record.codeJoin == true && sessionGroupIds != before { await finishCodeJoin() }
    }
    // The hub's Desk lists only what lies in the groups this device is in: one read before a session group was joined
    // (a sign-in's joins, a Welcome) would hold that session's open cards back as settled elsewhere. Read it again
    // (the web client's `deskSoon` on `groups`).
    if sessionGroupIds != groupsAtDesk {
      if record.past?.readBack == true && cursor > 0 { deskFullDue = true }
      until = await Spent.time("desk again") { await deskFirst(whole: true) }
    }
    // KeyPackages before the catch-up: a device back after long, or just signed in, may hold none at the hub, and
    // every session founded meanwhile leaves it out until it has (5.2.5); a long catch-up must not keep them back.
    // (upkeep: a failure here does not keep the board from showing; the end of this sync tries again)
    if (try? await publishKeyPackages()) != nil { pumpOutbox() }
    try await Spent.time("read changes") { try await readChanges(&report, &change) }
    if let until, cursor >= until { caughtUp(&change) }
    // (sessions archived while this device was away: held as archived, loaded only when opened; upkeep)
    await learnArchived(&change)
    // (the log said that this device was removed: nothing more is written or read)
    if removedAt != nil { emit(change); throw TrommiError("removed", "this device was removed from the room") }
    // (what was written before this device came is upkeep too: what is left of it is noted and goes on next time;
    //  a fresh sign-in reads it after this sync, in a job of its own: the Desk stands meanwhile)
    if pastBehind { pastBehind = false; Task { @MainActor [weak self] in await self?.readPastBehind() } }
    else { try? await Spent.time("past") { try await tendPast(&report, &change) } }
    // (stocking KeyPackages and writing `heads` are upkeep: a failure there does not keep the board from showing;
    // the next sync tries again. The KeyPackages again: what the catch-up's Welcomes used up)
    let upkeep = Spent.now
    try? await publishKeyPackages()
    await writeHeads()
    await tendKeys()
    await healSessions()
    // (the steps of the invites this device confirmed, left from a "They match" that could not finish or from an
    // earlier run: after this sync, outside its turn, since each step waits for the hub's word)
    invitesSoon(0)
    Spent.add("upkeep", since: upkeep)
    synced = true
    if preview != nil { preview = nil; change.room = true }
    Task { await self.refreshPresence() }
    board.project(&change)
    change.stack = true
    emit(change)
    pumpOutbox()
    return report
  }

  /**
   * A start behind the hub (a device back after a while, or one just signed in): before the history since the
   * cursor is read, which can be long, the hub's Desk says what is open now (the web client's `deskFirst`). Until the
   * catch-up reaches the Desk's cursor, a card or permission request this device holds as open that the Desk does not
   * list was settled elsewhere and is held back (the client core's Desk, `Board.setDeskListing`): no old question stands on the Desk as if it
   * waited. Returns that cursor; nil when the Desk was not had or is not ahead (a failure here only leaves the old
   * view a moment longer).
   */
  func deskFirst(whole: Bool = false) async -> UInt64? { await applyDesk(await fetchDesk(whole: whole)) }
  /** The hub's Desk as this run reads it (see `applyDesk`), with the list it builds on; nil: not had. */
  func fetchDesk(whole: Bool) async -> (desk: JSON, base: (open: Set<String>, at: UInt64)?)? {
    // A device that holds nothing yet reads the full Desk (its versions make the preview); any other reads the
    // brief list: whole at the first reading of a run, after its groups changed and every 30 minutes, else only
    // what changed since the last (the web client's `readDesk`). A hub without the brief list: the full Desk.
    let full = holdsNothingYet || deskFullDue
    let base = whole || deskWholeAt.map { nowMs() >= $0 + 30 * 60_000 } != false ? nil : deskSeen
    var desk: JSON? = nil
    if !full {
      desk = try? await noted({ try await hub.deskBrief(since: base?.at) })
      if desk?["truncated"] as? Bool == true || Wire.uint(desk?["change"]) == nil { desk = nil }
      else { Spent.count(base == nil ? "desk brief whole" : "desk brief since") }
    }
    if desk == nil { desk = try? await noted({ try await hub.desk(active: true) }) }
    return desk.map { ($0, base) }
  }
  /** Takes the hub's Desk read by `fetchDesk` (see `deskFirst`). */
  func applyDesk(_ got: (desk: JSON, base: (open: Set<String>, at: UInt64)?)?) async -> UInt64? {
    guard let desk = got?.desk, desk["truncated"] as? Bool != true, let at = Wire.uint(desk["change"]) else { deskAnswered(); return nil }
    let base = got?.base
    let delta = desk["settled"] != nil && base != nil
    // (by id, the change of its version: a delta builds on the list read before)
    var listed: [String: Int] = delta ? deskListed : [:]
    for kind in ["cards", "permission_requests"] {
      for o in desk[kind] as? [JSON] ?? [] {
        guard let id = (o["object_id"] as? String).flatMap({ try? unb64u($0) }) else { continue }
        listed[hex(id)] = Int(Wire.uint((o["version"] as? JSON)?["change"]) ?? Wire.uint(o["version_change"]) ?? 0)
      }
    }
    if delta { for s in desk["settled"] as? [Any] ?? [] { if let id = (s as? String).flatMap({ try? unb64u($0) }) { listed.removeValue(forKey: hex(id)) } } }
    else { deskWholeAt = nowMs() }
    deskListed = listed
    deskSeen = (Set(listed.keys), at)
    keepDesk(Set(listed.keys), at: at)
    board.deskWaiting = false
    // (a device that holds nothing yet: the Desk from the hub's list, before any of the history is read)
    if holdsNothingYet || deskFullDue { deskFullDue = false; await Spent.time("desk preview") { await deskPreview(desk) } }
    var change = Change()
    // The hub's list decides the Desk (the client core's rule, the same on every app): what it leaves out, with
    // nothing newer here than its change, was settled elsewhere and is held back; a hand back older than the version
    // it lists is over.
    board.setDeskListing(listed, at: Int(at))
    if at > cursor { catchingUp = (cursor, at) }
    change.stack = true
    board.project(&change)
    emit(change)
    deskAnswered()
    return at > cursor ? at : nil
  }
  /**
   * The Desk at once for a device that has nothing yet (a fresh sign-in): the newest version of every open card and
   * permission request, and the registers (names, desks), as the hub's Desk lists them, read out of order (not
   * `ordered`: the chains are not touched) into a board of their own. What does not open here yet (an epoch this
   * device was not in) is left out. The views show it until the first catch-up is through (`preview`).
   */
  func deskPreview(_ desk: JSON) async {
    // (a device that took the hub's order from its join: into the board itself, which its read back of the past
    //  shows as it stood until that is through; a preview board would go with the first sync, before the past)
    let into = cursor > 0 && record.past?.readBack == true
    let p = into ? board : Board()
    if !into {
      p.preview = true
      p.roomId = record.roomId; p.hubURL = record.hubURL; p.myDeviceId = record.myDeviceId; p.myRole = record.role
    }
    var change = Change()
    if !into { try? applyGroups(Array(groups.values), &change, into: p) }
    var items = [(UInt64, Bytes)]()
    func take(_ j: JSON?) { if let j, let n = Wire.uint(j["change"]), let b = (j["envelope"] as? String).flatMap({ try? unb64u($0) }) { items.append((n, b)) } }
    for r in desk["registers"] as? [JSON] ?? [] { take(r) }
    for kind in ["cards", "permission_requests"] { for o in desk[kind] as? [JSON] ?? [] { take(o["version"] as? JSON) } }
    items.sort { $0.0 < $1.0 }
    // (one flush of the store for all of them, at the end: DeviceStore.batched; one flush and one anchor write per
    //  item made a fresh sign-in into a 1,000-card Desk spend 22 s here)
    let store = deviceStore
    guard !items.isEmpty, let opened = try? await onCore({ device in
      try store.batched {
        items.compactMap { item -> ReceivedEnvelope? in
          guard let e = try? device.receiveEnvelope(item.1, change: item.0, ordered: false, voidCode: nil, nowMs: nowMs()), e.outcome != .refused else { return nil }
          return e
        }
      }
    }) else { return }
    let list = opened.filter { e in e.payload != nil && !(e.header.kind == .register && e.register?.name == "heads") }.map(receivedJSON)
    let shown = p.apply(received: list).filter { $0.applied }.count
    p.project(&change)
    Spent.count("desk preview items", shown)
    PerfLog.line("desk preview: \(shown) of \(items.count) taken, \(p.stack.count) cards on the Desk")
    if into { var ch = Change(); ch.room = true; ch.stack = true; emit(ch); return }
    guard !p.stack.isEmpty else { return }
    preview = p
    var ch = Change(); ch.room = true; ch.stack = true
    emit(ch)
  }
  /** A device that holds nothing of the room yet: a fresh sign-in, before its first catch-up or before the read back
   *  of the past that its sign-in left for later (`pastBehind`). The hub's Desk is read whole for it. */
  var holdsNothingYet: Bool { board.cards.isEmpty && (cursor == 0 || record.past?.readBack == true) }
  /** The past of a fresh sign-in (`pastBehind`): read back after its first sync, in a job of its own. */
  func readPastBehind() async {
    _ = try? await serial { [self] in
      var report = SyncReport(), change = Change()
      defer { self.board.project(&change); self.emit(change) }
      try await Spent.time("past") { try await self.tendPast(&report, &change) }
    }
  }
  /** The Desk the views show until the first catch-up of a fresh sign-in is through (`deskPreview`); nil: the board. */
  public internal(set) var preview: Board?

  /** The hub's Desk of the last `deskFirst`: its open cards and requests, and its cursor. */
  var deskSeen: (open: Set<String>, at: UInt64)?
  /** When the Desk was last read whole in this run (ms): a delta builds on it for 30 minutes. */
  var deskWholeAt: UInt64?
  /** The hub's Desk of the last reading, by id with the change of its version (a delta builds on it). */
  var deskListed: [String: Int] = [:]
  /** The catch-up reached the Desk's change of `deskFirst`. */
  func caughtUp(_ change: inout Change) {
    catchingUp = nil
    change.stack = true
  }

  /** The session groups this device is in (hex). */
  var sessionGroupIds: Set<String> { Set(groups.values.filter { $0.session != nil }.map { hex($0.group) }) }

  /** The Welcomes waiting for this device (a session group it was added to): joined, if the room's inviter made them. */
  func takeWelcomes(_ change: inout Change) async throws {
    let list = try await noted { try await hub.welcomes() }
    guard !list.isEmpty else { return }
    let room = roomId
    var joinedGroups = [GroupId]()
    for w in list {
      guard let bytes = (w["welcome"] as? String).flatMap({ try? unb64u($0) }) else { continue }
      // A Welcome that does not check is left alone: the hub keeps it, an alert says so, nothing is joined.
      do {
        let joined = try await onCore { try $0.joinWelcome(bytes, room: room, committer: nil, nowMs: nowMs()) }
        joinedGroups.append(joined.group)
        // (this device holds the group from here on; what was written in it before is learned: RoomPast.swift)
        notePast { if !$0.toLearn.contains(hex(joined.group)) { $0.toLearn.append(hex(joined.group)) } }
        pastDue = true
      }
      catch let e as TrommiError where e.code == "wrong-epoch" { continue }   // (joined before: the hub still had it)
      catch { board.pushAlert(&change, code: "welcome", message: "a Welcome was refused: \(Self.codeOf(error))") }
    }
    // A Welcome taken later than its place in the hub's order (the device read past its Add first): the group's
    // entries up to where the core has read are handed again, as the web engine's `handLog` does.
    for group in joinedGroups { try await handGroupLog(group, upTo: try await onCore { $0.cursor }, &change) }
    try await refreshGroups(&change)
    // What was passed over before these joins (an envelope of a group this device did not hold yet: its Welcome not at
    // the hub yet, or not read) is read back now, whatever learning the group's past brings (none for a helper session
    // whose walk waits, 5.2.6): as the web engine's `behind` rule.
    if !joinedGroups.isEmpty, record.past?.passedOver == true { notePast { $0.readBack = true } }
    // A sign-in with the code whose session another human device added this one to first (5.2.7, its Welcome won the
    // race): the code selected none of that session's keys, and what was written before the Welcome stays closed
    // until a human device hands them over. They are asked for at once, not after the grace (7.1.1): the person
    // signed in a moment ago and waits for exactly this (web room.ts `joinSessions`, `askKeysNow`). One request that
    // names no group asks for the keys of every group: one per session met the hub's limit on requests (10 at once,
    // then one per 5 s), and the sessions past the tenth stayed closed (their names, their cards).
    var wanted = false
    for group in joinedGroups where group != room && (record.codeJoin == true || codeJoinMissed.contains(hex(group))) {
      codeJoinMissed.remove(hex(group))
      wanted = true
    }
    if wanted { await askKeysNow(nil) }
  }
  /** A wish for content keys now (7.1.1), without the grace `tendKeys` keeps; `group` nil: of every group. A request
   *  the hub's limit holds back is made again a little later. */
  func askKeysNow(_ group: GroupId?) async {
    guard record.role == "human" else { return }
    var body: [String: Any] = ["kind": "handover"]
    if let group { body["group"] = b64u(group) }
    for tries in 0..<4 {
      do { _ = try await noted { try await hub.request("POST", "/requests", body: body) }; return } catch {
        guard tries < 3, let e = error as? HubError, e.status == 429 || e.status >= 500 else { return }
        let wait = max(UInt64(e.retryAfter ?? 0), UInt64(1) << UInt64(tries))
        try? await Task.sleep(nanoseconds: min(wait, 10) * 1_000_000_000)
      }
    }
  }

  /**
   * Hands a group's log to the core again from its start up to the change `upTo`: for a Welcome taken after the
   * device read past its Add (core/README.md). The core takes the group's next Commits and the messages behind the
   * Add (a handover of the earlier keys) also below its cursor, and passes over what it has (`duplicate`). Entries
   * after `upTo` come at their place in the hub's order with the changes. An entry that came early ends the
   * reading; one that does not process is shown, a Commit of it reported (14.7), and ends it too. The hub not
   * answering ends it quietly (as the web's): the entries then wait for the group's next Commit.
   */
  func handGroupLog(_ group: GroupId, upTo: UInt64, _ change: inout Change) async throws {
    var after: UInt64 = 0
    reading: for _ in 0..<10_000 {
      guard let page = try? await noted({ try await hub.groupLog(group, after: after, limit: 200) }) else { return }
      let raw = page["items"] as? [JSON] ?? []
      var entries = [(n: UInt64, entry: LogEntry)]()
      var end = page["more"] as? Bool != true || raw.isEmpty
      for j in raw {
        // (the entry's group is the one asked for: the log route need not name it)
        guard let n = Wire.uint(j["n"]), n > after, let c = Wire.uint(j["change"]), let bytes = (j["bytes"] as? String).flatMap({ try? unb64u($0) }) else {
          throw finding("bad-format", "the hub's log of a group does not read or is not in order")
        }
        after = n
        if c > upTo { end = true; break }
        switch j["kind"] as? String {
        case "commit": entries.append((n, LogEntry(change: c, group: group, kind: .commit(bytes: bytes, recoveryAuth: (j["recovery_auth"] as? String).flatMap { try? unb64u($0) }))))
        case "message": entries.append((n, LogEntry(change: c, group: group, kind: .message(bytes: bytes))))
        default: continue   // a kind a newer hub adds
        }
      }
      let taken: [LateTaken] = try await onCore { device in
        var out = [LateTaken]()
        for (_, entry) in entries {
          do { out.append(.processed(try device.processLogEntry(entry))) } catch {
            if !device.isOwner { throw error }
            switch device.logFinding(error) {
            case .local: throw error
            case .duplicate: out.append(.duplicate)
            case .early: out.append(.early); return out
            case .badGroup: out.append(.refused(Room.codeOf(error))); return out
            }
          }
        }
        return out
      }
      coreCursor = try await onCore { $0.cursor }
      for (i, t) in taken.enumerated() {
        let (n, entry) = entries[i]
        switch t {
        case .processed(.message(let m)): try await refreshGroups(&change); applyMessage(m, group: entry.group, change: entry.change, &change)
        case .processed(.commit(_, _, let superseded, _)), .processed(.joinSuperseded(_, _, let superseded)):
          if let id = superseded { lostToAnother(id) }
        case .processed, .duplicate: break
        case .early: break reading
        case .refused(let code):
          board.pushAlert(&change, code: code == "internal" ? "bad-group" : code, message: "an entry of a group's log does not process: the group is kept as it was", envelopeNumber: Int(entry.change))
          if case .commit = entry.kind { reportBadCommit(group, n) }
          break reading
        }
      }
      if end { break }
    }
  }
  /** What the core made of an entry handed again after a late Welcome (`handGroupLog`). */
  enum LateTaken { case processed(Processed), duplicate, early, refused(String) }

  /**
   * The hub says this device was removed. It is believed only with the proof: the room group's Commits up to the
   * removing one (GET /v1/groups/{group}/removal), handed to the core in order from this device's place on. When
   * the core itself says that a Commit removed this device, the room is marked as removed: nothing more is sent
   * or read, and the alert says so. Commits that do not check, or that end without removing this device, prove
   * nothing: everything is kept, a finding is shown, `not-member` is thrown and the next sync asks again. (Such
   * Commits are still the room group's own, so the core keeps the ones it took.)
   */
  func checkRemoval() async throws {
    let room = roomId
    // (the core may have taken the removing Commit already, from the changes, before the hub cut this device off)
    var confirmed = try await onCore { device in try device.groups().first { $0.group == room }.map { !$0.leaves.contains(device.id) } ?? false }
    var after: UInt64 = 0
    reading: while !confirmed {
      let page = try await noted { try await hub.request("GET", "/groups/\(b64u(room))/removal", query: ["after": String(after)]) }
      let items = page["items"] as? [JSON] ?? []
      var entries = [LogEntry]()
      for item in items {
        guard let n = Wire.uint(item["n"]), n > after, let change = Wire.uint(item["change"]), let bytes = (item["bytes"] as? String).flatMap({ try? unb64u($0) }) else {
          throw finding("bad-format", "the hub's proof of a removal does not read")
        }
        after = n
        entries.append(LogEntry(change: change, group: room, kind: .commit(bytes: bytes, recoveryAuth: (item["recovery_auth"] as? String).flatMap { try? unb64u($0) })))
      }
      // true: removed. false: a Commit did not check, so nothing after it can. nil: read on.
      let word: Bool? = try await onCore { device in
        for entry in entries where entry.change > device.cursor {
          guard let done = try? device.processLogEntry(entry) else { return false }
          if case .commit(let group, _, _, true) = done, group == room { return true }
        }
        return nil
      }
      coreCursor = try await onCore { $0.cursor }
      if word == true { confirmed = true }
      if word != nil || page["more"] as? Bool != true || items.isEmpty { break reading }
    }
    guard confirmed else { throw finding("not-member", "the hub says this device was removed and showed no Commit that says so: nothing was forgotten") }
    var change = Change()
    markRemoved(&change)
    emit(change)
  }
  /** The core said that a Commit of the room group removed this device: nothing more is sent or read. */
  func markRemoved(_ change: inout Change) {
    guard removedAt == nil else { return }
    removedAt = nowMs()
    // (kept: an app that ends before it forgot the room knows at its next start, without asking the hub again)
    record.removedAt = removedAt
    try? store.save(record)
    live = false; board.connection = "offline"
    goalsTask?.cancel(); cacheTask?.cancel()
    board.pushAlert(&change, code: "removed", message: "this device was removed from the room")
    change.room = true
  }

  /**
   * `heads` (9.0.7): in each live group the last envelope of every chain this device accepted there, written when
   * the core says it is due (coming online with a changed head, then at most every ten minutes). Another device
   * that holds less than a head named here knows that the hub withholds something.
   */
  func writeHeads() async {
    for g in groups.values where !g.archived {
      let group = g.group
      _ = try? await onCore { device -> Void in
        guard let value = try device.headsDue(group: group, nowMs: nowMs()) else { return }
        _ = try device.seal(.register(group: group, name: "heads", value: value), files: [], nowMs: nowMs())
      }
    }
  }

  /**
   * 7.1.1: the content keys this device still lacks a while after an envelope did not open for them are asked for
   * (the core says which, and when again); the keys come as a handover, after which what was skipped is read again.
   * The wishes of the other human devices that came while this one was away are answered from the hub's list, at
   * most every ten minutes.
   */
  func tendKeys() async {
    let due = (try? await onCore { try $0.keyRequests(nowMs: nowMs()) }) ?? []
    for group in due {
      _ = try? await noted { try await hub.request("POST", "/requests", body: ["kind": "handover", "group": b64u(group)]) }
    }
    guard nowMs() >= keyWishesReadAt + 600_000 else { return }
    keyWishesReadAt = nowMs()
    guard let rows = try? await noted({ try await hub.requests() }) else { return }
    for row in rows { await answerKeys(row) }
  }
  /**
   * Answers a human device's wish for content keys (7.1.1) once: a key handover in the room group. A wish older than a
   * day is passed over: a device that still lacks the keys has asked again since. The core refuses one that is not a
   * human device's, and answers the same requester and group once an hour.
   */
  func answerKeys(_ row: JSON) async {
    guard row["kind"] as? String == "handover", let id = Wire.int(row["id"]), !keyWishesAnswered.contains(id),
          let device = (row["device"] as? String).flatMap({ try? unb64u($0) }), hex(device) != deviceIdHex else { return }
    if let at = Wire.uint(row["at"]), at + 86_400_000 < nowMs() { return }
    let group = (row["group_id"] as? String).flatMap { try? unb64u($0) }
    // (counted as answered once the handover is made: a wish this device could not answer yet, or not now, is
    // tried again at the next reading of the hub's list)
    guard let made = try? await onCore({ try $0.answerKeyRequest(group: group, recipient: device, nowMs: nowMs()) }), !made.isEmpty else { return }
    keyWishesAnswered.insert(id)
    pumpOutbox()
  }

  /** Keeps the hub stocked with this device's KeyPackages (spec 14.2). */
  func publishKeyPackages() async throws {
    // How many are unused at the hub comes with its answer to an upload (`unused`). Before the first answer of a
    // run the hub is asked with an upload of nothing: a device told "none" while the hub holds a full set would make
    // a batch the hub refuses (`too-many`).
    if keyPackagesAtHub == nil {
      let r = try await noted { try await hub.request("PUT", "/key-packages", body: ["single_use": [String]()]) }
      keyPackagesAtHub = Wire.int(r["unused"])
    }
    guard let unused = keyPackagesAtHub else { return }
    _ = try await onCore { try $0.keyPackagesToUpload(unusedAtHub: unused, nowMs: nowMs()) }
  }

  /**
   * GET /v1/changes from the cursor until the hub has no more. The hub's answer is not believed about its shape:
   * every item must carry a change number above the one before it, and the page's own `change` may only stand at
   * or behind its last item. A page that is out of order, or that says "more" without moving on, ends the catch-up
   * with a finding and the cursor where the last good item left it. (That the hub gave EVERYTHING it has is not
   * something numbers can show: that is the chains' and the `heads` registers' work in the core, 9.0.7.)
   */
  func readChanges(_ report: inout SyncReport, _ change: inout Change) async throws {
    // The next page is asked for while the core takes this one (the hub's cursor after a page is where the next
    // starts); a page that does not process throws, and the one fetched ahead is dropped.
    var ahead: Task<JSON, Error>? = nil
    defer { ahead?.cancel() }
    for _ in 0..<10_000 {
      let before = cursor
      let page: JSON
      let waited = Spent.now
      if let fetching = ahead { ahead = nil; page = try await noted { try await fetching.value } }
      else if let read = takeAhead?(cursor) { page = read; Spent.count("pages read ahead", 1) }
      else { page = try await noted { try await hub.changes(after: cursor, active: true) } }
      Spent.add("hub page wait", since: waited)
      if page["more"] as? Bool == true, let next = Wire.uint(page["change"]), next > cursor {
        let hub = self.hub
        if let read = takeAhead?(next) { ahead = Task { read } } else { ahead = Task { try await hub.changes(after: next, active: true) } }
      }
      let raw = page["items"] as? [JSON] ?? []
      let items = try Self.ordered(raw, after: cursor)
      Spent.count(readingBack ? "items read back" : "items", items.count)
      try await process(items, report: &report, change: &change)
      // The hub moves the cursor over what this device may not see: forwards only, and only when the whole page
      // was taken (process throws otherwise).
      if let upTo = Wire.uint(page["change"]), upTo > cursor { cursor = upTo; board.lastEnvelopeNumber = max(board.lastEnvelopeNumber, Int(upTo)) }
      if page["more"] as? Bool != true { return }
      if cursor <= before { throw finding("bad-format", "the hub says there is more and gives nothing new") }
      // A long catch-up shows what it has page by page (what the hub settled meanwhile is held back: deskFirst);
      // reading back builds the board anew and shows it at its end.
      if !readingBack { board.project(&change); if !change.isEmpty { emit(change); change = Change() } }
    }
    throw finding("bad-format", "the hub's changes do not end")
  }
  /** The items of a page as they must be: each readable, each with a change number above the one before. */
  nonisolated static func ordered(_ raw: [JSON], after: UInt64) throws -> [Item] {
    var last = after
    return try raw.map { j in
      guard let item = Item(j) else {
        // (a kind a newer hub adds is passed over; anything else that does not read is not)
        if let k = j["kind"] as? String, !["envelope", "commit", "message"].contains(k), let n = Wire.uint(j["change"]), n > last { last = n; return nil }
        throw TrommiError("bad-format", "an item of the hub's changes does not read")
      }
      guard item.change > last else { throw TrommiError("bad-format", "the hub's changes are not in order") }
      last = item.change
      return item
    }.compactMap { $0 }
  }
  /** A finding about the hub: shown, and thrown. */
  func finding(_ code: String, _ message: String) -> TrommiError {
    var ch = Change()
    board.pushAlert(&ch, code: code, message: message)
    emit(ch)
    return TrommiError(code, message)
  }

  /** One item of GET /v1/changes or of the stream, as bytes for the core. */
  enum Item {
    /** `n`: the entry's number in its group's log, which a report to the hub names (14.7). */
    case log(n: UInt64?, LogEntry)
    case envelope(change: UInt64, bytes: Bytes, voidCode: String?)
    var change: UInt64 { switch self { case .log(_, let e): return e.change; case .envelope(let c, _, _): return c } }

    init?(_ j: JSON) {
      guard let change = Wire.uint(j["change"]), change > 0 else { return nil }
      if j["kind"] as? String == "envelope" {
        guard let bytes = (j["envelope"] as? String).flatMap({ try? unb64u($0) }) else { return nil }
        self = .envelope(change: change, bytes: bytes, voidCode: j["void_code"] as? String)
        return
      }
      guard let group = (j["group_id"] as? String).flatMap({ try? unb64u($0) }), let bytes = (j["bytes"] as? String).flatMap({ try? unb64u($0) }) else { return nil }
      switch j["kind"] as? String {
      case "commit": self = .log(n: Wire.uint(j["n"]), LogEntry(change: change, group: group, kind: .commit(bytes: bytes, recoveryAuth: (j["recovery_auth"] as? String).flatMap { try? unb64u($0) })))
      case "message": self = .log(n: Wire.uint(j["n"]), LogEntry(change: change, group: group, kind: .message(bytes: bytes)))
      default: return nil    // a kind a newer hub adds
      }
    }
  }

  /** What the core made of one item. */
  enum Outcome {
    /** `facts`: what the Commit did (CoreDevice.lastCommitFacts), for the dates Settings → Security shows. */
    case processed(Processed, facts: CommitFacts? = nil)
    /** `heads`: for a `heads` register of another device, how its heads compare with this device's chains. */
    case envelope(ReceivedEnvelope, heads: [(sender: DeviceId, standing: HeadStanding)])
    case refused(change: UInt64, code: String)
    /** The run ends before this item; the cursor stays in front of it. */
    case stopped(code: String)
    /** Refused with `gap`: the sender's chain misses what stands before this envelope (9.0.6). The run ends before
     *  it; the chain is fetched from where this device holds it, and the run goes on from this item. */
    case gap(group: GroupId, sender: DeviceId, seq: UInt64)
    /** Reading back: this device holds it whole already (`readBackWhole`); put back as it was. */
    case kept(Rec)
  }

  /**
   * A run of items in the hub's order: each to the core (off the main thread), then what it gave to the board. An
   * item the core refuses changes nothing and is counted; a local failure (the store, a lost owner) stops the run
   * with the cursor before it, so it is read again.
   */
  func process(_ parsed: [Item], report: inout SyncReport, change: inout Change) async throws {
    guard !parsed.isEmpty else { return }
    let readingBack = self.readingBack
    let filledBefore = self.gapsFilled
    let cutDropped = Flag()
    let coreStart = Spent.now
    // (one flush of the store for the whole run, at its end: DeviceStore.batched)
    let store = deviceStore
    var keep = [UInt64: Rec]()
    if readingBack && !readBackWhole.isEmpty { for item in parsed { if case .envelope(let c, _, _) = item, let r = readBackWhole[c] { keep[c] = r } } }
    let (outcomes, coreAt, findings): ([Outcome], UInt64, [ChainFinding]) = try await onCore { device in try store.batched { () -> ([Outcome], UInt64, [ChainFinding]) in
      var out = [Outcome]()
      var commits = false
      /// Per leaf of a group, the last envelope the core accepted of it: asked before and after a Commit, a number
      /// that went down says the core dropped what lay beyond a removed device's Cut (9.0.10).
      func heads(_ group: GroupId) -> [DeviceId: UInt64] {
        guard let held = (try? device.groups())?.first(where: { $0.group == group }) else { return [:] }
        var out = [DeviceId: UInt64]()
        for leaf in held.leaves { if let cut = try? device.cutOf(group: group, device: leaf) { out[leaf] = cut.seq } }
        return out
      }
      /// The findings the core made while it processed Commits (a Cut that names another envelope than the one
      /// accepted): taken once, then cleared there.
      func done() -> ([Outcome], UInt64, [ChainFinding]) {
        let tf = Spent.now
        defer { Spent.add("core findings", since: tf) }
        let found = commits ? ((try? device.findings()) ?? []) : []
        if !found.isEmpty { try? device.findingsRead() }
        return (out, device.cursor, found)
      }
      // What the core processed already and the board's cache has not (the app ended before the cache was written):
      // an envelope is read again without touching any state, a log entry is done with.
      let before = device.cursor
      for item in parsed {
        do {
          switch item {
          case .log(_, let entry):
            if entry.change <= before { out.append(.processed(.skipped)); continue }
            let isCommit: Bool = { if case .commit = entry.kind { return true }; return false }()
            let th = Spent.now
            let had = isCommit ? heads(entry.group) : [:]
            Spent.add("core heads", since: th)
            let tp = Spent.now
            let p = try device.processLogEntry(entry)
            let facts = isCommit ? device.lastCommitFacts() : nil
            Spent.add(isCommit ? "core commit" : "core message", since: tp)
            let tc = Spent.now
            for (leaf, seq) in had where seq > 0 {
              if let now = try? device.cutOf(group: entry.group, device: leaf), now.seq < seq { cutDropped.on = true }
            }
            Spent.add("core cuts", since: tc)
            if case .message = p {} else { commits = true }
            out.append(.processed(p, facts: facts))
          case .envelope(let c, let bytes, let void):
            if readingBack, c <= before, let r = keep[c] { out.append(.kept(r)); continue }
            let te = Spent.now
            var e = try device.receiveEnvelope(bytes, change: c, ordered: c > before || readingBack, voidCode: void, nowMs: nowMs())
            Spent.add("core envelope", since: te)
            // Reading back: `replay` says the chain holds this one already, so it is read again for display.
            if readingBack, c <= before, e.outcome == .refused, e.code == "replay" { e = try device.receiveEnvelope(bytes, change: c, ordered: false, voidCode: void, nowMs: nowMs()) }
            // Refused with `gap` in the hub's order: the sender's chain misses what stands before this one (it was
            // written before this device came into the group, or passed over). Stop before it: the chain is filled
            // and the run goes on from here (the web engine's `fillGap`). Once per sender and run.
            if e.outcome == .refused, e.code == "gap", c > before, !readingBack, !filledBefore.contains("\(hex(e.header.group))/\(hex(e.header.sender))") {
              out.append(.gap(group: e.header.group, sender: e.header.sender, seq: e.header.seq)); return done()
            }
            // Refused with `group-behind` and the cursor left where it was: the group's next Commits are still to
            // come. Stop before it, so that it is read again with what it needs, instead of being passed over.
            if e.outcome == .refused, c > before, device.cursor < c { out.append(.stopped(code: e.code ?? "group-behind")); return done() }
            var heads = [(sender: DeviceId, standing: HeadStanding)]()
            if e.outcome == .applied, e.register?.name == "heads", e.header.sender != device.id {
              heads = (try? device.compareHeads(group: e.header.group, writer: e.header.sender)) ?? []
            }
            out.append(.envelope(e, heads: heads))
          }
        } catch {
          // A local failure is not the item's fault: stop here, nothing after it is processed out of order.
          if !device.isOwner { throw error }
          switch device.logFinding(error) {
          // The store failed: not the item's fault. Stop here; nothing after it is processed out of order.
          case .local: out.append(.stopped(code: Room.codeOf(error))); return done()
          // Something this item builds on has not come yet (its group is behind): stop before it, so that it is
          // read again with what it needs, instead of being passed over.
          case .early: out.append(.stopped(code: Room.codeOf(error))); return done()
          // Processed before: nothing to do.
          case .duplicate: out.append(.processed(.skipped))
          // It does not verify or cannot be merged (13.4), or its bytes are no envelope: the device keeps its
          // last good state, the finding is shown and, for a Commit, reported to the hub (14.7). Later items go on.
          case .badGroup: out.append(.refused(change: item.change, code: Room.codeOf(error)))
          }
        }
      }
      return done()
    } }
    Spent.add("core (process)", since: coreStart)
    let boardStart = Spent.now
    defer { Spent.add("board (apply)", since: boardStart) }
    coreCursor = coreAt
    // (the core dropped what a removed device had signed beyond its Cut: what was shown of it goes, the board and
    // its cache are built again from the hub's changes, which the core now judges without it: RoomPast.swift)
    if cutDropped.on && !readingBack { notePast { $0.readBack = true; $0.afterCut = true }; pastDue = true }
    for f in findings { board.pushAlert(&change, code: f.code, message: "a removal names another last item of a device than the one this device holds", sender: hex(f.sender)) }
    // The groups are read again from the core after a Commit and before the next item is shown: an envelope right
    // behind the Commit that founded its session must find that session on the board.
    var groupsStale = false
    func freshGroups() async throws { if groupsStale { groupsStale = false; try await refreshGroups(&change) } }
    var stopped: String? = nil
    var gapAt: (index: Int, group: GroupId, sender: DeviceId, seq: UInt64)? = nil
    for (i, o) in outcomes.enumerated() {
      if case .stopped(let code) = o { stopped = code; break }
      if case let .gap(group, sender, seq) = o { gapAt = (i, group, sender, seq); break }
      cursor = max(cursor, parsed[i].change)
      board.lastEnvelopeNumber = max(board.lastEnvelopeNumber, Int(cursor))
      switch o {
      case .processed(let p, let facts):
        if let facts { board.applyCommitFacts(facts, room: facts.group == roomId); change.members = true }
        switch p {
        case .commit(let group, _, let superseded, let removed):
          groupsStale = true
          if let id = superseded { lostToAnother(id) }
          if removed && group == roomId { markRemoved(&change) }
          else if removed { board.pushAlert(&change, code: "removed", message: "this device was removed from a group") }
        case .joinSuperseded(_, _, let superseded):
          groupsStale = true
          if let id = superseded { lostToAnother(id) }
        case .ownCommit, .observed: groupsStale = true
        case .message(let m):
          try await freshGroups()
          if case .log(_, let entry) = parsed[i] { applyMessage(m, group: entry.group, change: entry.change, &change) }
        case .skipped: break
        }
      case .envelope(let e, let heads):
        try await freshGroups()
        apply(e, report: &report, &change)
        for h in heads { applyHead(h.standing, of: h.sender, group: e.header.group, saidBy: e.header.sender, &change) }
      case .refused(let c, let code):
        report.refused += 1
        if report.refused <= 5 { report.warnings.append("change \(c) refused: \(code)") }
        // Never swallowed (section 16): shown on the board; a Commit that cannot be merged is reported (14.7).
        board.pushAlert(&change, code: code == "internal" ? "bad-group" : code, message: "an item from the hub was refused (change \(c))", envelopeNumber: Int(c))
        if case .log(let n?, let entry) = parsed[i], case .commit = entry.kind { reportBadCommit(entry.group, n) }
      case .kept(let rec):
        try await freshGroups()
        report.envelopes += 1; report.opened += 1
        Spent.count("read back from the cache")
        replay(rec)
        log[UInt64(rec.envelopeNumber)] = rec
        sinceSnapshot += 1
        cacheSoon(UInt64(rec.envelopeNumber))
      case .stopped, .gap: break
      }
    }
    try await freshGroups()
    if let code = stopped { throw TrommiError(code, "the catch-up stopped before a change it could not take yet") }
    if let g = gapAt, case let .envelope(c, bytes, void) = parsed[g.index] {
      self.gapsFilled.insert("\(hex(g.group))/\(hex(g.sender))")
      try await fillGap(group: g.group, sender: g.sender, before: g.seq, &report, &change)
      // the envelope itself again, along its sender's chain; still a gap: the hub withholds a part of the chain
      if let e = try? await onCore({ try $0.receiveEnvelope(bytes, change: c, ordered: true, voidCode: void, nowMs: nowMs()) }) {
        apply(e, report: &report, &change)
      }
      cursor = max(cursor, c)
      board.lastEnvelopeNumber = max(board.lastEnvelopeNumber, Int(cursor))
      if g.index + 1 < parsed.count { try await process(Array(parsed[(g.index + 1)...]), report: &report, change: &change) }
    }
  }
  /**
   * The links of a sender's chain this device misses before `before` (9.0.6), fetched from where it holds the chain
   * and taken in order (each as read again, `ordered`: the chain's own order), so that the envelope behind them is
   * taken too. What the hub does not serve stays a gap, said once (the web engine's `fillGap`).
   */
  func fillGap(group: GroupId, sender: DeviceId, before: UInt64, _ report: inout SyncReport, _ change: inout Change) async throws {
    var after = (try? await onCore { try $0.cutOf(group: group, device: sender).seq }) ?? 0
    pages: for _ in 0..<1_000 {
      guard let page = try? await noted({ try await hub.chain(group: group, sender: sender, after: after) }) else { break }
      let items = page["items"] as? [JSON] ?? []
      for j in items {
        guard let seq = Wire.uint(j["seq"]), seq > after, let n = Wire.uint(j["change"]), let bytes = (j["envelope"] as? String).flatMap({ try? unb64u($0) }) else { break pages }
        if seq >= before { break pages }
        let void = j["void_code"] as? String
        if let e = try? await onCore({ try $0.receiveEnvelope(bytes, change: n, ordered: true, voidCode: void, nowMs: nowMs()) }), e.outcome != .refused {
          apply(e, report: &report, &change)
        }
        after = seq
      }
      if page["more"] as? Bool != true || items.isEmpty { break }
    }
  }
  /**
   * What another device's `heads` say of one sender's chain, against this device's own (9.0.7). Another envelope
   * under the same number is `equivocation`. Holding less than the head named is `withheld`: `heads` come in the
   * hub's order, behind everything their writer had accepted, so what they name should be here already. Not said
   * of a sender whose chain this device could not take from its start (it came into the group later): there the
   * core holds nothing to compare, and the `gap` shown for that sender says so.
   */
  private func applyHead(_ standing: HeadStanding, of sender: DeviceId, group: GroupId, saidBy: DeviceId, _ change: inout Change) {
    let code: String
    switch standing {
    case .held, .unknown: return
    case .equivocation: code = "equivocation"
    case .behind(let have):
      if have == 0 || findingsSaid.contains("\(hex(group))/\(hex(sender))/gap") { return }
      code = "withheld"
    }
    guard findingsSaid.insert("\(hex(group))/\(hex(sender))/\(code)").inserted else { return }
    board.pushAlert(&change, code: code, message: code == "withheld" ? "another device holds more items of a device than the hub gave this one" : "two different items under one number of a device's chain", sender: hex(sender))
  }
  /** Tells the hub that a Commit of its log cannot be merged (14.7); the hub withdraws nothing, it notes it. */
  private func reportBadCommit(_ group: GroupId, _ n: UInt64) {
    Task { _ = try? await hub.request("POST", "/groups/\(b64u(group))/reject", body: ["n": n]) }
  }
  /** An application message of a group: a step of a work trail, a piece of a stroke being drawn. */
  private func applyMessage(_ m: ReceivedMessage, group: GroupId, change n: UInt64, _ change: inout Change) {
    switch m {
    case let .workTrail(from, turn, number, time, step):
      // (which session: the group the message came in; the item is kept and folded like every chat item)
      guard let sid = sessionIdOfGroup[hex(group)] else { return }
      let rec = Rec(envelopeNumber: Int(n), json: .obj(["work": .obj(["session": .str(sid), "from": .str(hex(from)), "turn": .str(hex(turn)), "number": .num(Double(number)),
                                                                       "time": .num(Double(time)), "step": .str(String(decoding: step, as: UTF8.self))])]), opened: true)
      guard replay(rec) else { return }
      log[n] = rec
      cacheSoon(n)
    case let .strokePiece(from, boardId, piece):
      onStrokePiece?(hex(from), "desk/\(hex(boardId))", piece)
    case .newerVersion(let from):
      board.pushAlert(&change, code: "newer-version", message: Compat.UPDATE_MESSAGE, sender: hex(from))
    case .keys(_, let taken, _):
      // Keys of older epochs came (a handover, also one answering a key request, 7.1.1): items that took their place
      // without a body can be opened now, and so can items passed over as written before this device came into the
      // group (a sign-in with the code whose session a Welcome won: the keys of before it come by the handover).
      if taken > 0, record.past?.closed == true || record.past?.passedOver == true { notePast { $0.readBack = true }; pastDue = true; if readingBack { readBackAgain = true } }
    case .recoveryAuth, .recoveryAuthConflict, .dropped: break
    }
  }
  /** A piece of a stroke another device is drawing (relayed, never stored): sender, board timeline id, the piece's JSON. */
  public var onStrokePiece: ((String, String, Bytes) -> Void)?

  /** "human" or "agent", as the room names a device: a leaf of the room group is a human device; one that was, stays one. */
  func roleOf(_ device: DeviceId) -> String {
    if let known = board.members[hex(device)]?.deviceRole { return known }
    return roomGroup.flatMap { groups[hex($0)] }?.leaves.contains(device) == true ? "human" : "agent"
  }

  /** One received envelope into the board. */
  func apply(_ e: ReceivedEnvelope, report: inout SyncReport, _ change: inout Change) {
    let n = e.change, sender = hex(e.header.sender)
    report.envelopes += 1
    if let f = e.finding { board.pushAlert(&change, code: f, message: "the hub set an item of another device aside for a reason this device cannot check", envelopeNumber: Int(n), sender: sender) }
    // What was shown of this place in the sender's chain before the chain came is another envelope: dropped.
    if e.confirmed == false { dropProvisional(sender: sender, seq: e.header.seq, keep: hex(e.hash), &change) }
    switch e.outcome {
    case .refused:
      // It consumed nothing. An epoch before this device came into the group is nothing it can take, and no
      // finding. Everything else is one (a forged or replayed item, a chain that does not link): shown, once
      // per sender and code, since every later item of a broken chain is refused the same way (section 16).
      let code = e.code ?? "bad-format"
      // (an item read back that wants a key: the past stays closed until a handover brings it, 7.1.1)
      if code == "no-key" { notePast { $0.closed = true } }
      if code == "group-behind" { report.beforeJoining += 1; notePast { $0.passedOver = true }; return }
      report.refused += 1
      if report.refused <= 5 { report.warnings.append("change \(n) refused: \(code)") }
      if findingsSaid.insert("\(hex(e.header.group))/\(sender)/\(code)").inserted {
        board.pushAlert(&change, code: code, message: code == "gap" ? "items of a device cannot be read: its chain started before this device came" : "an item from the hub was refused", envelopeNumber: Int(n), sender: sender)
      }
      return
    case .void:
      report.voids += 1
      board.apply(received: [receivedJSON(e)])
      return
    case .chained:
      // Chained and never applied. A body that did not open still counts for its object's state (9.2.1) and goes
      // on as a header; a refusal by who may write it or by its epoch (checks 7 and 8) shows nothing (the client
      // core's model says so, as on the web).
      if e.code == "no-key" { notePast { $0.closed = true } }
    case .applied, .provisional: break
    }
    // `heads` is the devices' own bookkeeping (9.0.7), compared in the core: nothing of it is on the board.
    if e.header.kind == .register, e.register?.name == "heads" { return }
    if e.outcome == .applied || e.outcome == .provisional { if e.payload == nil { report.headerOnly += 1 } else { report.opened += 1 } }
    else if e.code == "pruned" { report.headerOnly += 1 } else { report.undecryptable += 1 }
    let json = receivedJSON(e)
    board.apply(received: [json])
    ownEchoes.removeValue(forKey: hex(e.hash))
    log[n] = Rec(envelopeNumber: Int(n), json: .obj(["received": json]), opened: e.payload != nil)
    sinceSnapshot += 1
    cacheSoon(n)
  }
  /** A record of the warm-start log into the board again: an envelope as the core read it, or a step of a work trail. */
  @discardableResult func replay(_ rec: Rec) -> Bool {
    if rec.json["received"].object != nil { return board.apply(received: [rec.json["received"]]).first?.applied ?? false }
    let w = rec.json["work"]
    guard let from = try? unhex(w["from"].string ?? ""), let turn = try? unhex(w["turn"].string ?? "") else { return false }
    return board.applyWorkTrail(session: w["session"].string ?? "", from: from, turn: turn, number: UInt32(w["number"].int ?? 0), time: UInt64(w["time"].int ?? 0),
                                step: Bytes((w["step"].string ?? "").utf8), change: UInt64(rec.envelopeNumber))
  }
  /** Takes out of every conversation what was shown as not yet confirmed under that number of a sender's chain, except the envelope `keep`. */
  private func dropProvisional(sender: String, seq: UInt64, keep: String, _ change: inout Change) {
    for t in board.timelines.values {
      for (n, item) in t.items where item.pending && item.senderDeviceId == sender && item.senderSequence == seq && item.envelopeHash != keep {
        t.items.removeValue(forKey: n)
        t.numbers.removeAll { $0 == n }
        change.timelines.insert(t.key)
      }
    }
  }

  nonisolated static func codeOf(_ error: Error) -> String {
    if let e = error as? TrommiError { return e.code }
    if let e = error as? HubError { return e.code }
    if let e = error as? StoreError { if case .conflict = e { return "conflict" }; return "storage" }
    return "internal"
  }

  // ---- live ----------------------------------------------------------------------------------------------

  /** One event of the hub's stream. */
  public func handleStreamEvent(_ event: String, _ data: JV) async {
    switch event {
    case "envelope", "log":
      // The hub sends the item as GET /v1/changes gives it. A `log` event that only names its group and number
      // (spec/hub-api.md allows that), or anything else that does not read, is a hint: catch up from the cursor.
      // An item that is not the next one is taken by the same catch-up, in order.
      let item = (data.any as? JSON).flatMap(Item.init)
      if let i = item, i.change <= cursor { return }
      _ = try? await serial { [self] in
        var report = SyncReport(), change = Change()
        defer { self.board.project(&change); self.emit(change); self.pumpOutbox() }
        var taken = false
        if let i = item, i.change > self.cursor { taken = (try? await self.process([i], report: &report, change: &change)) != nil }
        if !taken { try await self.readChanges(&report, &change) }
        if self.pastDue { try await self.tendPast(&report, &change) }
      }
    case "relay":
      // A message the hub only passes on (a piece of a stroke being drawn): no change number, not in the log.
      guard let group = data["group_id"].string.flatMap({ try? unb64u($0) }), let bytes = data["message"].string.flatMap({ try? unb64u($0) }) else { return }
      _ = try? await serial { [self] in
        if case let .strokePiece(from, boardId, piece)? = try await self.onCore({ try $0.receiveRelay(group: group, message: bytes, nowMs: nowMs()) }) { self.onStrokePiece?(hex(from), "desk/\(hex(boardId))", piece) }
      }
    case "welcome":
      _ = try? await serial { [self] in
        var report = SyncReport(), ch = Change()
        defer { self.board.project(&ch); self.emit(ch) }
        let before = self.sessionGroupIds
        try await self.takeWelcomes(&ch)
        // (a session group joined: the hub's Desk lists its open cards now, and is read again, as at a catch-up)
        if self.sessionGroupIds != before, let until = await self.deskFirst(whole: true) {
          try await self.readChanges(&report, &ch)
          if self.cursor >= until { self.caughtUp(&ch) }
        }
        if self.pastDue { try await self.tendPast(&report, &ch) }
      }
    case "presence":
      // (the client core reads it: client/src/presence.rs, the same on every app)
      board.applyPresenceEvent(data)
      emit(Change())
    case "here":
      // (the end of a new stream's list of who is here: a device shown online that it did not name left meanwhile)
      board.applyHere()
      emit(Change())
    case "request":
      // a wish of a signed-in device (readmit, handover, session) or a join request of an invite; a wish for keys
      // is answered (7.1.1)
      if data["kind"].string == "handover", let row = data.any as? JSON { await answerKeys(row) }
      var ch = Change(); ch.invites.insert(data["invite_id"].string ?? ""); emit(ch)
    case "file_evicted": break
    case "ping":
      // the upkeep of key requests (7.1.1) while the app stays open: a wish's grace and pauses run out without
      // any change arriving; a read back of the past that is still due (a pass that failed) is taken up too
      if nowMs() >= keyTendedAt + 60_000 {
        keyTendedAt = nowMs()
        _ = try? await serial { [self] in
          await self.tendKeys()
          // (the stored flag, not only `pastDue`: a pass that failed half way left it set)
          guard self.pastDue || self.record.past?.readBack == true else { return }
          var report = SyncReport(), ch = Change()
          defer { self.board.project(&ch); self.emit(ch) }
          try await self.tendPast(&report, &ch)
        }
      }
    default: break   // events a newer hub adds
    }
  }

  /**
   * The live stream while the app is in front: every item as the hub takes it, applied at once. Returns when the
   * task is cancelled; reconnects with a growing pause after a drop, and when nothing (not even a ping) came for
   * 40 s. The stream resumes after the cursor, so nothing between two streams is missed.
   */
  public func runLive() async {
    var pause: UInt64 = 300_000_000
    while !Task.isCancelled && removedAt == nil {
      let reader = SSEReader()
      var unauthorised = false, refused = false
      do {
        // (a fresh sign-in that took the hub's order from its join: the stream opens beside the first sync, at once,
        //  for presence; what it brings waits in the queue behind that sync and is passed over once it is read)
        if !synced {
          if pastBehind || record.codeJoin == true { Task { @MainActor [weak self] in _ = try? await self?.sync() } }
          else { _ = try await sync() }
        }
        let req = try await hub.streamRequest(after: cursor)
        for await ev in reader.start(req) {
          if Task.isCancelled { break }
          switch ev {
          case .status(let status):
            if status == 426 { upgrade = UpgradeNotice(minimumVersion: nil, message: "Please update Trommi."); var ch = Change(); ch.room = true; emit(ch); reader.stop(); return }
            if status == 401 { unauthorised = true; reader.stop(); break }
            if status != 200 { refused = true; reader.stop(); break }
            live = true; board.connection = "live"
            Spent.mark("stream open at \(cursor)")
            board.streamOpened()
            var ch = Change(); ch.room = true; emit(ch)
          case .event(let name, let data):
            pause = 300_000_000   // (a stream that brought something was a real one: the next pause is short again)
            if let v = JV.parse(Array(data.utf8)) { await handleStreamEvent(name, v) }
          }
        }
      } catch {}
      reader.stop()
      if Task.isCancelled { break }
      if unauthorised { hub.forgetToken(); pause = max(pause, 2_000_000_000) }
      // (a stream the hub refused: the next round signs in again and reads the role it is given, `catchUp`)
      if unauthorised || refused { synced = false }
      live = false; board.connection = "offline"
      var ch = Change(); ch.room = true; emit(ch)
      if upgrade != nil { return }
      try? await Task.sleep(nanoseconds: pause + UInt64.random(in: 0...(pause / 4)))
      pause = min(pause * 2, 30_000_000_000)
      Task { await self.refreshPresence() }
    }
    live = false
  }

  /** Who is online and what the agents' links say. */
  public func refreshPresence() async {
    // The hub tells presence on the stream only (a `presence` event per device as it comes and goes): there is
    // nothing to ask. Kept because the views call it when they come to the front.
  }
  /** The old name of `refreshPresence`, which the views call. */
  public func refreshDevices() async { await refreshPresence() }

  // ---- the outbox ----------------------------------------------------------------------------------------

  /**
   * Posts the core's outbox in order and reports each answer back. A network failure is retried with a growing
   * pause (the entry stays stored). A refusal that is the hub's last word goes to the core, which undoes what the
   * entry was for or holds a Commit back until the log decided (an own Commit that lost its epoch).
   *
   * An accepted COMMIT of this device is not merged by the answer: the group stands in its old epoch until the core
   * is handed that Commit from the hub's log, at its place among the changes of every group. So after such an
   * answer the changes are read from the cursor (`readOwnChanges`) before the entry counts as done. The live stream
   * may bring the Commit before the answer arrives: the core merged it then and the entry is gone (`not-found`).
   *
   * An ENVELOPE is different (9.0.1, 9.0.8): this device signed its number and gives it to no other envelope. It
   * leaves the outbox when the hub took it, when the hub kept its number as a void record (`voided`), or when this
   * device is out of the group (`not-member`, `removed-sender`) by the group state its own core holds. After any
   * other refusal the same bytes are sent again, and an alert says that something waits.
   */
  func pumpOutbox() {
    if pumping || removedAt != nil { return }
    pumping = true
    Task { @MainActor in
      defer { pumping = false }
      var backoff: UInt64 = 300_000_000, unknown = 0, stuck: UInt64? = nil
      func pause() async { try? await Task.sleep(nanoseconds: backoff); backoff = min(backoff * 2, 30_000_000_000) }
      while !Task.isCancelled && !closed && removedAt == nil {
        guard let entry = try? await onCore({ $0.outbox().first }) else { return }
        do {
          let r = try await noted { try await hub.post(entry) }
          // Only an answer of the documented shape is the hub's "taken": anything else (an empty 200, a page of
          // HTML) says nothing, and the entry is sent again.
          guard let answer = Self.accepted(entry.kind, r) else { throw HubError(status: 0, code: "offline", message: "the hub's answer to an outbox entry does not read") }
          if entry.kind == .keyPackages { keyPackagesAtHub = Wire.int(r["unused"]) }
          let commits = Self.carriesCommit(entry.kind)
          do { try await onCore { try $0.outboxAccepted(entry.id, change: answer) } }
          catch let e as TrommiError where commits && e.code == "not-found" { _ = e }   // merged from the log before the answer came, or dropped
          if commits { await readOwnChanges() }
          // A Commit counts as done only by the log: not dropped for another device's (superseded), and merged
          // (the group has no own Commit waiting any more); otherwise its caller is told it did not happen.
          if commits, supersededIds.remove(entry.id) != nil { backoff = 300_000_000; unknown = 0; continue }
          if commits, let g = entry.group, (try? await onCore({ try $0.groups() }))?.first(where: { $0.group == g })?.pending == true,
             !((try? await onCore({ $0.outbox() })) ?? []).contains(where: { $0.group == g && Self.carriesCommit($0.kind) }) {
            outcome[entry.id] = .failure(HubError(status: 0, code: "pending", message: "the hub took the change and the log has not shown it yet"))
            backoff = 300_000_000; unknown = 0; continue
          }
          outcome[entry.id] = .success(())
          backoff = 300_000_000; unknown = 0
          if entry.kind != .envelope && entry.kind != .message && entry.kind != .relayMessage { var ch = Change(); try? await refreshGroups(&ch); emit(ch) }
        } catch let e as HubError {
          // TROMMI_DEBUG: which kind of entry met which status and code (never its bytes).
          if ProcessInfo.processInfo.environment["TROMMI_DEBUG"] != nil { FileHandle.standardError.write(Data("[outbox] \(entry.kind) -> \(loggable(e))\n".utf8)) }
          if e.status == 426 { return }
          if e.status == 401 { hub.forgetToken() }
          // Not the hub's last word: no answer, it could not answer, it asks to sign in again, or it names a code
          // the core does not know. The entry stays and goes out again unchanged; a code nobody knows is said once.
          // (by the code, not the status: the hub answers 429 both for "slow down" and for a limit that is final)
          if e.isOffline || e.status >= 500 || e.status == 401 || !Core.tools.isFinalRefusal(e.code) {
            if !e.isOffline && e.status < 500 && e.status != 401 && !["rate-limited", "overloaded", "internal", "unauthorised"].contains(e.code) {
              unknown += 1
              if unknown == 3 { var ch = Change(); board.pushAlert(&ch, code: "hub-answer", message: "the hub answers something this app does not know (\(e.status)): what was written waits to be sent"); emit(ch) }
            }
            await pause(); continue
          }
          let voided = e.extra["voided"] as? Bool == true
          // "Out of the group" is the core's word, not the hub's: an envelope is given up only when the group as
          // this device holds it has no leaf of it any more. Otherwise it waits, and the next sync signs in again
          // and reads the role the hub names (a removed device proves its removal itself: `checkRemoval`).
          let out = entry.kind == .envelope && (e.code == "not-member" || e.code == "removed-sender")
          let outHere = out ? await outOfGroup(entry.group) : false
          if out && !outHere && !voided { synced = false; await pause(); continue }
          let gone = entry.kind == .envelope && (voided || outHere)
          do {
            try await onCore { device in
              if entry.kind != .envelope { try device.outboxRefused(entry.id, code: e.code) }
              else if voided { try device.outboxVoided(entry.id) }
              else if gone { try device.envelopeAbandon(entry.id) }
              else { try device.outboxRefused(entry.id, code: e.code) }
            }
          } catch { var ch = Change(); board.pushAlert(&ch, code: Room.codeOf(error), message: "a refusal of the hub could not be taken: sending stopped"); emit(ch); return }
          if entry.kind == .envelope && !gone {
            // Said once per entry; then the same bytes again, with a growing pause. The writer is not told "refused":
            // what it wrote still waits and may yet be taken.
            if stuck != entry.id {
              stuck = entry.id
              var ch = Change(); board.pushAlert(&ch, code: e.code, message: "the hub refused an item and took no number for it: it waits and is sent again (\(e.message))"); emit(ch)
            }
            await pause(); continue
          }
          outcome[entry.id] = .failure(e)
          var ch = Change()
          board.pushAlert(&ch, code: e.code, message: entry.kind == .envelope ? "the hub refused an item: \(e.message)" : "the hub refused a change to a group: \(e.message)")
          emit(ch)
        } catch { return }   // the store failed or this device is no longer the owner: nothing more is sent
      }
    }
  }
  /** Whether this device's core holds no leaf of its own in that group (it was removed, or the group is gone). */
  func outOfGroup(_ group: GroupId?) async -> Bool {
    guard let group = group, let list = try? await onCore({ try $0.groups() }) else { return false }
    guard let held = list.first(where: { $0.group == group }) else { return true }
    return !held.leaves.contains(device.id)
  }
  /** Whether an entry of that kind carries a Commit of this device, which the core merges only from the hub's log. */
  nonisolated static func carriesCommit(_ kind: OutboxKind) -> Bool { [.commit, .groupFounding, .externalCommit, .recoveryCode].contains(kind) }
  /**
   * Reads what the hub has after the cursor, in its order, and the groups as they stand then: this is where a
   * Commit of this device that the hub accepted takes effect. A failure is left to the next catch-up or the stream.
   */
  func readOwnChanges() async {
    if !synced { _ = try? await sync(); return }
    _ = try? await serial { [self] in
      var report = SyncReport(), ch = Change()
      defer { self.board.project(&ch); self.emit(ch) }
      try? await self.readChanges(&report, &ch)
      try await self.refreshGroups(&ch)
    }
  }
  /** What the hub's answer to an outbox entry must hold to count as "taken"; the change number the core wants. */
  nonisolated static func accepted(_ kind: OutboxKind, _ r: JSON) -> UInt64?? {
    switch kind {
    case .envelope, .commit, .externalCommit, .recoveryCode: return Wire.uint(r["change"]).flatMap { $0 > 0 ? .some(.some($0)) : nil }
    case .message: return Wire.uint(r["n"]) != nil ? .some(nil) : nil
    case .relayMessage: return r["n"] is NSNull ? .some(nil) : nil   // (passed on, not stored: `n` is null)
    case .keyPackages: return Wire.uint(r["unused"]) != nil ? .some(nil) : nil
    case .groupFounding: return r["group_id"] is String ? .some(nil) : nil
    case .roomFounding: return r["room_id"] is String ? .some(nil) : nil
    case .sealedKey: return .some(nil)
    case .recoveryCommit: return r["kept"] as? Bool == true ? .some(nil) : nil
    case .recoveryFinish: return Wire.uint(r["change"]).map { .some($0) }
    }
  }
  /**
   * Waits a moment for the hub's word on what was just put into the outbox: a refusal is thrown here; a network
   * delay is not waited out (the outbox keeps the entry and posts it later).
   */
  func awaitOutcome(_ id: UInt64, ms: UInt64 = 8_000, orThrow: Bool = false) async throws {
    let until = nowMs() + ms
    while nowMs() < until {
      if let o = outcome.removeValue(forKey: id) { if case .failure(let e) = o { throw e }; return }
      try? await Task.sleep(nanoseconds: 40_000_000)
    }
    // `orThrow`: the caller builds on the hub having taken it (a step of several): no word is not a yes.
    if orThrow { throw TrommiError("pending", "the hub has not answered yet: this waits in the outbox and is sent again") }
  }
  /** Waits until the hub has taken everything in the outbox and an own Commit among it was read back and merged. */
  public func flush(timeoutMs: UInt64 = 30_000) async throws {
    _ = try? await serial { }
    let until = nowMs() + timeoutMs
    while let n = try? await onCore({ $0.outbox().count }), n > 0 || pumping {
      if nowMs() > until { throw TrommiError("timeout", "the hub has not taken \(n) item(s)") }
      if !pumping { pumpOutbox() }
      try await Task.sleep(nanoseconds: 50_000_000)
    }
  }

  /** The open cards (decisions and infos) in stack order. */
  public var openCards: [Card] { board.stack.compactMap { board.cards[$0] } }
}

/** Signs hub challenges as the room's device, on the queue the core is called from; gone once the room is closed. */
final class QueueSigner: CoreSigner {
  let id: DeviceId
  private let sign: (RoomId, String, Bytes) throws -> (auth: Bytes, signature: Bytes)
  init(id: DeviceId, sign: @escaping (RoomId, String, Bytes) throws -> (auth: Bytes, signature: Bytes)) { self.id = id; self.sign = sign }
  func signHubAuth(room: RoomId, hub: String, challenge: Bytes) throws -> (auth: Bytes, signature: Bytes) { try sign(room, hub, challenge) }
}
