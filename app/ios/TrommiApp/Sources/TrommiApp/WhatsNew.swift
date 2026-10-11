// WhatsNew.swift: What's new, a row of the menu (MenuPill): the repository's CHANGELOG.md, bundled with the app
// (Resources/CHANGELOG.md, a link to the file at the root), one day under the other, newest first, its parts New,
// Better, Fixed. The same file and the same reading as the web app's page (app/web/dev/build.mjs parseChangelog,
// app/web/public/news.mjs). Opening it marks the newest day as seen on this phone; until then the menu's row says so.
import SwiftUI
import TrommiClient

/** One day of the changelog: its date, the releases it covers, its parts. */
struct ChangelogDay: Equatable {
  struct Part: Equatable { var kind: String; var items: [String] }
  var day: String
  var releases: String
  var parts: [Part]
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

  /** A day is a "## " heading ("11 October 2026 (v109 – v115)"), a part a line "**New**", an item a line "- …" (a line
   *  indented under it goes on with it). The words before the first day are the file's own. */
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
          SettingsGroup {
            ForEach(Array(d.parts.enumerated()), id: \.offset) { i, p in
              if i > 0 { RowRule(inset: 16) }
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
              .padding(16).frame(maxWidth: .infinity, alignment: .leading)
            }
          }
        }
      }
    }
    .onAppear { Changelog.markSeen() }
  }
}
