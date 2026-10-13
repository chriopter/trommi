// BoardMirror.swift: the board model is the client core's (core/client/src/model, TrommiCoreRust's `ClientModel`); this
// is its copy in the app's classes. The core hands, once per batch, the patch of what changed (JSON in the web model's
// shape: snake_case, ids hex); `Board.take(patch:)` puts it into the classes the views read and says what changed.
import Foundation

private func u64(_ v: JV) -> UInt64 { v.double.map { UInt64(max(0, $0)) } ?? 0 }
private func optU64(_ v: JV) -> UInt64? { v.double.map { UInt64(max(0, $0)) } }
private func int(_ v: JV) -> Int { v.int ?? 0 }
private func optInt(_ v: JV) -> Int? { v.int }
private func str(_ v: JV) -> String { v.string ?? "" }
private func strings(_ v: JV) -> [String] { (v.array ?? []).compactMap { $0.string } }

func causalOf(_ v: JV) -> Causal? {
  guard v.object != nil else { return nil }
  return Causal(senderDeviceId: str(v["sender_device_id"]), senderSequence: u64(v["sender_sequence"]), sentAt: u64(v["sent_at"]), lamport: int(v["lamport"]))
}
func linkOf(_ v: JV) -> AgentLink? {
  guard v.object != nil else { return nil }
  return AgentLink(hears: str(v["hears"]), attached: v["attached"].bool ?? true, lastCallAt: optU64(v["last_call_at"]), working: v["working"].bool ?? false,
                   since: optU64(v["since"]), cutSince: optU64(v["cut_since"]), exitReason: v["exit"]["reason"].string, exitClaude: v["exit"]["claude"].string)
}
func answerOf(_ v: JV) -> Answer {
  var notes: [String: String] = [:]
  for (k, x) in v["option_notes"].object ?? [:] { if let s = x.string { notes[k] = s } }
  return Answer(answerAction: v["answer_action"].string ?? "answer", choices: strings(v["choices"]), note: v["note"].string, optionNotes: notes,
                attachments: v["attachments"].array ?? [], marks: v["marks"].array ?? [], trusted: v["trusted"].truthy, boundObjectVersion: int(v["bound_object_version"]),
                envelopeNumber: optInt(v["envelope_number"]), envelopeHash: v["envelope_hash"].string, byDeviceId: str(v["by_device_id"]),
                answeredAt: u64(v["answered_at"]), takenBackAt: optInt(v["taken_back_at"]), takenBackSentAt: optU64(v["taken_back_sent_at"]), pending: v["pending"].truthy)
}
func itemOf(_ v: JV) -> TimelineItem {
  TimelineItem(envelopeNumber: optInt(v["envelope_number"]), localId: v["local_id"].string, pending: v["pending"].truthy || v["provisional"].truthy, envelopeHash: v["envelope_hash"].string,
               senderDeviceId: str(v["sender_device_id"]), senderSequence: optU64(v["sender_sequence"]), recipientDeviceId: v["recipient_device_id"].string,
               sentAt: u64(v["sent_at"]), itemState: v["item_state"].string ?? "header", contentType: v["content_type"].string, content: v["content"].isNull ? nil : v["content"])
}

extension Board {
  func cardOf(_ v: JV) -> Card {
    let c = Card(snapshot: str(v["object_id"]), agent: str(v["agent_device_id"]))
    c.agentDeviceId = str(v["agent_device_id"])
    c.objectState = v["object_state"].string ?? "open"
    c.urgency = v["urgency"].string ?? "normal"
    var f: [String: JV] = [:]
    for k in CARD_CONTENT_FIELDS { let x = v[k]; if !x.isNull { f[k] = x } }
    c.ownFields = f
    if v["lean"].truthy {
      // (a lean card: what it left out is read from the client core when first asked)
      c.lean = true
      c.lazyFields = Set(strings(v["lazy"]))
      let id = c.objectId
      c.whole = { [weak self] in (try? self?.engine.cardJson(objectId: id)).flatMap { $0.isEmpty ? nil : JV.parse(Bytes($0.utf8)) } }
    }
    c.objectVersion = int(v["object_version"])
    c.versionHash = v["version_hash"].string
    c.envelopeNumber = int(v["envelope_number"])
    c.firstEnvelopeNumber = int(v["first_envelope_number"])
    c.createdAt = u64(v["created_at"])
    c.sessionId = v["session_id"].string
    c.updatedAt = u64(v["updated_at"])
    c.versions = (v["versions"].array ?? []).map {
      CardVersion(objectVersion: int($0["object_version"]), versionHash: str($0["version_hash"]), previousVersionHash: $0["previous_version_hash"].string,
                  envelopeNumber: int($0["envelope_number"]), sentAt: u64($0["sent_at"]), objectState: str($0["object_state"]), urgency: str($0["urgency"]),
                  content: $0["content"].isNull ? nil : $0["content"], owner: c)
    }
    c.history = (v["history"].array ?? []).map {
      Revision(objectVersion: int($0["object_version"]), envelopeNumber: int($0["envelope_number"]), sentAt: u64($0["sent_at"]), note: str($0["note"]), title: str($0["title"]))
    }
    c.answer = v["answer"].isNull ? nil : answerOf(v["answer"])
    c.answers = (v["answers"].array ?? []).map(answerOf)
    c.closedHow = v["closed_how"].string
    c.inRevision = v["in_revision"].isNull ? nil : InRevision(by: str(v["in_revision"]["by"]), envelopeNumber: int(v["in_revision"]["envelope_number"]))
    c.timelineKey = v["timeline_key"].string ?? timelineKeyOf("chat", "card/\(c.objectId)")
    c.contentState = v["content_state"].string ?? "header"
    c.unsupported = !v["unsupported"].isNull
    c.catchingUp = v["catching_up"].truthy
    c.snoozedUntil = optU64(v["snoozed_until"]); c.snoozedAt = optU64(v["snoozed_at"]); c.unsnoozed = optU64(v["unsnoozed"])
    return c
  }
  func permissionOf(_ v: JV) -> Permission {
    let p = Permission(objectId: str(v["object_id"]), agentDeviceId: str(v["agent_device_id"]), sessionId: v["session_id"].string, toolName: str(v["tool_name"]),
                       description: str(v["description"]), inputPreview: str(v["input_preview"]), expiresAt: u64(v["expires_at"]), versionHash: str(v["version_hash"]),
                       envelopeNumber: int(v["envelope_number"]), sentAt: u64(v["sent_at"]))
    p.permissionState = v["permission_state"].string ?? "pending"
    p.contentState = v["content_state"].string ?? "header"
    p.withdrawReason = v["withdraw_reason"].string
    p.catchingUp = v["catching_up"].truthy
    if v["verdict"].object != nil { p.verdict = (allow: v["verdict"]["allow"].truthy, byDeviceId: str(v["verdict"]["by_device_id"]), envelopeNumber: int(v["verdict"]["envelope_number"])) }
    return p
  }
  func noteOf(_ v: JV) -> Note {
    let known: Set<String> = ["object_id", "by_device_id", "text", "object_version", "version_hash", "version_hashes", "causal", "envelope_number", "object_state", "pending", "local_id", "_base", "unsupported"]
    var extra: [String: JV] = [:]
    for (k, x) in v.object ?? [:] where !known.contains(k) { extra[k] = x }
    var n = Note(objectId: str(v["object_id"]), byDeviceId: str(v["by_device_id"]), text: str(v["text"]), extra: extra, objectVersion: int(v["object_version"]),
                 versionHash: str(v["version_hash"]), versionHashes: strings(v["version_hashes"]), causal: causalOf(v["causal"]), envelopeNumber: int(v["envelope_number"]),
                 objectState: v["object_state"].string ?? "open", pending: v["pending"].truthy, localId: v["local_id"].string)
    if v["_base"].object != nil { n.base = Box(noteOf(v["_base"])) }
    return n
  }
  func publishedOf(_ v: JV) -> PublishedObject {
    PublishedObject(objectId: str(v["object_id"]), agentDeviceId: str(v["agent_device_id"]), sessionId: v["session_id"].string, attachments: v["attachments"].array ?? [],
                    title: str(v["title"]), note: v["note"].string, releasedUntil: v["released_until"], objectVersion: int(v["object_version"]),
                    versionHash: str(v["version_hash"]), envelopeNumber: int(v["envelope_number"]), objectState: v["object_state"].string ?? "open", sentAt: u64(v["sent_at"]))
  }
}

extension Board {
  func put(member v: JV) {
    let id = str(v["device_id"])
    let m = members[id] ?? RoomMember(deviceId: id, role: str(v["device_role"]))
    m.deviceRole = str(v["device_role"]); m.fingerprint = v["fingerprint"].string ?? m.fingerprint
    m.deviceName = str(v["device_name"]); m.platform = v["platform"].string; m.folder = v["folder"].string; m.host = v["host"].string
    m.isActive = v["is_active"].truthy; m.addedEntryNumber = int(v["added_entry_number"]); m.removedEntryNumber = optInt(v["removed_entry_number"])
    m.addedAt = optU64(v["added_at"]); m.isMe = v["is_me"].truthy; m.isOnline = v["is_online"].truthy; m.offlineSince = optU64(v["offline_since"])
    m.link = linkOf(v["link"])
    members[id] = m
  }
  func put(session v: JV) {
    let id = str(v["session_id"])
    let s = sessions[id] ?? Session(id)
    s.agentDeviceIds = strings(v["agent_device_ids"]); s.agentDeviceId = v["agent_device_id"].string; s.agentSessionId = v["agent_session_id"].string
    s.deviceName = str(v["device_name"])
    // (no name of its device: the last part of its folder, as this app has always shown it)
    if s.deviceName.isEmpty { s.deviceName = deviceNameOf(s.agentDeviceId) }
    s.isActive = v["is_active"].truthy;
    s.isOnline = v["is_online"].truthy; s.offlineSince = optU64(v["offline_since"]); s.link = linkOf(v["link"])
    s.heardUpTo = optInt(v["heard_up_to"]); s.heardAt = optU64(v["heard_at"]); s.sessionKeyEpoch = int(v["session_key_epoch"])
    s.createdByAgent = v["created_by_agent"].truthy; s.creatorDeviceId = v["creator_device_id"].string; s.parentSessionId = v["parent_session_id"].string
    s.profile = v["profile"]
    s.statusLines = (v["status_lines"].array ?? []).map {
      StatusLine(id: str($0["id"]), label: $0["label"].string ?? str($0["id"]), state: $0["state"].string, detail: $0["detail"].string, objectId: $0["object_id"].string,
                 envelopeNumber: int($0["envelope_number"]), updatedAt: u64($0["updated_at"]))
    }
    s.agentAlerts = (v["agent_alerts"].array ?? []).map { (key: str($0["key"]), value: $0["value"], envelopeNumber: int($0["envelope_number"])) }
    var regs: [String: RegisterValue] = [:]
    for e in v["registers"].array ?? [] {
      let r = e[1]
      regs[str(e[0])] = RegisterValue(value: r["value"], envelopeNumber: int(r["envelope_number"]), senderSequence: optU64(r["sender_sequence"]),
                                      byDeviceId: r["causal"]["sender_device_id"].string, causal: causalOf(r["causal"]))
    }
    s.registers = regs
    s.settings = v["settings"]
    s.cardIds = strings(v["card_ids"]); s.openCardIds = strings(v["open_card_ids"])
    s.lastActivityAt = u64(v["last_activity_at"])
    // (a session this device knows by its group: shown even before its agent wrote anything)
    s.withHistory = !v["group_id"].isNull
    sessions[id] = s
  }
  /** One human register's raw entry, and the map the views read it from (`drafts`, `snoozes`, …). */
  func put(human key: String, raw v: JV) {
    let value: JV = v.isNull ? .null : v["value"]
    if v.isNull { human.raw.removeValue(forKey: key) }
    else { human.raw[key] = RegisterValue(value: value, envelopeNumber: int(v["envelope_number"]), senderSequence: nil, byDeviceId: v["by_device_id"].string, pending: v["pending"].truthy, causal: causalOf(v["causal"])) }
    let slash = key.firstIndex(of: "/")
    let prefix = slash.map { String(key[..<$0]) } ?? key, id = slash.map { String(key[key.index(after: $0)...]) } ?? ""
    func put(_ m: inout [String: JV]) { if value.isNull { m.removeValue(forKey: id) } else { m[id] = value } }
    switch prefix {
    case "draft": put(&human.drafts)
    case "snooze": put(&human.snoozes)
    case "duck": put(&human.ducks)
    case "star": put(&human.stars)
    case "desk": put(&human.desks)
    case "session": put(&human.sessionSettings)
    case "scribble_snapshot": put(&human.scribbleSnapshots)
    case "crown": human.crown = value
    default: break
    }
  }
}

/** An envelope as the client core's model reads it (core/client/src/model/received.rs `Received`): ids hex, the body as
 *  its JSON text. Also what the warm-start log keeps of it. */
func receivedJSON(_ e: ReceivedEnvelope) -> JV {
  let h = e.header
  func outcome(_ o: EnvelopeOutcome) -> String {
    switch o { case .applied: return "applied"; case .chained: return "chained"; case .void: return "void"; case .provisional: return "provisional"; case .refused: return "refused" }
  }
  func kind(_ k: EnvelopeKind) -> String {
    switch k {
    case .item: return "item"; case .version: return "version"; case .answer: return "answer"; case .request: return "request"
    case .verdict: return "verdict"; case .register: return "register"; case .takeBack: return "take_back"; case .reserved: return "reserved"
    }
  }
  func zeroOrHex(_ b: Bytes) -> JV { b.allSatisfy { $0 == 0 } ? .null : .str(hex(b)) }
  var out: [String: JV] = [
    "change": .num(Double(e.change)), "hash": .str(hex(e.hash)), "outcome": .str(outcome(e.outcome)), "code": e.code.map(JV.str) ?? .null,
    "kind": .str(kind(h.kind)), "sender": .str(hex(h.sender)), "seq": .num(Double(h.seq)), "recipient": h.recipient.map { .str(hex($0)) } ?? .null,
    "time": .num(Double(h.time)), "session_id": h.sessionId.map { .str(hex($0)) } ?? .null,
  ]
  if let t = h.timeline {
    switch t {
    case .sessionChat(let s): out["timeline"] = .obj(["kind": .str("session_chat"), "id": .str(hex(s))])
    case .cardChat(let c): out["timeline"] = .obj(["kind": .str("card_chat"), "id": .str(hex(c))])
    case .board(let b): out["timeline"] = .obj(["kind": .str("board"), "id": .str(hex(b))])
    }
  }
  if let o = h.object {
    let type: String = o.type == .card ? "card" : o.type == .note ? "note" : o.type == .request ? "request" : "artifact"
    let state: String = o.state == .open ? "open" : o.state == .answered ? "answered" : "closed"
    let urgency = ["low", "normal", "high", "critical"][o.urgency.rawValue]
    out["object"] = .obj(["object_id": .str(hex(o.objectId)), "object_type": .str(type), "object_state": .str(state), "urgency": .str(urgency),
                          "answered_at": .num(Double(o.answeredAt)), "object_ref": zeroOrHex(o.objectRef)])
  }
  if let a = e.objectAfter {
    let state: String = a.state == .open ? "open" : a.state == .answered ? "answered" : "closed"
    out["object_after"] = .obj(["owner": .str(hex(a.owner)), "object_state": .str(state), "current": .str(hex(a.current))])
  }
  if let b = e.bind {
    switch b {
    case .answer(_, let v, let choices): out["bind"] = .obj(["kind": .str("answer"), "version_hash": .str(hex(v)), "choices": .arr(choices.map(JV.str))])
    case .takeBack(_, let previous, _): out["bind"] = .obj(["kind": .str("take_back"), "previous_hash": .str(hex(previous))])
    case .request(_, let expires): out["bind"] = .obj(["kind": .str("request"), "expires_at": .num(Double(expires))])
    case .verdict(_, _, _, let allow): out["bind"] = .obj(["kind": .str("verdict"), "allow": .bool(allow)])
    }
  }
  if let r = e.register { out["register"] = .obj(["name": .str(r.name), "current": .bool(r.current)]) }
  if let p = e.payload { out["payload"] = .str(String(decoding: p, as: UTF8.self)) }
  return .obj(out)
}

extension Board {
  /** One patch of the core's model into the board: the records it names, and what changed. */
  @discardableResult func take(patch text: String) -> Change {
    var ch = Change()
    guard !text.isEmpty, let p = JV.parse(Bytes(text.utf8)) else { return ch }
    let room = p["room"]
    if let e = room["key_epoch"].int, e != keyEpoch { keyEpoch = e }
    let rk = room["recovery_key"]
    recoveryKey = rk.object == nil ? nil : TrommiClient.RecoveryKey(fingerprint: str(rk["fingerprint"]), since: optU64(rk["since"]))
    if p["newer"].object != nil { newerCount = int(p["newer"]["count"]); newerWhat = strings(p["newer"]["what"]); newerEnvelope = int(p["newer"]["envelope_number"]) }
    if let list = p["members"].array {
      let keep = Set(list.map { str($0[0]) })
      for id in members.keys where !keep.contains(id) { members.removeValue(forKey: id) }
      for e in list { put(member: e[1]) }
    }
    if let list = p["devregs"].array { deviceRegisters = [:]; for e in list { deviceRegisters[str(e[0])] = e[1] } }
    for e in p["sessions"].array ?? [] { if e[1].isNull { sessions.removeValue(forKey: str(e[0])) } else { put(session: e[1]) } }
    for e in p["cards"].array ?? [] { let id = str(e[0]); if e[1].isNull { cards.removeValue(forKey: id) } else { cards[id] = cardOf(e[1]) } }
    for e in p["permissions"].array ?? [] { let id = str(e[0]); if e[1].isNull { permissions.removeValue(forKey: id) } else { permissions[id] = permissionOf(e[1]) } }
    for e in p["notes"].array ?? [] { let id = str(e[0]); if e[1].isNull { notes.removeValue(forKey: id) } else { notes[id] = noteOf(e[1]) } }
    for e in p["published"].array ?? [] { let id = str(e[0]); if e[1].isNull { published.removeValue(forKey: id) } else { published[id] = publishedOf(e[1]) } }
    for tp in p["timelines"].array ?? [] {
      let key = str(tp["key"])
      let meta = tp["meta"]
      if meta.isNull { timelines.removeValue(forKey: key); continue }
      let t = timelines[key] ?? Timeline(key)
      t.itemCount = int(meta["item_count"]); t.newestEnvelopeNumber = int(meta["newest_envelope_number"]); t.hasMore = meta["has_more"].truthy
      t.loadedDownTo = meta["loaded_down_to"].int ?? Int.max
      var items = t.items, echoes = t.echoes
      for it in tp["items"].array ?? [] {
        let item = itemOf(it)
        if let n = item.envelopeNumber { items[n] = item } else if let l = item.localId { echoes[l] = item }
      }
      let keys = tp["keys"].array ?? []
      let numbers = Set(keys.compactMap { $0.int }), locals = Set(keys.compactMap { $0.string })
      items = items.filter { numbers.contains($0.key) }
      echoes = echoes.filter { locals.contains($0.key) }
      t.items = items; t.echoes = echoes
      t.numbers = items.keys.sorted()
      // (the newest of what a human and of what an agent wrote, for the unread marks)
      var human = 0, agent = 0
      for (n, it) in items { if members[it.senderDeviceId]?.deviceRole == "human" { human = max(human, n) } else { agent = max(agent, n) } }
      t.newestHumanEnvelopeNumber = human; t.newestAgentEnvelopeNumber = agent
      timelines[key] = t
    }
    for h in p["human"].array ?? [] { put(human: str(h["key"]), raw: h["raw"]) }
    if p["crown"].object != nil || p.object?["crown"] != nil { human.crown = p["crown"] }
    if let a = p["alerts"].array {
      alerts = a.map { BoardAlert(alertId: str($0["alert_id"]), code: str($0["code"]), message: str($0["message"]), envelopeNumber: optInt($0["envelope_number"]),
                                   senderDeviceId: $0["sender_device_id"].string, at: u64($0["at"]), source: $0["source"].string ?? "local") }
    }
    // the session list every device shows, as the client core makes it (core/client/src/sessions.rs)
    if let l = p["session_list"].array { sessionList = l }
    if let w = p["wake_at"].double { wakeAt = w > 0 ? UInt64(w) : nil }
    if let s = p["stack"].array { stack = s.compactMap { $0.string } }
    if let o = p["open_permission_ids"].array { openPermissionIds = o.compactMap { $0.string } }
    if let o = p["desk_order"].array { deskOrder = o.compactMap { $0.string } }
    held = Set(cards.values.filter { $0.catchingUp }.map { $0.objectId } + permissions.values.filter { $0.catchingUp }.map { $0.objectId })
    let c = p["change"]
    ch.cards = Set(strings(c["cards"])); ch.sessions = Set(strings(c["sessions"])); ch.permissions = Set(strings(c["permissions"])); ch.notes = Set(strings(c["notes"]))
    ch.published = Set(strings(c["published"])); ch.timelines = Set(strings(c["timelines"])); ch.registers = Set(strings(c["registers"]))
    ch.members = c["members"].truthy; ch.alerts = c["alerts"].truthy; ch.stack = c["stack"].truthy; ch.room = c["room"].truthy; ch.status = p["session_list"].array != nil
    return ch
  }
}

// ---- into the client core's model ------------------------------------------------------------------------

extension Board {
  /** The end of a batch: the Desk projected at `now`, and what changed taken into the copy; `change` gets it too. */
  public func project(_ change: inout Change, now: UInt64 = nowMs()) {
    // (in parts of a few hundred cards: the whole board's patch at once, as text and as values, was the peak of a
    // sign-in's memory)
    while let text = try? engine.finishPart(nowMs: now, maxCards: 200), !text.isEmpty { change.merge(take(patch: text)) }
  }
  /** As `project(_:)`, for a caller that has no change of its own. */
  @discardableResult public func project(now: UInt64 = nowMs()) -> Change { var c = Change(); project(&c, now: now); return c }
  /** Every record into the copy again (after a load of the core's model). */
  func takeAll() -> Change { try? engine.touchAll(); return project() }

  /** Envelopes as the core took them (Records: `receivedJSON`), in order: whether each changed what the model shows. */
  @discardableResult public func apply(received list: [JV], now: UInt64 = nowMs()) -> [(applied: Bool, refused: String?)] {
    guard !list.isEmpty else { return [] }
    let text: String
    do { text = try engine.applyReceivedJson(received: String(decoding: JV.arr(list).encoded(), as: UTF8.self), nowMs: now) }
    catch { PerfLog.line("the client core did not take a batch: \(error)"); return [] }
    guard let out = JV.parse(Bytes(text.utf8))?.array else { return [] }
    return out.map { ($0["applied"].truthy, $0["refused"].string) }
  }
  /** The deeds on cards of a page of card Chats read out of the hub's order. */
  func applyCardDeeds(_ list: [JV]) {
    guard !list.isEmpty else { return }
    try? engine.applyCardDeedsJson(received: String(decoding: JV.arr(list).encoded(), as: UTF8.self))
  }
  /** A page of one timeline read out of the hub's order: the items it gave. */
  func applyPage(_ key: String, _ list: [JV]) -> [TimelineItem] {
    guard let text = try? engine.applyPageJson(timelineKey: key, received: String(decoding: JV.arr(list).encoded(), as: UTF8.self), scratch: false, nowMs: nowMs()),
          let out = JV.parse(Bytes(text.utf8))?.array else { return [] }
    return out.map(itemOf)
  }

  /** The device's groups as the core holds them: members and sessions. `humans`: the room's leaves; `agents`: the enrolled agent devices. */
  func applyGroups(_ list: [GroupSummary], humans: [String], agents: [String], epoch: UInt64, recoveryHpkeKey: Bytes?) {
    let groups: [JV] = list.map { g in
      .obj(["group": .str(hex(g.group)), "session": g.session.map { s in .obj(["session_id": .str(hex(s.session)), "parent": s.parent.map { .str(hex($0)) } ?? .null]) } ?? .null,
            "leaves": .arr(g.leaves.map { .str(hex($0)) }), "disallowed": .num(Double(g.disallowed.count)), "archived": .bool(g.archived), "epoch": .num(Double(g.epoch))])
    }
    let roles: JV = .obj(["epoch": .num(Double(epoch)), "humans": .arr(humans.map(JV.str)), "agents": .arr(agents.map(JV.str)),
                          "recovery_fingerprint": recoveryHpkeKey.map { .str(keyFingerprint($0)) } ?? .null])
    try? engine.applyGroupsJson(groups: String(decoding: JV.arr(groups).encoded(), as: UTF8.self), roles: String(decoding: roles.encoded(), as: UTF8.self))
  }
  /** What a processed Commit did. */
  func applyCommitFacts(_ f: CommitFacts, room: Bool, removes: [DeviceId] = [], byRecovery: Bool = false, removedMe: Bool = false) {
    let j: JV = .obj(["group": .str(hex(f.group)), "room": .bool(room), "epoch": .num(Double(f.epoch)), "adds": .arr(f.adds.map { .str(hex($0)) }),
                      "removes": .arr(removes.map { .str(hex($0)) }), "external": .bool(f.external), "committer": .str(hex(f.committer)),
                      "agents": .arr((f.agents ?? []).map { .str(hex($0)) }), "time": f.time.map { .num(Double($0)) } ?? .null])
    try? engine.applyCommitJson(facts: String(decoding: j.encoded(), as: UTF8.self), byRecovery: byRecovery, removedMe: removedMe, nowMs: nowMs())
  }
  /** A `presence` event of the live stream, as the hub sent it (core/client/src/presence.rs). */
  func applyPresenceEvent(_ data: JV, now: UInt64 = nowMs()) {
    _ = try? engine.applyPresenceEvent(event: String(decoding: data.encoded(), as: UTF8.self), nowMs: now)
  }
  /** The live stream opened (again): the hub's list of who is here comes first, ended by its `here`. */
  func streamOpened() { try? engine.streamOpened() }
  /** The `here` that ends a new stream's list of who is here: every other device shown online is offline now. */
  func applyHere(now: UInt64 = nowMs()) { try? engine.applyHere(nowMs: now) }
  /** What the hub says of a device (the stream's `presence`, already in the model's names). */
  func applyPresence(_ d: JV) {
    guard d.object != nil else { return }
    try? engine.applyPresence(reports: String(decoding: JV.arr([d]).encoded(), as: UTF8.self), nowMs: nowMs())
  }
  /** One step of a running turn of a session. */
  func applyWorkTrail(session: String, from: Bytes, turn: Bytes, number: UInt32, time: UInt64, step: Bytes, change: UInt64) -> Bool {
    (try? engine.applyWorkTrail(sessionId: session, from: Data(from), turn: Data(turn), number: number, time: time, payload: Data(step), changeNumber: change)) ?? false
  }
  /** An alert of the app's own. */
  @discardableResult public func pushAlert(_ change: inout Change, code: String, message: String = "", envelopeNumber: Int? = nil, sender: String? = nil) -> String {
    change.alerts = true
    return (try? engine.pushAlert(code: code, message: message, envelopeNumber: envelopeNumber.map { UInt64(max(0, $0)) }, senderDeviceId: sender, nowMs: nowMs())) ?? ""
  }

  // ---- own sends, shown at once (the core's echoes) ----
  func echoItem(localId: String, timelineKey: String, content: JV, recipient: String?, objectId: String?) {
    _ = try? engine.echoItem(localId: localId, timelineKey: timelineKey, content: String(decoding: content.encoded(), as: UTF8.self), recipient: recipient, objectId: objectId, nowMs: nowMs())
  }
  func echoRegisters(localId: String, _ values: [String: JV]) {
    try? engine.echoRegisters(localId: localId, values: String(decoding: JV.obj(values).encoded(), as: UTF8.self))
  }
  /** The open question to offer after this card (decided just now): the client core's (core/client/src/model/walk.rs), the
   *  next in the Desk's order among the sessions `within` (by session key: the desk in view; nil: every desk). */
  public func nextOpenAfter(_ cardId: String, within: [String]?) -> String? {
    (try? engine.nextOpenAfter(cardId: cardId, within: within)) ?? nil
  }
  func echoAnswer(localId: String, cardId: String, answer: JV, state: String) {
    _ = try? engine.echoAnswer(localId: localId, objectId: cardId, answer: String(decoding: answer.encoded(), as: UTF8.self), objectState: state)
  }
  @discardableResult func echoNote(localId: String, objectId: String?, fields: [String: JV], closed: Bool) -> String {
    (try? engine.echoNote(localId: localId, objectId: objectId, fields: String(decoding: JV.obj(fields).encoded(), as: UTF8.self), closed: closed)) ?? localId
  }
  func confirmEcho(localId: String, sealed: Sealed) {
    try? engine.confirmEcho(localId: localId, hash: hex(sealed.hash), seq: sealed.seq, objectId: sealed.objectId.map(hex))
  }
  func rollbackEcho(localId: String) { try? engine.rollbackEcho(localId: localId) }

  // ---- the core's model as a whole ----
  /** The model for the warm-start snapshot. */
  func modelSnapshot() -> String { (try? engine.snapshot()) ?? "" }
  /** The model of a snapshot into the core, and into the copy. */
  func loadModel(_ snapshot: String) -> Bool {
    do { try engine.load(snapshot: snapshot) } catch { PerfLog.line("the client core did not load the model: \(error)"); return false }
    _ = takeAll()
    return true
  }
}

extension Board {
  /** A timeline's window as the app read it: loaded down to `loadedDownTo` (Int.max: nothing), more before it. */
  func setWindow(_ key: String, loadedDownTo: Int, hasMore: Bool) {
    try? engine.setWindow(timelineKey: key, open: true, loadedDownTo: loadedDownTo == Int.max ? 0 : UInt64(max(0, loadedDownTo)), hasMore: hasMore)
  }
}
