import Foundation
import TrommiClient
import PushNotify
#if canImport(ActivityKit) && os(iOS)
import ActivityKit
import UIKit
#endif

// ActivityKit I/O and drawing. The core supplies both counts and the already ordered waiting cards.
@MainActor final class NativeLive {
  static let shared = NativeLive()
  weak var shell: AppShell?
  private var watching = false
  private var followed = Set<String>()
  private var value: [String: Any] = [:]
  func attach(_ shell: AppShell) {
    self.shell = shell
    #if canImport(ActivityKit) && os(iOS)
    LiveAnswer.handler = { [weak shell] id, key in
      await MainActor.run { shell?.act(["do": "answer", "card_id": id, "choices": [key]]) }
    }
    guard !watching else { return }; watching = true
    Task {
      for await token in Activity<TrommiActivityAttributes>.pushToStartTokenUpdates { send(token, kind: "start", tag: nil) }
    }
    Task {
      for activity in Activity<TrommiActivityAttributes>.activities { follow(activity) }
      for await activity in Activity<TrommiActivityAttributes>.activityUpdates { follow(activity) }
    }
    #endif
  }
  func draw(_ snapshot: [String: Any]) {
    value = snapshot
    if snapshot["live_activity"] as? Bool == false { end(); return }
    #if canImport(ActivityKit) && os(iOS)
    Task {
      let rows = snapshot["cards"] as? [[String: Any]] ?? []
      var card: LiveCard?
      if UIApplication.shared.isProtectedDataAvailable, let first = rows.first {
        let options = first["options"] as? [[String: String]] ?? []
        let two = first["kind"] as? String == "decision" && first["multiple"] as? Bool != true && options.count == 2
        card = LiveCard(id: first["id"] as? String ?? "", title: LiveState.clip(first["title"] as? String ?? "", 90), agent: LiveState.clip(first["session"] as? String ?? "", 24), urgency: first["urgency"] as? String ?? "normal", kind: first["kind"] as? String ?? "decision", since: Date(timeIntervalSince1970: (first["created_ms"] as? Double ?? 0) / 1000), options: two ? options.map { LiveOption(key: $0["key"] ?? "", label: LiveState.clip($0["label"] ?? "", 18)) } : [])
      }
      let state = LiveCounts(working: snapshot["working"] as? Int ?? 0, waiting: snapshot["waiting"] as? Int ?? 0, card: card, exact: true, busy: snapshot["working"] as? Int)
      for activity in Activity<TrommiActivityAttributes>.activities where activity.content.state != state { await activity.update(ActivityContent(state: state, staleDate: nil)) }
    }
    #endif
  }
  func end() {
    #if canImport(ActivityKit) && os(iOS)
    Task { for activity in Activity<TrommiActivityAttributes>.activities { await activity.end(nil, dismissalPolicy: .immediate) } }
    #endif
  }
  #if canImport(ActivityKit) && os(iOS)
  private func follow(_ activity: Activity<TrommiActivityAttributes>) {
    if value["live_activity"] as? Bool == false { Task { await activity.end(nil, dismissalPolicy: .immediate) }; return }
    guard followed.insert(activity.id).inserted else { return }
    Task { for await token in activity.pushTokenUpdates { send(token, kind: "activity", tag: activity.attributes.tag) } }
    Task { for await _ in activity.contentUpdates { draw(value) } }
  }
  private func send(_ token: Data, kind: String, tag: String?) {
    #if DEBUG
    let environment = "sandbox"
    #else
    let environment = "production"
    #endif
    var value: [String: Any] = ["kind": kind, "token": token.map { String(format: "%02x", $0) }.joined(), "environment": environment, "topic": Bundle.main.bundleIdentifier ?? "com.trommi.ios"]
    if let tag { value["tag"] = tag }
    if let data = try? JSONSerialization.data(withJSONObject: value) { shell?.send(["type": "push_token", "kind": "apns_live_activity", "token": 0], blobs: [data]) }
  }
  #endif
}
