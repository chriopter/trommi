// The generated Swift view types (core/bindings/swift/TrommiCoreRust/Sources/TrommiCoreRust/Views.swift, from
// tests/bindings/views.mjs) read the core's real views and write them back as they came: every golden unit of the
// demo room is decoded, encoded again, and compared as JSON. Runs where swiftc is (Linux or a Mac):
//   swiftc -o "$TMPDIR/views-check" core/bindings/swift/TrommiCoreRust/Sources/TrommiCoreRust/Views.swift \
//     tests/bindings/views-check.swift && "$TMPDIR/views-check" core/client/tests/golden
import Foundation

@main
struct ViewsCheck {
  static func roundTrip<T: Codable>(_ type: T.Type, _ data: Data, _ name: String) throws -> Bool {
    let value = try JSONDecoder().decode(T.self, from: data)
    let again = try JSONEncoder().encode(value)
    let a = try JSONSerialization.jsonObject(with: data) as! NSDictionary
    let b = try JSONSerialization.jsonObject(with: again) as! NSDictionary
    if a != b { print("\(name): written back differently"); return false }
    return true
  }

  static func main() throws {
    let dir = CommandLine.arguments.dropFirst().first ?? "core/client/tests/golden"
    let names = try FileManager.default.contentsOfDirectory(atPath: dir).filter { $0.hasSuffix(".json") }.sorted()
    var ok = 0
    for name in names {
      let data = try Data(contentsOf: URL(fileURLWithPath: "\(dir)/\(name)"))
      let same: Bool
      if name.hasPrefix("demo-sidebar") { same = try roundTrip(CoreView.SidebarView.self, data, name) }
      else if name.hasPrefix("demo-desk") { same = try roundTrip(CoreView.DeskView.self, data, name) }
      else if name.hasPrefix("demo-session") { same = try roundTrip(CoreView.SessionView.self, data, name) }
      else if name.hasPrefix("demo-card") { same = try roundTrip(CoreView.CardView.self, data, name) }
      else { print("\(name): which view is it?"); exit(1) }
      if !same { exit(1) }
      ok += 1
    }
    // (a screen as a shell sends it, the fields it may leave out left out)
    let off = try JSONDecoder().decode(CoreView.Focus.self, from: Data(#"{"screen":{"screen":"off"}}"#.utf8))
    guard off.screen == .off(query: ""), off.visible else { print("Focus: defaults not taken"); exit(1) }
    print("\(ok) golden views read and written back as they came")
  }
}
