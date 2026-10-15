// Bundled demo I/O and navigation only. AppCore makes the room, screen state, actions and drawing data.
import Foundation
import TrommiClient

enum NativeDemo {
  static var screen: String? { ProcessInfo.processInfo.environment["TROMMI_SCREEN"].flatMap { $0.isEmpty ? nil : $0 } }
  private static func url(_ name: String) -> URL {
    (Bundle.module.resourceURL ?? Bundle.main.bundleURL).appendingPathComponent("Demo/" + name)
  }
  static func fixture() throws -> Any { try JSONSerialization.jsonObject(with: Data(contentsOf: url("fixture.json"))) }
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
