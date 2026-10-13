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
  public init(toLearn: [String] = []) { self.toLearn = toLearn }
}

extension Room {
  /** Reading back puts what is held whole back without the core (off only to measure the difference: MegaRunTests). */
  nonisolated(unsafe) static var readBackFromCache = true

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
    // What this device holds whole already (this run's records and the cache's) is put back as it is, without the
    // core: only what came without its body goes through the core again. (A big room read every envelope twice
    // here, one by one, each time keys came: most of a long "Catching up".)
    let rs = try? recordStore()
    let cached: [Rec] = await Task.detached(priority: .userInitiated) { rs?.readAll() ?? [] }.value
    var whole = [UInt64: Rec]()
    for r in cached where r.opened { whole[UInt64(r.envelopeNumber)] = r }
    for (n, r) in log where r.opened { whole[n] = r }
    readBackWhole = Room.readBackFromCache && record.past?.afterCut != true ? whole : [:]
    defer { readBackWhole = [:] }
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
    notePast { $0.passedOver = false; $0.closed = false; $0.afterCut = nil }
    readingBack = true
    readBackAgain = false
    defer { readingBack = false }
    // (a board shown is loaded anew from its snapshot by the core: what it holds now may lack what it showed)
    boardsReset &+= 1
    Spent.count("read backs")
    try await readChanges(&report, &change)
    // (keys that came during this read, after it had passed what they open: read back once more)
    let again = readBackAgain
    notePast { $0.readBack = again }
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
