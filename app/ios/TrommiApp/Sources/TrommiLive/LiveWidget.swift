// TrommiLive: the Live Activity's widget. The bell of the logo and the count of questions waiting; with the most
// urgent card, its title and its two answers, which answer in place (LiveAnswer: the intent runs in the app). The
// card comes from the app on this phone (LiveState); the hub's pushes carry two marks (anyone working, anything
// waiting) and the exact counts sealed under this phone's push key, which the widget opens with the core
// (NotifyCoreLive, the key from the Keychain item the app writes for the notification extension). What does not open
// leaves the marks: the widget says that something waits, without a number. The widget reads nothing else: no other
// Keychain item, no file, no content key. A tap opens the card (or
// the Desk). Past the push's stale date the widget dims and says so. The island is always black: its colours are
// the board's dark tones; the lock screen follows the system's light or dark.
import Foundation
import PushNotify
import NotifyCoreLive
#if canImport(ActivityKit) && canImport(WidgetKit) && os(iOS)
import ActivityKit
import AppIntents
import SwiftUI
import WidgetKit

@main
public struct TrommiLiveBundle: WidgetBundle {
  public init() {}
  public var body: some Widget { TrommiLiveActivity() }
}

private let deskURL = URL(string: "https://app.trommi.com/")!

/** The state to show: the hub's sealed counts opened under this phone's push key, when they open. */
func shown(_ s: LiveCounts) -> LiveCounts {
  #if os(iOS)
  let key = NotifyGroup.readContext().flatMap { NotifyOpenKey.bytes($0.pushKey) }
  return s.opened(key: key, core: NotifyCoreLive())
  #else
  return s
  #endif
}
func cardURL(_ c: LiveCard?) -> URL { c.flatMap { URL(string: "https://app.trommi.com/card/\($0.id)") } ?? deskURL }

struct TrommiLiveActivity: Widget {
  var body: some WidgetConfiguration {
    ActivityConfiguration(for: TrommiActivityAttributes.self) { ctx in
      LockLive(s: shown(ctx.state), stale: ctx.isStale)
    } dynamicIsland: { ctx in
      let s = shown(ctx.state), top = s.card, ring = Isle.ring(top)
      return DynamicIsland {
        DynamicIslandExpandedRegion(.bottom) {
          IslandBody(s: s, stale: ctx.isStale).padding(.horizontal, 4).padding(.bottom, 2)
        }
      } compactLeading: {
        Bell(size: 24, ink: .white, rings: ring).padding(.leading, 2).transaction { $0.animation = nil }
      } compactTrailing: {
        // (a fixed width and no transition: the island keeps its shape when the count or the card changes)
        Text(s.done != nil ? "✓" : s.badge).font(.system(size: 17, weight: .bold, design: .rounded)).monospacedDigit()
          .foregroundStyle(s.done != nil ? Isle.green : ring)
          .frame(minWidth: 22).contentTransition(.identity)
      } minimal: {
        MinimalLive(s: s)
      }
      .keylineTint(ring)
      .widgetURL(cardURL(top))
    }
  }
}

func verb(_ c: LiveCard) -> String { c.kind == "permission" ? "  asks to act" : c.knocks ? "  knocks" : "  asks" }

/** The island's colours (the board's dark tones). */
enum Isle {
  static let green = Color(red: 0x6f / 255, green: 0xd0 / 255, blue: 0xb5 / 255)
  static let knock = Color(red: 0xf2 / 255, green: 0xa5 / 255, blue: 0x6c / 255)
  static let critical = Color(red: 1, green: 0x8a / 255, blue: 0x80 / 255)
  static let dim = Color.white.opacity(0.6)
  /** The bell's rings: green, orange for a knock, red for a blocking question or a permission request. */
  static func ring(_ c: LiveCard?) -> Color { c.map { $0.critical ? critical : $0.knocks ? knock : green } ?? green }
}

/** The lock screen's tones, light and dark (the board's paper and ink). */
struct Paper {
  let dark: Bool
  var bg: Color { dark ? Color(red: 0x17 / 255, green: 0x1d / 255, blue: 0x1a / 255) : Color(red: 0xfa / 255, green: 0xfb / 255, blue: 0xf8 / 255) }
  var ink: Color { dark ? Color(red: 0xe9 / 255, green: 0xee / 255, blue: 0xea / 255) : Color(red: 0x14 / 255, green: 0x1c / 255, blue: 0x18 / 255) }
  var muted: Color { dark ? Color(red: 0x9a / 255, green: 0xa8 / 255, blue: 0xa0 / 255) : Color(red: 0x5c / 255, green: 0x68 / 255, blue: 0x62 / 255) }
  var green: Color { dark ? Isle.green : Color(red: 0x1b / 255, green: 0x6a / 255, blue: 0x57 / 255) }
  var knock: Color { dark ? Isle.knock : Color(red: 0xb4 / 255, green: 0x55 / 255, blue: 0x1b / 255) }
  var critical: Color { dark ? Isle.critical : Color(red: 0xb3 / 255, green: 0x26 / 255, blue: 0x1e / 255) }
  var soft: Color { dark ? Color.white.opacity(0.08) : Color.black.opacity(0.05) }
  func ring(_ c: LiveCard?) -> Color { c.map { $0.critical ? critical : $0.knocks ? knock : green } ?? green }
}

/** The expanded island under its top row: the title and the answers, the done line, or the count alone. */
struct IslandBody: View {
  let s: LiveCounts
  let stale: Bool
  var body: some View {
    let ring = Isle.ring(s.card)
    VStack(alignment: .leading, spacing: 6) {
      if let d = s.done {
        DoneLine(done: d, ink: .white, muted: Isle.dim, tick: Isle.green, more: s.counted ? s.waiting : 0).padding(.vertical, 6)
      } else if let top = s.card {
        // one row for the bell, who asks, and where in the queue (no leading or trailing region: the bottom starts
        // right under the island's top, so the title and both answers fit)
        HStack(spacing: 7) {
          Bell(size: 22, ink: .white, rings: ring)
          (Text(top.agent).font(.subheadline.weight(.semibold)).foregroundStyle(.white)
            + Text(verb(top)).font(.subheadline).foregroundStyle(ring))
            .lineLimit(1)
          Spacer(minLength: 4)
          if s.others > 0 {
            Text("1 of \(s.waiting)").font(.caption.weight(.medium)).monospacedDigit().foregroundStyle(Isle.dim)
              .padding(.horizontal, 8).padding(.vertical, 3)
              .background(Color.white.opacity(0.12), in: Capsule())
          }
        }
        Text(top.title).font(.headline).foregroundStyle(.white).lineLimit(2)
          .fixedSize(horizontal: false, vertical: true)
          .frame(maxWidth: .infinity, alignment: .leading)
        Answers(card: top, needsApp: s.needsApp == true, primary: ring, primaryFg: .black, secondaryBg: Color.white.opacity(0.14), secondaryFg: .white, height: 34)
      } else {
        HStack(spacing: 10) {
          Bell(size: 28, ink: .white, rings: ring)
          VStack(alignment: .leading, spacing: 2) {
            Text(stale ? "Not up to date" : s.waitingText).font(.headline).foregroundStyle(.white)
            Text(s.workingText).font(.subheadline).foregroundStyle(Isle.dim)
          }
        }
        .padding(.vertical, 4)
      }
    }
    .opacity(stale ? 0.6 : 1)
  }
}


/** "✓ Answered · Ship it", the title under it, and how many still wait. */
struct DoneLine: View {
  let done: LiveDone
  let ink: Color
  let muted: Color
  let tick: Color
  let more: Int
  var body: some View {
    VStack(alignment: .leading, spacing: 3) {
      HStack(spacing: 6) {
        Image(systemName: "checkmark.circle.fill").foregroundStyle(tick)
        Text("Answered · \(done.label)").font(.headline).foregroundStyle(ink).lineLimit(1)
      }
      Text(done.title).font(.subheadline).foregroundStyle(muted).lineLimit(1)
      if more > 0 { Text(more == 1 ? "1 more waiting" : "\(more) more waiting").font(.caption).foregroundStyle(muted) }
    }
    .frame(maxWidth: .infinity, alignment: .leading)
  }
}

/**
 * The two answers of a two-option card, as buttons that answer in place (AnswerCardIntent, in the app's process).
 * A card without two answers, or one the app could not answer from here, opens instead.
 */
struct Answers: View {
  let card: LiveCard
  let needsApp: Bool
  let primary: Color
  let primaryFg: Color
  let secondaryBg: Color
  let secondaryFg: Color
  var height: CGFloat = 36
  var body: some View {
    if card.options.count == 2 && !needsApp {
      HStack(spacing: 8) {
        Button(intent: AnswerCardIntent(card: card.id, key: card.options[0].key)) {
          Text(card.options[0].label).font(.body.weight(.semibold)).lineLimit(1).frame(maxWidth: .infinity, minHeight: height)
            .foregroundStyle(primaryFg).background(primary, in: Capsule())
        }
        .buttonStyle(.plain)
        Button(intent: AnswerCardIntent(card: card.id, key: card.options[1].key)) {
          Text(card.options[1].label).font(.body.weight(.semibold)).lineLimit(1).frame(maxWidth: .infinity, minHeight: height)
            .foregroundStyle(secondaryFg).background(secondaryBg, in: Capsule())
        }
        .buttonStyle(.plain)
      }
    } else {
      Link(destination: cardURL(card)) {
        Text(needsApp ? "Not sent · Open to answer" : "Open to answer").font(.body.weight(.semibold)).lineLimit(1)
          .frame(maxWidth: .infinity, minHeight: height)
          .foregroundStyle(primaryFg).background(primary, in: Capsule())
      }
    }
  }
}

/** Minimal (beside another app's activity): the bell, its badge the count, ringing in the card's colour. */
struct MinimalLive: View {
  let s: LiveCounts
  var body: some View {
    let ring = s.done != nil ? Isle.green : Isle.ring(s.card)
    ZStack(alignment: .topTrailing) {
      // the badge rings in place of the bell's two strokes
      BellBody().stroke(.white, style: StrokeStyle(lineWidth: 1.7, lineCap: .round, lineJoin: .round))
        .frame(width: 19, height: 15).frame(width: 24, height: 24).offset(x: -2, y: 2)
      if s.waiting > 0 || s.done != nil {
        Text(s.done != nil ? "✓" : s.badge).font(.system(size: 9, weight: .heavy, design: .rounded)).monospacedDigit().foregroundStyle(.black)
          .frame(minWidth: 12, minHeight: 12).background(ring, in: Circle()).offset(x: 2, y: -1)
      }
    }
  }
}

/** The lock screen (and the banner where there is no island): the same card on the board's paper. */
struct LockLive: View {
  let s: LiveCounts
  let stale: Bool
  @Environment(\.colorScheme) var scheme
  var body: some View {
    let p = Paper(dark: scheme == .dark)
    let top = s.card, ring = p.ring(top)
    VStack(alignment: .leading, spacing: 10) {
      HStack(spacing: 8) {
        Bell(size: 26, ink: p.ink, rings: ring)
        if s.done == nil, let top {
          Text(top.agent).font(.subheadline.weight(.semibold)).foregroundStyle(p.ink).lineLimit(1)
          Text("\(verb(top).trimmingCharacters(in: .whitespaces)) · \(age(top.since))").font(.subheadline).foregroundStyle(ring).lineLimit(1)
          Spacer(minLength: 0)
          if s.others > 0 { Text("+\(s.others) waiting").font(.caption.weight(.medium)).foregroundStyle(p.muted) }
        } else {
          Text(stale ? "Not up to date" : s.waitingText).font(.subheadline.weight(.semibold)).foregroundStyle(p.ink)
          Spacer(minLength: 0)
          Text("Trommi").font(.caption.weight(.semibold)).foregroundStyle(p.muted)
        }
      }
      if let d = s.done {
        DoneLine(done: d, ink: p.ink, muted: p.muted, tick: p.green, more: s.counted ? s.waiting : 0)
      } else if let top {
        Text(top.title).font(.headline).foregroundStyle(p.ink).lineLimit(2)
          .frame(maxWidth: .infinity, alignment: .leading)
        Answers(card: top, needsApp: s.needsApp == true, primary: ring, primaryFg: p.dark ? .black : .white, secondaryBg: p.soft, secondaryFg: p.ink)
      } else {
        Text(s.workingText).font(.subheadline).foregroundStyle(p.muted)
      }
    }
    .opacity(stale ? 0.6 : 1)
    .padding(16)
    .activityBackgroundTint(p.bg)
    .activitySystemActionForegroundColor(p.ink)
    .widgetURL(cardURL(top))
  }
}

/** "2m", "21m", "1h": how long the card has waited (as of the last update). */
func age(_ d: Date, now: Date = Date()) -> String {
  let m = max(0, Int(now.timeIntervalSince(d) / 60))
  return m < 1 ? "now" : m < 60 ? "\(m)m" : m < 1440 ? "\(m / 60)h" : "\(m / 1440)d"
}

/** The counter bell of the logo (icons/trommi.svg) in its pen strokes: the body, and the two rings beside it. */
struct BellBody: Shape {
  func path(in r: CGRect) -> Path {
    var p = Path()
    p.move(to: pt(3.1, 19)); p.addQuadCurve(to: pt(16.7, 19), control: pt(12.3, 18.6)); p.addLine(to: pt(21, 19.3))
    p.move(to: pt(4.8, 18.9))
    for (c, e) in [((5.2, 15.1), (5.7, 13.7)), ((6.2, 12.3), (7.5, 11.3)), ((8.7, 10.3), (10.3, 9.5)), ((12, 8.8), (13.7, 9.3)),
                   ((15.3, 9.8), (16.4, 11)), ((17.5, 12.2), (18.1, 13.7)), ((18.6, 15.2), (18.8, 17))] {
      p.addQuadCurve(to: pt(e.0, e.1), control: pt(c.0, c.1))
    }
    p.addLine(to: pt(18.9, 18.8))
    p.move(to: pt(11.8, 8.6)); p.addLine(to: pt(12.2, 6.9))
    p.move(to: pt(10, 6.6)); p.addQuadCurve(to: pt(12.8, 6.4), control: pt(11.8, 6)); p.addLine(to: pt(13.8, 6.8))
    return p.applying(fit(r))
  }
}

struct BellRings: Shape {
  func path(in r: CGRect) -> Path {
    var p = Path()
    p.move(to: pt(18.5, 7.3)); p.addQuadCurve(to: pt(19.8, 5.2), control: pt(19.5, 5.8)); p.addLine(to: pt(20, 4.5))
    p.move(to: pt(20.6, 10.3)); p.addQuadCurve(to: pt(22.3, 8.9), control: pt(21.5, 9.4)); p.addLine(to: pt(23.1, 8.5))
    return p.applying(fit(r))
  }
}

private func pt(_ x: Double, _ y: Double) -> CGPoint { CGPoint(x: x, y: y) }
/** The bell's strokes span x 2…24.2, y 3.4…20.4 of the icon's 24-unit box (with the pen's width): fitted, centred. */
private func fit(_ r: CGRect) -> CGAffineTransform {
  let w = 22.2, h = 17.0, s = min(r.width / w, r.height / h)
  let dx = r.midX - (2 + w / 2) * s, dy = r.midY - (3.4 + h / 2) * s
  return CGAffineTransform(translationX: dx, y: dy).scaledBy(x: s, y: s)
}

/** The bell, its rings in their own colour. */
struct Bell: View {
  var size: CGFloat
  var ink: Color
  var rings: Color
  var body: some View {
    let style = StrokeStyle(lineWidth: size * 0.095, lineCap: .round, lineJoin: .round)
    ZStack {
      BellBody().stroke(ink, style: style)
      BellRings().stroke(rings, style: style)
    }
    .frame(width: size, height: size * 0.8)
  }
}
#endif
