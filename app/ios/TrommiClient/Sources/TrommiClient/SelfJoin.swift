// SelfJoin.swift: self-join (spec/v1.md 12.4) on the human's side, as the web's client.ts answerAsks/vouchConfirm.
// An agent asks in its main session to let in a new session of its machine and account (`join_ask/<nonce>`). This
// device answers with an agent invite link in `join_link/<nonce>` (or `{ refused }` past the cap), confirms the
// Request on its own only when the ask names that Request's hash (no emoji: the agent's own machine vouches),
// names the new session after the ask with who vouched, and removes its device when the session is archived.

import Foundation

/** What the asks on a board call for: pure, so that it is tested without a room (SelfJoinTests). */
public enum SelfJoinPlan {
  /** The most live sessions of one account that joined by themselves. */
  public static let maxVouched = 8

  public struct Ask: Equatable {
    public let sessionId: String
    public let nonce: String
    public let ask: JV
    /** Why it is refused at once (`cap`, `bad-ask`), or nil: it gets a link. */
    public let refused: String?
  }

  /** The unanswered asks of live main sessions whose group this device holds (`holds`), not yet in `answered`. */
  public static func asks(_ board: Board, answered: Set<String>, holds: (String) -> Bool) -> [Ask] {
    var out = [Ask]()
    for s in board.sessions.values.sorted(by: { $0.sessionId < $1.sessionId })
    where s.isActive && s.parentSessionId == nil && !s.settings["archived"].truthy && holds(s.sessionId) {
      for key in s.registers.keys.sorted() where key.hasPrefix("join_ask/") {
        let nonce = String(key.dropFirst("join_ask/".count)), ask = s.registers[key]?.value ?? .null
        guard !nonce.isEmpty, ask.object != nil, !answered.contains(nonce), (board.human.raw["join_link/\(nonce)"]?.value ?? .null).isNull else { continue }
        let account = ask["account"].string ?? ""
        let live = board.sessions.values.filter { $0.isActive && !$0.settings["archived"].truthy && $0.settings["vouched"]["account"].string == account }.count
        out.append(Ask(sessionId: s.sessionId, nonce: nonce, ask: ask, refused: account.isEmpty ? "bad-ask" : live >= maxVouched ? "cap" : nil))
      }
    }
    return out
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

  /** Answers every ask of a live main session's agent that has no answer yet. */
  func answerAsks() async {
    guard live, upgrade == nil, board.myRole == "human" else { return }
    for a in SelfJoinPlan.asks(board, answered: vouchAnswered, holds: holdsSession) {
      vouchAnswered.insert(a.nonce)
      if let why = a.refused {
        try? await setRegisters(["join_link/\(a.nonce)": .obj(["refused": .str(why)])], sessionId: a.sessionId)
        continue
      }
      let s = board.sessions[a.sessionId]
      let name = String((a.ask["name"].string ?? "").prefix(120))
      guard let inv = try? await createAgentInvite(label: name.isEmpty ? nil : name, desk: s?.settings["desk"].string) else { vouchAnswered.remove(a.nonce); continue }
      let vouched: JV = .obj(["by": .str(a.sessionId), "by_name": .str(s?.settings["name"].string ?? a.ask["vouched_by"].string ?? ""),
                              "account": .str(a.ask["account"].string ?? ""), "account_label": .str(a.ask["account_label"].string ?? "")])
      do { try await setRegisters(["join_link/\(a.nonce)": .obj(["link": .str(inv.pairing.link), "invite_id": .str(inv.pairing.inviteId)])], sessionId: a.sessionId) }
      catch { vouchAnswered.remove(a.nonce); continue }
      let voucher = a.sessionId, nonce = a.nonce
      Task { @MainActor [weak self] in await self?.finishVouched(inv, voucher: voucher, nonce: nonce, vouched: vouched) }
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
    guard let matches = verdict else { return }
    guard let sid = try? await confirmAgent(inv, matches: matches) else { return }
    try? await editSession(sid, ["vouched": vouched])
  }

  /** An archived session that joined by itself: its device was made for it alone and goes with it. */
  func retireVouched(_ sid: String) async {
    guard let s = board.sessions[sid] ?? board.sessions.values.first(where: { $0.parentSessionId == nil && $0.agentDeviceId == sid }),
          let device = s.agentDeviceId else { return }
    try? await removeDevices([device])
  }
}
