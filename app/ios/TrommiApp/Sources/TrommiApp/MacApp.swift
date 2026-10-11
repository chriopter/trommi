// MacApp: the scenes of Trommi on a Mac (app/ios/README.md "On a Mac"). The main window is the iPad's layout (the
// sessions as a sidebar beside the stack); the menu bar holds what the Live Activity holds on the iPhone: the
// questions waiting, each one click away. The keys of the window are in the menu, as a Mac app's are.
#if os(macOS)
import AppKit
import SwiftUI
import TrommiClient

struct MacScenes: Scene {
  let model: BoardModel
  let push: PushDelegate
  @Environment(\.scenePhase) private var scenePhase

  var body: some Scene {
    Window("Trommi", id: "main") {
      RootView().environmentObject(model)
        .tint(Ink.accent)
        // the iPad's layout on any window width: sidebar and stack, never the iPhone's pill
        .environment(\.horizontalSizeClass, .regular)
        .frame(minWidth: 760, minHeight: 520)
        .onAppear { push.model = model; push.ask() }
        .onReceive(model.$phase.dropFirst().removeDuplicates()) { p in if p == .board { push.ask() } }
        .onChange(of: scenePhase) { _, p in model.scene(active: p == .active) }
        .onAppear { PerfScript.runIfAsked(model); MegaScript.runIfAsked(model); SpeedScript.runIfAsked(model) }
        .onOpenURL { model.open(url: $0) }
    }
    .defaultSize(width: 1180, height: 780)
    .commands { MacCommands(model: model) }

    MenuBarExtra {
      MenuBarCards().environmentObject(model)
    } label: {
      MenuBarLabel().environmentObject(model)
    }
    .menuBarExtraStyle(.window)
  }
}

/** The app's own menu items: back to the Desk, read again, Settings. */
struct MacCommands: Commands {
  let model: BoardModel
  var body: some Commands {
    CommandGroup(replacing: .appSettings) {
      Button("Settings…") { MacWindow.show(); model.tab = .desk; model.deskPath = [.settings("devices")] }
        .keyboardShortcut(",", modifiers: .command)
    }
    CommandMenu("Board") {
      Button("Desk") { MacWindow.show(); model.tab = .desk; model.deskPath = [] }
        .keyboardShortcut("1", modifiers: .command)
      Button("Read Again") { Task { await model.refresh() } }
        .keyboardShortcut("r", modifiers: .command)
    }
  }
}

enum MacWindow {
  /** The main window to the front (from the menu bar, where the app may have no window in front). */
  @MainActor static func show() {
    NSApplication.shared.activate()
    if let w = NSApplication.shared.windows.first(where: { $0.identifier?.rawValue.hasPrefix("main") == true || $0.title == "Trommi" }) {
      w.makeKeyAndOrderFront(nil)
    }
  }
}

/** The bell in the menu bar, with the number of questions waiting. */
struct MenuBarLabel: View {
  @EnvironmentObject var model: BoardModel
  var body: some View {
    let n = model.view?.fresh.count ?? 0
    HStack(spacing: 3) {
      Image(systemName: n > 0 ? "bell.badge" : "bell")
      if n > 0 { Text("\(n)") }
    }
    .accessibilityLabel(n == 0 ? "Trommi: nothing waiting" : "Trommi: \(n) waiting")
  }
}

/** The menu bar's panel: the open questions, most urgent first (the Desk's order); a click opens the card. */
struct MenuBarCards: View {
  @EnvironmentObject var model: BoardModel
  @Environment(\.openWindow) private var openWindow
  var body: some View {
    let cards = model.view?.fresh ?? []
    VStack(alignment: .leading, spacing: 0) {
      HStack {
        Text("Waiting for you").font(Face.text(13, .semibold)).foregroundStyle(Ink.muted)
        Spacer()
        Text("\(cards.count)").font(Face.text(13, .semibold)).foregroundStyle(Ink.muted)
      }
      .padding(.horizontal, 14).padding(.top, 12).padding(.bottom, 8)
      if model.phase != .board {
        Text("Not signed in.").font(Face.text(14)).foregroundStyle(Ink.muted).padding(14)
      } else if cards.isEmpty {
        Text("Nothing waiting.").font(Face.text(14)).foregroundStyle(Ink.muted).padding(14)
      } else {
        ScrollView {
          VStack(alignment: .leading, spacing: 2) {
            ForEach(cards.prefix(12)) { c in
              Button { open(c) } label: { row(c) }.buttonStyle(MenuRowStyle())
            }
          }
          .padding(.horizontal, 6)
        }
        .frame(maxHeight: 420)
      }
      Divider().padding(.top, 6)
      HStack {
        Button("Open Trommi") { openWindow(id: "main"); MacWindow.show() }
        Spacer()
        Button("Quit") { NSApplication.shared.terminate(nil) }
      }
      .buttonStyle(.borderless)
      .font(Face.text(13))
      .padding(.horizontal, 14).padding(.vertical, 10)
    }
    .frame(width: 340)
    .background(Ink.bg)
  }
  private func row(_ c: DeskCard) -> some View {
    HStack(alignment: .firstTextBaseline, spacing: 8) {
      Text("\(c.number)").font(Face.text(12, .semibold)).foregroundStyle(Ink.faint).frame(minWidth: 26, alignment: .trailing)
      VStack(alignment: .leading, spacing: 2) {
        if let word = c.knockWord { Text(word).font(Face.text(11, .semibold)).foregroundStyle(Ink.urgCritical) }
        Text(c.title).font(Face.text(14, .medium)).foregroundStyle(Ink.fg).lineLimit(2).multilineTextAlignment(.leading)
        if let a = model.agent(c.agent) { Text(a.name).font(Face.text(12)).foregroundStyle(Ink.muted).lineLimit(1) }
      }
      Spacer(minLength: 0)
    }
    .padding(.horizontal, 8).padding(.vertical, 7)
    .frame(maxWidth: .infinity, alignment: .leading)
    .contentShape(Rectangle())
  }
  private func open(_ c: DeskCard) {
    openWindow(id: "main")
    MacWindow.show()
    model.tab = .desk
    model.deskPath = [.card(c.id)]
  }
}

/**
 * The screenshot hook beside TROMMI_SCREEN (the demo, nothing sent): TROMMI_MAC_SHOT=<name> draws the main window
 * (the same views in a window of its own, 1180 x 780) and the menu bar's panel into <name>-window.png and
 * <name>-menu.png in the app's temporary folder, then quits. It draws the views itself, so it runs where no one is
 * logged in at the screen (a build Mac over ssh) and needs no screen recording right. TROMMI_THEME=dark: dark.
 */
enum MacShot {
  static var asked: String? {
    let env = ProcessInfo.processInfo.environment
    guard let name = env["TROMMI_MAC_SHOT"], !name.isEmpty, env["TROMMI_SCREEN"] != nil else { return nil }
    return name
  }
  @MainActor static func runIfAsked(_ model: BoardModel) {
    guard let name = asked else { return }
    let env = ProcessInfo.processInfo.environment
    let wait = Double(env["TROMMI_MAC_SHOT_WAIT"] ?? "") ?? 5
    let dark = env["TROMMI_THEME"] == "dark"
    NSApplication.shared.appearance = NSAppearance(named: dark ? .darkAqua : .aqua)
    let dir = FileManager.default.temporaryDirectory
    let main = FreeWindow(contentRect: NSRect(x: 0, y: 0, width: 1180, height: 780), styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
                        backing: .buffered, defer: false)
    main.title = "Trommi"
    let host = NSHostingController(rootView: RootView().environmentObject(model).tint(Ink.accent).environment(\.horizontalSizeClass, .regular))
    host.sizingOptions = []
    main.contentViewController = host
    main.setContentSize(NSSize(width: 1180, height: 780))
    main.orderFront(nil)
    let panel = NSHostingView(rootView: MenuBarCards().environmentObject(model))
    let holder = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 340, height: 480), styleMask: [.borderless], backing: .buffered, defer: false)
    holder.contentView = panel
    holder.orderFront(nil)
    DispatchQueue.main.asyncAfter(deadline: .now() + wait) {
      holder.setContentSize(NSSize(width: 340, height: max(120, panel.fittingSize.height)))
      main.setFrame(main.frameRect(forContentRect: NSRect(x: 0, y: 0, width: 1180, height: 780)), display: true)
      main.layoutIfNeeded(); main.displayIfNeeded()
      DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
        if let frame = main.contentView?.superview, let img = composed(frame) { save(img, to: dir.appendingPathComponent("\(name)-window.png")) }
        write(panel, to: dir.appendingPathComponent("\(name)-menu.png"))
        NSLog("trommi mac shot: written to %@", dir.path)
        exit(0)
      }
    }
  }
  /** A window the screen does not cut down: on a Mac with no screen (ssh) it keeps the size it is given. */
  final class FreeWindow: NSWindow {
    override func constrainFrameRect(_ frameRect: NSRect, to screen: NSScreen?) -> NSRect { frameRect }
  }
  static let scale: CGFloat = 2
  /** One view drawn alone, through its layers (SwiftUI draws into layers, which cacheDisplay misses). */
  @MainActor static func image(_ v: NSView) -> CGImage? {
    let size = v.bounds.size
    guard size.width > 0, size.height > 0, let layer = v.layer, let space = CGColorSpace(name: CGColorSpace.sRGB),
          let ctx = CGContext(data: nil, width: Int(size.width * scale), height: Int(size.height * scale), bitsPerComponent: 8, bytesPerRow: 0,
                              space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return nil }
    ctx.scaleBy(x: scale, y: scale)
    if v.isFlipped { ctx.translateBy(x: 0, y: size.height); ctx.scaleBy(x: 1, y: -1) }
    layer.render(in: ctx)
    return ctx.makeImage()
  }
  /**
   * The window as one picture. Drawn whole, the scrolled content behind the glass of macOS 26 (portal and backdrop
   * layers, composed by the window server) stays empty, and the glass comes out as black or white; so the page's
   * paper, the scrolled content and the sidebar are drawn each on its own where they lie, the title bar on top.
   * A drawing, not a photograph: the glass of the bars is missing.
   */
  @MainActor static func composed(_ frame: NSView) -> CGImage? {
    let size = frame.bounds.size
    guard let base = image(frame), let space = CGColorSpace(name: CGColorSpace.sRGB),
          let ctx = CGContext(data: nil, width: Int(size.width * scale), height: Int(size.height * scale), bitsPerComponent: 8, bytesPerRow: 0,
                              space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return nil }
    ctx.scaleBy(x: scale, y: scale)
    // the page's paper under everything (Theme.swift Ink.bg); the window's own layers leave it clear
    let dark = frame.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
    ctx.setFillColor(dark ? CGColor(srgbRed: 0x0e / 255, green: 0x13 / 255, blue: 0x11 / 255, alpha: 1) : CGColor(gray: 1, alpha: 1))
    ctx.fill(frame.bounds)
    var docs: [NSView] = [], sidebars: [NSView] = []
    func walk(_ v: NSView) {
      let t = String(describing: type(of: v))
      if t.hasPrefix("DocumentView") { docs.append(v) }
      if t.contains("ColumnView") && t.contains("Sidebar") { sidebars.append(v) }
      v.subviews.forEach(walk)
    }
    walk(frame)
    func put(_ v: NSView, clip: NSRect) {
      guard let img = image(v) else { return }
      let r = v.convert(v.bounds, to: nil)
      ctx.saveGState(); ctx.clip(to: clip); ctx.draw(img, in: r); ctx.restoreGState()
    }
    for d in docs { put(d, clip: d.convert(d.visibleRect, to: nil)) }
    for s in sidebars { put(s, clip: s.convert(s.bounds, to: nil)) }
    // the title bar's strip (the window's buttons, the toolbar's items) over the scrolled content and the sidebar
    let bar: CGFloat = 52
    if let strip = base.cropping(to: CGRect(x: 0, y: 0, width: size.width * scale, height: bar * scale)) {
      ctx.draw(strip, in: CGRect(x: 0, y: size.height - bar, width: size.width, height: bar))
    }
    return ctx.makeImage()
  }
  @MainActor private static func write(_ v: NSView, to url: URL) { if let i = image(v) { save(i, to: url) } }
  private static func save(_ i: CGImage, to url: URL) {
    try? NSBitmapImageRep(cgImage: i).representation(using: .png, properties: [:])?.write(to: url)
  }
}

/** A row of the menu bar's panel: lit under the pointer, as a menu's item. */
struct MenuRowStyle: ButtonStyle {
  func makeBody(configuration: Configuration) -> some View { Lit(configuration: configuration) }
  struct Lit: View {
    let configuration: ButtonStyleConfiguration
    @State private var over = false
    var body: some View {
      configuration.label
        .background(RoundedRectangle(cornerRadius: 6).fill(over || configuration.isPressed ? Ink.sunken : .clear))
        .onHover { over = $0 }
    }
  }
}
#endif
