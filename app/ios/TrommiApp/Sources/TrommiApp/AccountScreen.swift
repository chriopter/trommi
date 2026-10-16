import SwiftUI
import TrommiClient

struct AccountScreen: View {
  @EnvironmentObject var shell: AppShell
  @State private var email = ""
  @State private var password = ""
  @State private var words = ""
  @State private var link = ""
  @State private var scanning = false
  @State private var form = "sign_in"
  var body: some View {
    ScrollView { VStack(alignment: .leading, spacing: 22) {
      PenMark("bell").frame(width: 64, height: 64)
      Text("Trommi").font(Face.display(44))
      if let problem = shell.view.account.problem { Text(problem.en).foregroundStyle(Ink.deny).font(Face.text(16)) }
      if shell.view.account.step == "working" || shell.view.phase == .starting { ProgressView() }
      else if shell.view.account.step == "compare_emoji" {
        HStack { ForEach(Array(shell.view.account.emoji.enumerated()), id: \.offset) { Text($0.element.emoji).font(.largeTitle) } }
        Text(shell.view.account.emoji.map(\.word).joined(separator: " · ")).font(Face.text(15))
        Button("They match") { confirm(true) }.buttonStyle(.borderedProminent)
        Button("They don't match", role: .destructive) { confirm(false) }
      } else if shell.view.account.step == "keep_kit" || shell.view.account.kitDue {
        Text("Emergency Kit").font(Face.display(30))
        Text(shell.view.account.kit.joined(separator: " ")).font(Face.mono(19)).textSelection(.enabled)
        if let account = shell.view.account.kitAccount { Text(account).font(Face.mono(13)).textSelection(.enabled) }
        Button("I kept my Emergency Kit") { shell.account(["step": "kit_kept"]) }.buttonStyle(.borderedProminent)
      } else {
        Picker("Account", selection: $form) { Text("Sign in").tag("sign_in"); Text("Create account").tag("found"); Text("Emergency Kit").tag("sign_in_with_kit"); Text("Pair device").tag("join"); Text("Recover account").tag("recover") }.pickerStyle(.menu)
        if form == "join" {
          TextField("Invite link", text: $link).textFieldStyle(.roundedBorder)
          Button("Join") { shell.account(["step": "join", "link": link]); link = "" }.buttonStyle(.borderedProminent)
          #if canImport(UIKit)
          Button("Scan invite") { scanning = true }.sheet(isPresented: $scanning) { QRScannerView(onLink: { link = $0; scanning = false }, onUnavailable: { shell.platformError = $0; scanning = false }) }
          #endif
        } else {
          TextField("E-mail or account", text: $email).textFieldStyle(.roundedBorder)
          if form == "sign_in_with_kit" || form == "recover" { TextField("Emergency Kit words", text: $words, axis: .vertical).textFieldStyle(.roundedBorder) }
          else { SecureField("Password", text: $password).textFieldStyle(.roundedBorder) }
          if form == "recover" { SecureField("New password", text: $password).textFieldStyle(.roundedBorder); Text("Recovery removes your other devices and replaces your recovery code.").font(Face.text(13)) }
          Button(form == "found" ? "Create account" : "Sign in") {
            var action: [String: Any] = ["step": form, "email": email]
            if form == "recover" { action["words"] = words; action["password"] = password }
            else if form == "sign_in_with_kit" { action["words"] = words } else { action["password"] = password }
            shell.account(action); password = ""; words = ""
          }.buttonStyle(.borderedProminent)
        }
      }
      Button("Try the demo") { shell.enterDemo() }
    }.padding(32).frame(maxWidth: 520).frame(maxWidth: .infinity) }
  }
  func confirm(_ matches: Bool) { if let id = shell.view.account.inviteId { shell.account(["step": "confirm_emoji", "invite_id": id, "matches": matches]) } }
}
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
