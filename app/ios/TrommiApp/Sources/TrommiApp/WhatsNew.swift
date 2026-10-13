// WhatsNew.swift: What's new, a row of the menu (MenuPill): the days the web app serves at
// https://app.trommi.com/whats-new.json (app/web/whats-new.mjs: the GitHub releases' notes of every part, one summary
// line a day and its picture), newest first, the last answer kept on this phone for when it is offline. Each day is one
// card: the date, the summary line, the day's picture (a shot of the demo room, loaded from the web app:
// https://app.trommi.com/changelog/…), its counts and Details, which opens New, Better and Fixed with their first three
// lines by their short form, the rest under "+N more"; a line opens to its whole text. The same days and layout as the
// web app's page (app/web/public/news.mjs). Opening it marks the newest day as seen on this phone; until then the
// menu's row says so.
import Foundation
import Observation
import SwiftUI
import TrommiClient

/** One day of What's new: its date, the releases it covers, its summary line, its picture, its parts. */
struct ChangelogDay: Equatable, Codable {
  /** A line: its whole text, its tag ("Web", "iPhone" …, or empty) and its short form. */
  struct Line: Equatable, Codable { var text: String; var tag: String; var head: String }
  struct Part: Equatable, Codable { var kind: String; var items: [Line]; var lines: [Line] { items } }
  /** The day's one picture: what it shows, and where the web app serves it ("/changelog/<name>.webp"). */
  struct Picture: Equatable, Codable {
    var alt: String
    var src: String
    var url: URL? {
      guard src.hasPrefix("/changelog/"), src.hasSuffix(".webp"), !src.contains("..") else { return nil }
      return URL(string: src, relativeTo: Changelog.site)?.absoluteURL
    }
  }
  var day: String
  var releases: String
  var summary: String
  var picture: Picture?
  var groups: [Part]
  var parts: [Part] { groups }
  /** The parts in the order New, Better, Fixed (others after them). */
  var ordered: [Part] { parts.sorted { Self.rank($0.kind) < Self.rank($1.kind) } }
  /** "19 new · 41 better · 35 fixed" */
  var counts: String { ordered.map { "\($0.items.count) \(($0.kind.isEmpty ? "changes" : $0.kind).lowercased())" }.joined(separator: " · ") }
  private static func rank(_ kind: String) -> Int { ["New", "Better", "Fixed"].firstIndex(of: kind) ?? 3 }
}

enum Changelog {
  static let seenKey = "trommi-news-seen"
  /** The lines of a part shown before "+N more". */
  static let top = 3
  /** The web app: it serves the days (whats-new.json) and their pictures (changelog/). */
  static let site = URL(string: "https://app.trommi.com/")!
  /** What runs on this phone, for the top of What's New: "1.0.0 (37) · updated today, 14:32" (the app's version, its build number,
   *  when this build came onto the phone: the time of its bundle). */
  static var running: String {
    let info = Bundle.main.infoDictionary ?? [:]
    let version = info["CFBundleShortVersionString"] as? String ?? HubClient.appVersion, build = info["CFBundleVersion"] as? String
    var line = build.map { "\(version) (\($0))" } ?? version
    if let at = (try? Bundle.main.bundleURL.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate {
      let f = DateFormatter(); f.locale = Locale(identifier: "en_GB")
      f.dateFormat = Calendar.current.isDateInToday(at) ? "'today', HH:mm" : "d MMM, HH:mm"
      line += " · updated \(f.string(from: at))"
    }
    return line
  }
}

/** The days of What's new on this phone: the last answer kept (Caches/whats-new.json) at once, then the web app's,
 *  asked when the store is first used and each time the page opens. */
@MainActor @Observable final class WhatsNew {
  static let shared = WhatsNew()
  private(set) var days: [ChangelogDay]
  private var seenDay: String?
  private var asking = false
  private struct Answer: Codable { var days: [ChangelogDay] }
  private static var kept: URL? { FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask).first?.appendingPathComponent("whats-new.json") }

  private init() {
    days = Self.kept.flatMap { try? Data(contentsOf: $0) }.flatMap { try? JSONDecoder().decode(Answer.self, from: $0) }?.days ?? []
    seenDay = UserDefaults.standard.string(forKey: Changelog.seenKey)
    Task { await refresh() }
  }
  /** Whether the newest day was opened on this phone (true while there are no days). */
  var seen: Bool { days.first.map { seenDay == $0.day } ?? true }
  func markSeen() {
    guard let d = days.first else { return }
    UserDefaults.standard.set(d.day, forKey: Changelog.seenKey)
    seenDay = d.day
  }
  /** Asks the web app for the days; keeps them when they came. */
  func refresh() async {
    guard !asking else { return }
    asking = true
    defer { asking = false }
    var req = URLRequest(url: Changelog.site.appendingPathComponent("whats-new.json"), timeoutInterval: 15)
    req.setValue("application/json", forHTTPHeaderField: "Accept")
    guard let (data, response) = try? await URLSession.shared.data(for: req), (response as? HTTPURLResponse)?.statusCode == 200,
          data.count < 4 * 1024 * 1024, let answer = try? JSONDecoder().decode(Answer.self, from: data) else { return }
    days = answer.days
    if let file = Self.kept { try? data.write(to: file, options: .atomic) }
  }
}

struct WhatsNewPage: View {
  private let store = WhatsNew.shared
  var body: some View {
    SettingsPage(title: "What's New") {
      Text("What changed in Trommi, newest first.").font(Face.text(15)).foregroundStyle(Ink.muted)
      Text(Changelog.running).font(Face.text(12)).foregroundStyle(Ink.muted).accessibilityLabel("This app: \(Changelog.running)")
      if store.days.isEmpty { Text("Nothing written down yet.").font(Face.text(15)).foregroundStyle(Ink.muted) }
      ForEach(Array(store.days.enumerated()), id: \.offset) { _, d in WhatsNewDay(day: d) }
    }
    .onAppear { store.markSeen() }
    .onChange(of: store.days.first?.day) { store.markSeen() }
    .task { await store.refresh() }
  }
}

/** One day, one card: the date, the summary (else its first line's short form), the picture, the counts, and its parts
 *  behind Details. */
struct WhatsNewDay: View {
  let day: ChangelogDay
  @State private var open = false
  var body: some View {
    VStack(alignment: .leading, spacing: 10) {
      HStack(alignment: .firstTextBaseline, spacing: 8) {
        Text(day.day).font(Face.display(17, .bold))
        if !day.releases.isEmpty { Text(day.releases).font(Face.text(13)).foregroundStyle(Ink.muted) }
      }
      let summary = day.summary.isEmpty ? (day.ordered.first?.lines.first?.head ?? "") : day.summary
      if !summary.isEmpty {
        Text((try? AttributedString(markdown: summary)) ?? AttributedString(summary)).font(Face.text(16, .semibold))
          .fixedSize(horizontal: false, vertical: true)
      }
      if let pic = day.picture, let url = pic.url {
        AsyncImage(url: url) { phase in
          if let image = phase.image { image.resizable().aspectRatio(contentMode: .fit) } else { Rectangle().fill(Ink.sunken).aspectRatio(2.2, contentMode: .fit) }
        }
        .frame(maxWidth: 360, alignment: .leading)
        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 8, style: .continuous).strokeBorder(Ink.line))
        .accessibilityLabel(pic.alt)
      }
      DayDashedRule()
      Button { withAnimation(.snappy) { open.toggle() } } label: {
        HStack(alignment: .center, spacing: 12) {
          HStack(spacing: 12) {
            ForEach(Array(day.ordered.enumerated()), id: \.offset) { _, p in
              HStack(spacing: 4) {
                if !p.kind.isEmpty { Sketch(p.kind == "New" ? "spark" : p.kind == "Better" ? "wake" : "tick", color: p.kind == "New" ? Ink.accent : Ink.muted).frame(width: 14, height: 14) }
                Text("\(p.items.count)").font(Face.text(14, .bold)).foregroundStyle(Ink.fg)
                Text((p.kind.isEmpty ? "changes" : p.kind).lowercased()).font(Face.text(14)).foregroundStyle(Ink.muted)
              }
            }
          }.lineLimit(1).minimumScaleFactor(0.8)
          Spacer(minLength: 4)
          HStack(spacing: 4) {
            Text("Details").font(Face.text(14, .bold))
            Image(systemName: "chevron.down").font(.system(size: 12, weight: .bold)).rotationEffect(.degrees(open ? 180 : 0))
          }.foregroundStyle(Ink.accent)
        }
        .frame(minHeight: 32).contentShape(Rectangle())
      }
      .buttonStyle(.plain)
      .accessibilityLabel("\(day.counts). Details")
      .accessibilityHint(open ? "Hides the lines" : "Shows the lines")
      if open {
        SettingsGroup {
          ForEach(Array(day.ordered.enumerated()), id: \.offset) { i, p in
            if i > 0 { RowRule(inset: 0) }
            WhatsNewPart(part: p)
          }
        }
      }
    }
    .padding(16)
    .frame(maxWidth: .infinity, alignment: .leading)
    .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(Ink.surface))
    .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(Ink.lineStrong))
  }
}

/** The dashed line between a card's words and its counts. */
private struct DayDashedRule: View {
  private struct Across: Shape {
    func path(in r: CGRect) -> Path { Path { p in p.move(to: CGPoint(x: r.minX, y: r.midY)); p.addLine(to: CGPoint(x: r.maxX, y: r.midY)) } }
  }
  var body: some View { Across().stroke(Ink.lineStrong, style: StrokeStyle(lineWidth: 1, dash: [4, 4])).frame(height: 1) }
}

/** One part: New, Better or Fixed with its count, its first lines numbered, the rest under "+N more". */
struct WhatsNewPart: View {
  let part: ChangelogDay.Part
  @State private var all = false
  var body: some View {
    let lines = part.lines
    VStack(alignment: .leading, spacing: 6) {
      if !part.kind.isEmpty {
        HStack(spacing: 8) {
          Sketch(part.kind == "New" ? "spark" : part.kind == "Better" ? "wake" : "tick", color: part.kind == "New" ? Ink.accent : Ink.fg).frame(width: 16, height: 16)
          Text(part.kind.uppercased()).font(Face.text(12, .bold)).kerning(0.8).foregroundStyle(Ink.muted)
          Text("\(lines.count)").font(Face.text(12, .semibold)).foregroundStyle(Ink.muted)
        }
      }
      ForEach(Array(lines.prefix(all ? lines.count : Changelog.top).enumerated()), id: \.offset) { n, line in
        WhatsNewLine(number: n + 1, line: line)
      }
      if !all, lines.count > Changelog.top {
        Button("+\(lines.count - Changelog.top) more") { withAnimation(.snappy) { all = true } }
          .font(Face.text(15, .bold)).foregroundStyle(Ink.accent).padding(.leading, 26).padding(.top, 2)
      }
    }
    .padding(16).frame(maxWidth: .infinity, alignment: .leading)
  }
}

/** A line: its number, its tag and short form; a tap shows its whole text. */
struct WhatsNewLine: View {
  let number: Int
  let line: ChangelogDay.Line
  @State private var open = false
  var body: some View {
    Button { withAnimation(.snappy) { open.toggle() } } label: {
      HStack(alignment: .firstTextBaseline, spacing: 8) {
        Text("\(number).").font(Face.text(15, .bold)).foregroundStyle(Ink.muted).frame(minWidth: 18, alignment: .trailing)
        VStack(alignment: .leading, spacing: 4) {
          HStack(alignment: .firstTextBaseline, spacing: 6) {
            if !line.tag.isEmpty {
              Text(line.tag).font(Face.text(12, .semibold)).foregroundStyle(Ink.muted).padding(.horizontal, 6).padding(.vertical, 1)
                .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Ink.lineStrong))
            }
            Text(line.head).font(Face.text(15, .semibold)).foregroundStyle(Ink.fg).fixedSize(horizontal: false, vertical: true)
          }
          if open {
            Text((try? AttributedString(markdown: line.text)) ?? AttributedString(line.text)).font(Face.text(15)).foregroundStyle(Ink.muted)
              .fixedSize(horizontal: false, vertical: true)
          }
        }
      }
      .frame(maxWidth: .infinity, alignment: .leading).contentShape(Rectangle())
    }
    .buttonStyle(.plain)
    .accessibilityHint(open ? "" : "Shows the whole line")
  }
}
