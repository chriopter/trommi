import Foundation
import SwiftUI
import TrommiClient
import ShareInbox

// The App Group's sealed inbox only. The core chooses the room, recipients, uploads and what becomes a note.
@MainActor final class NativeShare {
  static let shared = NativeShare()
  private var inbox: ShareInbox?
  private var observing = false
  private var last: ShareSnapshot?
  private var claimed = Set<String>()
  private func open(_ shell: AppShell?) {
    #if os(iOS)
    if inbox == nil { inbox = ShareGroup.inbox(create: true); inbox?.sweep() }
    if !observing { observing = true; ShareSignal.observe { [weak shell] in NativeShare.shared.read(shell) } }
    #endif
  }
  func draw(_ view: CoreView.ViewModel, shell: AppShell?) { open(shell) }
  func read(_ shell: AppShell?) {
    guard let shell else { return }; open(shell)
    guard let inbox else { return }
    for request in inbox.pending() where !claimed.contains(request.id) && inbox.claim(request.id) {
      claimed.insert(request.id)
      var blobs: [Data] = []; var files: [[String: Any]] = []; var unreadable = false
      for item in request.items where item.kind == .image || item.kind == .file {
        if let data = try? inbox.payload(item) { files.append(["name": item.name, "media_type": item.type, "bytes": blobs.count]); blobs.append(data) } else { unreadable = true }
      }
      if unreadable { inbox.release(request.id); claimed.remove(request.id); shell.platformError = "A shared file could not be opened. The share is kept for another try."; continue }
      var action: [String: Any] = ["do": "import_share", "id": request.id, "send": request.action == .send, "text": request.words, "files": files]
      if let room = request.room { action["room"] = room }
      if let team = request.team { action["team"] = team }
      if let to = request.to { action["to"] = to }
      shell.act(action, blobs: blobs)
    }
  }
  func completed(_ id: String, consumed: Bool) {
    if consumed { inbox?.finish(id) } else { inbox?.release(id) }
    claimed.remove(id)
  }
  func snapshot(_ value: [String: Any], shell: AppShell?) {
    open(shell)
    guard let room = value["room"] as? String else {
      inbox?.clearSnapshot(); last = nil
      for id in claimed { inbox?.release(id) }; claimed = []
      return
    }
    let teams = (value["teams"] as? [[String: Any]] ?? []).map { team -> ShareSnapshot.Team in
      let crown = (team["crown"] as? [String: Any]).map { c in ShareSnapshot.Crown(id: c["session_id"] as? String ?? "", name: c["name"] as? String ?? "", mark: drawing(c["mark"] as? String ?? "")) }
      return ShareSnapshot.Team(id: team["id"] as? String ?? "", name: team["name"] as? String ?? "", crown: crown)
    }
    var snapshot = ShareSnapshot(room: room, teams: teams)
    if let last { snapshot.written = last.written; if snapshot == last { return } }
    snapshot.written = ShareInbox.nowMs()
    do { try inbox?.writeSnapshot(snapshot); last = snapshot } catch { inbox?.clearSnapshot(); last = nil }
  }
  func forget() {
    inbox?.wipe(); inbox = nil; last = nil; claimed = []
    #if os(iOS)
    if let group = ShareGroup.resolve() { ShareKeychain.remove(group: group.id) }
    #endif
  }
  private func drawing(_ mark: String) -> Data? {
    #if canImport(UIKit)
    let renderer = ImageRenderer(content: AgentMark(mark: mark).environment(\.colorScheme, .light)); renderer.scale = 3
    return renderer.uiImage?.pngData()
    #else
    return nil
    #endif
  }
}
