// The Swift fold (CoreView.ViewModel.apply, core/bindings/swift/TrommiCoreRust/Sources/TrommiCoreRust/ViewsFold.swift)
// replays every recorded view scene (core/client/tests/conformance/views/*.json, from core/client/tests/view_scenes.rs)
// over the generated types and must reach the view model the core reached. Runs where swiftc is (Linux or a Mac):
//   S=core/bindings/swift/TrommiCoreRust/Sources/TrommiCoreRust
//   swiftc -parse-as-library -o "$TMPDIR/views-replay" $S/Views.swift $S/ViewsFold.swift tests/bindings/views-replay.swift \
//     && "$TMPDIR/views-replay" core/client/tests/conformance/views
import Foundation

struct Recording: Decodable {
  let scene: String
  let deltas: [CoreView.ViewDelta]
  let view: CoreView.ViewModel
}

@main
struct ViewsReplay {
  static func main() throws {
    let dir = CommandLine.arguments.dropFirst().first ?? "core/client/tests/conformance/views"
    let names = try FileManager.default.contentsOfDirectory(atPath: dir).filter { $0.hasSuffix(".json") }.sorted()
    guard names.count >= 5 else { print("the recorded scenes are missing"); exit(1) }
    for name in names {
      let data = try Data(contentsOf: URL(fileURLWithPath: "\(dir)/\(name)"))
      let r = try JSONDecoder().decode(Recording.self, from: data)
      var v = CoreView.ViewModel()
      for d in r.deltas { try v.apply(d) }
      guard v == r.view else { print("\(r.scene): the folded view is not the core's"); exit(1) }
      // (and written as the core wrote it)
      let mine = try JSONSerialization.jsonObject(with: JSONEncoder().encode(v)) as! NSDictionary
      let core = (try JSONSerialization.jsonObject(with: data) as! NSDictionary)["view"] as! NSDictionary
      guard mine == core else { print("\(r.scene): the folded view is written differently"); exit(1) }
      if r.deltas.count > 1 {
        do { try v.apply(r.deltas[1]); print("\(r.scene): an old delta was folded"); exit(1) } catch is CoreView.OutOfOrder {}
      }
    }
    print("\(names.count) recorded scenes replayed by the Swift fold")
  }
}
