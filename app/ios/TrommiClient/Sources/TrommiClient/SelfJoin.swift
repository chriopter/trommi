// SelfJoin.swift: self-join (spec/v1.md 12.4) on the human's side, as the web's client.ts answerAsks/vouchConfirm.
// An agent asks in its main session to let in a new session of its machine and account (`join_ask/<nonce>`). This
// device asks the person once (`vouchAsks`, one tap instead of the six emoji): Allow (`answerVouch`) answers with an
// agent invite link in `join_link/<nonce>`, Deny with `{ refused: denied }`; past the bounds it refuses without
// asking. The Request is confirmed only when the ask names its hash and the voucher still stands; the new session is
// named after the ask with who vouched, and its device goes when the session is archived.

import Foundation

/** What the asks on a board call for: pure, so that it is tested without a room (SelfJoinTests). */
public enum SelfJoinPlan {
  /** The most live sessions of one account that joined by themselves, open vouched links counted in; and the most
   *  of all accounts together (the account is the asking agent's word, so this one bounds a lying agent). */
  public static let maxVouched = 8
  public static let maxVouchedRoom = 24
  /** The most vouched links open at once on this device (the hub keeps 16 open invites a room: the rest stay for the person). */
  public static let maxVouchedOpen = 8

  public struct Ask: Equatable {
    public let sessionId: String
    public let nonce: String
    public let ask: JV
    /** Why it is refused at once (`cap`, `busy`, `bad-ask`), or nil: it gets a link. */
    public let refused: String?
  }

  /** The unanswered asks of live main sessions whose group this device holds (`holds`), not yet in `answered`.
   *  `pending`: the accounts of this device's vouched links not through yet. */
  public static func asks(_ board: Board, answered: Set<String>, holds: (String) -> Bool, pending: [String] = []) -> [Ask] {
    var out = [Ask]()
    var accounts = board.sessions.values.filter { $0.isActive && !$0.settings["archived"].truthy && $0.settings["vouched"].object != nil }
      .map { $0.settings["vouched"]["account"].string ?? "" } + pending
    var open = pending.count
    for s in board.sessions.values.sorted(by: { $0.sessionId < $1.sessionId })
    where s.isActive && s.parentSessionId == nil && !s.settings["archived"].truthy && holds(s.sessionId) {
      for key in s.registers.keys.sorted() where key.hasPrefix("join_ask/") {
        let nonce = String(key.dropFirst("join_ask/".count)), ask = s.registers[key]?.value ?? .null
        guard !nonce.isEmpty, ask.object != nil, !answered.contains(nonce), (s.registers["join_link/\(nonce)"]?.value ?? .null).isNull else { continue }
        let account = ask["account"].string ?? ""
        let full = accounts.filter { $0 == account }.count >= maxVouched || accounts.count >= maxVouchedRoom
        let refused = account.isEmpty ? "bad-ask" : full ? "cap" : open >= maxVouchedOpen ? "busy" : nil
        if refused == nil { accounts.append(account); open += 1 }
        out.append(Ask(sessionId: s.sessionId, nonce: nonce, ask: ask, refused: refused))
      }
    }
    return out
  }

  /** Archived sessions that joined by themselves whose agent device is still in: their devices go (12.4.5). */
  public static func toRetire(_ board: Board) -> [String] {
    board.sessions.values.filter { $0.settings["archived"].truthy && $0.settings["vouched"].object != nil }
      .compactMap { $0.agentDeviceId }.filter { d in board.members[d].map { $0.isActive && $0.deviceRole != "human" } ?? false }.sorted()
  }

  /** Whether the Request with `requestHash` (base64url) is let in: nil while the ask names none yet. */
  public static func confirms(named: String?, requestHash: String) -> Bool? {
    guard let named = named, !named.isEmpty else { return nil }
    return named == requestHash
  }
}

extension Room {

  func vouchSoon() {
    guard vouchTask == nil else { return }
    vouchTask = Task { @MainActor [weak self] in
      try? await Task.sleep(nanoseconds: 300_000_000)
      guard let self = self else { return }
      self.vouchTask = nil
      await self.answerAsks()
    }
  }

  /**
   * The asks of the agents: those past the bounds are refused without asking; the others wait for the person's tap
   * and stand in `vouchAsks` (the app shows each as "Allow new Claude session …?").
   */
  func answerAsks() async {
    guard live, upgrade == nil, board.myRole == "human" else { return }
    // archived on another device, by an older app, or the removal did not go through: the device goes now
    for d in SelfJoinPlan.toRetire(board) where !vouchRetiring.contains(d) {
      vouchRetiring.insert(d)
      try? await removeDevices([d])
      vouchRetiring.remove(d)
    }
    var waiting = [SelfJoinPlan.Ask]()
    for a in SelfJoinPlan.asks(board, answered: vouchAnswered, holds: holdsSession, pending: Array(vouchPending.values)) {
      guard let why = a.refused else { waiting.append(a); continue }
      vouchAnswered.insert(a.nonce)
      try? await setRegisters(["join_link/\(a.nonce)": .obj(["refused": .str(why)])], sessionId: a.sessionId)
    }
    if waiting != vouchAsks { vouchAsks = waiting; var ch = Change(); ch.room = true; emit(ch) }
  }

  /**
   * The person's tap on an ask (12.4): `allow` answers it with an invite link for that session, whose Request is
   * confirmed when the ask names its hash (the tap stands for the emoji); otherwise it is refused (`denied`). An ask
   * answered meanwhile, here or on another device, is left.
   */
  public func answerVouch(_ nonce: String, allow: Bool) async throws {
    guard let a = SelfJoinPlan.asks(board, answered: vouchAnswered, holds: holdsSession, pending: Array(vouchPending.values)).first(where: { $0.nonce == nonce }) else { return }
    vouchAnswered.insert(nonce)
    vouchAsks.removeAll { $0.nonce == nonce }
    var ch = Change(); ch.room = true; emit(ch)
    let answer = { (v: JV) in try await self.setRegisters(["join_link/\(nonce)": v], sessionId: a.sessionId) }
    guard allow else { try await answer(.obj(["refused": "denied"])); return }
    if let why = a.refused { try await answer(.obj(["refused": .str(why)])); return }
    let s = board.sessions[a.sessionId]
    let name = String((a.ask["name"].string ?? "").prefix(120))
    guard let inv = try? await createAgentInvite(label: name.isEmpty ? nil : name, desk: s?.settings["desk"].string) else {
      try await answer(.obj(["refused": "busy"])); return
    }
    let vouched: JV = .obj(["by": .str(a.sessionId), "by_name": .str(s?.settings["name"].string ?? a.ask["vouched_by"].string ?? ""),
                            "account": .str(a.ask["account"].string ?? ""), "account_label": .str(a.ask["account_label"].string ?? "")])
    try await answer(.obj(["link": .str(inv.pairing.link), "invite_id": .str(inv.pairing.inviteId)]))
    let voucher = a.sessionId
    vouchPending[nonce] = a.ask["account"].string ?? ""
    Task { @MainActor [weak self] in
      await self?.finishVouched(inv, voucher: voucher, nonce: nonce, vouched: vouched)
      self?.vouchPending.removeValue(forKey: nonce)
    }
  }

  /** Waits for the new session's Request, then for the ask to name its hash: confirmed when it is that one, burned when not. */
  func finishVouched(_ invite: AgentInvite, voucher: String, nonce: String, vouched: JV) async {
    var inv = invite
    while nowMs() < inv.pairing.expiresAt {
      if (try? await checkPairing(&inv.pairing)) == true { break }
      try? await Task.sleep(nanoseconds: 1_000_000_000)
    }
    guard inv.pairing.code != nil else { return }
    var verdict: Bool? = nil
    let until = nowMs() + 5 * 60_000
    while verdict == nil && nowMs() < until {
      verdict = SelfJoinPlan.confirms(named: board.sessions[voucher]?.registers["join_ask/\(nonce)"]?.value["request_hash"].string, requestHash: b64u(inv.pairing.requestHash))
      if verdict == nil { try? await Task.sleep(nanoseconds: 500_000_000) }
    }
    guard var matches = verdict else { return }
    // the voucher must still be a live session with its agent in: a removed or archived one vouches for nobody
    if !(board.sessions[voucher].map { $0.isActive && !$0.settings["archived"].truthy } ?? false) { matches = false }
    guard let sid = try? await confirmAgent(inv, matches: matches) else { return }
    try? await editSession(sid, ["vouched": vouched])
  }

  /** An archived session that joined by itself: its device was made for it alone and goes with it. */
  func retireVouched(_ sid: String) async {
    guard let device = board.sessions[sid]?.agentDeviceId, !vouchRetiring.contains(device) else { return }
    vouchRetiring.insert(device)
    try? await removeDevices([device])
    vouchRetiring.remove(device)
  }
}
