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
    guard ProcessInfo.processInfo.environment["TROMMI_PERF"] == "1", ProcessInfo.processInfo.environment["TROMMI_SPEED"] != "1" else { return }
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

/**
 * The speed run: TROMMI_SPEED=1 TROMMI_PERF=1 TROMMI_SPEED_FIXTURE=<tests/tools/mega-fixture.mjs output>. On that
 * made-up room (the demo's way: no room, no network) it scrolls the Desk, lets news arrive in bursts, opens the
 * longest chat, scrolls it, lets news arrive there and elsewhere, and types into its composer; each step a HitchMeter
 * window, the memory after each (perf log "SPEED"). tests/ios/speed/sim.sh runs it in the simulator.
 */
@MainActor enum SpeedScript {
  static let typeKey = Notification.Name("trommi-speed-type")
  static func memoryMB() -> Int {
    #if canImport(Darwin)
    var info = task_vm_info_data_t()
    var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<integer_t>.size)
    let r = withUnsafeMutablePointer(to: &info) { $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) { task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count) } }
    return r == KERN_SUCCESS ? Int(info.phys_footprint / 1_048_576) : -1
    #else
    return -1
    #endif
  }
  /** The most memory the process ever held (MB). */
  static func peakMB() -> Int {
    #if canImport(Darwin)
    var info = task_vm_info_data_t()
    var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<integer_t>.size)
    let r = withUnsafeMutablePointer(to: &info) { $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) { task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count) } }
    return r == KERN_SUCCESS ? Int(info.ledger_phys_footprint_peak / 1_048_576) : -1
    #else
    return -1
    #endif
  }
  /** CPU time the process used so far (user and system, all threads), in seconds. */
  static func cpuSeconds() -> Double {
    var u = rusage()
    getrusage(RUSAGE_SELF, &u)
    return Double(u.ru_utime.tv_sec + u.ru_stime.tv_sec) + Double(u.ru_utime.tv_usec + u.ru_stime.tv_usec) / 1e6
  }
  static func runIfAsked(_ model: BoardModel) {
    #if canImport(UIKit)
    guard ProcessInfo.processInfo.environment["TROMMI_SPEED"] == "1", model.demo, let d = model.desk else { return }
    MainSampler.start()
    let b = d.board
    let big = b.timelines.values.filter { $0.timelineId.hasPrefix("session/") }.max { $0.items.count < $1.items.count }?.objectId ?? ""
    let others = b.sessions.keys.sorted().filter { $0 != big && b.sessions[$0]?.agentDeviceId != nil }
    var n = 2_000_000_000
    func say(_ s: String) { PerfLog.line("SPEED \(s)") }
    func message(_ session: String, batched: Bool = false) {
      n += 1
      guard let rec = PerfLog.chatLine(b, session: session, number: n, text: "Speed run line \(n): done with the build, all green.") else { return }
      var ch = Change()
      b.apply(rec, change: &ch)
      if batched { model.perfNews() } else { model.perfChanged() }
    }
    func window(_ name: String, seconds: Double, _ op: @escaping @MainActor () async -> Void) async {
      HitchMeter.shared.begin(name)
      if let w = ProcessInfo.processInfo.environment["TROMMI_SPEED_PROFILE_WINDOW"], !w.isEmpty, name.hasPrefix(w.replacingOccurrences(of: "_", with: " ")) { MainProfiler.begin() }   // ("_" for a space)
      let t0 = DispatchTime.now().uptimeNanoseconds, c0 = cpuSeconds()
      await op()
      let left = seconds - Double(DispatchTime.now().uptimeNanoseconds - t0) / 1e9
      if left > 0 { try? await Task.sleep(nanoseconds: UInt64(left * 1e9)) }
      HitchMeter.shared.end(name)
      MainProfiler.end(name)
      let wall = Double(DispatchTime.now().uptimeNanoseconds - t0) / 1e9
      say(String(format: "cpu in %@: %.0f%% of one core", name, 100 * (cpuSeconds() - c0) / max(0.001, wall)))
      say("memory after \(name): \(memoryMB()) MB (peak \(peakMB()) MB)")
    }
    func burst(_ into: @escaping @MainActor (Int) -> String) async {
      for i in 0..<30 { message(into(i)); try? await Task.sleep(nanoseconds: 100_000_000) }
    }
    Task { @MainActor in
      say("start: \(b.sessions.count) sessions, \(b.cards.count) cards, memory \(memoryMB()) MB")
      try? await Task.sleep(nanoseconds: 2_500_000_000)
      // nothing happens: what the app does on its own (should be nothing)
      await window("idle 6 s", seconds: 6) {}
      await window("desk scroll", seconds: 3.2) { model.perfScroll &+= 1 }
      await window("desk: 30 messages in 3 s", seconds: 3.5) { await burst { others[$0 % others.count] } }
      // a flood: 50 events a second for 8 s, through the same 80 ms gathering as a room's changes (BoardModel.changed)
      await window("desk: 50 events/s for 8 s", seconds: 8.5) {
        for i in 0..<400 { message(others[(i * 7) % others.count], batched: true); try? await Task.sleep(nanoseconds: 20_000_000) }
      }
      await window("idle 4 s after the flood", seconds: 4) {}
      await window("desk: 30 bare re-renders", seconds: 3.5) { for _ in 0..<30 { model.perfBump(); try? await Task.sleep(nanoseconds: 100_000_000) } }
      model.tab = .chat
      try? await Task.sleep(nanoseconds: 1_500_000_000)
      await window("chat list: 30 messages in 3 s", seconds: 3.5) { await burst { others[$0 % others.count] } }
      model.tab = .desk
      try? await Task.sleep(nanoseconds: 1_000_000_000)
      let agent = d.agents.first { $0.sessionId == big }?.id ?? ""
      let t0 = DispatchTime.now().uptimeNanoseconds
      await window("open the 5,000-item chat", seconds: 2) {
        model.openChat(agent)
        // (the first frame after the chat's body ran: the open as he sees it)
        await Task.yield()
        DispatchQueue.main.async { say(String(format: "open chat: first frame after %.0f ms", Double(DispatchTime.now().uptimeNanoseconds - t0) / 1e6)) }
      }
      await window("chat scroll", seconds: 3.8) { model.perfScroll &+= 1 }
      await window("chat open: 30 messages elsewhere", seconds: 3.5) { await burst { others[$0 % others.count] } }
      await window("chat open: 30 messages into it", seconds: 3.5) { await burst { _ in big } }
      await window("typing 40 keys", seconds: 2.5) {
        for _ in 0..<40 { NotificationCenter.default.post(name: typeKey, object: nil); try? await Task.sleep(nanoseconds: 50_000_000) }
      }
      // back from the chat to the list (the swipe back, his case on build 59): quiet, then while news arrive
      await window("back to the chat list", seconds: 1.5) { withAnimation { model.chatPath = [] } }
      model.openChat(agent)
      try? await Task.sleep(nanoseconds: 2_000_000_000)
      await window("back to the chat list, news arriving", seconds: 3.5) {
        Task { @MainActor in await burst { others[$0 % others.count] } }
        try? await Task.sleep(nanoseconds: 300_000_000)
        withAnimation { model.chatPath = [] }
      }
      RenderCount.flush("speed run")
      say("done")
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
// (raw memory, not a Swift array: the handler writes it while the sampling thread may still read it after a late
// answer, and an array's exclusive access then trapped the app, SIGTRAP in the speed run)
nonisolated(unsafe) private let sampleFrames = UnsafeMutablePointer<UnsafeMutableRawPointer?>.allocate(capacity: 64)
nonisolated(unsafe) private var sampleCount: Int32 = 0
nonisolated(unsafe) private var sampleReady: Int32 = 0
private func onSample(_ sig: Int32) {
  sampleCount = backtrace(sampleFrames, 64)
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
        // (the beat is written on the main thread and may be a moment newer than `now`: no subtraction below zero, it trapped)
        if b > 0 && now > b && now - b > 30_000_000 && now &- lastSampled > 15_000_000, let m = MainSampler.mainThread {
          lastSampled = now
          sampleReady = 0
          pthread_kill(m, SIGUSR2)
          var waited = 0
          while sampleReady == 0 && waited < 20 { usleep(500); waited += 1 }
          if sampleReady == 1 {
            let n = Int(sampleCount)
            var lines = [String]()
            if let syms = backtrace_symbols(sampleFrames, Int32(n)) {
              for i in 0..<n { if let c = syms[i] { lines.append(String(cString: c)) } }
              free(syms)
            }
            // "3 TrommiApp 0x... symbol + 12": the image and the symbol of each frame, the signal's own frames left out
            let frames = lines.dropFirst(3).prefix(28).map { l -> String in
              let parts = l.split(separator: " ", maxSplits: 3, omittingEmptySubsequences: true)
              return parts.count > 3 ? "\(parts[1]):\(parts[3].split(separator: " ").first ?? "")" : l
            }
            PerfLog.line("STALL \((now &- b) / 1_000_000) ms main at: " + frames.joined(separator: " | "))
          }
        }
      }
    }
  }
}
/**
 * The speed run's profiler (TROMMI_SPEED_PROFILE=1): while a window is open, the main thread's stack every 2 ms,
 * counted per symbol (each symbol once per sample: inclusive); the top lines go to the perf log at its end.
 */
enum MainProfiler {
  nonisolated(unsafe) static var on = false
  nonisolated(unsafe) static var counts: [UnsafeMutableRawPointer: Int] = [:]
  nonisolated(unsafe) static var samples = 0
  static let lock = NSLock()
  @MainActor static func begin() {
    guard ProcessInfo.processInfo.environment["TROMMI_SPEED_PROFILE"] == "1" else { return }
    if MainSampler.mainThread == nil { MainSampler.mainThread = pthread_self(); signal(SIGUSR2, onSample) }
    lock.lock(); counts = [:]; samples = 0; lock.unlock()
    on = true
    Thread.detachNewThread {
      while MainProfiler.on {
        usleep(2000)
        guard let m = MainSampler.mainThread else { continue }
        sampleReady = 0
        pthread_kill(m, SIGUSR2)
        var waited = 0
        while sampleReady == 0 && waited < 40 { usleep(250); waited += 1 }
        guard sampleReady == 1 else { continue }
        var seen = Set<UnsafeMutableRawPointer>()
        lock.lock()
        samples += 1
        for i in 0..<Int(sampleCount) { if let f = sampleFrames[i] { var info = Dl_info(); if dladdr(f, &info) != 0, let a = info.dli_saddr, seen.insert(a).inserted { counts[a, default: 0] += 1 } } }
        lock.unlock()
      }
    }
  }
  @MainActor static func end(_ name: String) {
    guard on else { return }
    on = false
    usleep(5000)
    lock.lock(); let c = counts, n = samples; lock.unlock()
    PerfLog.line("PROFILE \(name): \(n) samples")
    for (a, k) in c.sorted(by: { $0.value > $1.value }).prefix(70) {
      var info = Dl_info(); dladdr(a, &info)
      let sym = info.dli_sname.map { String(cString: $0) } ?? "?"
      let lib = info.dli_fname.map { String(cString: $0).split(separator: "/").last.map(String.init) ?? "" } ?? ""
      PerfLog.line(String(format: "PROFILE %5.1f%% %@ %@", 100 * Double(k) / Double(max(1, n)), lib, String(sym.prefix(160))))
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
