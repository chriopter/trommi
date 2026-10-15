// Bundled demo I/O and navigation only. AppCore makes the room, screen state, actions and drawing data.
import Foundation
import TrommiClient
#if canImport(Darwin)
import Darwin
#endif

enum NativeDemo {
  static var launchStart = ProcessInfo.processInfo.systemUptime
  static var launchTicks = Spent.now
  static var foldedAt: Double?
  static var checkpoints: [String: Double] = [:]
  static func mark(_ name: String) { if measuring && checkpoints[name] == nil { checkpoints[name] = (ProcessInfo.processInfo.systemUptime - launchStart) * 1000 } }
  static var measuring: Bool { screen != nil && ProcessInfo.processInfo.environment["TROMMI_SPEED_TRACE"] == "1" }
  static func ready(rows: Int) -> String? {
    guard measuring else { return nil }
    var resident: UInt64 = 0
    #if canImport(Darwin)
    var info = mach_task_basic_info()
    var count = mach_msg_type_number_t(MemoryLayout<mach_task_basic_info>.size / MemoryLayout<natural_t>.size)
    let status = withUnsafeMutablePointer(to: &info) { pointer in
      pointer.withMemoryRebound(to: integer_t.self, capacity: Int(count)) { task_info(mach_task_self_, task_flavor_t(MACH_TASK_BASIC_INFO), $0, &count) }
    }
    if status == KERN_SUCCESS { resident = info.resident_size }
    #endif
    let report: [String: Any] = ["rows": rows, "desk_ms": (ProcessInfo.processInfo.systemUptime - launchStart) * 1000, "resident_bytes": resident, "folded_ms": foldedAt ?? 0, "checkpoints": checkpoints, "steps": Spent.take()]
    guard let data = try? JSONSerialization.data(withJSONObject: report, options: [.sortedKeys]) else { return nil }
    let text = String(decoding: data, as: UTF8.self)
    // A normal simctl launch can be measured without XCTest injecting its startup services.
    if let folder = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first {
      try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
      try? data.write(to: folder.appendingPathComponent("native-desk-startup.json"), options: .atomic)
    }
    return text
  }
  static var screen: String? { ProcessInfo.processInfo.environment["TROMMI_SCREEN"].flatMap { $0.isEmpty ? nil : $0 } }
  private static func url(_ name: String) -> URL {
    (Bundle.module.resourceURL ?? Bundle.main.bundleURL).appendingPathComponent("Demo/" + name)
  }
  static func fixture() throws -> Any {
    let path = ProcessInfo.processInfo.environment["TROMMI_FIXTURE"]
    let source = screen != nil && path?.isEmpty == false ? URL(fileURLWithPath: path!) : url("fixture.json")
    return try JSONSerialization.jsonObject(with: Data(contentsOf: source))
  }
  static func file(_ request: NativePacket) -> NativePacket? {
    guard request.json["method"] as? String == "GET", let text = request.json["url"] as? String, text.hasPrefix("/demo/files/"),
          let name = text.split(separator: "/").last.map(String.init), !name.contains(".."), !name.contains("%"), let data = try? Data(contentsOf: url("files/" + name)) else { return nil }
    return NativePacket(["status": 200, "headers": [], "body": 0], blobs: [data])
  }
  @MainActor static func route(_ id: String, shell: AppShell) {
    if id == "desk--corner-note-open" { shell.focus(.notes); return }
    let data = try? Data(contentsOf: url("screens.json"))
    let json = data.flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] }
    let rows = json?["states"] as? [[String: Any]] ?? []
    let path = (rows.first { $0["id"] as? String == id }?["web"] as? [String: Any])?["path"] as? String ?? "/"
    let parts = URLComponents(string: path)?.path.split(separator: "/").map(String.init) ?? []
    switch parts.first {
    case "card" where parts.count > 1: shell.focus(.card(cardId: parts[1]))
    case "chat" where parts.count > 1: shell.focus(.session(sessionId: parts[1]))
    case "scribble-board": shell.focus(.scribble(boardId: shell.view.sidebar.desk))
    case "settings": shell.focus(.settings)
    case "stacks": shell.focus(.off(query: ""))
    case "artifacts": shell.focus(.artifacts)
    default: shell.focus(.desk)
    }
  }
}
