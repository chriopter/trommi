import SwiftUI
import TrommiClient
import UniformTypeIdentifiers
import WebKit

struct AttachmentPicker: View {
  @EnvironmentObject var shell: AppShell
  @Binding var ids: [String]
  @State private var picking = false
  var body: some View {
    VStack(alignment: .leading) {
      ForEach(ids, id: \.self) { id in HStack { Image(systemName: "paperclip"); Text(shell.view.files[id]?.name ?? "File").font(Face.text(13)); if let percent = shell.view.files[id]?.percent { ProgressView(value: Double(percent), total: 100) }; Button { ids.removeAll { $0 == id } } label: { Image(systemName: "xmark.circle") } } }
      Button("Attach", systemImage: "paperclip") { picking = true }
    }.fileImporter(isPresented: $picking, allowedContentTypes: [.item], allowsMultipleSelection: true) { result in
      if case .success(let urls) = result { for url in urls { if let id = shell.attach(url) { ids.append(id) } } }
    }
  }
}
struct WorkDrawing: View {
  let work: CoreView.WorkView
  var body: some View {
    DisclosureGroup {
      if let sum = work.sum { Text(sum.en).font(Face.text(13)).foregroundStyle(Ink.muted) }
      ForEach(work.lines, id: \.id) { line in
        DisclosureGroup {
          Text(line.input).font(Face.mono(12)).textSelection(.enabled)
          Text(line.output).font(Face.mono(12)).textSelection(.enabled)
        } label: { VStack(alignment: .leading) { HStack { Text(line.tool).font(Face.text(14, .medium)); Text(line.what).font(Face.text(14)); Spacer(); Text(line.took).font(Face.mono(11)) }; if !line.text.isEmpty { Markdown(text: line.text) }; if let says = line.says { Text(says.en).font(Face.text(12)) } } }
      }
      if let rest = work.rest { Text(rest.en).font(Face.text(12)) }
    } label: { HStack { WorkingRing(working: work.state == "running").frame(width: 18, height: 18); Text(work.head.en).font(Face.text(15, .medium)) } }
    .padding(14).background(Ink.surface2, in: PenBox())
  }
}
struct UsageDrawing: View {
  let usage: CoreView.UsageView
  var body: some View {
    VStack(alignment: .leading, spacing: 8) { Text(usage.plan).font(Face.text(14, .medium)); Text(usage.email).font(Face.text(12)).foregroundStyle(Ink.muted)
      ForEach(usage.meters, id: \.key) { meter in HStack { Text(meter.label).font(Face.text(12)); ProgressView(value: Double(meter.used), total: 100).tint(meter.hot ? Ink.urgHigh : Ink.accent); Text("\(meter.used)%").font(Face.mono(11)) } }
      ForEach(Array(usage.facts.enumerated()), id: \.offset) { Text($0.element.en).font(Face.text(12)) }
    }
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
  return "<meta name='viewport' content='width=device-width,initial-scale=1'><meta http-equiv='Content-Security-Policy' content=\"\(policy)\"><style>body{font:17px -apple-system;color:inherit;overflow-wrap:anywhere}img{max-width:100%}</style>" + html
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

struct GoalsDrawing: View {
  @EnvironmentObject var shell: AppShell
  let goals: CoreView.GoalsView
  @State private var editing = false
  @State private var text = ""
  var body: some View {
    VStack(alignment: .leading, spacing: 10) {
      HStack {
        Text("Goals").font(Face.text(14, .medium)); Spacer()
        if let toggle = goals.toggle { Button { shell.act(["do": "fold_goals", "desk_id": goals.deskId, "folded": !goals.folded]) } label: { Image(systemName: "chevron.down").rotationEffect(.degrees(goals.folded ? -90 : 0)) }.accessibilityLabel(toggle.en) }
        Button(editing ? "Save" : "Edit") {
          if editing { shell.act(["do": "set_goal", "desk_id": goals.deskId, "text": text]) } else { text = goals.lines.joined(separator: "\n") }
          editing.toggle()
        }
      }
      if editing { TextEditor(text: $text).font(Face.text(16)).frame(minHeight: 100) }
      else { Text((goals.folded ? Array(goals.lines.prefix(1)) : goals.lines).joined(separator: "\n")).font(Face.text(16)) }
    }.padding().frame(maxWidth: .infinity, alignment: .leading).background(Ink.surface2, in: PenBox())
  }
}
