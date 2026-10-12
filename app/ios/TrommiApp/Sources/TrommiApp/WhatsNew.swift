// WhatsNew.swift: What's new, a row of the menu (MenuPill): the repository's CHANGELOG.md, bundled with the app
// (Resources/CHANGELOG.md, a link to the file at the root), newest first. The newest three days are open: the date,
// the summary line, the day's picture small under it (a shot of the demo room, loaded from the web app:
// https://app.trommi.com/changelog/…), then New, Better and Fixed with their first three lines by their short form (Changelog.head), the
// rest under "+N more"; a line opens to its whole text. Older days are one row with their counts, open on a tap.
// The same file, reading and layout as the web app's page (app/web/dev/build.mjs parseChangelog and changelogHead,
// app/web/public/news.mjs). Opening it marks the newest day as seen on this phone; until then the menu's row says so.
import SwiftUI
import TrommiClient

/** One day of the changelog: its date, the releases it covers, its summary line, its parts. */
struct ChangelogDay: Equatable {
  /** A line: its whole text, its tag ("Web", "iPhone" …, or empty) and its short form. */
  struct Line: Equatable { var text: String; var tag: String; var head: String }
  struct Part: Equatable { var kind: String; var items: [String]; var lines: [Line] { items.map(Changelog.head) } }
  var day: String
  var releases: String
  var summary = ""
  /** The day's one picture: what it shows, and where the web app serves it. */
  var picture: Picture?
  var parts: [Part]
  struct Picture: Equatable { var alt: String; var url: URL }
  /** The parts in the order New, Better, Fixed (others after them). */
  var ordered: [Part] { parts.sorted { Self.rank($0.kind) < Self.rank($1.kind) } }
  /** "19 new · 41 better · 35 fixed" */
  var counts: String { ordered.map { "\($0.items.count) \(($0.kind.isEmpty ? "changes" : $0.kind).lowercased())" }.joined(separator: " · ") }
  private static func rank(_ kind: String) -> Int { ["New", "Better", "Fixed"].firstIndex(of: kind) ?? 3 }
}

enum Changelog {
  static let seenKey = "trommi-news-seen"
  /** The newest days shown open, and the lines of a part shown before "+N more". */
  static let openDays = 3, top = 3
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

  /** A day is a "## " heading ("11 October 2026 (v109 – v115)"); the plain lines before its first part are its summary;
   *  a part is a line "**New**", an item a line "- …" (a line indented under it goes on with it). The words before the
   *  first day are the file's own. */
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
      if days[days.count - 1].parts.isEmpty, let pic = picture(trimmed) {
        if days[days.count - 1].picture == nil { days[days.count - 1].picture = pic }
        continue
      }
      if days[days.count - 1].parts.isEmpty, !trimmed.isEmpty, !trimmed.hasPrefix("- "), !trimmed.hasPrefix("* "), !trimmed.hasPrefix("**") {
        let s = days[days.count - 1].summary
        days[days.count - 1].summary = s.isEmpty ? trimmed : s + " " + trimmed
        continue
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

  /** Where the web app serves the changelog's pictures (app/web/public/changelog/). */
  static let pictures = URL(string: "https://app.trommi.com/")!
  /** "![words](app/web/public/changelog/<name>.webp)" as its words and the picture's address on the web app. */
  static func picture(_ line: String) -> ChangelogDay.Picture? {
    guard line.hasPrefix("!["), line.hasSuffix(")"), let close = line.range(of: "](") else { return nil }
    let alt = String(line[line.index(line.startIndex, offsetBy: 2)..<close.lowerBound])
    var path = String(line[close.upperBound..<line.index(before: line.endIndex)])
    if path.hasPrefix("app/web/public/") { path.removeFirst("app/web/public/".count) }
    if path.hasPrefix("/") { path.removeFirst() }
    guard path.hasPrefix("changelog/"), path.hasSuffix(".webp"), !path.contains(".."), let url = URL(string: path, relativeTo: pictures)?.absoluteURL else { return nil }
    return .init(alt: alt, url: url)
  }

  /** A line's short form, as the web app's changelogHead: a bold start ("**Readable addresses**: …") is the head; else
   *  a leading "Web:", "iPhone:" (iPad, Mac, Desk, Codex, Connector) becomes the tag, and the head is the line up to its
   *  first colon, semicolon, bracket, comma or full stop (when that leaves fewer than three words, up to the first colon,
   *  semicolon or full stop), at most 78 characters. */
  static func head(_ line: String) -> ChangelogDay.Line {
    var text = line.trimmingCharacters(in: .whitespaces), tag = "", head: String?
    let tagRe = "^(Web|iPhone|iPad|Mac|Desk|Codex|Connector): +"
    if let bold = match("^\\*\\*([^*]+)\\*\\*", text) {
      head = bold[1].trimmingCharacters(in: .whitespaces)
      text = capital(String(text.dropFirst(bold[0].count)).replacingOccurrences(of: "^:\\s*", with: "", options: .regularExpression))
      if let t = match(tagRe, head!) { tag = t[1]; head = String(head!.dropFirst(t[0].count)) }
    } else if let t = match(tagRe, text) {
      tag = t[1]; text = capital(String(text.dropFirst(t[0].count)))
    }
    if head == nil {
      let plain = text.replacingOccurrences(of: "\\[([^\\]]+)\\]\\([^)]+\\)", with: "$1", options: .regularExpression).replacingOccurrences(of: "`", with: "")
      var h = upTo(plain, "[:;]\\s|\\s\\(|,\\s|\\.\\s|\\.$")
      if h.components(separatedBy: " ").count < 3 { h = upTo(plain, "[:;]\\s|\\.\\s|\\.$") }
      if (h as NSString).length > 78 {
        h = ((h as NSString).substring(to: 75) as String).replacingOccurrences(of: "\\s+\\S*$", with: "", options: .regularExpression) + "…"
      }
      head = h
    }
    var shown = head ?? ""
    if shown.range(of: "^i[A-Z]", options: .regularExpression) == nil { shown = capital(shown) }
    return .init(text: text, tag: tag, head: shown)
  }
  private static func capital(_ s: String) -> String { s.prefix(1).uppercased() + s.dropFirst() }
  /** The text before the first match of the pattern (all of it when there is none). */
  private static func upTo(_ s: String, _ pattern: String) -> String {
    guard let r = s.range(of: pattern, options: .regularExpression) else { return s }
    return String(s[..<r.lowerBound])
  }
  /** The groups of the pattern's first match at the start of s: [whole, group 1, …]. */
  private static func match(_ pattern: String, _ s: String) -> [String]? {
    guard let re = try? NSRegularExpression(pattern: pattern), let m = re.firstMatch(in: s, range: NSRange(s.startIndex..., in: s)) else { return nil }
    return (0..<m.numberOfRanges).map { i in Range(m.range(at: i), in: s).map { String(s[$0]) } ?? "" }
  }
}

struct WhatsNewPage: View {
  var body: some View {
    SettingsPage(title: "What's New") {
      Text("What changed in Trommi, newest first.").font(Face.text(15)).foregroundStyle(Ink.muted)
      Text(Changelog.running).font(Face.text(12)).foregroundStyle(Ink.muted).accessibilityLabel("This app: \(Changelog.running)")
      if Changelog.days.isEmpty { Text("Nothing written down yet.").font(Face.text(15)).foregroundStyle(Ink.muted) }
      ForEach(Array(Changelog.days.enumerated()), id: \.offset) { i, d in
        if i < Changelog.openDays {
          VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
              Text(d.day).font(Face.display(19, .bold))
              if !d.releases.isEmpty { Text(d.releases).font(Face.text(13)).foregroundStyle(Ink.muted) }
            }.padding(.horizontal, 4)
            WhatsNewDay(day: d, picture: true)
          }
        } else {
          WhatsNewOlderDay(day: d)
        }
      }
    }
    .onAppear { Changelog.markSeen() }
  }
}

/** A day's summary line, its picture (on the open days only), and its parts, each with its first lines and "+N more". */
struct WhatsNewDay: View {
  let day: ChangelogDay
  var picture = false
  var body: some View {
    VStack(alignment: .leading, spacing: 8) {
      if !day.summary.isEmpty {
        Text((try? AttributedString(markdown: day.summary)) ?? AttributedString(day.summary)).font(Face.text(17, .semibold))
          .fixedSize(horizontal: false, vertical: true).padding(.horizontal, 4)
      }
      if picture, let pic = day.picture {
        AsyncImage(url: pic.url) { phase in
          if let image = phase.image { image.resizable().aspectRatio(contentMode: .fit) } else { Rectangle().fill(Ink.sunken).aspectRatio(2.2, contentMode: .fit) }
        }
        .frame(maxWidth: 280, alignment: .leading)
        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(Ink.line))
        .padding(.horizontal, 4)
        .accessibilityLabel(pic.alt)
      }
      SettingsGroup {
        ForEach(Array(day.ordered.enumerated()), id: \.offset) { i, p in
          if i > 0 { RowRule(inset: 0) }
          WhatsNewPart(part: p)
        }
      }
    }
  }
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

/** An older day: one row with its date and counts; a tap opens it. */
struct WhatsNewOlderDay: View {
  let day: ChangelogDay
  @State private var open = false
  var body: some View {
    VStack(alignment: .leading, spacing: 8) {
      Button { withAnimation(.snappy) { open.toggle() } } label: {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
          Text(day.day).font(Face.display(17, .bold)).foregroundStyle(Ink.fg)
          Text(day.counts).font(Face.text(12)).foregroundStyle(Ink.muted).lineLimit(1)
          Spacer(minLength: 4)
          Image(systemName: "chevron.right").font(.system(size: 13, weight: .semibold)).foregroundStyle(Ink.muted).rotationEffect(.degrees(open ? 90 : 0))
        }
        .padding(.vertical, 10).padding(.horizontal, 4).contentShape(Rectangle())
      }
      .buttonStyle(.plain)
      if open { WhatsNewDay(day: day) }
      Rectangle().fill(Ink.line).frame(height: 1)
    }
  }
}
