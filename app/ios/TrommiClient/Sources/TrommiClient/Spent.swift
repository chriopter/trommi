// Spent.swift: where a sign-in or a catch-up spends its time, summed per step (the web's app/web/core/spent.ts). Cheap
// enough to stay on: a lock and two adds per measured step. Read and reset by the perf run (tests/ios MegaRunTests,
// the app's TROMMI_PERF log); nothing of it leaves the device.
import Foundation

public enum Spent {
  private static let lock = NSLock()
  nonisolated(unsafe) private static var ms: [String: Double] = [:]
  nonisolated(unsafe) private static var counts: [String: Int] = [:]

  /** Adds `n` to the count `name`. */
  public static func count(_ name: String, _ n: Int = 1) { lock.withLock { counts[name, default: 0] += n } }
  /** Adds the time since `start` (DispatchTime uptime ns) to the step `name`, and one to its count. */
  public static func add(_ name: String, since start: UInt64) {
    let end = DispatchTime.now().uptimeNanoseconds
    let d = Double(end &- start) / 1e6
    lock.withLock {
      ms[name, default: 0] += d; counts[name, default: 0] += 1
      if spans != nil && spans!.count < 500_000 { spans!.append((name, start, end)) }
    }
  }
  /** For a measurement only (tests/client/mega/first-login.mjs): while a trace runs, every measured step is also kept
   *  as (name, start, end) in uptime ns, and marks as steps of no length. Off unless a measurement starts it. */
  nonisolated(unsafe) private static var spans: [(String, UInt64, UInt64)]?
  public static func traceStart() { lock.withLock { spans = [] } }
  public static func traceTake() -> [(String, UInt64, UInt64)] { lock.withLock { let s = spans ?? []; spans = nil; return s } }
  public static func mark(_ name: String) { let t = now; lock.withLock { if spans != nil { spans!.append(("mark " + name, t, t)) } } }
  public static var now: UInt64 { DispatchTime.now().uptimeNanoseconds }
  @discardableResult public static func time<T>(_ name: String, _ body: () throws -> T) rethrows -> T {
    let t = now
    defer { add(name, since: t) }
    return try body()
  }
  public static func time<T>(_ name: String, _ body: () async throws -> T) async rethrows -> T {
    let t = now
    defer { add(name, since: t) }
    return try await body()
  }
  /** The sums so far, one line ("hub 1234 ms ×12, …"), and cleared. */
  public static func take() -> String {
    lock.withLock {
      let names = Set(ms.keys).union(counts.keys).sorted()
      let line = names.map { n in ms[n].map { String(format: "%@ %.0f ms ×%d", n, $0, counts[n] ?? 0) } ?? "\(n) \(counts[n] ?? 0)" }.joined(separator: ", ")
      ms = [:]; counts = [:]
      return line
    }
  }
}
