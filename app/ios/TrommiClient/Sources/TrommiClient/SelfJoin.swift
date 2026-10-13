// SelfJoin.swift: self-join (spec/v1.md 12.4) on the human's side, as the web's client.ts answerAsks/vouchConfirm.
// An agent asks in its main session to let in a new session of its machine and account (`join_ask/<nonce>`). This
// device asks the person once (`vouchAsks`, one tap instead of the six emoji): Allow (`answerVouch`) answers with an
// agent invite link in `join_link/<nonce>`, Deny with `{ refused: denied }`; past the bounds it refuses without
// asking. The Request is confirmed only when the ask names its hash and the voucher still stands; the new session is
// named after the ask with who vouched, and its device goes when the session is archived.

import Foundation

/** What the asks on a board call for: the client core's rule (client/src/selfjoin.rs), the same as the web's. */
public enum SelfJoinPlan {
  public struct Ask: Equatable {
    public let sessionId: String
    public let nonce: String
    public let ask: JV
    /** Why it is refused at once (`cap`, `busy`, `bad-ask`), or nil: it gets a link. */
    public let refused: String?
  }

  /** The unanswered asks of live main sessions whose group this device holds, not yet in `answered`. `pending`: the
   *  accounts of this device's vouched links not through yet. */
  public static func asks(_ board: Board, answered: Set<String>, pending: [String] = []) -> [Ask] {
    let enc = { (v: JV) in String(decoding: v.encoded(), as: UTF8.self) }
    guard let text = try? board.engine.selfJoinAsks(answered: enc(.arr(answered.sorted().map { .str($0) })), pending: enc(.arr(pending.map { .str($0) }))),
          let list = JV.parse(Bytes(text.utf8))?.array else { return [] }
    return list.map { Ask(sessionId: $0["session_id"].string ?? "", nonce: $0["nonce"].string ?? "", ask: $0["ask"], refused: $0["refused"].string) }
  }

  /** As `asks`, of the sessions `holds` names only (the core already leaves out the groups this device does not hold). */
  public static func asks(_ board: Board, answered: Set<String>, holds: (String) -> Bool, pending: [String] = []) -> [Ask] {
    asks(board, answered: answered, pending: pending).filter { holds($0.sessionId) }
  }

  /** Archived sessions that joined by themselves whose agent device is still in: their devices go (12.4.5). */
  public static func toRetire(_ board: Board) -> [String] {
    guard let text = try? board.engine.selfJoinRetire() else { return [] }
    return (JV.parse(Bytes(text.utf8))?.array ?? []).compactMap { $0.string }
  }

  /** Whether the Request with `requestHash` (base64url) a link vouched for by `session`'s ask `nonce` brought is let in:
   *  nil while the ask names no hash yet; false when it names another or the voucher no longer stands. */
  public static func verdict(_ board: Board, session: String, nonce: String, requestHash: String) -> Bool? {
    (try? board.engine.selfJoinVerdict(sessionId: session, nonce: nonce, requestHash: requestHash)) ?? nil
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
    for a in SelfJoinPlan.asks(board, answered: vouchAnswered, pending: Array(vouchPending.values)) {
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
    guard let a = SelfJoinPlan.asks(board, answered: vouchAnswered, pending: Array(vouchPending.values)).first(where: { $0.nonce == nonce }) else { return }
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
      // (the client core's verdict: the ask names this Request's hash and its voucher still stands)
      verdict = SelfJoinPlan.verdict(board, session: voucher, nonce: nonce, requestHash: b64u(inv.pairing.requestHash))
      if verdict == nil { try? await Task.sleep(nanoseconds: 500_000_000) }
    }
    guard let matches = verdict else { return }
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
