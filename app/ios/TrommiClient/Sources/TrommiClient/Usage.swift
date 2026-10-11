// Usage.swift: an agent's account and usage, as its connector names them in its main session's profile (`account`,
// connector/src/usage.rs): the plan, the e-mail, the 5-hour window and the week (used percent, when each starts again),
// for Codex its credits and free full resets. The same reading as the web's ui.mjs usageMeters/usageFull.
import Foundation

public struct UsageMeter: Identifiable, Equatable {
  public let id: String          // "five_hour" or "weekly"
  public let label: String       // "5 h", "week"
  public let long: String        // "5-hour window", "This week"
  public let used: Int           // 0...100
  public let resetsAt: UInt64?   // ms
}

public struct UsageAccount: Equatable {
  public let codex: Bool
  public let plan: String
  public let email: String
  public let meters: [UsageMeter]
  public let credits: Double?
  public let fullResets: Int?

  static let plans = ["pro": "Pro", "max": "Max", "team": "Team", "enterprise": "Enterprise", "free": "Free", "plus": "Plus", "prolite": "Pro Lite", "business": "Business", "edu": "Edu"]

  /** nil for anything but an object with a plan or an e-mail. A window that started again by `now` is left out: what
   *  the agent said of it is old. */
  public init?(_ j: JV, now: UInt64) {
    guard j.object != nil else { return nil }
    plan = j["plan"].string ?? ""; email = j["email"].string ?? ""
    guard !plan.isEmpty || !email.isEmpty else { return nil }
    codex = j["kind"].string == "codex"
    meters = [("five_hour", "5 h", "5-hour window"), ("weekly", "week", "This week")].compactMap { key, label, long in
      guard let u = j[key]["used"].double, u.isFinite else { return nil }
      let at = j[key]["resets_at"].double.flatMap { $0 > 0 && $0.isFinite ? UInt64($0) : nil }
      if let at = at, at <= now { return nil }
      return UsageMeter(id: key, label: label, long: long, used: Int(min(100, max(0, u)).rounded()), resetsAt: at)
    }
    credits = j["credits"].double.flatMap { $0.isFinite ? $0 : nil }
    fullResets = j["full_resets"].int
  }

  /** "Claude Max", "ChatGPT Pro Lite". */
  public var planTag: String {
    let p = UsageAccount.plans[plan.lowercased()] ?? (plan.prefix(1).uppercased() + plan.dropFirst())
    return "\(codex ? "ChatGPT" : "Claude") \(p)".trimmingCharacters(in: .whitespaces)
  }
  /** "2,400 credits". */
  public var creditsText: String? {
    guard let c = credits else { return nil }
    let f = NumberFormatter(); f.numberStyle = .decimal; f.maximumFractionDigits = 0; f.locale = Locale(identifier: "en_GB")
    return "\(f.string(from: NSNumber(value: c)) ?? String(Int(c))) \(Int(c.rounded()) == 1 ? "credit" : "credits")"
  }
  public var fullResetsText: String? { fullResets.map { "\($0) full \($0 == 1 ? "reset" : "resets")" } }
}

/** When a window starts again: "13:43" within a day, else "Wed 14 Oct" (as the web's usageReset). */
public func usageReset(_ at: UInt64, now: UInt64, timeZone: TimeZone = .current) -> String {
  let f = DateFormatter(); f.locale = Locale(identifier: "en_GB"); f.timeZone = timeZone
  f.dateFormat = at > now && at - now < 86_400_000 ? "HH:mm" : "EEE d MMM"
  return f.string(from: Date(timeIntervalSince1970: Double(at) / 1000))
}

/** The account an agent process names: its main session's profile `account`, else one of its helpers'. */
public func usageOf(_ board: Board, agentDeviceId: String?, now: UInt64) -> UsageAccount? {
  guard let dev = agentDeviceId else { return nil }
  let mine = board.sessions.values.filter { $0.agentDeviceId == dev && $0.profile["account"].object != nil }
  let s = mine.first { $0.parentSessionId == nil && $0.profile["parent_session"].string == nil } ?? mine.min { $0.sessionId < $1.sessionId }
  return s.flatMap { UsageAccount($0.profile["account"], now: now) }
}
