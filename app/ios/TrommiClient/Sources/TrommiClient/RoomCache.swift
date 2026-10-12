// RoomCache.swift: what this device already opened, kept so the next launch shows the board at once and before any
// network: the records (replayed into the board through the same reducer) and now and then the board as a whole.
// Sealed at rest under a key of this device (RecordStore.swift, Keychain.swift). It is a cache of what was shown,
// not state: keys, chains, the groups and the outbox are the core's, in the device's store (DeviceStore.swift); if
// the cache is damaged or missing it is thrown away and the board is read again from the hub.
import Foundation

extension Room {
  /** The cache's small head: where the records stop. */
  struct Head: Codable {
    var v = 4
    /** How many records the store held when this head was written: fewer on reading means a store damaged beyond its tail. */
    var records: Int
    var cursor: UInt64
    var lamport: Int
  }
  static let snapshotEvery = 2000
  /** The hub's Desk as last read, kept beside the cache (`deskFirst`, `restore`). */
  struct DeskKept: Codable {
    var v = 1
    var open: [String]
    var at: UInt64
  }
  static let deskAAD = "trommi ios desk"
  /** How long the first paint of a launch from the cache waits for the hub's Desk; then the cached Desk stands as
   *  the list kept from the last launch filters it, and "Catching up…" says the rest is on its way. */
  static let deskFirstNs: UInt64 = 300_000_000

  /** Keeps the hub's Desk as read, for the next launch. */
  func keepDesk(_ open: Set<String>, at: UInt64) {
    guard let rs = try? recordStore() else { return }
    // (a few ids: written at once, so that a launch right after finds it)
    try? rs.writeSmall(DeskKept(open: open.sorted(), at: at), to: rs.deskURL, aad: Room.deskAAD)
  }
  /**
   * The first paint of a launch from the cache: until the hub's Desk of this run answered, or `deskFirstNs` passed,
   * the app draws nothing of the Desk (`deskHolding`). Past it the cached Desk stands, filtered by the list kept from
   * the last launch (`restore`), never an empty one (the owner's rule: only a first sign-in may show an empty Desk);
   * the hub's Desk corrects it when it answers.
   */
  func holdDesk() {
    deskHolding = true
    deskHoldTask?.cancel()
    deskHoldTask = Task { @MainActor [weak self] in
      try? await Task.sleep(nanoseconds: Room.deskFirstNs)
      guard let self, !Task.isCancelled, self.deskHolding else { return }
      self.deskHolding = false
      self.deskHoldTask = nil
      self.wakeDeskWaiters()
      var ch = Change(); ch.room = true; ch.stack = true
      self.emit(ch)
    }
  }
  /** The hub's Desk of this run answered (or none comes): what it lists, or the kept list, decides from here. */
  func deskAnswered() {
    guard deskHolding || board.deskWaiting else { return }
    deskHoldTask?.cancel(); deskHoldTask = nil
    deskHolding = false
    board.deskWaiting = false
    wakeDeskWaiters()
    board.project()
    var ch = Change(); ch.room = true; ch.stack = true
    emit(ch)
  }
  func wakeDeskWaiters() { let w = deskWaiters; deskWaiters = []; for c in w { c.resume() } }
  /** Waits until the first paint may be drawn (`holdDesk`): at once when nothing holds it. */
  public func waitForDesk() async {
    if !deskHolding { return }
    await withCheckedContinuation { (c: CheckedContinuation<Void, Never>) in
      if deskHolding { deskWaiters.append(c) } else { c.resume() }
    }
  }

  func recordStore() throws -> RecordStore {
    if let r = recordStoreMemo { return r }
    let r = RecordStore(dir: store.dir, key: try store.cacheKey())
    recordStoreMemo = r
    return r
  }

  /** Write a moment after the last change (a catch-up brings thousands at once). */
  func cacheSoon(_ n: UInt64? = nil) {
    if closed { return }
    if let n = n { dirtyRecs.insert(n) }
    cacheDirty = true
    if cacheTask != nil { return }
    cacheTask = Task { @MainActor [weak self] in
      try? await Task.sleep(nanoseconds: 1_500_000_000)
      self?.cacheTask = nil
      self?.saveCache()
    }
  }
  /** Write it now (the app going to the background). */
  public func saveCache(snapshot: Bool = false) { Task { await saveCacheAndWait(snapshot: snapshot) } }

  /**
   * Appends the changed records and writes the small head, off the main thread, one save after the other. With
   * `snapshot` (the app going to the background), or every `snapshotEvery` records, also the board as a whole; only
   * a settled board is written (nothing of this device in flight).
   */
  public func saveCacheAndWait(snapshot: Bool = false) async {
    let wantSnap = ((synced && sinceSnapshot >= Room.snapshotEvery) || (snapshot && sinceSnapshot >= Room.snapshotEvery / 10)) && ownEchoes.isEmpty && board.settled
    guard !closed, !cacheHeld, cacheDirty || wantSnap, let rs = try? recordStore() else { return }
    cacheDirty = false
    let numbers = dirtyRecs
    let recs = numbers.sorted().compactMap { log[$0] }
    dirtyRecs = []
    let head = Head(records: restoredCount + log.count, cursor: cursor, lamport: lamport)
    var snap: [UInt8]? = nil
    if wantSnap {
      var w = RecCodec.W()
      w.b.append(2); w.u(cursor); w.i(lamport); w.u(UInt64(head.records))
      BoardCodec.encode(board, into: &w)
      snap = w.b
      sinceSnapshot = 0
    }
    let prev = saveTail
    let t = Task.detached(priority: .utility) { () -> Bool in
      _ = await prev?.value
      do {
        try rs.append(recs)
        try rs.writeSmall(head, to: rs.headURL, aad: "trommi ios head")
        if let b = snap { try rs.writeBoard(b) }
        return true
      } catch { return false }
    }
    saveTail = Task { _ = await t.value }
    // What could not be written is written with the next save.
    if !(await t.value) { dirtyRecs.formUnion(numbers); cacheDirty = true }
  }

  /**
   * The board from the cache, before any network: opened and decoded off the main thread, then replayed. Returns
   * false when there is none (or it does not fit: then it is thrown away and the catch-up starts at 0).
   */
  @discardableResult public func restore() async -> Bool {
    if let t = restoring { return await t.value }
    let t = Task { @MainActor in await Spent.time("restore") { await self.restoreOnce() } }
    restoring = t
    return await t.value
  }
  private func restoreOnce() async -> Bool {
    guard let rs = try? recordStore() else { return false }
    typealias Snap = (cursor: UInt64, lamport: Int, records: Int, board: Board)
    let found: (head: Head, snap: Snap?, ix: RecordStore.Index)? = await Task.detached(priority: .userInitiated) {
      guard let h = rs.readSmall(Head.self, from: rs.headURL, aad: "trommi ios head"), h.v == 4 else { return nil }
      var snap: Snap? = nil
      if let plain = rs.readBoard() {
        snap = plain.withUnsafeBufferPointer { buf -> Snap? in
          var r = RecCodec.R(b: buf)
          guard (try? r.byte()) == 2, let c = try? r.u(), let l = try? r.i(), let n = try? r.u() else { return nil }
          let b = Board()
          guard (try? BoardCodec.decode(&r, into: b)) != nil, r.at == buf.count, c <= h.cursor else { return nil }
          return (c, l, Int(n), b)
        }
      }
      guard let ix = rs.index(after: snap.map { Int($0.cursor) }) else { return nil }
      return (h, snap, ix)
    }.value
    guard let f = found else { rs.wipe(); return false }
    // The cache never runs ahead of the device: a cursor beyond what the core processed is the cache of another
    // state (the device's state was put back, or made anew). Catching up from it would skip what the core needs.
    guard f.head.cursor <= coreCursor else { rs.wipe(); return false }
    let base = Int(f.snap?.cursor ?? 0)
    let lo0 = f.ix.entries.firstIndex { $0.n > base } ?? f.ix.entries.count
    let upto = f.ix.entries.firstIndex { $0.n > Int(f.head.cursor) } ?? f.ix.entries.count
    guard upto - lo0 >= f.head.records - (f.snap?.records ?? 0) else { rs.wipe(); return false }
    var ch = Change()
    if let sn = f.snap {
      board.adopt(sn.board)
      try? applyGroups(Array(groups.values), &ch)
    }
    let slice = 2048
    let ix = f.ix
    let parts = stride(from: lo0, to: upto, by: slice).map { lo in
      Task.detached(priority: .userInitiated) { rs.decode(ix, lo..<min(lo + slice, upto)) }
    }
    for p in parts {
      guard let recs = await p.value else {
        for q in parts { q.cancel() }
        rs.wipe(); board.reset()
        board.roomId = record.roomId; board.hubURL = record.hubURL; board.myDeviceId = record.myDeviceId; board.myRole = record.role
        var c = Change(); try? applyGroups(Array(groups.values), &c)
        return false
      }
      for r in recs { board.apply(r, change: &ch) }
    }
    restoredCount = f.head.records
    sinceSnapshot = f.snap == nil ? Room.snapshotEvery : upto - lo0
    cursor = f.head.cursor
    lamport = max(lamport, f.head.lamport)
    board.lastEnvelopeNumber = max(board.lastEnvelopeNumber, Int(cursor))
    // What the hub's Desk last listed decides at once: a card it had settled stays off the Desk although this device
    // never read its closing (it would stand again on every launch). The Desk of this run follows (`deskFirst`); until
    // it answers no open card stands at all (`holdDesk`).
    if let kept = rs.readSmall(DeskKept.self, from: rs.deskURL, aad: Room.deskAAD), kept.v == 1, kept.at <= cursor {
      board.hubSettled = board.settled(byDesk: Set(kept.open), at: Int(kept.at))
    }
    if record.role == "human" { holdDesk() }
    board.project()
    restored = true
    ch.stack = true; ch.room = true
    emit(ch)
    return true
  }
  /**
   * A read back is through (RoomPast.swift tendPast): the cache of before gives way to what it read, as a whole, and
   * the hub's Desk last read is kept again beside it.
   */
  func replaceCache() async {
    await saveTail?.value
    cacheHeld = false
    guard let rs = try? recordStore() else { return }
    await Task.detached(priority: .utility) { rs.wipe() }.value
    if let seen = deskSeen { keepDesk(seen.open, at: seen.at) }
    dirtyRecs = Set(log.keys)
    cacheDirty = true
    sinceSnapshot = Room.snapshotEvery
    await saveCacheAndWait()
  }
  /** Throw the cache away (the board is read again from the hub). */
  public func dropCache() { (try? recordStore())?.wipe(); log = [:]; restoredCount = 0; dirtyRecs = []; cursor = 0 }
}
