// RoomDevices.swift: the devices of the room, from this one: pair another device of the person (link or QR code,
// six emoji), invite an agent (its connector) or let one take a session over, remove devices, sign out.
//
// The steps and their order are spec/v1.md 12.1 (joining by link), 5.2.7 (a new human device into every live session
// group), 5.3 (takeover) and 9.0.10 (cuts). The core makes every Commit and checks every answer; this file fetches
// KeyPackages, posts what the core put into its outbox and waits for the hub's word before the next step.
import Foundation

/** The six numbers of a check code as the text the views pass around ("3-41-7-…"). */
func checkCodeText(_ numbers: [UInt8]) -> String { numbers.map { String($0) }.joined(separator: "-") }
/** The six emoji and their words for a check code, from the core's list of 64 (`invite::CHECK_EMOJI`); empty for anything but six numbers of that list. */
public func checkEmoji(_ code: String) -> [(emoji: String, word: String)] {
  let numbers = code.split(separator: "-").compactMap { Int($0) }
  guard numbers.count == 6, Core.isInstalled else { return [] }
  let all = Core.tools.checkEmoji()
  guard numbers.allSatisfy({ $0 >= 0 && $0 < all.count }) else { return [] }
  return numbers.map { all[$0] }
}
public func checkEmojiLine(_ code: String) -> String { checkEmoji(code).map { "\($0.emoji) \($0.word)" }.joined(separator: " · ") }

extension Room {
  /**
   * 5.2.7: a live session group of this device that lacks a human device of the room (one left out of the founding
   * while the hub held no KeyPackage of it, 5.2.5) gets it added: one Commit and one KeyPackage, one device per group
   * and round. A device the hub still holds none of is looked at again at the next sync. While an invite of this
   * device is at work its own steps add the newcomer: nothing is done beside them. Runs inside a sync; the Commit
   * comes back with the log.
   */
  func healSessions() async {
    guard let room = roomGroup, let humans = groups[hex(room)]?.leaves, humans.contains(where: { hex($0) == deviceIdHex }) else { return }
    guard let steps = try? await onCore({ try $0.inviteSteps() }), steps.isEmpty else { return }
    for g in groups.values where g.session != nil && !g.archived && !g.pending && g.disallowed.isEmpty && g.leaves.contains(where: { hex($0) == deviceIdHex }) {
      guard let device = humans.first(where: { !g.leaves.contains($0) }) else { continue }
      guard let kp = try? await noted({ try await hub.claimKeyPackages([device]) }).first?.keyPackage else { continue }
      if (try? await onCore({ try $0.addToSession(group: g.group, device: device, keyPackage: kp, nowMs: nowMs()) })) != nil { pumpOutbox() }
    }
  }

  public struct Pairing {
    public let inviteId: String      // hex
    public let link: String
    public let expiresAt: UInt64
    let invite: Bytes
    /** The check code to compare, once the newcomer's request came (see `checkEmoji`). */
    public var code: String? = nil
    public var newcomerId: String? = nil
    /** What this device showed, and for which Request: the core confirms nothing else. */
    var numbers: [UInt8] = []
    var requestHash: Hash32 = []
  }

  /**
   * Runs one step that ends in an own Commit and waits until the hub took it (or refused it: thrown) and the Commit
   * came back in the hub's order, where it takes effect (the outbox reads the changes before it calls the entry
   * done: `pumpOutbox`).
   */
  private func commit(_ step: @escaping (CoreDevice) throws -> UInt64?) async throws {
    let id: UInt64? = try await serial { [self] in
      if !self.synced { _ = try await self.catchUp() }
      let id = try await self.onCore(step)
      self.pumpOutbox()
      return id
    }
    if let id = id { try await awaitOutcome(id, ms: 20_000, orThrow: true) }
  }

  private func openInvite(role: InviteRole, session: SessionId?, app: String) async throws -> Pairing {
    try await serial { [self] in
      if !self.synced { _ = try await self.catchUp() }
      let hubURL = self.hubURL
      let made = try await self.onCore { try $0.inviteOpen(role: role, session: session, app: app, hub: hubURL, nowMs: nowMs()) }
      try await self.noted { try await self.hub.postInvite(offer: made.offer.offer, signature: made.offer.signature, mac: made.offer.mac) }
      return Pairing(inviteId: hex(made.inviteId), link: made.link, expiresAt: made.expiresAt, invite: made.inviteId)
    }
  }

  /** An invite link for a new human device (the QR code of "Pair a device"). It holds ten minutes, for one device. `app` is the app's origin. */
  public func createPairing(app: String = "https://app.trommi.com") async throws -> Pairing {
    try await openInvite(role: .human, session: nil, app: app)
  }

  /** Looks for the newcomer's Request; on the first valid one: the Reveal is published and the check code is here. */
  public func checkPairing(_ p: inout Pairing) async throws -> Bool {
    if p.code != nil { return true }
    let r = try await noted { try await hub.getInviteRequests(p.invite) }
    let invite = p.invite
    for q in r["requests"] as? [JSON] ?? [] {
      guard let request = (q["request"] as? String).flatMap({ try? unb64u($0) }), let mac = (q["mac"] as? String).flatMap({ try? unb64u($0) }),
            let signature = (q["signature"] as? String).flatMap({ try? unb64u($0) }) else { continue }
      let signed = SignedRequest(request: request, mac: mac, signature: signature)
      let accepted: InviteAccepted
      do { accepted = try await serial { [self] in try await self.onCore { try $0.inviteAccept(invite: invite, request: signed, nowMs: nowMs()) } } }
      catch let e as TrommiError where e.code == "invite-expired" { throw e }
      catch { continue }   // a Request that does not check uses nothing up (12.1.3)
      try await noted { try await hub.putInviteReveal(invite, reveal: accepted.reveal.reveal, signature: accepted.reveal.signature) }
      p.numbers = accepted.code.numbers; p.requestHash = accepted.requestHash
      p.code = checkCodeText(accepted.code.numbers); p.newcomerId = hex(accepted.newDevice)
      return true
    }
    if nowMs() > p.expiresAt { throw TrommiError("invite-expired", "the invite has expired") }
    return false
  }

  /** "They don't match": the core burns the invite, the hub forgets it. Nobody is added. */
  private func burn(_ p: Pairing) async {
    let invite = p.invite, numbers = p.numbers, hash = p.requestHash
    _ = try? await serial { [self] in try await self.onCore { try $0.inviteConfirm(invite: invite, numbers: numbers, requestHash: hash, matches: false, nowMs: nowMs()) } }
    try? await hub.deleteInvite(invite)
  }

  /**
   * The person compared the emoji and they match: the core commits the new device only for the numbers and the
   * Request this device showed, then everything the core names as left to do is done (`finishInvite`). Returns the
   * session a new agent device got.
   */
  private func confirm(_ p: Pairing, matches: Bool, history: Bool = true) async throws -> SessionId? {
    if !matches {
      await burn(p)
      throw TrommiError("code-mismatch", "the check codes do not match: nobody was added, the invite is spent")
    }
    guard p.code != nil else { throw TrommiError("bad-invite", "this invite waits for no code") }
    let invite = p.invite, numbers = p.numbers, hash = p.requestHash
    // (at work from the Commit on: the upkeep's round leaves this invite to this call)
    invitesAtWork.insert(hex(invite))
    defer { invitesAtWork.remove(hex(invite)) }
    try await commit { try $0.inviteConfirm(invite: invite, numbers: numbers, requestHash: hash, matches: true, nowMs: nowMs())?.outboxId }
    return try await finishInvite(invite, history: history)
  }

  /**
   * The upkeep's round of invite steps (the web's `invited`, engine.ts): every invite of this device that the core
   * still names steps for and that no other call is running is taken on where it stopped (`finishInvite`). The steps
   * live in the core's state, so they survive the app: a "They match" that could not finish (the newcomer had no
   * KeyPackage at the hub yet, the hub was away, the app was closed) goes on at the next sync or launch. While steps
   * stay open, another round follows after a pause that grows to 30 s.
   */
  func invitesSoon(_ delay: UInt64? = nil) {
    if invitesPlanned != nil || closed || removedAt != nil { return }
    let pause = delay ?? invitesPause
    invitesPlanned = Task { @MainActor [weak self] in
      if pause > 0 { try? await Task.sleep(nanoseconds: pause) }
      guard let self = self, !Task.isCancelled else { return }
      let open = await self.tendInvites()
      let seating = (try? await self.serial { await self.seatAgents() }) ?? false
      self.invitesPlanned = nil
      if Task.isCancelled || self.closed { return }
      if open || seating { self.invitesSoon(); self.invitesPause = min(self.invitesPause * 2, 30_000_000_000) } else { self.invitesPause = 2_000_000_000 }
    }
  }
  /**
   * An agent device of the room that sits in no session group gets its main session (5.2.2, 5.2.5), founded by this
   * device with the agent as its seat (the web's engine.ts `seatAgents`). Its inviter founds it right after "They
   * match" (`finishInvite`); when that inviter cannot (a tab closed, an iPhone sent away in the middle), any human
   * device does it, once the agent is connected and has waited `seatGraceMs` without one. An agent this device saw
   * in a session group (archived ones too), or that the hub lists in one, is never given a second: the hub refuses
   * a second live main session for the same agent anyway (5.2.2). Runs in the queue's turn (`serial`); returns
   * whether an agent still waits for a session (the upkeep looks again).
   */
  func seatAgents() async -> Bool {
    guard let agents = (try? await onCore({ try $0.roomAgents() })) ?? nil, !agents.isEmpty else { return false }
    var coming = Set<String>()
    for s in (try? await onCore({ try $0.inviteSteps() })) ?? [] {
      switch s {
      case .addToSession(_, _, let d): coming.insert(hex(d))
      case .foundSession(_, let a, _), .takeOver(_, _, _, let a, _): coming.insert(hex(a))
      default: break
      }
    }
    // Who waits and who is due is the client core's rule (core/client/src/seats.rs); here the hub is asked and the
    // session founded.
    let seated = Set(groups.values.filter { $0.session != nil }.flatMap { $0.leaves.map(hex) })
    let pendingFounding = groups.values.contains(where: { $0.session != nil && $0.session?.parent == nil && $0.pending && $0.epoch == 0 })
    let known: JV = .obj(["agents": .arr(agents.map { .str(hex($0)) }), "seated": .arr(seated.sorted().map { .str($0) }),
                          "coming": .arr(coming.sorted().map { .str($0) }), "founding_pending": .bool(pendingFounding)])
    try? board.engine.setSeatGrace(ms: seatGraceMs)
    guard let text = try? board.engine.seatPlan(known: String(decoding: known.encoded(), as: UTF8.self), nowMs: nowMs()),
          let plan = JV.parse(Bytes(text.utf8)) else { return false }
    guard let candidates = plan["candidates"].array?.compactMap({ $0.string }) else { return pendingFounding || plan["recheck_at"].double != nil }
    guard let rows = try? await noted({ try await hub.groups() }) else { return true }
    var hubSeated = [String]()
    for g in rows where g["kind"] as? String != "room" { for l in g["leaves"] as? [String] ?? [] { if let b = try? unb64u(l) { hubSeated.append(hex(b)) } } }
    let enc = { (l: [String]) in String(decoding: JV.arr(l.map { .str($0) }).encoded(), as: UTF8.self) }
    guard let id = (try? board.engine.seatAfterHub(candidates: enc(candidates), hubSeated: enc(hubSeated))) ?? nil, let agent = try? unhex(id) else { return true }
    do {
      guard let kp = try await noted({ try await hub.claimKeyPackages([agent]) }).first?.keyPackage else { return true }
      let others = (roomGroup.flatMap { groups[hex($0)]?.leaves } ?? []).filter { hex($0) != deviceIdHex }
      let claimed = others.isEmpty ? [] : try await noted { try await hub.claimKeyPackages(others, partial: true) }
      _ = try await onCore { try $0.foundSession(agent: agent, keyPackages: [kp] + claimed.map(\.keyPackage), nowMs: nowMs()) }
      pumpOutbox()
      try? board.engine.seatFounded(agent: id)
    } catch {
      // (tried again soon: a pause that grows, the core's rule; the upkeep looks again by then)
      _ = try? board.engine.seatFailed(agent: id, nowMs: nowMs())
    }
    return true   // one founding at a time: the next round sees it pending, then merged
  }

  /** One round: returns whether steps are left (of invites this round ran, or of ones another call runs now). */
  @discardableResult func tendInvites() async -> Bool {
    guard let steps = try? await serial({ [self] in try await self.onCore { try $0.inviteSteps() } }), !steps.isEmpty else { return false }
    var ids = [Bytes]()
    for s in steps where !ids.contains(s.invite) { ids.append(s.invite) }
    for invite in ids where !invitesAtWork.contains(hex(invite)) {
      invitesAtWork.insert(hex(invite))
      _ = try? await finishInvite(invite)
      invitesAtWork.remove(hex(invite))
    }
    let left = (try? await serial({ [self] in try await self.onCore { try $0.inviteSteps() } })) ?? steps
    return !left.isEmpty
  }

  /**
   * Does what the core names as left to do for a confirmed invite (`inviteSteps`), one step after the other, until
   * it names none: the Commit again if it lost its epoch, the old keys to the new device, a human device into every
   * live session group (5.2.7), an agent device's own session (12.1.6) or the takeover of one with its helper
   * sessions (5.3). A step that fails is thrown; calling this again goes on where it stopped, since the core reads
   * the steps from the state of the groups. Returns the session that was founded, if one was.
   */
  @discardableResult func finishInvite(_ invite: Bytes, history: Bool = true) async throws -> SessionId? {
    var founded: SessionId? = nil
    var waited = 0
    for _ in 0..<200 {
      let steps = try await serial { [self] in try await self.onCore { try $0.inviteSteps() } }
      guard let step = steps.first(where: { $0.invite == invite }) else { return founded }
      switch step {
      case .wait:
        // A Commit of this device waits for the hub or for its place in the log.
        waited += 1
        if waited > 100 { throw TrommiError("pending", "the hub has not answered yet: adding the device goes on later") }
        pumpOutbox()
        try await Task.sleep(nanoseconds: 200_000_000)
        await readOwnChanges()
        continue
      case .commit:
        try await commit { try $0.inviteRecommit(invite: invite, nowMs: nowMs()) }
      case .handover:
        if history { try await serial { [self] in _ = try await self.onCore { try $0.inviteHandover(invite: invite) }; self.pumpOutbox() } }
        else { try await serial { [self] in try await self.onCore { try $0.inviteForget(invite: invite) } }; return founded }
      case .addToSession(_, let group, let device):
        // The newcomer is in the room; it puts its KeyPackages up only once it joined there, so the hub may hold none
        // of it yet (404). Not a failure of the pairing: the step stays with the core, and the upkeep's next round
        // (`invitesSoon`) adds the device to its sessions once it has.
        let kp: Bytes
        do {
          guard let got = try await noted({ try await hub.claimKeyPackages([device]) }).first?.keyPackage else { invitesSoon(); return founded }
          kp = got
        } catch { invitesSoon(); return founded }
        try await commit { try $0.addToSession(group: group, device: device, keyPackage: kp, nowMs: nowMs()) }
        // (the old keys of that session, 7.1: the core names no step for them)
        try await serial { [self] in _ = try await self.onCore { try $0.sendHandover(group: group, recipient: device) }; self.pumpOutbox() }
      case .foundSession(_, let agent, let keyPackage):
        // A session group holds the agent device and the human devices the hub has a KeyPackage of (5.2.5): a
        // partial claim; one without is left out and added later by a human device of the group (5.2.7).
        let others = (roomGroup.flatMap { groups[hex($0)]?.leaves } ?? []).filter { hex($0) != deviceIdHex }
        let claimed = others.isEmpty ? [] : try await noted { try await hub.claimKeyPackages(others, partial: true) }
        let (sid, founding): (SessionId, UInt64?) = try await serial { [self] in
          let r = try await self.onCore { d -> (SessionId, UInt64?) in
            let s = try d.foundSession(agent: agent, keyPackages: [keyPackage] + claimed.map(\.keyPackage), nowMs: nowMs())
            return (s, d.outbox().last(where: { $0.kind == .groupFounding })?.id)
          }
          self.pumpOutbox()
          return r
        }
        if let id = founding { try await awaitOutcome(id, ms: 20_000, orThrow: true) }
        founded = sid
      case .takeOver(_, let group, let cuts, let agent, let keyPackage):
        // The core names one such step for the main session's group, then one per live helper session under it
        // (5.3.1 c). A helper session's step comes without a KeyPackage: a fresh one of the agent is claimed.
        let kp: Bytes
        if let given = keyPackage { kp = given }
        else {
          guard let claimed = try await noted({ try await hub.claimKeyPackages([agent]) }).first?.keyPackage else { throw TrommiError("not-found", "the new agent device has no KeyPackage left at the hub") }
          kp = claimed
        }
        try await commit { try $0.cleanSession(group: group, cuts: cuts, replacement: (agent, kp), nowMs: nowMs()) }
      case .checkHelpers(_, let session):
        // Nothing more to do in the groups this device holds. The hub lists the live helper sessions under the
        // session taken over; the Welcomes still waiting are taken first, so that this device holds them all.
        try await serial { [self] in var ch = Change(); try await self.takeWelcomes(&ch); self.emit(ch) }
        let listed = try await noted { try await hub.groups() }
        let helpers: [GroupId] = listed.compactMap { g in
          guard g["live"] as? Bool != false, (g["parent"] as? String).flatMap({ try? unb64u($0) }) == session else { return nil }
          return (g["group_id"] as? String).flatMap { try? unb64u($0) }
        }
        try await serial { [self] in try await self.onCore { try $0.inviteChecked(invite: invite, helpers: helpers) } }
      }
    }
    throw TrommiError("pending", "adding the device did not come to an end: it goes on later")
  }

  /**
   * The human compared the emoji. A match adds the device to the room group (the core hands it the old keys), then
   * to every live session group; no match burns the invite and nobody is added.
   */
  public func confirmPairing(_ p: Pairing, matches: Bool) async throws {
    _ = try await confirm(p, matches: matches)
  }

  // ---- agents --------------------------------------------------------------------------------------------

  public struct AgentInvite {
    public var pairing: Pairing
    public var label: String?
    public var desk: String?
    /** The session the new agent device takes over (5.3); nil: it gets a new session of its own. */
    public var takesOver: String?
  }
  /**
   * An invite link for an agent (the connector's command). Confirmed, it gets a new session on `desk`, or with
   * `takesOver` that session: the one tap that moves an agent's session to another machine.
   */
  public func createAgentInvite(app: String = "https://app.trommi.com", label: String? = nil, desk: String? = nil, takesOver: String? = nil) async throws -> AgentInvite {
    let session = try takesOver.map { try unhex($0) }
    return AgentInvite(pairing: try await openInvite(role: .agent, session: session, app: app), label: label, desk: desk, takesOver: takesOver)
  }
  /**
   * The human confirmed the agent's emoji: it is enrolled (the `agents` change in the room group, 12.1.6) and gets
   * its session, or takes over the one the invite names, with the old keys (history is the default, 5.3.2).
   * Returns the session id.
   */
  @discardableResult public func confirmAgent(_ inv: AgentInvite, matches: Bool) async throws -> String {
    let founded = try await confirm(inv.pairing, matches: matches)
    if let sid = inv.takesOver { return sid }
    guard let sid = founded.map(hex) else { throw TrommiError("pending", "the agent is enrolled; its session is not founded yet") }
    var place: [String: JV] = [:]
    if let l = inv.label, !l.isEmpty { place["name"] = .str(l) }
    if let d = inv.desk { place["desk"] = .str(d) }
    if !place.isEmpty { try? await editSession(sid, place) }
    return sid
  }

  // ---- removing, signing out -----------------------------------------------------------------------------

  /**
   * Removes devices: a human device from the room group and every session group, an agent device from `agents` and
   * its sessions. Each removal names the device's last envelope this device accepted (the cut, 9.0.10).
   */
  public func removeDevices(_ ids: [String]) async throws {
    let wanted = try ids.map { try unhex($0) }
    guard let room = roomGroup, let roomLeaves = groups[hex(room)]?.leaves else { throw TrommiError("no-key", "this device is in no room") }
    let humans = wanted.filter { roomLeaves.contains($0) }
    let agents = wanted.filter { !roomLeaves.contains($0) }
    if !humans.isEmpty { try await commit { device in try device.removeHumanDevices(try humans.map { try device.cutOf(group: room, device: $0) }, nowMs: nowMs()) } }
    if !agents.isEmpty { try await commit { try $0.removeAgents(agents, nowMs: nowMs()) } }
    for g in groups.values where g.session != nil && !g.archived {
      let gone = wanted.filter { g.leaves.contains($0) }
      guard !gone.isEmpty else { continue }
      let group = g.group
      try await commit { device in try device.cleanSession(group: group, cuts: try gone.map { try device.cutOf(group: group, device: $0) }, replacement: nil, nowMs: nowMs()) }
    }
  }

  /**
   * Sign out. An MLS member cannot commit its own removal, so this device asks nothing of the group: it forgets its
   * state here, and its push registration and its token at the hub (both best effort, before anything is forgotten
   * here). It stays a leaf until another device of the person removes it (Settings → Devices), which the next device
   * to sign in is told by the list it shows.
   */
  public func leaveRoom() async throws {
    // Each step has a bound: a catch-up or a request that hangs never keeps the person from leaving (it took two
    // minutes and more on a simulator, waiting for a sync in the queue).
    await Room.waitAtMost(3) { _ = try? await self.flush(timeoutMs: 3000) }
    await Room.waitAtMost(5) { _ = try? await self.hub.request("DELETE", "/push") }
    await Room.waitAtMost(5) { await self.hub.signOut() }
    await shutdown()
    Store.lifecycle.withLock { store.wipe() }
  }
  /** Forget this room on this device only. */
  public func forgetHere() {
    close()
    Store.lifecycle.withLock { store.wipe() }
  }
}

/**
 * What a person types on the computer of an agent, in order (the web's invite-an-agent page): the installer, only the
 * first time on that computer (it sets up Claude Code and Codex, where installed), then, in the project folder inside
 * claude or codex, a plain prompt with this invite's link (the agent calls the connector's connect tool with it, with or
 * without the plugin's slash command), and the slash command as the other way (`other`). The link stands in single
 * quotes; a link never holds one.
 */
public func agentConnectSteps(link: String) -> [(title: String, command: String, other: String, note: String)] {
  [("First time on this computer? Install:", "curl -fsSL https://raw.githubusercontent.com/trommi/trommi/main/install.sh | sh", "",
    "Skip this if you have installed Trommi before (check: trommi-connector --version)."),
   ("In your project folder, start claude (or codex) and paste:", "Connect to Trommi with this invite link: '\(link)'",
    "/trommi:connect '\(link)'",
    "Claude Code asks once whether to use the trommi MCP server: choose \"Use this MCP server\".")]
}
