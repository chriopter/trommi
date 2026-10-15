import Foundation
import SwiftUI
import TrommiClient
import PushNotify
import ShareInbox
import UserNotifications
#if canImport(UIKit)
import UIKit
#else
import AppKit
#endif

@MainActor final class NativeServices {
  static let shared = NativeServices()
  weak var shell: AppShell?
  private var registration: Int?
  private var oldRooms: [String] = []
  func attach(_ shell: AppShell) { self.shell = shell; NativeLive.shared.attach(shell); NativeShare.shared.read(shell) }
  func foreground(_ active: Bool) { if active { NativeShare.shared.read(shell) } }
  func draw(_ view: CoreView.ViewModel) { NativeShare.shared.draw(view, shell: shell) }
  func platform(_ packet: NativePacket, shell: AppShell) async {
    let id = packet.json["id"] as? Int ?? 0
    let op = packet.json["op"] as? [String: Any] ?? [:]
    switch op["op"] as? String {
    case "push_register":
      do {
        let allowed = try await UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound, .badge])
        if allowed {
          registration = id
          #if canImport(UIKit)
          UIApplication.shared.registerForRemoteNotifications()
          #else
          NSApplication.shared.registerForRemoteNotifications()
          #endif
          return
        }
      } catch {}
      shell.send(["type": "platform", "id": id, "result": ["ok": ["answer": "refused"]]])
    case "push_unregister":
      #if canImport(UIKit)
      UIApplication.shared.unregisterForRemoteNotifications()
      #else
      NSApplication.shared.unregisterForRemoteNotifications()
      #endif
      NativeLive.shared.end()
      shell.send(["type": "platform", "id": id, "result": ["ok": ["answer": "done"]]])
    default: shell.send(["type": "platform", "id": id, "result": ["ok": ["answer": "refused"]]])
    }
  }
  func token(_ data: Data) {
    guard let shell else { return }
    #if DEBUG
    let environment = "sandbox"
    #else
    let environment = "production"
    #endif
    let target = ["token": data.map { String(format: "%02x", $0) }.joined(), "environment": environment, "topic": Bundle.main.bundleIdentifier ?? "com.trommi.ios"]
    guard let bytes = try? JSONSerialization.data(withJSONObject: target) else { return }
    if let id = registration { registration = nil; shell.send(["type": "platform", "id": id, "result": ["ok": ["answer": "push", "kind": "apns", "token": 0]]], blobs: [bytes]) }
    shell.send(["type": "push_token", "kind": "apns", "token": 0], blobs: [bytes])
  }
  func tokenFailed() {
    if let id = registration { registration = nil; shell?.send(["type": "platform", "id": id, "result": ["ok": ["answer": "refused"]]]) }
  }
  func notify(_ notice: [String: Any]) {
    let content = UNMutableNotificationContent()
    content.title = (notice["title"] as? [String: Any])?["en"] as? String ?? "Trommi"
    content.body = (notice["body"] as? [String: Any])?["en"] as? String ?? ""
    if notice["urgent"] as? Bool == true { content.sound = .default }
    UNUserNotificationCenter.current().add(UNNotificationRequest(identifier: notice["id"] as? String ?? UUID().uuidString, content: content, trigger: nil))
  }
  func context(_ packet: NativePacket) {
    #if os(iOS)
    func clear() {
      for room in oldRooms { NotifyGroup.prefetch(room: room)?.clear() }
      NotifyGroup.clear(); NotifyKeyGroup.clear(accessGroup: AppKeychain.notifyKeysGroup); oldRooms = []
    }
    guard let c = packet.json["context"] as? [String: Any] else { clear(); return }
    do {
      guard let room = c["room"] as? String, let hub = c["hub"] as? String, let client = c["client"] as? String, let cursor = c["cursor"] as? NSNumber else { throw TrommiError("bad-format") }
      let context = PushNotify.NotifyContext(pushKey: Bytes(try packet.bytes(c["push_key"])), rooms: [room], hub: hub, client: client, cursor: cursor.uint64Value)
      try NotifyGroup.writeContext(context); oldRooms = [room]
      let keys = try (c["keys"] as? [[String: Any]] ?? []).map { k -> PushNotify.NotifyKey in
        guard let group = k["group"] as? String, let bytes = NativeBytes.unhex(group), let epoch = k["epoch"] as? NSNumber else { throw TrommiError("bad-format") }
        return PushNotify.NotifyKey(group: bytes, epoch: epoch.uint64Value, key: Bytes(try packet.bytes(k["key"])))
      }
      if keys.isEmpty { NotifyKeyGroup.clear(accessGroup: AppKeychain.notifyKeysGroup) }
      else {
        guard let access = AppKeychain.notifyKeysGroup else { throw TrommiError("keychain") }
        try NotifyKeyGroup.write(NotifyKeySet(room: room, keys: keys), accessGroup: access)
      }
    } catch { clear() }
    #endif
  }
  func prefetched(_ request: NativePacket) -> NativePacket? {
    #if os(iOS)
    guard request.json["method"] as? String == "GET", let text = request.json["url"] as? String, let url = URLComponents(string: text), url.path.hasSuffix("/v1/changes"),
          let after = url.queryItems?.first(where: { $0.name == "after" })?.value.flatMap(UInt64.init), let room = oldRooms.first,
          let data = NotifyGroup.prefetch(room: room)?.take(after: after) else { return nil }
    return NativePacket(["status": 200, "headers": [["name": "content-type", "value": "application/json"]], "body": 0], blobs: [data])
    #else
    return nil
    #endif
  }
  func snapshot(_ value: [String: Any]) { NativeLive.shared.draw(value); NativeShare.shared.snapshot(value, shell: shell) }
  func open(_ url: URL, shell: AppShell) {
    // Universal links are navigation only. Shared files are opened by the core, which verifies their link.
    guard url.scheme == "https", url.host == "app.trommi.com" else { return }
    if url.path.hasPrefix("/artifact/") { shell.act(["do": "open_shared", "link": url.absoluteString]); return }
    let parts = url.path.split(separator: "/").map(String.init)
    if parts.first == "card", parts.count > 1 { shell.focus(.card(cardId: parts[1])) }
    else if parts.first == "chat", parts.count > 1 { shell.focus(.session(sessionId: parts[1])) }
    else if parts.first == "settings" { shell.focus(.settings) }
    else { shell.focus(.desk) }
  }
}

#if canImport(UIKit)
final class PushDelegate: NSObject, UIApplicationDelegate, UNUserNotificationCenterDelegate {
  func application(_ application: UIApplication, didFinishLaunchingWithOptions options: [UIApplication.LaunchOptionsKey: Any]? = nil) -> Bool { NativeDemo.mark("delegate.begin"); Spent.time("notification.delegate") { UNUserNotificationCenter.current().delegate = self }; NativeDemo.mark("delegate.end"); return true }
  func application(_ application: UIApplication, didRegisterForRemoteNotificationsWithDeviceToken token: Data) { Task { @MainActor in NativeServices.shared.token(token) } }
  func application(_ application: UIApplication, didFailToRegisterForRemoteNotificationsWithError error: Error) { Task { @MainActor in NativeServices.shared.tokenFailed() } }
  func application(_ application: UIApplication, didReceiveRemoteNotification userInfo: [AnyHashable: Any], fetchCompletionHandler done: @escaping (UIBackgroundFetchResult) -> Void) {
    Task { @MainActor in
      if let text = userInfo["e"] as? String, let bytes = NativeBytes.unb64(text) { AppShell.shared.send(["type": "push", "payload": 0], blobs: [Data(bytes)]) }
      AppShell.shared.send(["type": "foreground"]); done(.newData)
    }
  }
  func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification) async -> UNNotificationPresentationOptions { await MainActor.run { AppShell.shared.send(["type": "foreground"]) }; return [] }
  func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse) async { await MainActor.run { AppShell.shared.send(["type": "foreground"]) } }
}
#else
final class PushDelegate: NSObject, NSApplicationDelegate, UNUserNotificationCenterDelegate {
  func applicationDidFinishLaunching(_ notification: Notification) { UNUserNotificationCenter.current().delegate = self }
  func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }
  func application(_ application: NSApplication, didRegisterForRemoteNotificationsWithDeviceToken token: Data) { Task { @MainActor in NativeServices.shared.token(token) } }
  func application(_ application: NSApplication, didFailToRegisterForRemoteNotificationsWithError error: Error) { Task { @MainActor in NativeServices.shared.tokenFailed() } }
  func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification) async -> UNNotificationPresentationOptions { await MainActor.run { AppShell.shared.send(["type": "foreground"]) }; return [] }
  func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse) async { await MainActor.run { AppShell.shared.send(["type": "foreground"]) } }
}
#endif

enum NativeBytes {
  static func unhex(_ text: String) -> [UInt8]? {
    guard text.count % 2 == 0 else { return nil }; let chars = Array(text); var out: [UInt8] = []
    for i in stride(from: 0, to: chars.count, by: 2) { guard let b = UInt8(String(chars[i...i+1]), radix: 16) else { return nil }; out.append(b) }; return out
  }
  static func unb64(_ text: String) -> [UInt8]? {
    let value = text.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
    return Data(base64Encoded: value + String(repeating: "=", count: (4 - value.count % 4) % 4)).map { Array($0) }
  }
}
