// Perf.swift: main-thread hitches, measured on the phone without Instruments: a display link notes every frame that
// came more than 16.7 ms after the one before (a frame missed: the main thread was busy), counted per window (a
// refresh, a scroll). TROMMI_PERF=1 runs a fixed script after launch (three refreshes, a scroll) and logs the counts
// (os log com.trommi.ios perf).
import SwiftUI
import os
import TrommiClient
#if canImport(UIKit)
import UIKit

@MainActor final class HitchMeter: NSObject {
  static let shared = HitchMeter()
  static let log = Logger(subsystem: "com.trommi.ios", category: "perf")
  private var link: CADisplayLink?
  private var last: CFTimeInterval = 0
  private var windows: [String: (hitches: Int, worst: Double, frames: Int)] = [:]
  private var open = Set<String>()
  func start() {
    guard link == nil else { return }
    let l = CADisplayLink(target: self, selector: #selector(tick(_:)))
    l.add(to: .main, forMode: .common)
    link = l
  }
  @objc private func tick(_ l: CADisplayLink) {
    let now = l.timestamp
    defer { last = now }
    guard last > 0, !open.isEmpty else { return }
    let ms = (now - last) * 1000
    for w in open {
      var v = windows[w] ?? (0, 0, 0)
      v.frames += 1
      if ms > 16.7 + 1 { v.hitches += 1; v.worst = max(v.worst, ms); PerfLog.line(String(format: "hitch %@ %.0f ms", w, ms)) }
      windows[w] = v
    }
  }
  func begin(_ w: String) { start(); open.insert(w); windows[w] = windows[w] ?? (0, 0, 0) }
  func end(_ w: String) {
    open.remove(w)
    guard let v = windows[w] else { return }
    Self.log.notice("trommi perf: \(w, privacy: .public): \(v.hitches, privacy: .public) hitches > 16 ms in \(v.frames, privacy: .public) frames, worst \(Int(v.worst), privacy: .public) ms")
    PerfLog.line(String(format: "WINDOW %@: %d hitches > 16 ms in %d frames, worst %d ms", w, v.hitches, v.frames, Int(v.worst)))
  }
}
#endif

enum PerfScript {
  /** TROMMI_PERF=1: three refreshes and a scroll of the Desk, measured. */
  @MainActor static func runIfAsked(_ model: BoardModel) {
    #if canImport(UIKit)
    guard ProcessInfo.processInfo.environment["TROMMI_PERF"] == "1" else { return }
    MainSampler.start()
    Task { @MainActor in
      try? await Task.sleep(nanoseconds: 3_000_000_000)
      for i in 1...2 {
        HitchMeter.shared.begin("refresh \(i)")
        await model.refresh()
        try? await Task.sleep(nanoseconds: 600_000_000)
        HitchMeter.shared.end("refresh \(i)")
        try? await Task.sleep(nanoseconds: 1_000_000_000)
      }
      HitchMeter.shared.begin("scroll")
      model.perfScroll &+= 1
      try? await Task.sleep(nanoseconds: 3_000_000_000)
      HitchMeter.shared.end("scroll")
    }
    #endif
  }
}

/** Counts of work per render (TROMMI_PERF): pen marks drawn and their time, row bodies built. */
enum RenderCount {
  nonisolated(unsafe) static var penDraws = 0
  nonisolated(unsafe) static var penNs: UInt64 = 0
  nonisolated(unsafe) static var bodies: [String: Int] = [:]
  static func body(_ name: String) { if PerfLog.on { bodies[name, default: 0] += 1 } }
  static func flush(_ label: String) {
    guard PerfLog.on else { return }
    PerfLog.line(String(format: "%@: pen draws %d (%.1f ms), bodies %@", label, penDraws, Double(penNs) / 1e6, bodies.map { "\($0.key) \($0.value)" }.sorted().joined(separator: ", ")))
    penDraws = 0; penNs = 0; bodies = [:]
  }
}

#if canImport(UIKit)
import Darwin

/**
 * A sampler for TROMMI_PERF: when the main thread has not come back for 30 ms, it is interrupted (SIGUSR2) and its
 * stack written to the perf log, so a hitch says where the time went (no Instruments on this machine).
 */
nonisolated(unsafe) private var sampleFrames = [UnsafeMutableRawPointer?](repeating: nil, count: 64)
nonisolated(unsafe) private var sampleCount: Int32 = 0
nonisolated(unsafe) private var sampleReady: Int32 = 0
private func onSample(_ sig: Int32) {
  sampleCount = backtrace(&sampleFrames, 64)
  sampleReady = 1
}
enum MainSampler {
  nonisolated(unsafe) static var beat: UInt64 = 0
  nonisolated(unsafe) static var mainThread: pthread_t? = nil
  @MainActor static func start() {
    guard PerfLog.on, mainThread == nil else { return }
    mainThread = pthread_self()
    signal(SIGUSR2, onSample)
    // the main thread's heartbeat: a timer every 5 ms in common modes
    let t = Timer(timeInterval: 0.005, repeats: true) { _ in MainSampler.beat = DispatchTime.now().uptimeNanoseconds }
    RunLoop.main.add(t, forMode: .common)
    Thread.detachNewThread {
      var lastSampled: UInt64 = 0
      while true {
        usleep(5000)
        let now = DispatchTime.now().uptimeNanoseconds
        let b = MainSampler.beat
        if b > 0 && now - b > 30_000_000 && now - lastSampled > 15_000_000, let m = MainSampler.mainThread {
          lastSampled = now
          sampleReady = 0
          pthread_kill(m, SIGUSR2)
          var waited = 0
          while sampleReady == 0 && waited < 20 { usleep(500); waited += 1 }
          if sampleReady == 1 {
            let n = Int(sampleCount)
            var lines = [String]()
            sampleFrames.withUnsafeMutableBufferPointer { buf in
              if let syms = backtrace_symbols(buf.baseAddress, Int32(n)) {
                for i in 0..<n { if let c = syms[i] { lines.append(String(cString: c)) } }
                free(syms)
              }
            }
            // "3 TrommiApp 0x... symbol + 12": the image and the symbol of each frame, the signal's own frames left out
            let frames = lines.dropFirst(3).prefix(28).map { l -> String in
              let parts = l.split(separator: " ", maxSplits: 3, omittingEmptySubsequences: true)
              return parts.count > 3 ? "\(parts[1]):\(parts[3].split(separator: " ").first ?? "")" : l
            }
            PerfLog.line("STALL \((now - b) / 1_000_000) ms main at: " + frames.joined(separator: " | "))
          }
        }
      }
    }
  }
}
#endif

/**
 * The mega run in the simulator (tests/ios/mega/sim.sh): TROMMI_MEGA=1 with TROMMI_HUB_URL, TROMMI_MEGA_EMAIL and
 * TROMMI_MEGA_PASSWORD logs in when there is no room yet; then every render notes whether the Desk shows a card the
 * hub has closed (a stale card), the first Desk with cards, the end of the first catch-up, the main thread's stalls
 * (TROMMI_PERF=1 as well). TROMMI_MEGA_LOGOUT=1 logs out from Settings once the catch-up ended. All of it goes to the
 * perf log (PerfLog).
 */
@MainActor enum MegaScript {
  static let env = ProcessInfo.processInfo.environment
  static let on = env["TROMMI_MEGA"] == "1"
  static var truth: Set<String>?
  static var firstCards: Double?
  static var right: Double?
  static var staleMax = 0
  static var syncs = 0
  /** Since the process started (StartClock.t0). */
  static var ms: Int { Int((ProcessInfo.processInfo.systemUptime - StartClock.t0) * 1000) }
  static func note(_ what: String) { if on { PerfLog.line("MEGA \(what) at \(ms) ms") } }

  static func runIfAsked(_ model: BoardModel) {
    guard on else { return }
    PerfLog.line("MEGA launch (room: \(model.room != nil))")
    #if canImport(UIKit)
    MainSampler.start()
    HitchMeter.shared.begin("mega")
    #endif
    if model.room == nil, let email = env["TROMMI_MEGA_EMAIL"], let pw = env["TROMMI_MEGA_PASSWORD"] {
      Task { @MainActor in
        model.phase = .email
        await model.login(account: email, password: pw)
        PerfLog.line("MEGA login returned at \(ms) ms, phase \(model.phase == .board ? "board" : "not board"), error \(model.error ?? "-")")
      }
    }
  }
  /** The hub's open cards, read once the room is there. */
  static func readTruth(_ room: Room) {
    guard on, truth == nil else { return }
    truth = []
    Task { @MainActor in
      let desk = try? await room.hub.desk()
      truth = Set((desk?["cards"] as? [JSON] ?? []).compactMap { ($0["object_id"] as? String).flatMap { try? unb64u($0) }.map(hex) })
      PerfLog.line("MEGA hub desk: \(truth?.count ?? 0) open cards at \(ms) ms")
    }
  }
  /** After a render's projection: what the Desk shows against the hub. */
  static func rendered(_ model: BoardModel) {
    guard on, let room = model.room, let d = model.desk else { return }
    readTruth(room)
    let shown = Set(d.cards.filter { $0.status == "open" && $0.kind != "permission" }.map { $0.id })
    if firstCards == nil, !shown.isEmpty { firstCards = Double(ms); PerfLog.line("MEGA first cards on the Desk at \(ms) ms (\(shown.count))") }
    guard let truth, !truth.isEmpty else { return }
    let stale = shown.subtracting(truth).count
    if stale > staleMax { staleMax = stale; PerfLog.line("MEGA stale \(stale) at \(ms) ms") }
    let held = Set(d.cards.filter { $0.status == "open" || $0.status == "decided" }.map { $0.id })
    if right == nil, stale == 0, held.isSuperset(of: truth) { right = Double(ms); PerfLog.line("MEGA Desk right at \(ms) ms (\(shown.count) open)") }
  }
  /** A catch-up ended. */
  static func synced(_ model: BoardModel, ms took: Double) {
    guard on else { return }
    syncs += 1
    PerfLog.line("MEGA sync \(syncs) ended at \(ms) ms (took \(Int(took)) ms): \(Spent.take())")
    if syncs == 1 {
      #if canImport(UIKit)
      HitchMeter.shared.end("mega")
      #endif
      if env["TROMMI_MEGA_LOGOUT"] == "1" {
        Task { @MainActor in
          try? await Task.sleep(nanoseconds: 2_000_000_000)
          model.deskPath = [.settings("account")]
          try? await Task.sleep(nanoseconds: 1_500_000_000)
          let t = Date()
          PerfLog.line("MEGA logout start")
          model.logOut()
          while model.phase != .start && Date().timeIntervalSince(t) < 60 { try? await Task.sleep(nanoseconds: 100_000_000) }
          PerfLog.line("MEGA logout done in \(Int(Date().timeIntervalSince(t) * 1000)) ms, phase start: \(model.phase == .start)")
          try? await Task.sleep(nanoseconds: 3_000_000_000)
          PerfLog.line("MEGA alive after logout")
        }
      }
    }
  }
}
