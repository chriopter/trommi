# Changelog

What changed in Trommi, newest first. One section per day; each GitHub release adds the day's release numbers to its heading.

A day starts with a short summary (one to three plain sentences) and at most one picture, a line
`![what it shows](app/web/public/changelog/<YYYY-MM-DD>.webp)`: a screenshot of the demo room only (`?mock=1`, never
real data), webp, at most 150 KB. Then its lines under **New**, **Better** and **Fixed**. What's new (web and iPhone)
shows the summary and the picture, the lines folded under "All changes".

## 12 October 2026

Star a card to keep it at the top of Off your mind. A card you snooze, a draft, a duck or a session's settings now
look the same on the web and the iPhone.

**New**
- Star a card (the star beside its close button) to find it again: starred cards stand at the top of Off your
  mind, private and end-to-end encrypted, the same on all your devices (web and iPhone).

**Fixed**
- A card snoozed on the web no longer stays on the iPhone's Desk: the iPhone now reads the card names the web
  writes, and writes them the same way (drafts, ducks, session and desk settings too; web and iPhone).
On the iPhone, the Live Activity no longer shows a bare "0" when everything is answered: it says so, and shows how
many agents are still at work.

**Better**
- iPhone Live Activity with nothing waiting: "All answered" with the working ring and how many agents are at work
  (Dynamic Island and Lock Screen), a tick when none works; it stays while agents work and goes when all is quiet.

## 11 October 2026

Each agent now has its own page in Settings, with its plan, its usage and its sessions. On the iPhone you answer
the most urgent question straight from the Lock Screen, pushes give nothing away, and the connector updates itself
while it runs.

![Settings → Agents in the demo room](app/web/public/changelog/2026-10-11.webp)

**New**
- A new Claude Code or Codex session on a computer and account that already has a connected agent gets onto the
  board with one tap ("Allow new Claude session …?") instead of an invite link and the six emoji, as its own
  session. Its first message says which session vouched for it; at most 8 such sessions per account, and archiving
  one removes its device. The web and iPhone apps answer it now; the connector turns it on in a later release.
- A card its session is working on says so above its title: "<session> is working on it", its status line and how
  long ago, with a running pen line (handed back, or answered while the session works on it; web and iPhone).
- Trommi for Mac: the iPad's layout in a window, the questions waiting in the menu bar, Return sends and
  Shift+Return hands a card back; pairing by pasting the link.
- Settings → Security lists every key that opens your data: the account key (your recovery key) with its
- Settings → Security (web and iPhone) lists every key that opens your data: the account key (your recovery key) with its
  fingerprint, your devices with theirs and the day each came, your agents, and what each of them can read. The self
  test is a row there now.
- iPhone: Settings → Data shows what this iPhone downloaded and uploaded (with a reset), and what your account keeps on
  the hub: messages, cards, files and the total size.
- Settings → Agents shows each agent's plan, e-mail and usage (5-hour window and week, for Codex its credits);
  a session's heading shows the two bars, end-to-end encrypted like the rest.
- Public iPhone beta via TestFlight: join from [trommi.com/beta](https://trommi.com/beta); every release arrives there
  first.
- Settings → Devices names a device of yours that never finished signing in, and removes it with one tap
  (while it is there, new sessions can't be opened).
- iPhone: the Live Activity and Dynamic Island show the most urgent question with its two answers; answer it right
  there, without opening the app.
- The connector updates itself while it runs: no reconnect after an update.
- Drag sessions in the sidebar into your own order; the iPhone shows the same order.
- Delete your account right from the iPhone app (Settings → Account).
- Log out straight from the desk menu.
- What's new in the desk menu (web and iPhone): this list, with a dot until you have read the newest day.
- Settings → Agents (web and iPhone): each agent as Claude Code or Codex on its computer, its sessions under it; remove an agent from there. Devices lists only your phones, tablets and browsers.
- Codex hears the board: a new answer or message wakes an idle Codex session, which then reads it.
- What's new tells each day in a few sentences, with a picture; all of its changes unfold under "All changes" (web
  and iPhone).

**Better**
- A session's heading is one line: the name, its connection as a dot (what it is at, model and machine in its tip),
  All | Questions beside it, and its usage as one mark of two rings, the week outside and 5 h inside (web and iPhone).
- A session shows a switch "All | Questions" under its heading, with the number of questions waiting; the funnel beside
  the message field is gone (web and iPhone).
- A session's heading is calmer: under the name a dot, what it is at and its model; its usage (5 h and week) as two
  small rings at the right, beside the ⋯ on a phone; its files moved into the ⋯ menu.
- On an opened question ← → step through its pictures and never switch questions; Shift ↓ ↑ go to the next or
  previous question (web).
- A question's options explained stand at the end of the card, after the pictures, all open; the pointer on an
  answer marks its text and the pen's arrow points at it (web; iPhone lists them at the end too).
- The reverse card on a card: its black oval smaller, with air above and below, as on the send button (web and iPhone).
- Desk: the headings "Off your mind" and "Artifacts" are links themselves, underlined with an arrow; no separate "Show more" (web and iPhone).
- A helper's status line follows its question: filing a card turns the line to waiting on you with that card, and
  your answer turns it back to working with what you chose; no more lines that still wait after you answered.
- Agents answer a request or question you write in chat with a card (an info card for results, a decision card for
  choices) instead of only a chat reply.
- Less for anyone between you and your devices: every push says the same ("Something new.", your iPhone shows what it
  is); the Live Activity's updates from the hub say only whether agents work and whether something waits (the app
  shows the numbers); the hub sees only whether a card knocks, not its urgency, and no longer when you answered;
  messages and new files are padded to fixed sizes.
- iPhone: a notification makes the app open almost up to date (what it brings is fetched ahead, still encrypted),
  and the Live Activity shows the real numbers again, sealed for your phone alone.
- iPhone: right after you log in, the Desk shows the open questions from the hub's list, before the history is read.
- Off your mind: an answered or done card has a green tick on the right, a shredded one a shredder (and stays struck through), a withdrawn one a return arrow (web and iPhone).
- Web: coming back to the tab after a while, or getting the network back, reconnects at once; new cards no longer
  wait up to a minute behind a connection the sleeping phone left half open.
- Pairing a device or an agent survives a network blip: a step that did not reach Trommi is tried again, and if it
  still fails you are told it was the connection, with "Try again" on the same code instead of asking for a new one.
- What's new says what runs: at its top the release, its build and when it was updated (web; on the iPhone the app's version and build).
- Helpers read their own board events: a subagent's child session goes to it directly, so the main agent no longer
  passes them on; it gets one line when the helper is done, and the events back if the helper stops reading.
- Codex as Claude Code: `setup codex` adds Trommi's hooks to Codex, so its terminal, its answers (under the card
  they belong to), its permission prompts and its work trail reach the board too, once you trust the hooks in Codex.
  `trommi-connector remove codex` takes them out again. A Codex with its own `CODEX_HOME` is woken too.
- Codex: a session that was woken but did not read the board is woken again by the next message (at most every
  3 minutes); it answers a card's question under that card; restart and "key in use" messages speak Codex's language
  instead of Claude Code's.
- iPhone: after another device of yours hands over older keys, the app no longer empties the Desk and reads the whole
  room again: what it has is kept, only what was missing is read (about fifteen times faster).
- Status lines under a chat: the time short with a small clock after it ("4m"); on the phone one line each, the title and the time, and a tap shows the detail and the way to its question (iPhone and web).
- Logging in on the web checks only your account's own history before it opens; each conversation is checked
  right after, in the background.
- Long chats open on the last day and load further back as you scroll up; this browser keeps only the newest
  500 messages of each chat and reads older ones again when needed.
- iPhone: logging in to a big account opens the Desk at once and fills it page by page, with "Catching up… N%";
  catching up is about ten times faster (one write per page instead of one per item).
- iPhone: big boards stay smooth while news comes in: a change of the board costs a fraction of before, a long chat
  opens three times faster with its newest 150 messages ("Earlier messages" shows more), swiping back from a chat no
  longer stutters, and a huge desk needs a fifth of the memory.
- When an agent answers something you asked on a card, its answer stands in that card's thread; the chat shows one
  line pointing to the card.
- The website shows the apps: the web app and the iPhone app in real screenshots, light and dark.
- trommi.com reworked: a light head with Trommi, Apps, Agents and the GitHub mark, the picture right under the
  headline, Web, iPhone and Mac side by side, and a section on the agents and how to connect one.
- trommi.com: headline at the left and the laptop and iPhone at the right in the first view; Apps as whole cards;
  Agents with a terminal holding the three steps to connect one.
- The reverse card's middle is now an oval drawn by the pen, a little uneven and tilted (web and iPhone).
- The writing field under a card is calmer: one "+" on the left for attaching and drawing, Send on the right. Shift+Enter hands the card back with your words, Alt+Enter (⌥+Enter) starts a new line (web; on the iPad with a keyboard too).
- The demo looks calmer and friendlier: sessions with plain names, design choices with pictures first, one urgent card
  instead of a stack of red ones.
- Settings: Invite a Device and Invite an Agent stand side by side as equals; Devices and Account are laid out
  like the rest of Settings, each device and each way in (password, passkeys, Emergency Kit) with its own page.
- An agent reporting on an ongoing matter keeps one card and updates it, with the newest state in the title.
- An agent you ask for something keeps one card for it, from Working on to Done.
- A main agent coordinates: cards about a topic (design, iOS, release …) sit in that helper's session, so your
  answers go straight to the helper working on it.
- The self-test screen is now called "Security Info".
- Settings → Account no longer has the "Advanced" block with the old account address; the old recovery-code screen (/recover) is gone. Room, key epoch and hub are shown under Security Info.
- Codex: the board tools are there from the start, Trommi tool calls no longer ask for approval one by one, and the texts say to restart Codex (not /mcp) when needed.
- A Codex session between turns shows as "on its next turn", never as gone or not listening.
- In a conversation, a running status line opens the card it works on, the same card its row on the Desk opens
  (web and iPhone).

**Fixed**
- Web: signing in on a new browser in a room with a long history is live in seconds again, not after a minute.
- Desk: a row no longer grows when the pointer rests on it; the text under the title stays hidden (web).
- Web, signing in on a new browser: the Desk no longer shows old, empty approvals or "Go outside." while your room is
  still being read. It says "Catching up…", open questions show as quiet "Loading …" placeholders until they are
  decrypted, sessions are "Loading …" instead of an id, and agents show as connected right away instead of "gone".
- Web: a permission request that was answered or ran out on another device no longer stands on the Desk as open.
- A second Claude Code or Codex session started in the same folder no longer writes into the first one's board
  session: each session is its own agent and needs its own invite; only resuming a session continues it.
- Web: a picture that could not be loaded keeps its place as a small tile (the reason on hover); a Desk row keeps its
  title and answers in view. An open tab looks for a new version every 10 minutes, and at once on a file of a newer Trommi.
- Web: a card handed back and then put before you again no longer slips back to "with the agent" (off the Desk)
  when you open it; a card already stuck that way returns to the Desk by itself.
- After /mcp → Reconnect a connected agent no longer shows as gone: the old connector's goodbye can't override the new one's hello.
- iPhone: the app no longer crashes when it starts with an account already signed in.
- A card you put off (Later) wakes up when you answer it or hand it back with the reverse card: reworked, it is back on the Desk (web and iPhone).
- iPhone: a session renamed on another device no longer shows up twice; the new name lands on the session itself.
- Web: after catching up, the app could keep saying "Connecting" although it was connected and up to date.
- Open cards are the same on every device: the web takes them from the hub's list of open questions, so a card
  settled elsewhere no longer stays on the Desk, and a card put before you again no longer hides under "Off your mind".
- Renaming a session on the web no longer makes a second session appear on the iPhone.
- Web: an answer to a form that came only after you had moved to another page (a removed device, say) no longer
  takes you away from that page.
- Logging in on the web reads a big account several times faster, keeps saying how far it is, and a log in that
  was given up no longer leaves this browser stuck on "Not opened": logging in again works.
- iPhone: the Dynamic Island shows only while a question waits (no more "0"), and its bell no longer jumps on every
  update.
- Web: a message to a session that still held a removed device ("Not sent: stale-session") now goes out: the
  session is repaired first and the message is sent again.
- iPhone: "N new" in a chat goes away once you are at the end or see the newest message.
- iPhone: decisions answered, read or closed on another device no longer stay on the Desk as open questions, also
  while the phone catches up.
- iPhone: logging out no longer crashes the app.
- Opening a session no longer fails when one of your devices has been away for long: it joins the session when it comes
  back.
- Web: logging in to a big account shows how far it is ("Catching up … of …"), opens before every session is
  joined, and a stuck login stops with a message instead of spinning.
- iPhone: logging in to a big account no longer hangs at "Logging in…": the button shows how far it is, the
  sessions join after you are in, and a stuck login stops with a message.
- Opening Trommi after a long time away: questions answered elsewhere no longer show up as open while it catches
  up, and logging in on a new browser is ready in seconds instead of a minute.
- Web: the number keys 1, 2, 3 … switch the desk without opening the desk menu, which then blocked the next key.
- A device that missed the keys for older content now asks your other devices for them, even weeks later.
- Changing your password, Emergency Kit or passkeys now asks for proof that you know a current one:
  being logged in is not enough.
- A phone added to a running session could show the agent's raw id instead of its name.
- The iPhone app could crash while sending.
- After logging out, the browser kept the key that had protected its local data.
- Every agent session showed "hears you on its next step" (and later "not listening") even when it heard the board at once.
- Codex no longer tries to start Trommi through a script when its tools are missing; it tells you to restart Codex.
- The web app on phones: Settings pages fit a small screen (password, passkeys, the device fingerprint shown whole),
  the ••• of a chat is no longer under the page corner, the tab bar is solid and stands beside the page in landscape,
  nothing sits in the home-indicator strip, fields no longer make iPhone zoom in, long answers stay inside their
  buttons, the invite commands break at sensible places, and Artifacts has its heading again.
- A note of only a pasted picture, folded right after the paste, is no longer thrown away.

## 10 October 2026

Trommi now runs on one encryption core built on OpenMLS, the same code on the web, the iPhone and the connector.
Installing the connector is one line, and a new demo room lets you look around before signing up.

![The Desk of the demo room](app/web/public/changelog/2026-10-10.webp)

**New**
- Trommi now runs on one Rust core built on OpenMLS, the same code in the web app, the iPhone app
  and the connector. Everything on your board stays end-to-end encrypted.
- One line installs the connector for Claude Code and Codex: a single signed program, no Node needed.
- Inviting an agent takes three short steps, each line ready to copy.
- Sign in with your e-mail or your account id; "All my devices are lost" restores your account
  from the Emergency Kit.
- Every release is signed, and every account starts with a desk of its own.
- A new demo room with two desks to look around in before you sign up.
- Readable addresses: `/chat/<id>` and `/artifact/<id>`.

**Better**
- "New Agent…" makes a fresh link each time; the previous unused link stops working.
- Logging out never hangs, even on a slow connection.

**Fixed**
- The invite page showed the same short link for every agent.
- A corner note with only pictures could not be sent.
- Deleting a session failed with "ids is not defined".
- The demo's Desk and Scribble Board no longer spin forever.

## 9 October 2026

Desks get goals of their own, the connector runs on macOS, and the iPhone keeps what waits for you on the Lock
Screen with Live Activities.

![A desk's goals under the greeting, in the demo room](app/web/public/changelog/2026-10-09.webp)

**New**
- Live Activities on the iPhone keep what waits for you in view; pushes carry the card's title.
- Links to Trommi open in the iPhone app.
- The connector runs on macOS too and keeps its key in the Keychain.
- Write goals for a desk; they stand under the greeting.
- Duck it and Blitz work on info cards too.

**Better**
- "Off your mind" is a plain feed of what is done; "Archive" replaces the tick box.
- Copying an artifact's link shares it for 30 days, without a dialog.
- A calmer iPhone app: a chat list like Messages, floating glass buttons, attachments inside the composer.
- Dark mode keeps dark ink on the paper and quiet frames.

**Fixed**
- Long strokes on the Scribble Board no longer slow down the pen.

## 8 October 2026

The iPhone app arrives: the Desk, chats, notes and the Scribble Board, with push notifications and sharing from
other apps.

**New**
- The iPhone app: the Desk, chats, notes and the Scribble Board (with Apple Pencil),
  signed in by QR code or with e-mail and password.
- Push notifications on the iPhone, and a choice per device: all, only knocking, or none.
- Share from any iPhone app into Trommi; it arrives as a note.
- Artifacts: media and pages together in one place.
- Light, Dark or System theme.
- Each option on a card can carry its own picture.

**Better**
- The app starts faster and stays smooth in big rooms; the heavy work runs in the background.
- On a phone the web app has a menu pill and a tab bar instead of the sidebar.
- "Off your mind" gathers finished and handed-out cards in one list.
- Content from a newer Trommi shows a calm placeholder in an older app instead of breaking it.

**Fixed**
- Keys no longer stop working on the Desk after a visit to the Scribble Board.
- Nothing scrolls sideways on a phone.

## 7 October 2026

Calmer on a phone and quicker to answer: a drawer for the sidebar, six emoji to connect an agent, and Blitz for
rapid answers.

**New**
- On a phone the sidebar is a drawer.
- Compare six emoji when you connect an agent, instead of picking a number.
- Delete a session from its menu.
- A third pile on the Desk for links and pages.
- Full screen for a card, and a tracing sheet to draw over the whole card.
- Read earlier versions of a card right in its thread.

**Better**
- Rapid fire is now Blitz, with a lightning bolt.
- "Got it" instead of "Acknowledge" everywhere.
- Tap targets on a phone are a finger's size.
- The advised option leads a row; every answer tile has the same size.

## 6 October 2026

Hand a session to a new agent, answer everything at once with Duck it, and put a card off with Later.

**New**
- Continue a session: hand it to a new agent, with or without its history.
- Duck it: answer every open decision on the Desk at once with "whatever you think".
- A greeting at the top of the Desk.
- Drag the corner note onto the Scribble Board and leave it there as a sticky.
- Later is a tag under the card: pull it down to put the card off.

**Better**
- Inviting an agent is a small clipboard with the command to copy.
- Cards are as long as their content, and the page scrolls.
- Arrow keys walk a card's answers.
- The demo speaks English.

**Fixed**
- Two connectors in one folder no longer fight over the key: it follows the session in use.

## 5 October 2026

Permission prompts from Claude Code reach the board, and each desk shows only its own sessions.

**New**
- Permission prompts from Claude Code reach the board, and leave it once answered in the terminal.
- When auto mode blocks a tool, the session says so in one quiet line.
- Move a session to another desk from its own page.
- The page turns from its top-right corner between the Desk and the Scribble Board.

**Better**
- A desk shows only what its own sessions hold.
- The note is back as a yellow sticky in the bottom-right corner.

## 4 October 2026

End-to-end encryption: the hub keeps only sealed messages. Accounts, pairing by QR code, the Scribble Board and
a redesigned Desk.

**New**
- End-to-end encryption: the hub stores only sealed messages, and only your devices can read them.
- Accounts with e-mail and password, an Emergency Kit, and login on a new device.
- Pair a device by QR code.
- The connector as a Claude Code plugin, set up on any machine with one command.
- Share a file with someone outside your room through a private link.
- Videos on cards.
- The Scribble Board, notes with attachments and a media gallery.
- Agents can open helper sessions of their own.

**Better**
- The Desk redesigned: drawing, urgency, teaser, pictures and square answer tiles in one row;
  finished items move to a single pile.
- Infos sit apart from the questions as a short checklist.
- Switch desks with the keys 1 to 9.
- Several tabs of the app work side by side.

**Fixed**
- The phone no longer loses its login.
- Agents keep working through a hub restart.

## 3 October 2026

Push notifications.

**New**
- Push notifications and the bell icon.

## 2 October 2026

The first board: your agents ask, you answer with a tap.

**New**
- The first board: an inbox of questions from your agents, with sessions, a canvas and dictation.
- Answer tiles, the agent's advice circled by hand, and questions agents can revise or merge.
- Agents can publish pages and files to the board.
