// WhatsNew.swift: What's new, a row of the menu (MenuPill): the repository's CHANGELOG.md, bundled with the app
// (Resources/CHANGELOG.md, a link to the file at the root), one day under the other, newest first: its summary, its
// picture (a shot of the demo room, loaded from the web app: https://app.trommi.com/changelog/…), its parts New,
// Better, Fixed folded under "All changes" (open when the day has no summary). The same file and the same reading as the web app's page (app/web/dev/build.mjs parseChangelog,
// app/web/public/news.mjs). Opening it marks the newest day as seen on this phone; until then the menu's row says so.
import SwiftUI
import TrommiClient

/** One day of the changelog: its date, the releases it covers, its parts. */
struct ChangelogDay: Equatable {
  struct Part: Equatable { var kind: String; var items: [String] }
  var day: String
  var releases: String
  var summary = ""
  /** The day's one picture: what it shows, and where the web app serves it. */
  var picture: (alt: String, url: URL)?
  var parts: [Part]
  static func == (a: ChangelogDay, b: ChangelogDay) -> Bool {
    a.day == b.day && a.releases == b.releases && a.summary == b.summary && a.picture?.url == b.picture?.url && a.picture?.alt == b.picture?.alt && a.parts == b.parts
  }
}

enum Changelog {
  static let seenKey = "trommi-news-seen"
  /** The bundled file's days, newest first (empty when the file is not there). */
  static let days: [ChangelogDay] = {
    guard let url = Bundle.module.url(forResource: "CHANGELOG", withExtension: "md"), let text = try? String(contentsOf: url, encoding: .utf8) else { return [] }
    return parse(text)
  }()
  /** Whether the newest day was opened on this phone. */
  static var seen: Bool { days.first.map { UserDefaults.standard.string(forKey: seenKey) == $0.day } ?? true }
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
  static func markSeen() { if let d = days.first { UserDefaults.standard.set(d.day, forKey: seenKey) } }

  /** Where the web app serves the changelog's pictures (app/web/public/changelog/). */
  static let pictures = URL(string: "https://app.trommi.com/")!
  /** A day is a "## " heading ("11 October 2026 (v109 – v115)"); the plain lines before its first part are its summary,
   *  a line "![words](app/web/public/changelog/<name>.webp)" there its one picture; a part is a line "**New**", an item
   *  a line "- …" (a line indented under it goes on with it). The words before the first day are the file's own.
   *  The same reading as the web app's (app/web/dev/build.mjs parseChangelog). */
  static func parse(_ text: String) -> [ChangelogDay] {
    var days: [ChangelogDay] = []
    for raw in text.components(separatedBy: .newlines) {
      let line = raw.trimmingCharacters(in: CharacterSet(charactersIn: "\r"))
      if line.hasPrefix("## ") {
        var head = String(line.dropFirst(3)).trimmingCharacters(in: .whitespaces), releases = ""
        if head.hasSuffix(")"), let open = head.lastIndex(of: "(") {
          releases = String(head[head.index(after: open)..<head.index(before: head.endIndex)])
          head = head[..<open].trimmingCharacters(in: .whitespaces)
        }
        days.append(ChangelogDay(day: head, releases: releases, parts: []))
        continue
      }
      guard !days.isEmpty else { continue }
      let trimmed = line.trimmingCharacters(in: .whitespaces)
      if days[days.count - 1].parts.isEmpty, !trimmed.isEmpty {
        if let pic = picture(trimmed) {
          if days[days.count - 1].picture == nil { days[days.count - 1].picture = pic }
          continue
        }
        if !trimmed.hasPrefix("- "), !trimmed.hasPrefix("* "), !trimmed.hasPrefix("**") {
          let s = days[days.count - 1].summary
          days[days.count - 1].summary = s.isEmpty ? trimmed : s + " " + trimmed
          continue
        }
      }
      if trimmed.hasPrefix("**"), trimmed.hasSuffix("**"), trimmed.count > 4, !line.hasPrefix(" ") {
        days[days.count - 1].parts.append(.init(kind: String(trimmed.dropFirst(2).dropLast(2)).trimmingCharacters(in: .whitespaces), items: []))
      } else if line.hasPrefix("- ") || line.hasPrefix("* ") {
        if days[days.count - 1].parts.isEmpty { days[days.count - 1].parts.append(.init(kind: "", items: [])) }
        let p = days[days.count - 1].parts.count - 1
        days[days.count - 1].parts[p].items.append(String(line.dropFirst(2)).trimmingCharacters(in: .whitespaces))
      } else if line.first?.isWhitespace == true, !trimmed.isEmpty, let p = days[days.count - 1].parts.indices.last, !days[days.count - 1].parts[p].items.isEmpty {
        days[days.count - 1].parts[p].items[days[days.count - 1].parts[p].items.count - 1] += " " + trimmed
      }
    }
    return days
  }
  /** "![words](app/web/public/changelog/<name>.webp)" as its words and the picture's address on the web app. */
  static func picture(_ line: String) -> (alt: String, url: URL)? {
    guard line.hasPrefix("!["), line.hasSuffix(")"), let close = line.range(of: "](") else { return nil }
    let alt = String(line[line.index(line.startIndex, offsetBy: 2)..<close.lowerBound])
    var path = String(line[close.upperBound..<line.index(before: line.endIndex)])
    if path.hasPrefix("app/web/public/") { path.removeFirst("app/web/public/".count) }
    if path.hasPrefix("/") { path.removeFirst() }
    guard path.hasPrefix("changelog/"), path.hasSuffix(".webp"), !path.contains(".."), let url = URL(string: path, relativeTo: pictures)?.absoluteURL else { return nil }
    return (alt, url)
  }
}

struct WhatsNewPage: View {
  var body: some View {
    SettingsPage(title: "What's New") {
      Text("What changed in Trommi, newest first.").font(Face.text(15)).foregroundStyle(Ink.muted)
      Text(Changelog.running).font(Face.text(12)).foregroundStyle(Ink.muted).accessibilityLabel("This app: \(Changelog.running)")
      if Changelog.days.isEmpty { Text("Nothing written down yet.").font(Face.text(15)).foregroundStyle(Ink.muted) }
      ForEach(Array(Changelog.days.enumerated()), id: \.offset) { _, d in
        VStack(alignment: .leading, spacing: 8) {
          HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(d.day).font(Face.display(19, .bold))
            if !d.releases.isEmpty { Text(d.releases).font(Face.text(13)).foregroundStyle(Ink.muted) }
          }.padding(.horizontal, 4)
          WhatsNewDay(day: d)
        }
      }
    }
    .onAppear { Changelog.markSeen() }
  }
}

/** A day's card: its summary and picture, its parts folded under "All changes" (open when there is no summary). */
struct WhatsNewDay: View {
  let day: ChangelogDay
  @State private var open: Bool
  init(day: ChangelogDay) { self.day = day; _open = State(initialValue: day.summary.isEmpty) }
  /** "12 new · 29 better · 25 fixed" */
  private var counts: String { day.parts.map { "\($0.items.count) \(($0.kind.isEmpty ? "changes" : $0.kind).lowercased())" }.joined(separator: " · ") }
  var body: some View {
    SettingsGroup {
      if !day.summary.isEmpty {
        Text((try? AttributedString(markdown: day.summary)) ?? AttributedString(day.summary)).font(Face.text(17)).lineSpacing(3)
          .fixedSize(horizontal: false, vertical: true).frame(maxWidth: .infinity, alignment: .leading).padding(16)
      }
      if let pic = day.picture {
        AsyncImage(url: pic.url) { phase in
          if let image = phase.image { image.resizable().aspectRatio(contentMode: .fit) } else { Rectangle().fill(Ink.sunken).aspectRatio(2, contentMode: .fit) }
        }
        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(Ink.line))
        .padding(.horizontal, 16).padding(.bottom, 16)
        .accessibilityLabel(pic.alt)
      }
      if !day.summary.isEmpty || day.picture != nil { RowRule(inset: 0) }
      DisclosureGroup(isExpanded: $open) {
        VStack(alignment: .leading, spacing: 0) {
          ForEach(Array(day.parts.enumerated()), id: \.offset) { i, p in
            if i > 0 { RowRule(inset: 0) }
            VStack(alignment: .leading, spacing: 8) {
              if !p.kind.isEmpty {
                HStack(spacing: 8) {
                  Sketch(p.kind == "New" ? "spark" : p.kind == "Better" ? "wake" : "tick", color: p.kind == "New" ? Ink.accent : Ink.fg).frame(width: 16, height: 16)
                  Text(p.kind.uppercased()).font(Face.text(12, .bold)).kerning(0.8).foregroundStyle(Ink.muted)
                }
              }
              ForEach(Array(p.items.enumerated()), id: \.offset) { _, item in
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                  Text("•").foregroundStyle(Ink.muted)
                  Text((try? AttributedString(markdown: item)) ?? AttributedString(item)).font(Face.text(15)).fixedSize(horizontal: false, vertical: true)
                }
              }
            }
            .padding(.vertical, 12).frame(maxWidth: .infinity, alignment: .leading)
          }
        }
      } label: {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
          Text("All changes").font(Face.text(15, .bold)).foregroundStyle(Ink.fg)
          Text(counts).font(Face.text(12)).foregroundStyle(Ink.muted)
        }
      }
      .tint(Ink.muted)
      .padding(.horizontal, 16).padding(.vertical, 12)
    }
  }
}
