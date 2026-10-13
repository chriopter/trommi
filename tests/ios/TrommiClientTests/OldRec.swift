// OldRec.swift: the record the board's reducer took before the model was the client core's (core/client/src/model), for
// the tests written in its words: a decoded envelope (`content` in the model's names). `Board.apply(_:change:)` turns
// it into what the client core reads (an envelope as the core took it: the body in the wire's names, the object as the
// core's replay leaves it) and hands it to the core's model. `applyMembers` and `applySession` give the core the
// groups those members and sessions make.
import Foundation
@testable import TrommiClient

public enum DecodedBind: Equatable {
  case answer(cardId: String, versionHash: String, choices: [String])
  case decideAgain(cardId: String, previousHash: String, versionHash: String)
  case permissionRequest(requestId: String, expiresAt: UInt64)
  case verdict(requestId: String, requestHash: String, expiresAt: UInt64, allow: Bool)
}

public struct ObjectHead: Equatable {
  public var objectId: String
  public var objectState: Int
  public var urgency: Int
  public var answeredAt: UInt64
}

/** A decoded envelope in the reducer's words of before. */
public struct OldRec {
  public var envelopeNumber: Int
  public var envelopeHash: String
  public var senderDeviceId: String
  public var senderRole: String
  public var recipientDeviceId: String?
  public var sentAt: UInt64
  public var kind: Int
  public var isHead: Bool
  public var object: ObjectHead?
  public var timelineKind: String?
  public var timelineId: String?
  public var sessionId: String?
  public var content: JV?
  public var contentState: String
  public var bind: DecodedBind?
  public var causal: Causal
  public var senderSequence: UInt64
  public var epoch: Int
  public var objectIdOk: Bool? = nil
  public var localId: String? = nil
  public var pending = false
  public init(envelopeNumber: Int, envelopeHash: String, senderDeviceId: String, senderRole: String, recipientDeviceId: String? = nil, sentAt: UInt64, kind: Int, isHead: Bool,
              object: ObjectHead? = nil, timelineKind: String? = nil, timelineId: String? = nil, sessionId: String? = nil, content: JV? = nil, contentState: String,
              bind: DecodedBind? = nil, causal: Causal, senderSequence: UInt64, epoch: Int, objectIdOk: Bool? = nil, localId: String? = nil) {
    self.envelopeNumber = envelopeNumber; self.envelopeHash = envelopeHash; self.senderDeviceId = senderDeviceId; self.senderRole = senderRole
    self.recipientDeviceId = recipientDeviceId; self.sentAt = sentAt; self.kind = kind; self.isHead = isHead; self.object = object
    self.timelineKind = timelineKind; self.timelineId = timelineId; self.sessionId = sessionId; self.content = content; self.contentState = contentState
    self.bind = bind; self.causal = causal; self.senderSequence = senderSequence; self.epoch = epoch; self.objectIdOk = objectIdOk; self.localId = localId
  }

  /** The envelope as the client core reads it. */
  var received: JV {
    let state = ["open", "open", "answered", "closed"][min(3, max(0, object?.objectState ?? 1))]
    let urgency = ["low", "normal", "high", "critical"][min(3, max(0, object?.urgency ?? 1))]
    let kinds = [KIND.TIMELINE_ITEM: "item", KIND.OBJECT_VERSION: "version", KIND.ANSWER: "answer", KIND.PERMISSION_REQUEST: "request", KIND.VERDICT: "verdict",
                 KIND.STATUS: "register", KIND.DECIDE_AGAIN: "take_back"]
    let kindName = kinds[kind] ?? "reserved"
    var out: [String: JV] = ["change": .num(Double(envelopeNumber)), "hash": .str(envelopeHash.isEmpty ? String(format: "%064x", envelopeNumber) : envelopeHash),
                             "kind": .str(kindName), "sender": .str(senderDeviceId), "seq": .num(Double(senderSequence)), "time": .num(Double(sentAt)),
                             "recipient": recipientDeviceId.map(JV.str) ?? .null, "session_id": sessionId.map(JV.str) ?? .null, "code": .null]
    // the body: the model's names put back into the wire's (one register per envelope)
    var body: JV? = content
    if kind == KIND.STATUS, let values = content?["values"].object, let (k, v) = values.first {
      body = .obj(["name": .str(Records.registerName(k, toWire: true)), "value": Records.registerValue(k, v, toWire: true), "lamport": .num(Double(causal.lamport))])
      out["register"] = .obj(["name": .str(Records.registerName(k, toWire: true)), "current": true])
    } else if let c = content {
      body = Records.renameFiles(c, toWire: true, depth: 0)
      if kind == KIND.TIMELINE_ITEM, timelineKind == "scribble" { body = Records.boardItem(c, toWire: true) }
    }
    switch contentState {
    case "ok": out["outcome"] = pending ? "provisional" : "applied"; if let b = body { out["payload"] = .str(String(decoding: b.encoded(), as: UTF8.self)) }
    case "newer_schema": out["outcome"] = "applied"; out["payload"] = .str(String(decoding: JV.obj(["schema_version": 9]).encoded(), as: UTF8.self))
    case "pruned": out["outcome"] = "chained"; out["code"] = "pruned"
    default: out["outcome"] = "chained"; out["code"] = "no-key"
    }
    if let tk = timelineKind, let tid = timelineId {
      let slash = tid.firstIndex(of: "/") ?? tid.endIndex
      let scope = String(tid[..<slash]), id = slash < tid.endIndex ? String(tid[tid.index(after: slash)...]) : ""
      out["timeline"] = .obj(["kind": .str(tk == "scribble" ? "board" : scope == "card" ? "card_chat" : "session_chat"), "id": .str(id)])
    }
    var head = object
    if head == nil {
      switch bind {
      case .answer(let id, _, _)?, .decideAgain(let id, _, _)?: head = ObjectHead(objectId: id, objectState: kind == KIND.DECIDE_AGAIN ? 1 : 2, urgency: 1, answeredAt: 0)
      case .permissionRequest(let id, _)?: head = ObjectHead(objectId: id, objectState: 1, urgency: 1, answeredAt: 0)
      case .verdict(let id, _, _, _)?: head = ObjectHead(objectId: id, objectState: 3, urgency: 1, answeredAt: 0)
      case nil: break
      }
    }
    if let o = head {
      let type: String
      switch kind {
      case KIND.PERMISSION_REQUEST, KIND.VERDICT: type = "request"
      case KIND.OBJECT_VERSION: type = content?["object_type"].string.map { $0 == "published" ? "artifact" : $0 } ?? "card"
      default: type = "card"
      }
      var ref: JV = .null
      if let c = content, let p = c["previous_version_hash"].string, !p.allSatisfy({ $0 == "0" }) { ref = .str(p) }
      if case .answer(_, let v, _)? = bind { ref = .str(v) }
      out["object"] = .obj(["object_id": .str(o.objectId), "object_type": .str(type), "object_state": .str(state), "urgency": .str(urgency),
                            "answered_at": .num(Double(o.answeredAt)), "object_ref": ref])
      if !pending {
        out["object_after"] = .obj(["owner": .str(senderDeviceId), "object_state": .str(state), "current": out["hash"]!])
      }
    }
    switch bind {
    case .answer(_, let v, let choices)?: out["bind"] = .obj(["kind": "answer", "version_hash": .str(v), "choices": .arr(choices.map(JV.str))])
    case .decideAgain(_, let p, _)?: out["bind"] = .obj(["kind": "take_back", "previous_hash": .str(p)])
    case .permissionRequest(_, let e)?: out["bind"] = .obj(["kind": "request", "expires_at": .num(Double(e))])
    case .verdict(_, _, _, let allow)?: out["bind"] = .obj(["kind": "verdict", "allow": .bool(allow)])
    case nil: break
    }
    return .obj(out)
  }
}

/** The groups a test's members and sessions make, per board. */
private final class TestGroups {
  var members: [(id: String, role: String, active: Bool)] = []
  var sessions: [(id: String, agents: [String], epoch: Int, parent: String?, archived: Bool)] = []
  var recovery: Bytes?
}
private final class WeakBoard { weak var board: Board?; let groups = TestGroups(); init(_ b: Board) { board = b } }
private var testGroups: [ObjectIdentifier: WeakBoard] = [:]

extension Board {
  /** One decoded envelope into the core's model. */
  @discardableResult func apply(_ rec: OldRec, change: inout Change) -> (applied: Bool, refused: String?) {
    // (a register is one name per envelope: several names are several envelopes)
    var list = [rec.received]
    if rec.kind == KIND.STATUS, let values = rec.content?["values"].object, values.count > 1 {
      list = values.keys.sorted().enumerated().map { i, k in
        var one = rec
        one.content = .obj(["values": .obj([k: values[k]!])])
        one.envelopeHash = String(format: "%064x", rec.envelopeNumber * 1000 + i)
        return one.received
      }
    }
    let r = apply(received: list).first ?? (false, nil)
    project(&change)
    return r
  }
  // (a board's memory is used again by a later board: the groups go with the board itself)
  private var tg: TestGroups {
    if let w = testGroups[ObjectIdentifier(self)], w.board === self { return w.groups }
    let w = WeakBoard(self); testGroups[ObjectIdentifier(self)] = w; return w.groups
  }
  private func pushGroups() {
    let g = tg
    let humans = g.members.filter { $0.role == "human" && $0.active }.map(\.id)
    let agents = g.members.filter { $0.role == "agent" && $0.active }.map(\.id)
    let room: JV = .obj(["group": .str(String(repeating: "e", count: 64)), "session": .null, "leaves": .arr(humans.map(JV.str)), "disallowed": 0, "archived": false, "epoch": 4])
    let sessions: [JV] = g.sessions.map { s in
      .obj(["group": .str(String(repeating: "e", count: 64) + s.id), "session": .obj(["session_id": .str(s.id), "parent": s.parent.map(JV.str) ?? .null]),
            "leaves": .arr((humans + s.agents).map(JV.str)), "disallowed": 0, "archived": .bool(s.archived), "epoch": .num(Double(s.epoch))])
    }
    let roles: JV = .obj(["epoch": 4, "humans": .arr(humans.map(JV.str)), "agents": .arr(agents.map(JV.str)), "recovery_fingerprint": g.recovery.map { .str(keyFingerprint($0)) } ?? .null])
    try? engine.applyGroupsJson(groups: String(decoding: JV.arr([room] + sessions).encoded(), as: UTF8.self), roles: String(decoding: roles.encoded(), as: UTF8.self))
  }
  /** Members as the room's groups name them. */
  func applyMembers(_ list: [(id: String, role: String, active: Bool, added: Int, removed: Int?)], change: inout Change) {
    tg.members = list.map { ($0.id, $0.role, $0.active) }
    pushGroups(); project(&change)
  }
  /** The room's recovery key as the core holds it (read with the groups). */
  func applyRecoveryKey(_ key: Bytes?, change: inout Change) { tg.recovery = key; pushGroups(); project(&change) }
  /** What a processed Commit did. */
  func applyCommitFacts(_ f: CommitFacts, room: Bool, change: inout Change) { applyCommitFacts(f, room: room); project(&change) }
  /** One session as its group says it. */
  func applySession(sessionId: String, agentIds: [String], epoch: Int, parentSessionId: String?, archived: Bool, opener: String? = nil, change: inout Change) {
    let g = tg
    for a in agentIds where !g.members.contains(where: { $0.id == a }) { g.members.append((a, "agent", true)) }
    if let o = opener, !g.members.contains(where: { $0.id == o }) { g.members.append((o, "agent", true)) }
    g.sessions.removeAll { $0.id == sessionId }
    g.sessions.append((sessionId, (opener.map { [$0] } ?? []) + agentIds.filter { $0 != opener }, epoch, parentSessionId, archived))
    pushGroups(); project(&change)
  }
}

extension Board {
  /** A card version from an agent, at change `n`: the board's tests' short way to put a card into the core's model. */
  func feedCard(_ id: String, at n: Int, state: String = "open", agent: String = "bb", session: String? = nil, title: String? = nil) {
    let code = ["open": 1, "answered": 2, "closed": 3][state] ?? 1
    let rec = OldRec(envelopeNumber: n, envelopeHash: String(format: "%064x", n), senderDeviceId: agent, senderRole: "agent", sentAt: UInt64(n), kind: KIND.OBJECT_VERSION, isHead: true,
                     object: ObjectHead(objectId: id, objectState: code, urgency: 1, answeredAt: 0), sessionId: session,
                     content: ["object_type": "card", "card_type": "decision", "title": .str(title ?? id)], contentState: "ok",
                     causal: Causal(senderDeviceId: agent, senderSequence: UInt64(n), sentAt: UInt64(n), lamport: n), senderSequence: UInt64(n), epoch: 1)
    var ch = Change()
    apply(rec, change: &ch)
  }
}
