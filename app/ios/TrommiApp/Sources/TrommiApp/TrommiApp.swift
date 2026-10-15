import SwiftUI
import TrommiClient

@main struct TrommiApp: App {
  @StateObject private var shell = AppShell.shared
  @Environment(\.scenePhase) private var phase
  #if canImport(UIKit)
  @UIApplicationDelegateAdaptor(PushDelegate.self) private var push
  #else
  @NSApplicationDelegateAdaptor(PushDelegate.self) private var push
  #endif
  init() { NativeDemo.launchTicks = Spent.now; NativeDemo.launchStart = ProcessInfo.processInfo.systemUptime; Spent.time("fonts") { Face.registerFonts() } }
  private var window: some Scene {
    WindowGroup {
      RootView().environmentObject(shell).tint(Ink.accent)
        .task { await shell.start() }
        .onChange(of: phase) { _, value in shell.scene(value == .active) }
        .onOpenURL { NativeServices.shared.open($0, shell: shell) }
    }
  }
  var body: some Scene {
    #if os(macOS)
    window.defaultSize(width: 1180, height: 780)
    MenuBarExtra("Trommi", systemImage: "bell") {
      ForEach(shell.view.desk.rows, id: \.id) { row in Button(row.title) { shell.focus(.card(cardId: row.id)); NSApplication.shared.activate() } }
    }
    #else
    window
    #endif
  }
}
