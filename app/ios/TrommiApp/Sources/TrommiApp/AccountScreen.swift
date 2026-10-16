// AccountScreen.swift: the screens before the board, as the app drew them before the shared app core (and as the
// web's auth.mjs draws them): one calm column, the bell and the name, a short headline, at most one line under it,
// one primary button, "Try the demo" at the foot. Start (Create account, Log in), Create account, Log in (with "Scan a
// code" as the quiet third way), New password (the Emergency Kit's words), Scan a code, the six emoji, the kit.
// Which of these forms is open is this screen's own navigation; what is under way, what went wrong, the emoji and the
// kit's words are the core's (`view.account`).
import SwiftUI
import TrommiClient
#if canImport(UIKit)
import UIKit
#endif

/** The forms of these screens (UI navigation only). */
enum AccountForm: String { case start, create, login, forgot, scan }

/** One screen: the bell (the way back to the start), a headline, at most one line under it, the demo at the foot. */
struct ObShell<Content: View>: View {
  @EnvironmentObject var shell: AppShell
  let title: String
  var lead = ""
  var home: (() -> Void)? = nil
  /** The last word of the headline gets the pen's line under it (the start: "anywhere."). */
  var underlined = false
  @ViewBuilder var content: () -> Content
  var body: some View {
    ScrollView {
      VStack(alignment: .leading, spacing: 0) {
        Button { home?() } label: {
          HStack(spacing: 8) {
            PenMark("ui:BELL", color: Ink.fg).frame(width: 26, height: 26)
            Text("Trommi").font(Face.display(20, .bold)).foregroundStyle(Ink.fg)
          }
        }
        .buttonStyle(.plain).allowsHitTesting(home != nil)
        .accessibilityLabel(home != nil ? "Trommi: back to the start" : "Trommi")
        Group {
          if underlined { Greeting(text: title, size: 34) } else { Text(title).font(Face.display(34, .heavy)).foregroundStyle(Ink.fg).accessibilityAddTraits(.isHeader) }
        }.padding(.top, 40)
        if !lead.isEmpty { Text(lead).font(Face.text(17)).foregroundStyle(Ink.muted).padding(.top, 10).fixedSize(horizontal: false, vertical: true) }
        VStack(alignment: .leading, spacing: 16) { content() }.padding(.top, 28)
        Button("Try the demo") { shell.enterDemo() }.buttonStyle(ObLink())
          .accessibilityHint("A made-up room on this device: nothing is sent")
          .frame(maxWidth: .infinity).padding(.top, 40)
      }
      .padding(.horizontal, 24).padding(.vertical, 28)
      .frame(maxWidth: 440).frame(maxWidth: .infinity)
    }
    .scrollDismissesKeyboard(.interactively)
    .background(Ink.bg.ignoresSafeArea())
  }
}

/** A field of these screens: its label above one outlined line. */
struct ObField: View {
  let label: String
  @Binding var text: String
  var id: String
  var body: some View {
    VStack(alignment: .leading, spacing: 6) {
      obLabel(label)
      TextField("", text: $text)
        .textContentType(.username).keyboardType(.emailAddress).textInputAutocapitalization(.never).autocorrectionDisabled()
        .accessibilityLabel(label).accessibilityIdentifier(id)
        .obInput()
    }
  }
}

/** A password: Show/Hide in the field. */
struct ObPassword<Side: View>: View {
  var label = "Password"
  var id = "account-password"
  @Binding var text: String
  @ViewBuilder var side: () -> Side
  @State private var shown = false
  var body: some View {
    VStack(alignment: .leading, spacing: 6) {
      HStack { obLabel(label); Spacer(); side() }
      HStack(spacing: 8) {
        Group { if shown { TextField("", text: $text) } else { SecureField("", text: $text) } }
          .textContentType(.password).textInputAutocapitalization(.never).autocorrectionDisabled()
          .accessibilityLabel(label).accessibilityIdentifier(id)
        Button(shown ? "Hide" : "Show") { shown.toggle() }.buttonStyle(ObLink())
      }
      .obInput()
    }
  }
}
extension ObPassword where Side == EmptyView {
  init(label: String = "Password", id: String = "account-password", text: Binding<String>) { self.init(label: label, id: id, text: text, side: { EmptyView() }) }
}

struct AccountScreen: View {
  @EnvironmentObject var shell: AppShell
  @State private var form: AccountForm
  @State private var email = ""
  @State private var password = ""
  @State private var words = ""
  @State private var link = ""
  @State private var shown = false
  @State private var camera = true
  private var account: CoreView.AccountView { shell.view.account }
  init(form: AccountForm = .start) { _form = State(initialValue: form) }
  private var busy: Bool { account.step == "working" || shell.view.phase == .starting }
  /** The core's word on this step; on Log in also why it is asked for again (a sign-in cut off: `resume`). */
  private var problem: String? { account.problem?.en ?? (form == .login ? account.resume?.en : nil) }
  var body: some View {
    Group {
      if account.step == "compare_emoji" { emoji }
      else if account.step == "keep_kit" || account.kitDue { kit }
      else {
        switch form {
        case .start: start
        case .create: create
        case .login: login
        case .forgot: forgot
        case .scan: scan
        }
      }
    }
  }
  private func go(_ f: AccountForm) { withAnimation(.snappy) { form = f } }

  // ---- the forms ------------------------------------------------------------------------------------------------

  private var start: some View {
    ObShell(title: "Your agents ring. Answer anywhere.", lead: "Claude Code and Codex ask as cards on your phone. One tap, and they keep working. End-to-end encrypted.", underlined: true) {
      // (how this device came to be signed out: the core's word)
      if let notice = account.notice { Text(notice.en).font(Face.text(15, .medium)).foregroundStyle(Ink.accent).fixedSize(horizontal: false, vertical: true).accessibilityIdentifier("account-notice") }
      if let problem { ObError(text: problem) }
      VStack(spacing: 12) {
        Button("Create account") { go(.create) }.buttonStyle(ObGo()).accessibilityIdentifier("way-create")
        Button("Log in") { go(.login) }.buttonStyle(ObSecond()).accessibilityIdentifier("way-login")
      }
    }
  }

  private var create: some View {
    ObShell(title: "Create account", home: { go(.start) }) {
      ObField(label: "Email", text: $email, id: "account-email")
      ObPassword(text: $password)
      ObError(text: problem)
      Button(busy ? "Creating…" : "Create account") { send(["step": "found", "email": email, "password": password]) }
        .buttonStyle(ObGo()).disabled(busy).accessibilityIdentifier("account-submit")
      HStack(spacing: 6) {
        Text("Have an account?").font(Face.text(16)).foregroundStyle(Ink.muted)
        Button("Log in") { go(.login) }.buttonStyle(ObLink())
      }.frame(maxWidth: .infinity)
    }
  }

  private var login: some View {
    ObShell(title: "Log in", home: { go(.start) }) {
      ObField(label: "Email or account ID", text: $email, id: "account-email")
      ObPassword(text: $password) { Button("Forgot?") { go(.forgot) }.buttonStyle(ObLink()).frame(height: 20) }
      ObError(text: problem)
      Button(busy ? "Logging in…" : "Log in") { send(["step": "sign_in", "email": email, "password": password]) }
        .buttonStyle(ObGo()).disabled(busy).accessibilityIdentifier("account-submit")
      ObOr()
      Button(Self.scanWay) { go(.scan) }.buttonStyle(ObSecond()).disabled(busy)
      HStack(spacing: 6) {
        Text("New here?").font(Face.text(16)).foregroundStyle(Ink.muted)
        Button("Create account") { go(.create) }.buttonStyle(ObLink())
      }.frame(maxWidth: .infinity)
    }
  }

  private var forgot: some View {
    ObShell(title: "New password", lead: "With the words of your Emergency Kit.", home: { go(.start) }) {
      ObField(label: "Email or account ID", text: $email, id: "account-email")
      VStack(alignment: .leading, spacing: 6) {
        obLabel("Twelve words")
        TextField("", text: $words, axis: .vertical).lineLimit(3...5)
          .textInputAutocapitalization(.never).autocorrectionDisabled()
          .accessibilityLabel("Twelve words")
          .padding(.vertical, 12).obInput()
      }
      ObPassword(label: "New password", id: "account-new-password", text: $password)
      ObError(text: problem)
      Button(busy ? "Setting…" : "Set password") { send(["step": "recover", "email": email, "words": words, "password": password]) }
        .buttonStyle(ObGo()).disabled(busy)
      Button("Log in with the words only") { send(["step": "sign_in_with_kit", "email": email, "words": words]) }
        .buttonStyle(ObLink()).frame(maxWidth: .infinity).disabled(busy)
      Button("Back to log in") { go(.login) }.buttonStyle(ObLink()).frame(maxWidth: .infinity)
    }
  }

  #if os(iOS)
  static let scanWay = "Scan a code"
  #else
  static let scanWay = "Paste a link"
  #endif
  private var scan: some View {
    #if os(iOS)
    let title = "Scan a code", lead = "On a logged-in device: Settings → Invite a Device."
    #else
    let title = "Paste a link", lead = "On a logged-in device: Settings → Invite a Device, then copy its link."
    #endif
    return ObShell(title: title, lead: lead, home: { go(.start) }) {
      #if canImport(UIKit)
      if camera && !shell.isDemo {
        QRScannerView(onLink: { text in link = text; send(["step": "join", "link": text]) }, onUnavailable: { _ in camera = false })
          .aspectRatio(1, contentMode: .fit)
          .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
      }
      #endif
      VStack(alignment: .leading, spacing: 6) {
        obLabel(camera ? "Or paste its link" : "Paste its link")
        TextField("https://app.trommi.com/join#v1…", text: $link)
          .textInputAutocapitalization(.never).autocorrectionDisabled().keyboardType(.URL)
          .accessibilityLabel("Its link")
          .obInput()
      }
      ObError(text: problem)
      if busy { HStack(spacing: 10) { ProgressView(); Text("Asking the other device…").font(Face.text(17)).foregroundStyle(Ink.muted) } }
      Button("Next") { send(["step": "join", "link": link]) }.buttonStyle(ObGo()).disabled(busy || link.isEmpty)
      Button("Back to log in") { go(.login) }.buttonStyle(ObLink()).frame(maxWidth: .infinity)
    }
  }

  // ---- the steps the core leads ---------------------------------------------------------------------------------

  private var emoji: some View {
    ObShell(title: "Same six emoji?", lead: "If they match, tap “They match” on the other device.") {
      LazyVGrid(columns: Array(repeating: GridItem(.flexible()), count: 3), spacing: 16) {
        ForEach(Array(account.emoji.enumerated()), id: \.offset) { _, x in
          VStack { Text(x.emoji).font(.system(size: 48)); Text(x.word).font(Face.text(13)).foregroundStyle(Ink.muted) }
        }
      }
      .accessibilityElement(children: .ignore)
      .accessibilityLabel("Check code: \(account.emoji.map(\.word).joined(separator: ", "))")
      ObError(text: problem)
      if let id = account.inviteId {
        Button("They match") { shell.account(["step": "confirm_emoji", "invite_id": id, "matches": true]) }.buttonStyle(ObGo())
        Button("They don't match") { shell.account(["step": "confirm_emoji", "invite_id": id, "matches": false]) }.buttonStyle(ObLink()).frame(maxWidth: .infinity)
      } else {
        HStack(spacing: 10) { ProgressView(); Text("Waiting for the other device…").font(Face.text(17)).foregroundStyle(Ink.muted) }
      }
    }
  }

  private var kit: some View {
    ObShell(title: "Your Emergency Kit", lead: "Twelve words that set a new password if you forget yours.") {
      VStack(alignment: .leading, spacing: 14) {
        HStack(spacing: 8) {
          PenMark("ui:BELL", color: Ink.fg).frame(width: 20, height: 20)
          Text("Trommi Emergency Kit").font(Face.display(16, .bold)).foregroundStyle(Ink.fg)
        }
        if let email = account.kitEmail { Text(email).font(Face.text(14)).foregroundStyle(Ink.muted) }
        LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], alignment: .leading, spacing: 10) {
          ForEach(Array(account.kit.enumerated()), id: \.offset) { i, word in
            HStack(spacing: 8) {
              Text("\(i + 1)").font(Face.mono(12)).foregroundStyle(Ink.faint).frame(width: 18, alignment: .trailing)
              if shown { Text(word).font(Face.mono(16, .medium)).foregroundStyle(Ink.fg) }
              else { PenUnderline().stroke(Ink.lineStrong, style: StrokeStyle(lineWidth: 2.4, lineCap: .round)).frame(width: CGFloat(48 + (i * 7) % 24), height: 8) }
            }
          }
        }
        .accessibilityElement(children: shown ? .combine : .ignore)
        .accessibilityLabel(shown ? "" : "Twelve words, hidden")
        if let id = account.kitAccount { Text(id).font(Face.mono(12)).foregroundStyle(Ink.muted).textSelection(.enabled) }
      }
      .padding(18)
      .background(PenBox(r: 14).fill(Ink.surface))
      .overlay(PenBox(r: 14).stroke(Ink.fg, lineWidth: 1.6))
      .onTapGesture { shown.toggle() }
      HStack(spacing: 12) {
        Button(shown ? "Hide" : "Show") { shown.toggle() }.buttonStyle(ObChip())
        if shown { Button("Copy") { copyText(account.kit.joined(separator: " ")) }.buttonStyle(ObChip()) }
      }.frame(maxWidth: .infinity)
      Text("Without it or your password, nobody can recover your account. Not even Trommi.").font(Face.text(15)).foregroundStyle(Ink.muted)
        .fixedSize(horizontal: false, vertical: true)
      ObError(text: problem)
      Button("Open Trommi") { shell.account(["step": "kit_kept"]) }.buttonStyle(ObGo()).accessibilityIdentifier("kit-kept")
    }
  }

  private func send(_ action: [String: Any]) {
    shell.account(action)
    password = ""; words = ""
  }
}

/** An invite this device shows (Settings): its words, its link or prompt, the emoji, the self-join asks. */
struct AccountInvite: View {
  @EnvironmentObject var shell: AppShell
  var body: some View {
    if let invite = shell.view.account.invite {
      Text(invite.title.en).font(Face.display(22))
      Text(invite.notice.en).font(Face.text(14))
    }
    if let prompt = shell.view.account.connectPrompt { Text(prompt).font(Face.mono(13)).textSelection(.enabled); ShareLink(item: prompt) }
    else if let link = shell.view.account.link { Text(link).font(Face.mono(13)).textSelection(.enabled); ShareLink(item: link) }
    if !shell.view.account.emoji.isEmpty { Text(shell.view.account.emoji.map(\.emoji).joined(separator: " ")).font(.largeTitle) }
    if let id = shell.view.account.inviteId { HStack { Button("They match") { shell.account(["step": "confirm_emoji", "invite_id": id, "matches": true]) }; Button("Cancel") { shell.account(["step": "drop_invite", "invite_id": id]) } } }
    ForEach(shell.view.account.asks, id: \.nonce) { ask in
      VStack(alignment: .leading, spacing: 12) {
        Text(ask.name).font(Face.display(22))
        Text(ask.host).font(Face.text(14))
        Text(ask.says.en).font(Face.text(14))
        ProgressView()
      }.padding().background(Ink.surface2, in: PenBox())
    }
  }
}
