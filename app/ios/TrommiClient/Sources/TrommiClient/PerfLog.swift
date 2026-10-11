// PerfLog.swift: timings on the device without Instruments: lines appended to Documents/perf.log (pulled with
// `pymobiledevice3 apps pull`), only when TROMMI_PERF=1 is set or the file trommi-perf exists in Documents.
import Foundation

public enum PerfLog {
  private static let lock = NSLock()
  nonisolated(unsafe) private static var handle: FileHandle? = nil
  nonisolated(unsafe) private static var checked = false
  nonisolated(unsafe) private static var isOn = false
  public static var on: Bool { setUp(); return isOn }
  public static var file: URL {
    (FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first ?? URL(fileURLWithPath: NSTemporaryDirectory())).appendingPathComponent("perf.log")
  }
  public static func setUp() {
    lock.lock(); defer { lock.unlock() }
    guard !checked else { return }
    checked = true
    isOn = ProcessInfo.processInfo.environment["TROMMI_PERF"] == "1"
    guard isOn else { return }
    try? FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
    if !FileManager.default.fileExists(atPath: file.path) { FileManager.default.createFile(atPath: file.path, contents: nil) }
    handle = try? FileHandle(forWritingTo: file)
    _ = try? handle?.seekToEnd()
  }
  public static func line(_ s: String) {
    guard on else { return }
    let t = String(format: "%.3f", Date().timeIntervalSince1970)
    lock.lock(); defer { lock.unlock() }
    handle?.write(Data("\(t) \(Thread.isMainThread ? "M" : "-") \(s)\n".utf8))
  }
  /** Time a piece of work; logged when it took at least `min` ms. */
  @discardableResult public static func time<T>(_ what: String, min: Double = 0, _ op: () throws -> T) rethrows -> T {
    guard on else { return try op() }
    let t0 = DispatchTime.now().uptimeNanoseconds
    let r = try op()
    let ms = Double(DispatchTime.now().uptimeNanoseconds - t0) / 1e6
    if ms >= min { line(String(format: "%@ %.1f ms", what, ms)) }
    return r
  }
  public static func time<T>(_ what: String, min: Double = 0, _ op: () async throws -> T) async rethrows -> T {
    guard on else { return try await op() }
    let t0 = DispatchTime.now().uptimeNanoseconds
    let r = try await op()
    let ms = Double(DispatchTime.now().uptimeNanoseconds - t0) / 1e6
    if ms >= min { line(String(format: "%@ %.1f ms (wall)", what, ms)) }
    return r
  }
}

extension PerfLog {
  /** The speed run (the app's SpeedScript, SpeedTests): a chat line from the agent of `session`, as the live stream
   *  brings one; made up, applied only to a board without a room. */
  public static func chatLine(_ b: Board, session: String, number n: Int, text: String) -> Rec? {
    guard let agent = b.sessions[session]?.agentDeviceId else { return nil }
    return Rec(envelopeNumber: n, envelopeHash: String(format: "%064x", n), senderDeviceId: agent, senderRole: "agent", recipientDeviceId: nil, sentAt: nowMs(),
               kind: KIND.TIMELINE_ITEM, isHead: false, object: nil, timelineKind: "chat", timelineId: "session/\(session)", sessionId: session,
               content: .obj(["text": .str(text), "attachments": .arr([])]), contentState: "ok", bind: nil,
               causal: Causal(senderDeviceId: agent, senderSequence: UInt64(n), sentAt: nowMs(), lamport: 1), senderSequence: UInt64(n), epoch: 1)
  }
}
