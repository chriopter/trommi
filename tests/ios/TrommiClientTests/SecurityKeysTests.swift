// SecurityKeysTests: what Settings → Security shows of the keys that open the account's data, as the web's
// tests/web/model/keys.test.mjs: when each device came in (RoomMember.addedAt, from the Commit that added it) and the
// room's recovery key (Board.recoveryKey: its fingerprint, and since when it is in force), kept in the board's cache.
import XCTest
@testable import TrommiClient

final class SecurityKeysTests: XCTestCase {
  let room: Bytes = Array(repeating: 0xaa, count: 32)
  let session: Bytes = Array(repeating: 0xbb, count: 48)
  func dev(_ k: UInt8) -> Bytes { var b = Bytes(repeating: 0, count: 32); b[0] = 0xd0; b[1] = k; return b }
  func key(_ tag: UInt8...) -> Bytes { var b = Bytes(repeating: 0, count: 32); for (i, t) in tag.enumerated() { b[i] = t }; return b }
  func facts(_ group: Bytes, epoch: UInt64 = 3, committer: Bytes? = nil, external: Bool = false, adds: [Bytes] = [], agents: [Bytes]? = nil, time: UInt64?) -> CommitFacts {
    CommitFacts(group: group, epoch: epoch, committer: committer ?? dev(1), external: external, adds: adds, agents: agents, time: time)
  }
  func board() -> Board {
    let b = Board(); b.myDeviceId = hex(dev(1))
    var ch = Change()
    b.applyMembers([(id: hex(dev(1)), role: "human", active: true, added: 0, removed: nil), (id: hex(dev(2)), role: "human", active: true, added: 0, removed: nil),
                    (id: hex(dev(10)), role: "agent", active: true, added: 0, removed: nil)], change: &ch)
    return b
  }

  func testDevicesAreDatedByTheCommitThatAddedThem() {
    let b = board()
    var ch = Change()
    XCTAssertNil(b.members[hex(dev(2))]?.addedAt, "a device the lists held when this device came: unknown")
    b.applyCommitFacts(facts(room, adds: [dev(3)], time: 1000), room: true, change: &ch)
    XCTAssertEqual(b.members[hex(dev(3))]?.addedAt, 1000); XCTAssertEqual(b.members[hex(dev(3))]?.deviceRole, "human"); XCTAssertTrue(ch.members)
    b.applyCommitFacts(facts(room, epoch: 4, committer: dev(4), external: true, time: 2000), room: true, change: &ch)
    XCTAssertEqual(b.members[hex(dev(4))]?.addedAt, 2000, "a join with the recovery code")
    b.applyCommitFacts(facts(room, epoch: 5, agents: [dev(10), dev(12)], time: 3000), room: true, change: &ch)
    XCTAssertEqual(b.members[hex(dev(12))]?.addedAt, 3000); XCTAssertEqual(b.members[hex(dev(12))]?.deviceRole, "agent")
    XCTAssertNil(b.members[hex(dev(10))]?.addedAt, "an agent named again is not dated anew")
    b.applyCommitFacts(facts(room, epoch: 6, adds: [dev(3)], time: 9000), room: true, change: &ch)
    XCTAssertEqual(b.members[hex(dev(3))]?.addedAt, 1000, "a date stays")
    // a session: a helper device is new; a human device added to it is not
    b.applyCommitFacts(facts(session, epoch: 2, committer: dev(10), adds: [dev(20), dev(2)], time: 4000), room: false, change: &ch)
    XCTAssertEqual(b.members[hex(dev(20))]?.addedAt, 4000); XCTAssertNil(b.members[hex(dev(2))]?.addedAt)
  }

  func testOwnCommitAfterTheListsStillDates() {
    let b = board()
    var ch = Change()
    b.applyMembers([(id: hex(dev(3)), role: "human", active: true, added: 0, removed: nil)], change: &ch)   // the lists first
    b.applyCommitFacts(facts(room, adds: [dev(3)], time: 77), room: true, change: &ch)
    XCTAssertEqual(b.members[hex(dev(3))]?.addedAt, 77)
  }

  func testRecoveryKeyIsDatedOnlyByTheOneRoomCommitThatReplacedIt() {
    let b = board()
    var ch = Change()
    b.applyRecoveryKey(key(0xc4, 0x1e, 0x8b, 0x07), change: &ch)
    XCTAssertEqual(b.recoveryKey, RecoveryKey(fingerprint: "c41e 8b07 0000 0000", since: nil)); XCTAssertTrue(ch.room)
    XCTAssertEqual(keyFingerprint(key(0xab, 0xcd)), "abcd 0000 0000 0000")
    b.applyCommitFacts(facts(room, time: 100), room: true, change: &ch); b.applyRecoveryKey(key(0xc4, 0x1e, 0x8b, 0x07), change: &ch)
    XCTAssertNil(b.recoveryKey?.since, "the same key: nothing changes")
    b.applyCommitFacts(facts(room, time: 200), room: true, change: &ch); b.applyRecoveryKey(key(0x11), change: &ch)
    XCTAssertEqual(b.recoveryKey, RecoveryKey(fingerprint: "1100 0000 0000 0000", since: 200))
    b.applyCommitFacts(facts(room, time: 300), room: true, change: &ch); b.applyCommitFacts(facts(room, time: 400), room: true, change: &ch); b.applyRecoveryKey(key(0x22), change: &ch)
    XCTAssertEqual(b.recoveryKey, RecoveryKey(fingerprint: "2200 0000 0000 0000", since: nil), "a catch-up of two: which one is not known")
    b.applyCommitFacts(facts(session, time: 500), room: false, change: &ch); b.applyRecoveryKey(key(0x33), change: &ch)
    XCTAssertNil(b.recoveryKey?.since, "a session Commit dates nothing")
  }

  func testTheCacheKeepsBoth() throws {  // (the client core's snapshot)
    let b = board()
    var ch = Change()
    b.applyRecoveryKey(key(0x44), change: &ch)
    b.applyCommitFacts(facts(room, adds: [dev(3)], time: 11), room: true, change: &ch); b.applyRecoveryKey(key(0x55), change: &ch)
    let back = Board()
    XCTAssertTrue(back.loadModel(b.modelSnapshot()))
    XCTAssertEqual(back.members[hex(dev(3))]?.addedAt, 11); XCTAssertNil(back.members[hex(dev(2))]?.addedAt)
    XCTAssertEqual(back.recoveryKey, RecoveryKey(fingerprint: "5500 0000 0000 0000", since: 11))
  }
}
