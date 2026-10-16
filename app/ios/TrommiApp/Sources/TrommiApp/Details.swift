// Details.swift: the smaller parts of a talk as the app drew them before the shared app core (WorkTrail.swift,
// Usage.swift of that time): the files picked for a message, a turn's work trail, an agent's usage, the sandboxed page.
import SwiftUI
import TrommiClient
import UniformTypeIdentifiers
import WebKit

/** The files picked for a note or a message: quiet chips with their progress, and "Attach". */
struct AttachmentPicker: View {
  @EnvironmentObject var shell: AppShell
  @Binding var ids: [String]
  @State private var picking = false
  var body: some View {
    VStack(alignment: .leading, spacing: 8) {
      ForEach(ids, id: \.self) { id in
        HStack(spacing: 8) {
          Image(systemName: "paperclip").font(.system(size: 13, weight: .semibold)).foregroundStyle(Ink.muted)
          Text(shell.view.files[id]?.name ?? "File").font(Face.text(14)).foregroundStyle(Ink.fg).lineLimit(1).truncationMode(.middle)
          if let percent = shell.view.files[id]?.percent { ProgressView(value: Double(percent), total: 100).frame(width: 60) }
          Button { ids.removeAll { $0 == id } } label: { Image(systemName: "xmark").font(.system(size: 11, weight: .bold)).foregroundStyle(Ink.muted).frame(minWidth: 32, minHeight: 32).contentShape(Rectangle()) }
            .buttonStyle(.plain).accessibilityLabel("Remove")
        }
        .padding(.leading, 12).padding(.trailing, 4).background(Capsule().fill(Ink.sunken))
      }
      Button { picking = true } label: {
        HStack(spacing: 6) { Image(systemName: "paperclip").font(.system(size: 14, weight: .semibold)); Text("Attach").font(Face.text(15, .semibold)) }
          .foregroundStyle(Ink.fg).padding(.horizontal, 14).frame(minHeight: 40).glass(Capsule(), interactive: true)
      }
      .buttonStyle(.plain)
    }.fileImporter(isPresented: $picking, allowedContentTypes: [.item], allowsMultipleSelection: true) { result in
      if case .success(let urls) = result { for url in urls { if let id = shell.attach(url) { ids.append(id) } } }
    }
  }
}

/** Which trails and which of their steps he opened, kept outside the rows (a lazy conversation makes rows again). */
@MainActor final class TrailFolds: ObservableObject {
  static let shared = TrailFolds()
  @Published var open = Set<String>()
  func toggle(_ key: String) { if open.contains(key) { open.remove(key) } else { open.insert(key) } }
}

/** A turn's work trail: one quiet folded box (its sign, the head, the sum, the step it is at, the time); open, its
 *  steps, each opening to what went in and came out. */
struct WorkDrawing: View {
  let work: CoreView.WorkView
  var timeMs: UInt64 = 0
  @ObservedObject private var folds = TrailFolds.shared
  var body: some View {
    let open = folds.open.contains(work.turn)
    VStack(alignment: .leading, spacing: 0) {
      Button { folds.toggle(work.turn) } label: {
        HStack(alignment: .firstTextBaseline, spacing: 6) {
          WorkSign(state: work.state == "done" ? "ok" : work.state)
          Text(work.head.en).font(Face.text(14, .semibold)).foregroundStyle(Ink.fg).lineLimit(1).fixedSize()
          if let sum = work.sum { Text("· \(sum.en)").font(Face.text(14)).foregroundStyle(Ink.muted).lineLimit(1).layoutPriority(1) }
          if !work.now.isEmpty { Text("· \(work.now)").font(Face.text(14)).foregroundStyle(Ink.fg).lineLimit(1).truncationMode(.tail) }
          Spacer(minLength: 4)
          if timeMs > 0 { Text(talkClock(timeMs)).font(Face.text(11)).foregroundStyle(Ink.faint).lineLimit(1).fixedSize() }
          Image(systemName: "chevron.right").font(.system(size: 11, weight: .semibold)).foregroundStyle(Ink.faint).rotationEffect(.degrees(open ? 90 : 0))
        }
        .padding(.horizontal, 12).padding(.vertical, 10)
        .contentShape(Rectangle())
      }
      .buttonStyle(.plain)
      .accessibilityElement(children: .ignore)
      .accessibilityLabel(work.head.en)
      .accessibilityValue(open ? "open" : "folded")
      if open {
        Rectangle().fill(Ink.line).frame(height: 1)
        VStack(alignment: .leading, spacing: 7) {
          ForEach(work.lines, id: \.id) { line in WorkStepRow(line: line, key: "\(work.turn)#\(line.id)") }
          if let rest = work.rest { Text(rest.en).font(Face.text(13)).foregroundStyle(Ink.faint).padding(.leading, 24) }
        }
        .padding(.horizontal, 12).padding(.vertical, 10)
      }
    }
    .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Ink.surface2))
    .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(Ink.line, lineWidth: 1))
    .padding(.trailing, 24)
    .frame(maxWidth: .infinity, alignment: .leading)
  }
}
/** One line of an open trail: the agent's words, or a step that opens to its input and output. */
private struct WorkStepRow: View {
  let line: CoreView.WorkLineView
  let key: String
  @ObservedObject private var folds = TrailFolds.shared
  var body: some View {
    if line.kind == "text" {
      Markdown(text: line.text, size: 14).padding(.leading, 24).padding(.vertical, 2)
    } else if !line.input.isEmpty || !line.output.isEmpty {
      let open = folds.open.contains(key)
      VStack(alignment: .leading, spacing: 6) {
        Button { folds.toggle(key) } label: { row(chevron: open) }.buttonStyle(.plain)
          .accessibilityElement(children: .ignore).accessibilityLabel(spoken).accessibilityValue(open ? "open" : "folded")
        if open {
          if !line.input.isEmpty { WorkIO(text: line.input, color: Ink.fg) }
          if !line.output.isEmpty { WorkIO(text: line.output, color: Ink.muted) }
        }
      }
    } else {
      row(chevron: nil).accessibilityElement(children: .ignore).accessibilityLabel(spoken)
    }
  }
  private func row(chevron: Bool?) -> some View {
    HStack(alignment: .firstTextBaseline, spacing: 6) {
      WorkSign(state: line.state)
      Text(line.tool).font(Face.text(14, .semibold)).foregroundStyle(Ink.fg).lineLimit(1).fixedSize()
      if !line.what.isEmpty { Text(line.what).font(Face.text(14)).foregroundStyle(Ink.muted).lineLimit(1).truncationMode(.middle) }
      Spacer(minLength: 4)
      if let count = line.count { Text(count.en).font(Face.text(12)).foregroundStyle(line.state == "failed" ? Ink.deny : Ink.muted).lineLimit(1).fixedSize() }
      if !line.took.isEmpty { Text(line.took).font(Face.text(12)).monospacedDigit().foregroundStyle(Ink.faint).lineLimit(1).fixedSize() }
      if let open = chevron { Image(systemName: "chevron.right").font(.system(size: 10, weight: .semibold)).foregroundStyle(Ink.faint).rotationEffect(.degrees(open ? 90 : 0)) }
    }
    .contentShape(Rectangle())
  }
  private var spoken: String { [line.tool, line.what, line.says?.en ?? "", line.count?.en ?? "", line.took].filter { !$0.isEmpty }.joined(separator: ", ") }
}
/** A step's or a turn's state as a small sign: a breathing dot while it runs, a tick, a cross, a stop. */
private struct WorkSign: View {
  let state: String
  @Environment(\.accessibilityReduceMotion) private var still
  @State private var lit = false
  var body: some View {
    Group {
      switch state {
      case "running":
        Circle().fill(Ink.muted).frame(width: 7, height: 7)
          .opacity(still ? 0.7 : (lit ? 1 : 0.25))
          .animation(still ? nil : .easeInOut(duration: 0.6).repeatForever(autoreverses: true), value: lit)
          .onAppear { lit = true }
      case "ok": Image(systemName: "checkmark").font(.system(size: 10, weight: .semibold)).foregroundStyle(Ink.muted)
      case "failed": Image(systemName: "xmark").font(.system(size: 10, weight: .bold)).foregroundStyle(Ink.deny)
      case "": Color.clear.frame(width: 7, height: 7)
      default: Image(systemName: "stop.fill").font(.system(size: 8)).foregroundStyle(Ink.muted)
      }
    }
    .frame(width: 18, alignment: .center)
    .accessibilityHidden(true)
  }
}
/** A step's input or output: monospace as the terminal showed it; a long one scrolls in its box. */
private struct WorkIO: View {
  let text: String
  let color: Color
  var body: some View {
    let words = Text(text).font(Face.mono(12)).foregroundStyle(color).textSelection(.enabled)
      .frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, 10).padding(.vertical, 8)
    ScrollView { words }.frame(maxHeight: 300).fixedSize(horizontal: false, vertical: true)
      .background(RoundedRectangle(cornerRadius: 6, style: .continuous).fill(Ink.sunken))
      .padding(.leading, 24)
      .contextMenu { Button { copyText(text) } label: { Label("Copy", systemImage: "doc.on.doc") } }
  }
}

/** A usage meter as a bar: the faint track and the part used (urgent when the core says it is hot). */
struct UsageBar: View {
  let used: UInt32
  var hot = false
  var height: CGFloat = 5
  var body: some View {
    GeometryReader { g in
      ZStack(alignment: .leading) {
        Capsule().fill(Ink.fg.opacity(0.12))
        Capsule().fill(hot ? Ink.urgHigh : Ink.accent).frame(width: g.size.width * CGFloat(min(used, 100)) / 100)
      }
    }.frame(height: height)
  }
}
private struct MeterRing: View {
  let meter: CoreView.MeterView
  let width: CGFloat
  var body: some View {
    ZStack {
      Circle().stroke(Ink.fg.opacity(0.12), lineWidth: width)
      Circle().trim(from: 0, to: CGFloat(min(meter.used, 100)) / 100)
        .stroke(meter.hot ? Ink.urgHigh : Ink.accent, style: StrokeStyle(lineWidth: width, lineCap: .round))
        .rotationEffect(.degrees(-90))
    }
  }
}
/** Beside a session's More button: two rings, one inside the other (the week outside, the 5-hour window inside). */
struct UsageMark: View {
  let usage: CoreView.UsageView
  var size: CGFloat = 30
  var body: some View {
    let five = usage.meters.first { $0.key == "five_hour" }, week = usage.meters.first { $0.key == "weekly" }
    let w = size * 0.115
    if !usage.meters.isEmpty {
      ZStack {
        if let week { MeterRing(meter: week, width: w).padding(w / 2) }
        if let five { MeterRing(meter: five, width: w).padding(week == nil ? w / 2 : w * 2.05) }
      }
      .frame(width: size, height: size)
      .accessibilityElement(children: .ignore)
      .accessibilityLabel(usage.meters.map { "\($0.long): \($0.used) percent used" }.joined(separator: ", "))
    }
  }
}
/** An agent's account, fuller: the plan as a small tag and the e-mail, one row per window, the facts. */
struct UsageDrawing: View {
  let usage: CoreView.UsageView
  var body: some View {
    VStack(alignment: .leading, spacing: 9) {
      HStack(spacing: 8) {
        if !usage.plan.isEmpty {
          Text(usage.plan).font(Face.text(12, .bold)).padding(.horizontal, 7).padding(.vertical, 3)
            .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Ink.fg, lineWidth: 1.3))
        }
        if !usage.email.isEmpty { Text(usage.email).font(Face.text(14)).foregroundStyle(Ink.muted).lineLimit(1).truncationMode(.middle) }
      }
      ForEach(usage.meters, id: \.key) { m in
        VStack(alignment: .leading, spacing: 4) {
          HStack {
            Text(m.long).font(Face.text(14)).foregroundStyle(Ink.muted)
            Spacer()
            Text("\(m.used) %").font(Face.text(14, .bold)).foregroundStyle(m.hot ? Ink.urgHigh : Ink.fg)
          }
          UsageBar(used: m.used, hot: m.hot, height: 7)
          if let at = m.resetMs { Text("resets \(Date(timeIntervalSince1970: Double(at) / 1000).formatted(.dateTime.weekday(.abbreviated).hour().minute()))").font(Face.text(12)).foregroundStyle(Ink.muted) }
        }
      }
      if !usage.facts.isEmpty {
        HStack(spacing: 8) {
          ForEach(Array(usage.facts.enumerated()), id: \.offset) { _, f in
            Text(f.en).font(Face.text(12, .semibold)).padding(.horizontal, 7).padding(.vertical, 3).background(RoundedRectangle(cornerRadius: 6).fill(Ink.fg.opacity(0.07)))
          }
        }
      }
    }
    .padding(.horizontal, 16).padding(.vertical, 12).frame(maxWidth: .infinity, alignment: .leading)
  }
}
@MainActor private final class HTMLNavigation: NSObject, WKNavigationDelegate {
  static let shared = HTMLNavigation()
  func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction, decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
    decisionHandler(action.request.url?.scheme == "about" ? .allow : .cancel)
  }
}
private func htmlDocument(_ html: String) -> String {
  let policy = "default-src 'none'; style-src 'unsafe-inline'; img-src data: blob:; base-uri 'none'; form-action 'none'; frame-src 'none'"
  return "<meta name='viewport' content='width=device-width,initial-scale=1'><meta http-equiv='Content-Security-Policy' content=\"\(policy)\"><style>:root{color-scheme:light dark}body{font:17px -apple-system;overflow-wrap:anywhere;margin:0}img{max-width:100%}</style>" + html
}
@MainActor private func htmlView() -> WKWebView {
  let config = WKWebViewConfiguration(); config.websiteDataStore = .nonPersistent()
  config.defaultWebpagePreferences.allowsContentJavaScript = false
  let view = WKWebView(frame: .zero, configuration: config); view.navigationDelegate = HTMLNavigation.shared
  return view
}
#if canImport(UIKit)
struct InlineHTML: UIViewRepresentable {
  let html: String
  func makeUIView(context: Context) -> WKWebView { htmlView() }
  func updateUIView(_ view: WKWebView, context: Context) { if context.coordinator.last != html { context.coordinator.last = html; view.loadHTMLString(htmlDocument(html), baseURL: nil) } }
  func makeCoordinator() -> HTMLDraw { HTMLDraw() }
}
#else
struct InlineHTML: NSViewRepresentable {
  let html: String
  func makeNSView(context: Context) -> WKWebView { htmlView() }
  func updateNSView(_ view: WKWebView, context: Context) { if context.coordinator.last != html { context.coordinator.last = html; view.loadHTMLString(htmlDocument(html), baseURL: nil) } }
  func makeCoordinator() -> HTMLDraw { HTMLDraw() }
}
#endif
final class HTMLDraw { var last: String? }

