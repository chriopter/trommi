// RoomPast.swift: what was in the room before this device came. A device that joined by link, or was added to a
// session group by a Welcome, holds each group from the epoch it joined at; the core takes an envelope of an earlier
// epoch only once it learned the group's past from its public history (`CoreDevice.learnStart`, spec/v1.md 4.6).
//
// What is left to do is kept in room.json (`RoomRecord.past`), so an interrupted run goes on at the next sync:
//
//   learn      per group: the founding GroupInfo and the Commits of its log are fetched page by page and handed to
//              the core in slices (PastWalk.swift), the room group first, then main sessions, then helper sessions; a
//              walk cut short (the hub failed, the app ended) starts again from the founding at the next sync;
//   read back  the board is emptied and the hub's changes are read again from the start, in the hub's order. An
//              envelope below the device's cursor is handed to the core as the next of its sender's chain; one the
//              chain holds already is read again for display. The keys of the old epochs come by the handover of
//              the device that let this one in: an item whose key is not here yet takes its place in its chain
//              without its body, and the changes are read back once more when a handover brought keys.
import Foundation

/** What is left to do so that this device shows what was written before it came. No secret in it. */
public struct PastWork: Codable, Equatable {
  /** The groups (hex) whose past is still to be learned. */
  public var toLearn: [String] = []
  /** An envelope was passed over because its group's past was not known: reading back is worth it once a past is learned. */
  public var passedOver = false
  /** An item took its place without its body, for want of its epoch's key: reading back is worth it once keys come. */
  public var closed = false
  /** The changes are to be read again from the start; false again once that read came to its end. */
  public var readBack = false
  /** The read back is for a Cut (the core dropped what a removed device signed beyond it): nothing is taken from the
   *  cache, everything is judged by the core again. nil: no. */
  public var afterCut: Bool? = nil
  /** What the passed over and closed items wait for: "group/epoch" (hex) of each whose key this device did not hold
   *  when it came. Keys that came read back only once one of these is held. nil: not known (an older record, or
   *  more than `waitingMax`): any keys read back. */
  public var waiting: [String]? = []
  /** The bytes of every passed over and closed item since the last read back are kept beside the cache
   *  (RecordStore.keepWaiting): a read back takes the rest from the cache and reads nothing again from the hub. */
  public var rawComplete: Bool? = nil
  public init(toLearn: [String] = []) { self.toLearn = toLearn; rawComplete = true }
}

extension Room {
  /** Reading back puts what is held whole back without the core (off only to measure the difference: MegaRunTests). */
  nonisolated(unsafe) static var readBackFromCache = true
  static let waitingMax = 512
  /** At most this many bytes of waiting items are kept; past it a read back reads the hub's changes again. */
  static let waitingBytesMax = 16 << 20

  /**
   * An item was passed over or closed for want of something (`group-behind`, `no-key`): what it waits for is noted
   * (`PastWork.waiting`, only a key this device did not hold) and its bytes kept beside the cache.
   */
  func noteWaiting(change n: UInt64, bytes: Bytes, group: GroupId, epoch: UInt64, keyHeld: Bool?) {
    let place = "\(hex(group))/\(epoch)"
    notePast { p in
      guard keyHeld == false, var w = p.waiting, !w.contains(place) else { return }
      w.append(place)
      p.waiting = w.count > Room.waitingMax ? nil : w
    }
    guard waitingBytes + bytes.count <= Room.waitingBytesMax, let rs = try? recordStore() else { waitingDropped = true; return }
    waitingBytes += bytes.count
    let prev = waitingTail
    waitingTail = Task.detached(priority: .utility) { [weak self] in
      _ = await prev?.value
      if (try? rs.keepWaiting(n, bytes)) == nil { await MainActor.run { self?.waitingDropped = true } }
    }
  }
  /** Keys came (a handover): a read back is due if they open what waits, or if what waits is not known. */
  func keysCame() {
    guard record.past?.closed == true || record.past?.passedOver == true else { return }
    if record.past?.waiting == nil { notePast { $0.readBack = true }; if readingBack { readBackAgain = true } }
    pastDue = true
  }
  /** Whether a key that a waiting item lacked is held now. */
  private func waitingOpens() async -> Bool {
    guard let w = record.past?.waiting, !w.isEmpty else { return false }
    let places = w.compactMap { s -> (GroupId, UInt64)? in
      let parts = s.split(separator: "/")
      guard parts.count == 2, let g = try? unhex(String(parts[0])), let e = UInt64(parts[1]) else { return nil }
      return (g, e)
    }
    return (try? await onCore { device in places.contains { (try? device.holdsKey(group: $0.0, epoch: $0.1)) == true } }) ?? false
  }

  /** Changes what is left to do for the past and writes it down, if it changed. */
  func notePast(_ edit: (inout PastWork) -> Void) {
    var past = record.past ?? PastWork()
    edit(&past)
    guard past != (record.past ?? PastWork()) else { return }
    record.past = past
    try? store.save(record)
  }

  /** Room group, main sessions, helper sessions: the order the core learns in. */
  private func learningRank(_ group: String) -> Int {
    guard let session = groups[group]?.session else { return 0 }
    return session.parent == nil ? 1 : 2
  }

  /**
   * Walks the past of `group` (PastWalk.swift): its founding GroupInfo, then its log page by page, each page in
   * slices, until the walk reaches the epoch this device's own knowledge begins at; then the core compares and
   * writes. A failure, of the hub or of the core, ends the walk: the next try starts again from the founding.
   * How many epochs were recorded.
   */
  private func learnPast(of group: GroupId) async throws -> UInt64 {
    let founding = try await noted { try await hub.groupInfoBytes(group, epoch: 0) }.bytes
    var progress = try await onCore { try $0.learnStart(group: group, founding: founding) }
    do {
      var pages = GroupLogPages(hub: hub, group: group)
      var cap = Slices.maxCommits
      while !progress.reachedEnd, let page = try await noted({ try await pages.next() }) {
        let (reached, halved) = try await onCore { device -> (LearnProgress, Int) in
          var at = progress, cap = cap
          try Slices.feed(page, size: { $0.commit.count }, cap: &cap) { slice in
            at = try device.learnSlice(group: group, commits: slice)
            return !at.reachedEnd
          }
          return (at, cap)
        }
        progress = reached; cap = halved
      }
      return try await onCore { try $0.learnFinish(group: group) }
    } catch {
      try? await onCore { $0.learnAbandon(group: group) }
      throw error
    }
  }

  /**
   * Does what is left to do for the past (see the head of this file); nothing when nothing is left. Inside `serial`.
   * A failure of the hub is thrown with everything still noted: the next sync goes on. A group whose turn has not
   * come (`room-behind`, `group-behind`) stays noted; a history that is not the group's own (`bad-group`) is a
   * finding about the hub, and that group's past stays closed.
   */
  func tendPast(_ report: inout SyncReport, _ change: inout Change) async throws {
    pastDue = false
    // (keys that came open what waits: read back)
    if record.past?.readBack != true, record.past?.closed == true || record.past?.passedOver == true {
      if await waitingOpens() { notePast { $0.readBack = true } }
    }
    guard let noted = record.past, !noted.toLearn.isEmpty || noted.readBack else { return }
    for id in noted.toLearn.sorted(by: { learningRank($0) < learningRank($1) }) {
      // (a group this device is no longer in has no past to show)
      guard groups[id] != nil, let group = try? unhex(id) else { notePast { $0.toLearn.removeAll { $0 == id } }; continue }
      do {
        let epochs = try await learnPast(of: group)
        notePast { $0.toLearn.removeAll { $0 == id }; if epochs > 0 && $0.passedOver { $0.readBack = true } }
      } catch let e as TrommiError where e.code == "room-behind" || e.code == "group-behind" {
        continue
      } catch let e as TrommiError where e.code == "bad-group" {
        board.pushAlert(&change, code: "bad-group", message: "the hub served a history that is not this group's: what was written before this device came stays closed")
        notePast { $0.toLearn.removeAll { $0 == id } }
      }
    }
    guard record.past?.readBack == true else { return }
    // The board and its cache are built again from the hub's changes. What the core holds (keys, chains, objects)
    // stays; the two notes are taken anew by this read.
    await saveTail?.value
    await waitingTail?.value
    // What this device holds whole already (this run's records and the cache's) is put back as it is, without the
    // core: only what came without its body goes through the core again. (A big room read every envelope twice
    // here, one by one, each time keys came: most of a long "Catching up".)
    let rs = try? recordStore()
    let (cached, waitingRaw): ([Rec], [UInt64: Bytes]?) = await Task.detached(priority: .userInitiated) { (rs?.readAll() ?? [], rs?.readWaiting()) }.value
    // From the cache alone when the bytes of every item that waits are kept: what is held is put back as it is, and
    // only those go through the core again; nothing is read again from the hub. Else the hub's changes from the
    // start, what is held whole put back without the core.
    let fromCache = Room.readBackFromCache && record.past?.afterCut != true && record.past?.rawComplete == true && waitingRaw != nil
    var whole = [UInt64: Rec]()
    for r in cached where fromCache || r.opened { whole[UInt64(r.envelopeNumber)] = r }
    for (n, r) in log where fromCache || r.opened { whole[n] = r }
    if fromCache { for n in waitingRaw!.keys { whole[n] = nil } }
    readBackWhole = Room.readBackFromCache && record.past?.afterCut != true ? whole : [:]
    defer { readBackWhole = [:] }
    let was = cursor
    // The Desk on screen stays as it was until the read is through: the views show a copy of the board as it stood
    // (`readBackView`, kept also when the read fails half way, until one is through); its progress is shown. The
    // cache on disk stays too (`cacheHeld`): an app ended meanwhile starts warm from it and reads back again.
    if readBackView == nil { readBackView = board.heldCopy() }
    catchingUp = (0, max(cursor, coreCursor))
    defer { catchingUp = nil }
    cacheHeld = true
    log = [:]; restoredCount = 0; dirtyRecs = []; cacheDirty = false; cursor = 0
    board.reset()
    try applyGroups(Array(groups.values), &change)
    notePast { $0.passedOver = false; $0.closed = false; $0.afterCut = nil; $0.waiting = []; $0.rawComplete = false }
    // (what waits is noted anew by this read, its bytes kept anew)
    rs?.wipeWaiting()
    waitingBytes = 0; waitingDropped = false
    readingBack = true
    readBackAgain = false
    defer { readingBack = false }
    // (a board shown is loaded anew from its snapshot by the core: what it holds now may lack what it showed)
    boardsReset &+= 1
    Spent.count(fromCache ? "read backs from the cache" : "read backs")
    if fromCache {
      // the cache's items and the waiting ones in the hub's order, as the hub's pages would bring them
      let numbers = Set(whole.keys).union(waitingRaw!.keys).sorted()
      for at in stride(from: 0, to: numbers.count, by: 2000) {
        let part = numbers[at..<min(at + 2000, numbers.count)].map { Item.envelope(change: $0, bytes: waitingRaw![$0] ?? [], voidCode: nil) }
        try await process(part, report: &report, change: &change)
      }
      cursor = max(cursor, was)
      board.lastEnvelopeNumber = max(board.lastEnvelopeNumber, Int(cursor))
    }
    try await readChanges(&report, &change)
    // (keys that came during this read, after it had passed what they open: read back once more)
    let opens = await waitingOpens()
    let again = readBackAgain || opens
    await waitingTail?.value   // (the waiting items' bytes are written)
    let complete = !waitingDropped
    notePast { $0.readBack = again; $0.rawComplete = complete }
    readingBack = false
    // (the read reads the live sessions alone: the archived ones this device opened are loaded again, still behind the
    // held view, so that the cache that replaces the one of before holds them too)
    await reloadArchived(&report, &change)
    // the cache of before gives way to the board read back
    await replaceCache()
    readBackView = nil
    change.room = true; change.stack = true
    // (each further pass needs keys that came during the one before: it ends)
    if again { try await tendPast(&report, &change) }
  }
}
