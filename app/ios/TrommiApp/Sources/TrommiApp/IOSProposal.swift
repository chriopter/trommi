// IOSProposal.swift: the iPhone's frame. One navigation stack and no tab bar: at its root chats on no team, then every
// team (the core's `sidebar.root` and `sidebar.stacks`, in drawing order, as two inset groups); a team pushes its
// group page (`Focus.team`): its open cards, the questions first, then what is to read; a chat opens its page as on
// every device, and the system's back returns home. Both end in one row of controls at the bottom: the system's search,
// then the bolt that walks every open card including chats on no team (`sidebar.global_blitz`) and the + in one glass
// group. The look itself: NativeLook.swift, NativeGroup.swift. Drawing only: rows, order, counts and words belong to
// the core.
import SwiftUI
import TrommiClient

#if os(iOS)
struct PhoneFrame: View {
  @EnvironmentObject var shell: AppShell
  var body: some View {
    NavigationStack(path: $shell.path) {
      NativeRoot()
        .navigationDestination(for: CoreView.Screen.self) { screen in
          // (`.group` pushed is a team's page: its open cards, the questions first, in the native look)
          CorePage(screen: screen)
        }
    }
    .tint(Ink.accent)
    // (the search sheet of the pages that keep their own bar, such as the Desk: every team's chats under the words)
    .sheet(isPresented: $shell.searchOpen, onDismiss: { shell.chatsQuery = ""; shell.show(shell.screen) }) {
      NavigationStack {
        ChatsScreen(searching: true)
          .searchable(text: $shell.chatsQuery, placement: .navigationBarDrawer(displayMode: .always), prompt: "Search chats")
          .onChange(of: shell.chatsQuery) { _, _ in shell.show(shell.screen) }
      }
    }
    // (the note lies over everything as a yellow page)
    .overlay { if shell.noteOpen { NotePage().transition(.move(edge: .bottom)) } }
    .animation(.snappy, value: shell.noteOpen)
    .onChange(of: shell.path) { _, path in
      if path.isEmpty { shell.teamPage = nil }
      // (back on a team's page from Blitz over every group: that team in view again)
      if path.last == .group, let team = shell.teamPage, shell.walkAll { shell.show(.group, team: team); return }
      let screen = path.last ?? (shell.noteOpen ? .notes : .group)
      if shell.screen != screen { shell.show(screen) }
    }
  }
}

/** The bolt at the bottom of the chat lists: Blitz over every group's open cards (`sidebar.global_blitz`), with its
 *  count; off while nothing is open. The card's page walks on to the next. */
struct BlitzAllButton: View {
  @EnvironmentObject var shell: AppShell
  var body: some View {
    let blitz = shell.view.sidebar.globalBlitz
    Button { shell.blitzAll() } label: {
      HStack(spacing: 4) {
        Image(systemName: blitz != nil ? "bolt.fill" : "bolt")
        if let n = blitz?.says.args["n"] { Text(n).font(.caption.weight(.semibold)).monospacedDigit() }
      }
    }
    .disabled(blitz == nil)
    .accessibilityLabel(blitz?.says.en ?? "Blitz")
    .accessibilityIdentifier("chats-blitz-all")
  }
}
#endif
