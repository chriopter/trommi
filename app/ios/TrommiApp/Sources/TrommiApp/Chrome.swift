// Chrome.swift: the parts every screen draws with, as the app looked before the shared app core: the floating glass
// pills at the top (place pill, back circle), the drawn paper button, the answer tiles, the big greeting with the pen's
// line under its last word, the hand-drawn rules and the buttons and fields of the screens before the board. Drawing
// only: what a part says and whether it shows come from the core's view model.
import SwiftUI
import TrommiClient
#if canImport(UIKit)
import UIKit
#else
import AppKit
#endif

// ---- the colour of a session's drawing ---------------------------------------------------------------------------

extension AgentMark {
  /** A session's drawing in its own colour (Pen.hueFor), as the web and the Linux app draw it. */
  init(from: CoreView.FromView, size: CGFloat = 26) {
    self.init(mark: from.mark, size: size, hue: Pen.hueFor(id: from.sessionId, mark: from.mark))
  }
  init(row: CoreView.SessionRowView, size: CGFloat = 26, crown: Bool = true) {
    self.init(mark: row.mark, size: size, crowned: crown && row.crowned, hue: Pen.hueFor(id: row.slug.isEmpty ? row.id : row.slug, mark: row.mark))
  }
}

// ---- the top of a page -----------------------------------------------------------------------------------------

extension View {
  /** The pills at the top of the iPhone's root pages: the place pill at the left, the page's own round buttons at the
   *  right, each a clear glass capsule floating on the content. No navigation bar, no band, no hairline. */
  func topPills<L: View, T: View>(@ViewBuilder leading: () -> L, @ViewBuilder trailing: () -> T) -> some View {
    self.toolbar(.hidden, for: .navigationBar)
      .safeAreaBar(edge: .top, spacing: 0) {
        HStack(spacing: 10) { leading(); Spacer(minLength: 8); trailing() }.padding(.horizontal, 16).padding(.top, 4).padding(.bottom, 6)
      }
      .scrollEdgeEffectStyle(.soft, for: [.top, .bottom])
  }
  /** The same row on a pushed screen: the back circle at the left, the screen's own pill in the middle, its own
   *  buttons at the right. The swipe from the left edge still goes back (PopGesture). */
  func pushedPills<C: View, T: View>(extraTrailing: CGFloat = 0, @ViewBuilder center: () -> C, @ViewBuilder trailing: () -> T) -> some View {
    self.toolbar(.hidden, for: .navigationBar)
      .safeAreaBar(edge: .top, spacing: 0) {
        HStack(spacing: 10) { BackPill(); Spacer(minLength: 0); trailing() }
          .overlay { center().padding(.leading, 54).padding(.trailing, 54 + extraTrailing) }
          .padding(.horizontal, 16).padding(.top, 4).padding(.bottom, 6)
      }
      .scrollEdgeEffectStyle(.soft, for: [.top, .bottom])
      .background { PopGesture().frame(width: 0, height: 0).accessibilityHidden(true) }
      #if os(macOS)
      .toolbar(removing: .title)
      #endif
  }
  /** A round glass button of the top row (44 pt, as the place pill is tall). */
  func pillCircle() -> some View {
    self.frame(width: 44, height: 44).glass(Circle(), interactive: true).contentShape(Circle())
  }
}

/** Back, as a round glass button at the left of a pushed screen's row. */
struct BackPill: View {
  @EnvironmentObject var shell: AppShell
  var body: some View {
    Button { if !shell.path.isEmpty { shell.path.removeLast() } } label: {
      Image(systemName: "chevron.left").font(.system(size: 18, weight: .semibold)).foregroundStyle(Ink.fg).pillCircle()
    }
    .buttonStyle(.plain)
    .accessibilityLabel("Back")
    .accessibilityIdentifier("back")
  }
}

/** A pushed screen's own name in the middle of its row: a small glass pill. */
struct CenterPill<Content: View>: View {
  @ViewBuilder var content: () -> Content
  var body: some View {
    HStack(spacing: 8) { content() }.padding(.horizontal, 14).frame(height: 44).glass(Capsule())
  }
}

#if os(macOS)
struct PopGesture: View { var body: some View { EmptyView() } }
#else
/** The swipe from the left edge goes back although the navigation bar is hidden. */
struct PopGesture: UIViewControllerRepresentable {
  func makeUIViewController(context: Context) -> Holder { Holder() }
  func updateUIViewController(_ c: Holder, context: Context) {}
  final class Holder: UIViewController {
    override func viewDidAppear(_ animated: Bool) {
      super.viewDidAppear(animated)
      guard let g = navigationController?.interactivePopGestureRecognizer else { return }
      g.delegate = PopGate.shared
      g.isEnabled = true
    }
  }
}
final class PopGate: NSObject, UIGestureRecognizerDelegate {
  static let shared = PopGate()
  func gestureRecognizerShouldBegin(_ g: UIGestureRecognizer) -> Bool {
    var r: UIResponder? = g.view
    while let x = r { if let nav = x as? UINavigationController { return nav.viewControllers.count > 1 }; r = x.next }
    return false
  }
}
#endif

// ---- the greeting, the paper button, the rules --------------------------------------------------------------------

/** One big line in the display face, its last word underlined with the pen. */
struct Greeting: View {
  let text: String
  var size: CGFloat = 36
  var body: some View {
    let words = text.split(separator: " ").map(String.init)
    let head = words.dropLast().joined(separator: " ")
    Text("\(Text(head.isEmpty ? "" : head + " "))\(Text(words.last ?? "").underline(false))")
      .font(Face.display(size, .heavy)).foregroundStyle(Ink.fg)
      .background(alignment: .bottomTrailing) {
        PenUnderline().stroke(Ink.accent, style: StrokeStyle(lineWidth: 2.2, lineCap: .round))
          .frame(width: CGFloat((words.last ?? "").count) * size * 0.47, height: 8).offset(y: 6)
      }
      .fixedSize(horizontal: false, vertical: true)
      .accessibilityAddTraits(.isHeader)
  }
}

/** A drawn paper button: the pen's box with a shadow line under it. */
struct PaperButton: ButtonStyle {
  func makeBody(configuration: Configuration) -> some View {
    configuration.label
      .background(PenBox(r: 14).fill(Ink.surface))
      .overlay(PenBox(r: 14).stroke(Ink.fg, lineWidth: 2))
      .background(PenBox(r: 14).fill(Ink.lineStrong).offset(y: 4))
      .scaleEffect(configuration.isPressed ? 0.97 : 1)
      .animation(.easeOut(duration: 0.12), value: configuration.isPressed)
  }
}

struct DashedRule: View {
  var color: Color = Ink.lineStrong
  var body: some View { GeometryReader { g in Path { p in p.move(to: CGPoint(x: 0, y: 0.5)); p.addLine(to: CGPoint(x: g.size.width, y: 0.5)) }.stroke(color, style: StrokeStyle(lineWidth: 1, dash: [4, 4])) } }
}

/** A hand-drawn rule with a small heading in its middle; the heading may lead to a page. */
struct EndDivider: View {
  let title: String
  var chevron: String? = nil
  var more: (() -> Void)? = nil
  var body: some View {
    HStack(spacing: 10) {
      PenMark(doc: SVGReader.parse("<svg viewBox=\"0 0 300 8\" preserveAspectRatio=\"none\"><path d=\"M2 4.6 Q60 2.6 120 4.2 T238 3.6 T298 4.4\"/></svg>"), inks: PenInks(stroke: Ink.lineStrong, width: 1.4))
        .frame(height: 8).frame(maxWidth: .infinity)
      if let more {
        Button(action: more) {
          HStack(spacing: 5) {
            Text(title.uppercased()).font(Face.text(12, .semibold)).kerning(1.2).foregroundStyle(Ink.fg).underline(color: Ink.accent).fixedSize()
            Sketch("go", color: Ink.accent).frame(width: 15, height: 15)
          }.frame(minHeight: 44).contentShape(Rectangle())
        }.buttonStyle(.plain).accessibilityLabel("All of \(title)")
      } else {
        HStack(spacing: 6) {
          Text(title.uppercased()).font(Face.text(12, .semibold)).kerning(1.2).foregroundStyle(Ink.muted).fixedSize()
          if let c = chevron { Image(systemName: c).font(.system(size: 10, weight: .semibold)).foregroundStyle(Ink.faint).accessibilityHidden(true) }
        }
      }
      PenMark(doc: SVGReader.parse("<svg viewBox=\"0 0 300 8\" preserveAspectRatio=\"none\"><path d=\"M2 4.2 Q70 5.4 140 3.8 T298 4.6\"/></svg>"), inks: PenInks(stroke: Ink.lineStrong, width: 1.4))
        .frame(height: 8).frame(maxWidth: .infinity)
    }.padding(.vertical, more == nil ? 4 : 0)
  }
}

/** The tally of a session's open questions: up to four strokes, the fifth across. */
struct Tally: Shape {
  let n: Int
  func path(in r: CGRect) -> Path {
    let ds = ["M3.9 2.2Q2.6 7.6 2.5 13.9", "M8.6 1.6Q8.4 8.4 7.3 13.2", "M13.7 2.6Q12.5 7.2 12.6 14.2", "M18.6 1.9Q18.3 8.6 17.2 13.5"]
    var p = Path()
    for d in ds.prefix(min(n, 4)) { p.addPath(SVGReader.path(d)) }
    if n > 4 { p.addPath(SVGReader.path("M.9 11.8Q10.5 7.6 21.1 3.5")) }
    let w: CGFloat = n > 4 ? 22 : CGFloat(n * 5 + 1)
    return p.applying(CGAffineTransform(scaleX: r.width / w, y: r.height / 16).translatedBy(x: r.minX, y: r.minY))
  }
}

/** What waits on a session: the raised hand when it is stopped, else a tally of its open questions. */
struct SessionBadge: View {
  let open: UInt32
  let stopped: Bool
  var says: String? = nil
  var body: some View {
    if stopped {
      PenMark("hand", color: Ink.surface, blocked: true).frame(width: 26, height: 26).accessibilityLabel(says ?? "Stopped")
    } else if open > 0 {
      let n = Int(open)
      if n > 5 { Text("\(n)").font(Face.text(17, .medium)).foregroundStyle(Ink.muted) }
      else { Tally(n: n).stroke(Ink.muted, style: StrokeStyle(lineWidth: 1.8, lineCap: .round)).frame(width: CGFloat(n > 4 ? 22 : n * 5 + 1), height: 16).accessibilityLabel(says ?? "\(n) open") }
    }
  }
}

// ---- the answer tiles --------------------------------------------------------------------------------------------

/** A tile as the web's desk rows draw it: the lead in the bell's green (a knock in its colour), the others a light
 *  green wash; in dark the quiet ones also get the card line. */
struct TileStyle: ButtonStyle {
  let lead: Bool
  var urgency = "normal"
  func makeBody(configuration: Configuration) -> some View { TileBody(configuration: configuration, lead: lead, urgency: urgency) }
}
private struct TileBody: View {
  let configuration: ButtonStyleConfiguration
  let lead: Bool
  let urgency: String
  @Environment(\.colorScheme) private var scheme
  var body: some View {
    let knock = urgency == "high" || urgency == "critical"
    let fill = lead ? (knock ? Ink.urgency(urgency) : Ink.accent) : Ink.accent.opacity(0.09)
    configuration.label
      .foregroundStyle(lead ? (knock ? Ink.surface : Ink.accentFg) : Ink.fg)
      .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(fill))
      .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Ink.surface))
      .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(!lead && scheme == .dark ? Ink.lineStrong : .clear, lineWidth: 1))
      .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(lead ? Color.black.opacity(0.14) : Ink.accent.opacity(0.12)).opacity(configuration.isPressed ? 1 : 0).allowsHitTesting(false))
      .scaleEffect(configuration.isPressed ? 0.97 : 1)
      .animation(.easeOut(duration: 0.1), value: configuration.isPressed)
  }
}

/** The calm card every list row and box stands on: white, a hair line, a soft shadow. */
extension View {
  func cardBox(radius: CGFloat = 16, padding: CGFloat = 14) -> some View {
    self.padding(padding)
      .background(RoundedRectangle(cornerRadius: radius, style: .continuous).fill(Ink.surface))
      .overlay(RoundedRectangle(cornerRadius: radius, style: .continuous).strokeBorder(Ink.lineStrong, lineWidth: 1))
      .shadow(color: .black.opacity(0.05), radius: 3, y: 1)
  }
}

// ---- the screens before the board --------------------------------------------------------------------------------

/** The one primary button of a screen; while it works it says so and cannot be pressed again. */
struct ObGo: ButtonStyle {
  @Environment(\.isEnabled) private var enabled
  func makeBody(configuration: Configuration) -> some View {
    configuration.label.font(Face.text(18, .semibold)).foregroundStyle(Ink.accentFg)
      .frame(maxWidth: .infinity, minHeight: 54)
      .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(Ink.accent))
      .contentShape(Rectangle())
      .opacity(!enabled ? 0.45 : configuration.isPressed ? 0.8 : 1)
  }
}
/** The second way of a screen: an outline. */
struct ObSecond: ButtonStyle {
  @Environment(\.isEnabled) private var enabled
  func makeBody(configuration: Configuration) -> some View {
    configuration.label.font(Face.text(18, .semibold)).foregroundStyle(Ink.fg)
      .frame(maxWidth: .infinity, minHeight: 54)
      .background(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(Ink.fg, lineWidth: 1.5))
      .contentShape(Rectangle())
      .opacity(!enabled ? 0.45 : configuration.isPressed ? 0.6 : 1)
  }
}
/** A quiet way in a line of text. */
struct ObLink: ButtonStyle {
  func makeBody(configuration: Configuration) -> some View {
    configuration.label.font(Face.text(16, .semibold)).foregroundStyle(Ink.accent).frame(minHeight: 44).contentShape(Rectangle())
      .opacity(configuration.isPressed ? 0.5 : 1)
  }
}
/** A small outlined word beside a thing (Show, Copy). */
struct ObChip: ButtonStyle {
  func makeBody(configuration: Configuration) -> some View {
    configuration.label.font(Face.text(15, .semibold)).foregroundStyle(Ink.fg)
      .padding(.horizontal, 14).frame(minHeight: 36)
      .background(Capsule().fill(Ink.surface)).overlay(Capsule().strokeBorder(Ink.fg, lineWidth: 1.5))
      .opacity(configuration.isPressed ? 0.6 : 1)
  }
}
extension View {
  /** A field of these screens: one outlined line. */
  func obInput() -> some View {
    self.font(Face.text(17)).padding(.horizontal, 14).frame(minHeight: 50)
      .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Ink.surface))
      .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(Ink.lineStrong, lineWidth: 1.5))
  }
}
func obLabel(_ s: String) -> some View { Text(s).font(Face.text(14, .semibold)).foregroundStyle(Ink.fg) }

/** The one line a form says when it failed: its place is kept, so nothing jumps. */
struct ObError: View {
  let text: String?
  var body: some View {
    Text(text ?? " ").font(Face.text(15)).foregroundStyle(Ink.urgCritical).frame(maxWidth: .infinity, minHeight: 22, alignment: .leading)
      .accessibilityHidden(text == nil)
      .accessibilityIdentifier("account-problem")
  }
}

/** The line between two ways of one screen. */
struct ObOr: View {
  var body: some View {
    HStack(spacing: 12) {
      Rectangle().fill(Ink.line).frame(height: 1)
      Text("or").font(Face.text(14)).foregroundStyle(Ink.muted)
      Rectangle().fill(Ink.line).frame(height: 1)
    }
  }
}

/** A text onto the clipboard. */
func copyText(_ s: String) {
  #if canImport(UIKit)
  UIPasteboard.general.string = s
  #else
  NSPasteboard.general.clearContents(); NSPasteboard.general.setString(s, forType: .string)
  #endif
}
