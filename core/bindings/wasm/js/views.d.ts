// Written by tests/bindings/views.mjs from core/client/src/views/mod.rs: do not edit. The view model the core sends (Effect::Render)
// and the parts of it a shell sends back (Event::Focus). A field marked ? is left out when it is empty or none.

/** A sentence of the core: `key` names it, `args` fill it, `en` is it in English. */
export interface Text {
  key: string
  args?: Record<string, string>
  en: string
}

/** Where the app stands, for the whole window. */
export type Phase = 'starting' | 'signed_out' | 'opening' | 'live' | 'offline' | 'stopped'

/** What the person looks at. Sent by the shell as `Event::Focus`. */
export interface Focus {
  screen: Screen
  /** The window (or app) is in front and visible. */
  visible: boolean
  /** The team in view: a team's id; none when none was chosen, or one that is gone: the first team. */
  team?: string
  /** The whiteboard is kept beside the screen (under the turned corner of a group, chat or card page): any value
   *  (a board id or team id of before is read the same). There is one sheet, the board of "All desks" (spec 10.1);
   *  `Screen::Scribble` shows it too. The core reads it and sends `ViewModel.board`. */
  board?: string
  /** The wide sidebar is folded to a rail (this device's, the shell keeps it): its rows carry no counts and no
   *  signs but one quiet dot on a chat that needs the person (`SessionRowView.signal`). */
  rail_folded: boolean
  /** The teams unfolded in the wide sidebar, their chats under their stack (`TeamStackView.open`). This device's:
   *  the shell keeps them. */
  open_teams?: string[]
  /** The words searched for among every team's chats (the phone's Search tab, `SidebarView.found`); none or empty:
   *  no search. */
  chats_query?: string
  /** The calm chat (the iPhone and the web send it): a session's page folds a run of card events into one row of
   *  chips (`kind` "events"), leaves the helper list out and says "and 2 more" beside the working line. A shell
   *  that draws the full talk (Linux) leaves it false. */
  calm: boolean
  /** A card's page is a step of one chat's Blitz (its session key or short id): its way on goes only through that
   *  chat's open cards (`SessionView.pinned`'s order), and when none is left back to the chat. None: the group
   *  page's (or, with `walk_all`, every group's). */
  walk_chat?: string
  /** A card's page is a step of Blitz over every group (`GroupView.blitz`, `SidebarView.global_blitz`): its way on
   *  goes through every group's open cards in their order, not only the group page's. */
  walk_all: boolean
}

export type Screen =
  /** The page of the group in view (`ViewModel.group`). (`desk` is its name of before.) */
  | { screen: 'group' }
  | { screen: 'session'; session_id: string }
  | { screen: 'card'; card_id: string }
  | { screen: 'notes' }
  /** The page Off your mind, the complete list, with the search's words (empty: all). */
  | { screen: 'off'; query: string }
  /** The whiteboard: the one sheet of the person, the board of "All desks" (spec 10.1). The group boards of before
   *  are no longer shown (their items stay on the hub). A `board_id` an older shell sends is ignored. */
  | { screen: 'scribble' }
  /** The gallery of every Artifact the agents published, with its filter: "" every one, "media" the pictures,
   *  videos and files, "pages" the pages. */
  | { screen: 'artifacts'; kind?: string }
  /** The page Knowledge of the team in view, with the search's words, its filter ("" every entry, "new",
   *  "pinned", "forgotten") and the entry open (its history shows). */
  | { screen: 'knowledge'; query?: string; filter?: string; open?: string }
  /** The page Wakeups of the team in view: when its agents start again on their own. */
  | { screen: 'wakeups' }
  /** Settings: its list (`page` ""), or one of its pages ("agents", "teams", "devices", "account", "theme",
   *  "data", "security"). */
  | { screen: 'settings'; page?: string }

/** Everything the shell draws. Serialised, this is what the conformance scripts compare. */
export interface ViewModel {
  /** Counts the deltas folded in; a delta whose `base` is not this is out of order (a shell bug). */
  rev: number
  phase: Phase
  /** One line on where the app stands when it is not plainly live ("Catching up", "Offline: retry in 30 s"). */
  status?: Text
  sidebar: SidebarView
  group: GroupView
  sessions?: Record<string, SessionView>
  cards?: Record<string, CardView>
  /** The page Off your mind, while it is in view (`Screen::Off`). */
  off?: OffListView
  notes: NotesView
  settings: SettingsView
  /** Signing in, joining, the emoji to compare, the Emergency Kit to keep, self-join asks. */
  account: AccountView
  /** Files the views name (attachments, pictures), by id: uploading, ready, failed. Their bytes, when loaded,
   *  came as `Effect::FileReady` under the same id. */
  files?: Record<string, FileView>
  /** The Scribble Board in view (`Focus.board`, `Screen::Scribble`); none when no board is shown. */
  board?: BoardView
  /** The gallery of Artifacts, while it is in view (`Screen::Artifacts`), the newest first. */
  artifacts?: ArtifactView[]
  /** The gallery's filter and its words, while `Screen::Artifacts` is in view (`artifacts` then holds what the
   *  filter lets through); none else. */
  gallery?: GalleryView
  /** Every team's Scribble Board small, in the menu's order, while a page shows them (Settings, the boards of All
   *  teams); none else. */
  thumbs?: ThumbView[]
  /** The page Knowledge of the team in view, while it is in view (`Screen::Knowledge`); none else. */
  knowledge?: KnowledgeView
  /** The page Wakeups of the team in view, while it is in view (`Screen::Wakeups`); none else. */
  wakeups?: WakeupsView
  /** Sealed dictation (spec/v1.md 18, `crate::dictation`): the microphone of the chat field and its words. */
  dictation: DictationView
  toasts?: Toast[]
}

/** One change of the view model, from `base` to `base + 1`. */
export interface ViewDelta {
  base: number
  parts: ViewPart[]
}

export type ViewPart =
  /** Everything, replacing what the shell had (the first render, after a restart of the core). */
  | { part: 'whole'; model: ViewModel }
  | { part: 'phase'; phase: Phase; status: Text | null }
  | { part: 'sidebar'; view: SidebarView }
  | { part: 'group'; view: GroupView }
  | { part: 'session'; id: string; view: SessionView | null }
  | { part: 'card'; id: string; view: CardView | null }
  /** The page Off your mind; none when it is left. */
  | { part: 'off'; view: OffListView | null }
  | { part: 'notes'; view: NotesView }
  | { part: 'settings'; view: SettingsView }
  | { part: 'account'; view: AccountView }
  | { part: 'file'; id: string; view: FileView | null }
  /** The Scribble Board in view, whole; none when it is left. */
  | { part: 'board'; view: BoardView | null }
  /** The gallery of Artifacts, whole; none when it is left. */
  | { part: 'artifacts'; view: ArtifactView[] | null }
  /** The gallery's filter and words, whole; none when it is left. */
  | { part: 'gallery'; view: GalleryView | null }
  /** The teams' boards small, whole; none when no page shows them. */
  | { part: 'thumbs'; view: ThumbView[] | null }
  /** The page Knowledge, whole; none when it is left. */
  | { part: 'knowledge'; view: KnowledgeView | null }
  /** The page Wakeups, whole; none when it is left. */
  | { part: 'wakeups'; view: WakeupsView | null }
  /** Sealed dictation, whole. */
  | { part: 'dictation'; view: DictationView }
  | { part: 'toasts'; toasts: Toast[] }

/** Edits of a long list, by position: at `at`, `gone` rows leave and `rows` come. */
export interface RowEdit<T> {
  at: number
  gone: number
  rows: T[]
}

export type Rows<T> =
  | { rows: 'whole'; items: T[] }
  | { rows: 'edits'; edits: RowEdit<T>[] }

/** The sidebar (the phone: the menu's teams, the list of chats, the tab pill's numbers): the teams, and one row per
 *  session of the team in view, in the order they are drawn. */
export interface SidebarView {
  /** The teams, in the menu's order. A room without teams has one: "Personal". */
  teams: TeamTab[]
  /** "New chat with …" under the + at the top (the phone: + at Chats): the agents that start a new chat when asked (a
   *  chat agent such as Hermes says so in its profile, `chats`), each with its name and drawing. Empty: no such agent. */
  new_chat?: NewChatView[]
  /** The person's picture where none is uploaded: the account e-mail's first letter, upper case (the phone's top
   *  left, the sidebar's foot); empty until the account is read: the shell draws a plain figure. */
  me: string
  /** The sessions of the team in view, one row each, in drawing order: a main, then its helpers. */
  sessions: SessionRowView[]
  /** The number on the app icon and the tab: the open questions of the group in view (no infos, nothing that is
   *  with its agent). Never drawn red. */
  badge: number
  /** The team in view: its id. */
  team: string
  /** Where the person is, for the head and the team switch: the team's name. */
  place: Text
  /** The open questions of the group in view (its lamp is lit while there are some). */
  waiting: number
  /** How many of them knock. */
  knocking: number
  /** How many rows of the team in view are stopped (the red hand). */
  stopped: number
  /** How many of them are at work. */
  working: number
  /** How many of his answers the sessions of the team in view have not picked up. */
  unheard: number
  /** Every team as a stack, in the menu's order (the wide sidebar): folded, its head over a fan of its sessions'
   *  card edges; open, its chats under it. The phone draws `sessions` instead. */
  stacks?: TeamStackView[]
  /** The sidebar is folded to a rail (`Focus.rail_folded`): its rows and stacks carry no counts and no signs but
   *  one quiet dot (`SessionRowView.signal` "waiting" or "stopped"). */
  folded: boolean
  /** Under a search (`Focus.chats_query`): every team with a chat whose name or teaser has the words, with those
   *  chats, in the menu's order. */
  found?: ChatGroupView[]
  /** Under a search that finds no chat: so it says. */
  found_none?: Text
  /** The bolt at the phone's top right: Blitz over every team's open cards, whatever team is in view. None while
   *  nothing waits on any team. */
  global_blitz?: GlobalBlitzView
  /** The chats the person put on no team, one row each, in drawing order: loose at the top of the list, with no head
   *  over them, alongside the teams (the phone's folders, the wide sidebar's stacks), never inside one. Their
   *  `team_id` is empty. They have no group page: their cards stand in the chat and in Blitz. */
  root?: SessionRowView[]
  /** Where an agent invited now can stand ("Copy Invite Link"): "No team", then the teams in the menu's order; the
   *  team in view is `chosen`; in a room without teams only "No team", chosen. */
  places?: TeamPlaceView[]
}

/** A place a chat can stand: a team, or no team (`team_id` none: at the top of the list). The action it goes with
 *  takes `team_id` as it is (null for no team): `move_session`, the account step `invite_agent`'s `team`. */
export interface TeamPlaceView {
  /** The team's id; none for no team. */
  team_id?: string
  /** "No team", or the team's name. */
  name: Text
  /** The place taken when the person picks none. */
  chosen: boolean
}

/** Blitz over every group (`SidebarView.global_blitz`, `GroupView.blitz`): the only view of everything. Pressed, the
 *  shell opens its first card with `Focus.walk_all`; the card's page walks on through every group's open cards. */
export interface GlobalBlitzView {
  /** The first open card (or request) of every group, in their order. */
  card: string
  /** What it walks: "Blitz: 3 open cards", with `n`. */
  says: Text
  /** Where it walks, small beside its number: "all groups". */
  scope: Text
}

/** A team in the search's list of chats: its name as a plain section title over the chats found. */
export interface ChatGroupView {
  id: string
  name: Text
  /** Its chats that have the words, one row each, in drawing order. */
  rows: SessionRowView[]
}

/** A team in the wide sidebar: its head (a click opens its group page) over its sessions, folded to a stack or open. */
export interface TeamStackView {
  id: string
  name: Text
  /** The group in view. */
  selected: boolean
  /** Open on this device (`Focus.open_teams`): its chats stand under its head; folded, they lie as its stack. */
  open: boolean
  /** The open questions of the team's sessions. */
  waiting: number
  /** Its open cards, infos too, and its requests: what its group page lists. The calm grey count (0 on the rail). */
  cards: number
  /** One of them knocks. */
  knocks: boolean
  /** What the head says to a screen reader: "3 agents, 2 questions open". */
  says: Text
  /** A card edge per session (at most [`STACK_EDGES`]), in the rows' order. What knocks is said by the head's
   *  number, not by an edge. */
  edges?: StackEdgeView[]
  /** Its picture, as a group's: up to three of its sessions' own marks, overlapping, the crowned one first, then in
   *  the rows' order; none for a team without sessions (a quiet placeholder). */
  faces?: StackEdgeView[]
  /** Its sessions, one row each, in drawing order (connected first); drawn while open. */
  sessions?: SessionRowView[]
  /** What was said last in any of its chats, that chat's name before it ("Fernly Website: Only the Safari test is
   *  left."): the phone's line under the team's name. None while this device holds nothing said there. */
  last?: LastSaidView
}

/** A session in a team's stack: its card edge, or its face in the team's picture. */
export interface StackEdgeView {
  /** The session's key (its row's id). */
  id: string
  /** What draws it (its row's mark): the edge takes its colour. */
  mark: string
}

export interface TeamTab {
  id: string
  name: Text
  /** The open questions of the team's sessions. */
  waiting: number
  /** Its open cards, infos too, and its requests: what its group page lists. */
  cards: number
  /** It is the group in view. */
  selected: boolean
  /** One of them knocks. */
  knocks: boolean
}

/** One session in the sidebar. */
export interface SessionRowView {
  /** The session's key in the model (`Screen::Session`, the actions' `session_id`, `ViewModel.sessions`). */
  id: string
  name: Text
  /** "live", "oncall", "asleep", "cut", "gone" (core/client/src/status.rs). */
  link: string
  /** The red hand. */
  blocked: boolean
  /** How many of his answers it has not picked up for two minutes. */
  unheard: number
  /** Its short id on the board: its address (`/chat/<slug>`). */
  slug: string
  /** What draws it: always a face, "draw:<face>" (`faces::mark`: the icon the person or the agent chose, else the face
   *  its name fits, else the one its id picks). */
  mark: string
  /** The team it stands on. */
  team_id: string
  online: boolean
  /** Connected and at work (a status line of it shows "working"). */
  working: boolean
  /** A main session: one that says so. */
  main: boolean
  /** The crowned session of its team (the one a note goes to). */
  crowned: boolean
  /** Its helpers (its agent's subagents), small under its name: "3 helpers" with a dot each; none without. */
  helpers?: HelperDotsView
  /** Its open questions: what waits for the person on its group page (no infos, nothing that is with its agent). */
  open: number
  /** Its open cards, infos too, and its requests: what Blitz walks of it. The calm grey count beside its name. */
  cards: number
  /** One of them knocks. */
  knocks: boolean
  /** Why it is stopped, in words. */
  hand?: HandView
  /** The small line under its name: its link when it does not hear at once, else the answers it has not picked up. */
  cap?: CapView
  /** Not connected: it stands under "Disconnected". */
  away: boolean
  /** The part of the list a row may be dragged within (its team, connected or not). */
  group: string
  /** What its badge says to a screen reader ("Working, 2 questions open"); none without a badge. */
  says?: Text
  /** What was said last in its conversation (his words with "You: "), or the question it put to him last: the
   *  Chat list's teaser; none while this device holds nothing said there. */
  last?: LastSaidView
  /** Its agent said something this device has not shown yet (the chat was not open since): the Chat list's dot. */
  unread: boolean
  /** The one quiet sign of a calm chat row: "stopped" (the red hand), "waiting" (a question waits for him),
   *  "unread" (new words), or "" for none. */
  signal: string
}

/** What a conversation said last, for its row in the Chat list. */
export interface LastSaidView {
  /** Its first words as one plain line (200 characters at most): "You: …" for his own. */
  text: string
  at_ms: number
  /** When, as a chat list says it: "14:05" today, "Yesterday", a weekday within the week, "3 Oct", "03.10.25". */
  when: Text
}

/** A session's helpers in its sidebar row: how many, and a dot each. */
export interface HelperDotsView {
  /** "1 helper", "3 helpers". */
  says: Text
  /** One dot per helper, in order: filled while that helper works. */
  working: boolean[]
}

/** A session's helpers on its page: a short list, name and state, no way into a chat of their own. */
export interface HelperListView {
  /** "3 helpers". */
  head: Text
  /** "1 working · their cards come with this session". */
  sub: Text
  rows: HelperRowView[]
}

/** One helper of a session's page. */
export interface HelperRowView {
  name: string
  working: boolean
  /** "Working · 1 open question", "1 open question", "Done". */
  says: Text
}

/** Why a session is stopped: the red hand. */
export interface HandView {
  /** "cut", "offline", "permission", "blocking". */
  why: string
  text: Text
  since?: number
}

/** The small line under a session's name. */
export interface CapView {
  /** "oncall", "asleep", "cut", "gone", or "unheard" (it hears at once and has answers to pick up). */
  kind: string
  /** Two or three words: "cut off · 14:05", "1 answer waits". */
  word: Text
  /** The tooltip: "Cut off since 14:05". */
  short: Text
  /** The whole sentence about the session, for a screen reader; none for "unheard". */
  line?: Text
}

/** A session's link in words: whether it hears the person, and what to do when it does not. */
export interface LinkWords {
  /** "live", "oncall", "asleep", "cut", "gone". */
  state: string
  /** Since when it is cut off or gone; for "oncall" and "asleep" its last step. */
  since?: number
  /** Two or three words under its name ("not listening · 12 min"); none while it hears at once. */
  word?: Text
  /** The same as a short sentence ("Not listening for 12 min"). */
  short?: Text
  /** The whole sentence about the session. */
  line: Text
  /** The way out. */
  fix?: FixView
}

/** What to do about a link: a sentence, and a line to type (empty when there is none). */
export interface FixView {
  say: Text
  code: string
}

/** The page of the group in view (a team): its head, and its open cards in one block per chat, the chat with the
 *  urgent question first. A block's head is the way into its chat; there is no filter (the owner, 28 October
 *  2026: "Nach Chat gruppiert"). There is no page of every group: Blitz (`blitz`) is the only view of everything. */
export interface GroupView {
  /** Card and request ids in the page's order: block after block, in each its questions, then what is only to read.
   *  A card's page opened from here walks on in this order. */
  stack: string[]
  /** What stands in the list's place when nothing is open at all. */
  empty?: GroupEmptyView
  /** "invite" (no session stands here yet: invite the first agent), "reading" (none yet, and the room is still
   *  being read), "group". */
  mode: string
  /** How far the reading of the room's past is ("Reading your history: 12,400 of 27,000"), while it is read. */
  reading?: Text
  /** The group's name and its ⋯ menu; none in a room without teams and chats (a new account). */
  head?: GroupHeadView
  /** The team's goals under the head. */
  goals?: GoalsView
  /** One quiet line under the head over the blocks: "4 wait for you · 3 to read"; none while nothing is open. */
  summary?: Text
  /** The open cards, one block per chat that has some: a chat whose question knocks or blocks first, then the chats
   *  with a question, then those with only something to read, each rank in the sidebar's order. */
  blocks?: ChatBlockView[]
  /** The open questions of the group (no infos): the number in the window's title. */
  waiting: number
  /** Blitz over every group's open cards ("Blitz: 14 open cards", "all groups"); none while nothing is open
   *  anywhere. */
  blitz?: GlobalBlitzView
  /** The duck for all beside Blitz: what one press leaves to the agents (a decision) or reads (an info) of the
   *  cards on this page, after one question; none when nothing of that waits here. */
  duck?: DuckAllView
  /** The sessions of the group that are cut off, above the list: one line each, with the step in its terminal. */
  slip?: SlipView[]
}

/** The open cards of one chat on the group page: its head (the chat's face and name, how many of its questions wait,
 *  whether it is at work, "Open chat ›"), which is the link into the chat, then its cards. */
export interface ChatBlockView {
  /** The chat's session key (`Screen::Session`). */
  session_id: string
  /** The chat's short id, for its address (`/chat/<slug>`); empty when it has none (the key serves then). */
  slug: string
  name: Text
  /** What draws the chat (`SessionRowView.mark`). */
  mark: string
  /** "2 open": its questions and requests that wait; none while only something to read is open. */
  open?: Text
  /** "working": the chat is at work now. */
  working?: Text
  /** "Open chat ›" at the head's end. */
  open_chat: Text
  /** What the head says to a screen reader: "socom: 2 open. Open the chat". */
  says: Text
  /** Its questions and approval requests, in the walk's order. */
  questions?: DeskRowView[]
  /** What is only to read (infos, a card not read here yet, one of a newer Trommi), drawn smaller, below them. */
  reads?: DeskRowView[]
}

/** The group page when nothing is open: a calm word, who is at work, where else something waits, and what the person
 *  answered here lately. */
export interface GroupEmptyView {
  /** "Nothing waits for you in Personal." (while the room is still read: "Catching up: …"). */
  title: Text
  /** "trommi and release are at work. What they ask lands here." */
  line?: Text
  /** Another group where a question waits: "1 question waits in Valiido"; none when nothing waits anywhere else. */
  elsewhere?: ElsewhereView
  /** The heading over `lately`: "You answered lately"; none while it is empty. */
  lately_says?: Text
  /** The questions of this group the person answered last, the newest first (up to three). */
  lately?: AnsweredView[]
}

/** Another group where questions wait (`GroupEmptyView.elsewhere`): pressed, that group comes into view. */
export interface ElsewhereView {
  /** The team's id (`Focus.team`). */
  team_id: string
  /** The team's picture: its first chat's mark; empty when it has none. */
  mark: string
  /** "1 question waits in Valiido", the team's name as `name`. */
  says: Text
}

/** A question the person answered (`GroupEmptyView.lately`): its chat, its title, what he chose, how long ago. */
export interface AnsweredView {
  /** The card (`Screen::Card`). */
  id: string
  /** Its chat: its session key, short id, name and mark. */
  session_id: string
  slug: string
  name: Text
  mark: string
  title: string
  /** What he chose: the options' labels ("Ship today"), "I don’t give a duck". */
  choice: Text
  /** When he answered. */
  answered_ms: number
  /** How long ago, short ("2h"). */
  age: Text
}

/** The head of a group page: its name and its ⋯ menu (its chats stand in the sidebar, not here again). */
export interface GroupHeadView {
  /** The team's id. */
  team_id: string
  name: Text
  /** The ⋯ menu, in order: "knowledge", "wakeups", "board" (the Scribble Board), then "rename" (none in a room
   *  without teams). */
  menu?: GroupMenuItem[]
  /** What the ⋯ says to a screen reader: "Fernly: knowledge, wakeups, Scribble Board, rename". */
  menu_says: Text
}

/** One entry of a group's ⋯ menu. */
export interface GroupMenuItem {
  /** "knowledge", "wakeups", "board" or "rename". */
  key: string
  /** Its words: "Knowledge", "Wakeups", "Scribble Board", "Rename group". */
  label: Text
  /** How many, small at its end: "7 entries", "7 entries · 2 new", "3 set"; none where there is nothing to count. */
  count?: Text
  /** Where it goes: `Screen::Knowledge`, `Screen::Wakeups`; none for "rename" (the shell asks
   *  for the new name and sends `Action::RenameTeam`). */
  opens?: Screen
  /** Below a line of its own (the last group of entries: "rename"). */
  apart: boolean
}

/** The duck for all: the cards it clears (`Action::Batch` with way "sweep") and its words (the web's `duckAll`). */
export interface DuckAllView {
  /** The cards it clears, in the page's order: the open decisions (left to their agents) and what is only to
   *  read (closed as read); never a request, nothing not read on this device yet. */
  card_ids: string[]
  /** The button's name: "I don’t give a duck: for all 3 open cards". */
  tip: Text
  /** Its short name where its question follows at once (the iPhone's ⋯ menu): "IDGAD". */
  short: Text
  /** How many of which, for a screen reader: "2 decisions go to the agents, 1 card to read is closed". */
  what: Text
  /** The one question it asks: "Duck all 3?"; only cards to read: "Close all 2 as read?". */
  ask: Text
  /** The yes: "Yes, duck them all", "Yes, all read". */
  yes: Text
  /** The quiet way out: "Cancel". */
  no: Text
}

/** The gallery's filter (`Screen::Artifacts.kind`) and what its page says. */
export interface GalleryView {
  /** The filter in force: "", "media" or "pages". */
  kind: string
  /** The filter's choices, in order ("All", "Media", "Pages"); none while there is no Artifact at all. */
  kinds?: GalleryKindView[]
  /** The line under the page's title. */
  lead: Text
  /** What the page says when the filter lets nothing through ("No pages yet."); none when it shows something. */
  none?: Text
}

/** One choice of the gallery's filter. */
export interface GalleryKindView {
  /** "", "media" or "pages": what `Screen::Artifacts.kind` takes. */
  key: string
  label: Text
  /** How many it lets through. */
  count: number
  /** It is the one in force. */
  on: boolean
}

/** A put-off or closed card, one small quiet row. */
export interface OffRowView {
  id: string
  /** "later", "done" or "trash". */
  place: string
  /** Its mark: "snooze" (put off), "shred" (shredded: the title struck through), "withdraw" (withdrawn by its
   *  agent), "tick" (done). */
  mark: string
  title: string
  starred: boolean
  /** When it closed (ms); 0 for a put-off card and when no time is known (show none, never the epoch). */
  at_ms: number
  /** A put-off card: until when ("until Tue 07:00", the person's zone). */
  until?: Text
  /** What it is, to a screen reader: "Later", "Done", "Shredded", "Withdrawn". */
  says: Text
}

/** The page Off your mind: every open card that is not waiting on the group page of the team in view, and the ones closed. What runs as live cards, the one that moved
 *  last first; then what is put off, the last put off first; then what is closed, the newest first. Each card in its
 *  place by time, none lifted. Kept to the search's words. */
export interface OffListView {
  /** The search's words as given (`Screen::Off`). */
  query: string
  /** The cards with their agents. */
  works?: OffWorkView[]
  /** The put-off cards, then the closed ones. */
  rest?: OffRowView[]
  /** In the list's place when nothing is shown: "Nothing yet.", "Nothing here has these words.". */
  none?: Text
  /** The line under `rest` when this device holds more of it than the window shows: `rest` is the first
   *  [`board::OFF_PAGE`] rows, a page more for each time the person asked (`Lens::off_window`). */
  more?: MoreView
  /** Older closed cards are on the hub and not on this device yet (history on demand): the line under the list
   *  that fetches the next page. Always none until the engine pages the Done list. */
  older?: Text
  /** A page of older cards is being fetched: the shell shows its quiet "Loading …" line at the end. */
  loading: boolean
  /** The list in its groups, in this order and only those with rows: "Running" (`works`), "Put off" and "Done"
   *  (both from `rest`). Each names its rows; its count is every row of it the search keeps, not only the window. */
  groups?: OffGroupView[]
}

/** One group of the complete list Off your mind. */
export interface OffGroupView {
  /** "running", "later" or "done". */
  key: string
  /** Its heading: "Running", "Put off", "Done". */
  says: Text
  /** How many cards are in it (the search's words kept), shown beside the heading. */
  count: number
  /** The ids of its rows as shown, in order: of `works` for "running", of `rest` for the others. */
  ids: string[]
}

/** More of a long list than its window shows: the line that asks for the next page (an action moves the window,
 *  `Lens::show_more`; where this device does not hold it yet, the engine reads it from the hub). */
export interface MoreView {
  /** Rows this device holds beyond the window. */
  left: number
  /** The hub holds more beyond those (not read on this device yet). */
  hub: boolean
  /** "Earlier messages", "Show more". */
  says: Text
  /** How many pages the window shows after the person asks: the number the action sets. */
  pages: number
}

/** A card with its agent, on the page Off your mind. */
export interface OffWorkView {
  id: string
  title: string
  starred: boolean
  from: FromView
  /** Its status line's words, else "on it". */
  label: Text
  /** When it last moved (ms): the shell's "5 min". */
  at_ms: number
  /** The whole row to a screen reader: "Fix the login, Fernly is on it". */
  says: Text
}

/** The page Wakeups of a team (`Screen::Wakeups`, core/client/src/views/wakeups.rs): every wakeup its agents set
 *  themselves, the next first. */
export interface WakeupsView {
  team_id: string
  team: Text
  /** "Wakeups". */
  title: Text
  count: number
  /** The same in words ("3 wakeups"). */
  summary: Text
  lead: Text
  rows: WakeupRowView[]
  /** What the page says when it lists none. */
  none?: Text
  words: WakeupWordsView
}

/** One wakeup on the page. */
export interface WakeupRowView {
  /** Its id and its session's key: what `Action::WakeupCancel` and `WakeupMove` take. */
  id: string
  session_id: string
  /** The session that set it, and its drawing. */
  who: Text
  mark: string
  /** The helper it is for; none: the agent. */
  helper?: string
  /** What it will do then. */
  note: string
  /** When it runs next ("Today 09:00"), and that moment. */
  next: Text
  at_ms: number
  /** How often ("once", "every day"). */
  how_often: Text
  recurring: boolean
  /** What became of its last run ("Ran today 07:00", "Waited: quiet hours (…)"). */
  last?: Text
  /** What the person asked of it, on its way to the agent's computer ("Cancelling…", "Moving to …"). */
  pending?: Text
}

/** The words of the page Wakeups beside its rows. */
export interface WakeupWordsView {
  cancel: Text
  move_to: Text
  save: Text
  back: Text
  cancel_ask: Text
}

/** Sealed dictation (spec/v1.md 18): what the chat field's microphone shows. While the field is empty its send
 *  button is the microphone (`idle`); a tap starts (`starting`), `recording` runs from `since_ms` (stopped by the
 *  core at `max_ms`), `working` while the recording is sealed and transcribed, `done` with the words to put in the
 *  field (once per `text.id`: the shell answers `Action::Dictate { step: taken }`), `failed` with `problem`.
 *  `off` (or empty): no microphone here: the send button stays. */
export interface DictationView {
  /** `off`, `idle`, `starting`, `recording`, `working`, `done`, `failed`. */
  state: string
  /** The chat field the dictation belongs to (as `DictateStep::Start` named it); empty while idle. Another field
   *  shows itself idle, and the words go only into this one. */
  session: string
  /** When the recording began (the shell's clock), for the running time; 0 else. */
  since_ms: number
  /** The longest recording. */
  max_ms: number
  problem?: Text
  text?: DictatedText
  /** While recording: that only the verified enclave hears it. */
  hint?: Text
  /** Settings: whether this device can dictate at all (a microphone and the verifier, and a room). */
  available: boolean
  /** Settings: the person's switch on this device (on unless switched off). */
  on: boolean
  /** Settings: the dot beside the row: `connected` (green), `checking`, `failed` (red), `off` or `unknown` (grey). */
  status_kind: string
  /** Settings: the row's sentence ("Connected · sealed"). */
  status?: Text
  /** Security: the facts of the sealed connection, in order (Enclave, Model, Attestation, Code, Hub); none while
   *  dictation is off or not available here. */
  facts?: DictationFact[]
  /** Security: the test of the sealed connection: `idle`, `running`, `passed`, `failed`. */
  test_state: string
  /** Security: the test's result in words ("✓ sealed round trip · 1.9 s", or what failed). */
  test_result?: Text
}

/** One fact of the sealed connection (Security): a label, its value, a link where there is one (the release in
 *  Sigstore's log), and a dot (`connected`, `checking`, `failed`, `unknown`; empty: none). */
export interface DictationFact {
  key: string
  label: Text
  value: Text
  href?: string
  dot: string
}

/** Words a dictation brought, to go into the chat field once. */
export interface DictatedText {
  id: string
  text: string
}

/** The page Knowledge of a team (`Screen::Knowledge`, core/client/src/views/knowledge.rs): its entries, pinned first
 *  then the one changed last, as the search and the filter let them through. */
export interface KnowledgeView {
  team_id: string
  /** The team's name. */
  team: Text
  /** "Knowledge". */
  title: Text
  /** How many entries the team holds, all of them. */
  count: number
  /** The same in words ("12 entries"). */
  summary: Text
  /** The line under the title. */
  lead: Text
  /** The search's words in force, trimmed; "" for none. */
  query: string
  /** The filter in force: "" every entry, "pinned" the pinned ones. */
  filter: string
  /** The filter's choices, in order: All, Pinned. */
  filters: KnowledgeFilterView[]
  /** The search field's hint. */
  search: Text
  /** What the search and the filter let through, in order. */
  rows: KnowledgeRowView[]
  /** What the page says when it lists nothing; none when it lists something. */
  none?: Text
  /** The team holds as many entries as it may: no new one (`words.full` says so). */
  full: boolean
  /** A title's and a text's limits, in characters. */
  title_max: number
  text_max: number
  /** The page's other words: its buttons, the editor's hints, the question before a delete. */
  words: KnowledgeWordsView
  /** The kinds an entry can be, in order, for the editor. */
  kinds: KnowledgeKindView[]
}

/** A kind of entry, for the editor's choice. */
export interface KnowledgeKindView {
  key: string
  label: Text
}

/** One choice of the page's filter. */
export interface KnowledgeFilterView {
  /** "" or "pinned": what `Screen::Knowledge.filter` takes. */
  key: string
  label: Text
  count: number
  on: boolean
}

/** One entry on the page. */
export interface KnowledgeRowView {
  /** Its id: what `Action::KnowledgeSave`, `KnowledgePin` and `KnowledgeDelete` take. */
  id: string
  title: string
  text: string
  pinned: boolean
  /** Who brought it and when ("proposed by Claude: Web · Yesterday", "added by you · Today"). */
  by: Text
  /** When it last changed. */
  at_ms: number
  /** What it is ("rule", "decision", "fact", "where") and that in words ("Rule"). */
  kind: string
  kind_label: Text
  /** When it was last said to hold ("confirmed Yesterday"), when that was after it was last written. */
  confirmed?: Text
  /** An agent changed it since the person last looked. */
  new: boolean
  /** Forgotten: it can be brought back (`restore`) until `gone_in` says. */
  forgotten: boolean
  /** The way back from its last change when an agent made it: the change's id for `Action::KnowledgeUndo`. */
  undo?: string
  /** A forgotten entry's way back: the forget's change id for `Action::KnowledgeUndo`. */
  restore?: string
  gone_in?: Text
  /** It is the entry open (`Screen::Knowledge.open`): its history is here. */
  open: boolean
  /** Its changes, the newest first (only while it is open; at most ten). */
  history: KnowledgeLogView[]
}

/** One change of an entry: who did what when ("Codex changed it · Yesterday"), the words it replaced, and the way
 *  back (`Action::KnowledgeUndo` with its id). */
export interface KnowledgeLogView {
  id: string
  says: Text
  /** "Before:" with the title and text it replaced, or "It was new." (both empty). */
  before: Text
  title: string
  text: string
  /** An agent made it (not the person). */
  by_agent: boolean
  at_ms: number
}

/** The words of the page Knowledge beside its entries. */
export interface KnowledgeWordsView {
  add: Text
  edit: Text
  delete: Text
  pin: Text
  unpin: Text
  save: Text
  cancel: Text
  title_hint: Text
  text_hint: Text
  delete_ask: Text
  full: Text
  /** The mark of an entry new to him, the way back from a change, from a forget, and the history's heading. */
  new_mark: Text
  undo: Text
  restore: Text
  history: Text
}

/** A team's goals: a note of the person's, at most twenty lines. */
export interface GoalsView {
  team_id: string
  /** The lines; empty when there are none (the shell shows its faint "Goals…"). */
  lines: string[]
  /** Two lines or more: one toggle folds them to the first line. Whether they are folded is this device's. */
  foldable: boolean
  /** What the toggle says while they are folded ("Show all 4 goals"). */
  show_all?: Text
  /** Folded to the first line on this device (`Lens::goals_folded`; the owner's pick of 13 October 2026: one
   *  toggle, folded shows the first line). Only while `foldable`. */
  folded: boolean
  /** What the toggle says: "Show all 4 goals" while folded, "Show the first goal only" while open; none while not
   *  foldable. */
  toggle?: Text
}

/** A session that is cut off, as its group page says it. */
export interface SlipView {
  session_id: string
  text: Text
  fix: FixView
}

/** One card row (a group page's, a chat's): a question, a request for approval, or something to read. */
export interface DeskRowView {
  id: string
  /** "decision", "info" (to read: also a card not read here yet, and one of a newer Trommi), "request". */
  kind: string
  /** Its body is not read on this device yet: a quiet placeholder. */
  loading: boolean
  /** A card of a newer Trommi: its title where readable, nothing to answer. */
  newer: boolean
  title: string
  /** One or two lines under the title: its teaser, else its body as plain words. */
  words: string
  /** Small words before them: "replaces 2 questions", "revised", "Back from Later", why it is urgent. */
  notes?: Text[]
  /** "low", "normal", "high", "critical". */
  urgency: string
  /** The sign before the title: "block" (blocking, or an approval), "knock" (urgent), "whenever" (low), "". */
  sign: string
  /** What the sign says ("Knock! Blocking"). */
  sign_says?: Text
  starred: boolean
  /** When it was asked. */
  created_ms: number
  /** How long ago, short ("12m", "3h"), on the group page: the small line above the title beside who asked. */
  age?: Text
  /** Who asked; none for a session the list does not show. */
  from?: FromView
  /** On the first row of a run (rows of one session that follow each other): how many rows it has. Else 0. */
  run: number
  /** Its pictures and videos, for the small fan beside the title: the first three. */
  media?: MediaView[]
  /** What the fan says ("Look at 4 pictures and videos"). */
  media_says?: Text
  /** The answers the row itself offers. */
  tiles: TilesView
  /** The ways to put it aside that apply, in order: "later", "reverse", "duck", "shred". */
  ways: string[]
}

/** Who a card comes from. */
export interface FromView {
  session_id: string
  name: string
  /** What draws the session (`SessionRowView::mark`). */
  mark: string
  /** Where it stands, for the line above a card's title ("Team › Session"): its team while there are two or
   *  more, itself. */
  path: string[]
  /** The helper (its agent's subagent) it comes from, for a chip beside the path: "Helper Icons". */
  helper?: Text
}

export interface MediaView {
  /** The file (`ViewModel.files`, `Action::OpenFile`). */
  file_id: string
  /** "image" or "video". */
  kind: string
}

/** The answers a row offers without opening the card. */
export interface TilesView {
  /** "none" (not read on this device yet: nothing to answer), "read" (What?? and Got it), "pair" (two answers, the lead last), "advised" (the advised answer, and
   *  "+N other ways" that opens the card), "choose" (one tile that opens the card). */
  kind: string
  /** The answers of "pair" and "advised", in drawing order. */
  tiles?: TileView[]
  /** "advised": how many other ways there are. */
  others: number
  /** "advised": "3 other ways: open the card"; "choose": "Choose: 4 options, several". */
  says?: Text
}

export interface TileView {
  /** The option's key (`Action::Answer`; "allow" and "deny" of a request: `Action::Verdict`). */
  key: string
  /** The words on the tile; empty for a tile that shows its drawing alone. */
  label: string
  /** Its drawing: "yes" (the lead: thumb up, or the filled tile) or "no". */
  thumb: string
  /** A real yes or no (thumbs); else two named tiles. */
  thumbs: boolean
  lead: boolean
  /** Its agent advises it. */
  advised: boolean
  /** Its words need the small type. */
  small: boolean
  /** `label` is the option's short name (its full one fits no tile). */
  short: boolean
  /** Choosing it settles the card (its agent marked it final). */
  settles: boolean
  /** The tooltip ("Recommended", "Settles it", or the option's full name); none without one. */
  tip?: Text
  /** What it says to a screen reader ("Ship it (settles it)"). */
  says: Text
}

/** An agent that starts a new chat when asked (`SidebarView::new_chat`): the session the ask goes to, its name and
 *  drawing. */
export interface NewChatView {
  session_id: string
  /** Its short id (what draws it with its hue, as its row). */
  slug: string
  name: Text
  mark: string
}

/** One session's page: its heading, its conversation, the status lines of its running work, and what stands above
 *  the field to write in. Made for the session in view (`Lens::session`). */
export interface SessionView {
  id: string
  name: Text
  /** "live", "oncall", "asleep", "cut", "gone"; "" for an archived or removed session. */
  link: string
  blocked: boolean
  /** Talk rows, newest last. */
  talk: TalkRow[]
  /** The lines that show: what waits on him first, then what is worked on, put off, done and quiet, each the one
   *  its agent set last first. The first `status_shown` stand open (the calm chat: a row each under "… is
   *  working"; else the first beside the working dots), and `status_more` for the rest, which a tap opens. */
  status_lines: StatusLineView[]
  /** How many of `status_lines` stand open, from the first: the calm chat up to three, else one. */
  status_shown: number
  /** How many lines stand behind the open ones ("4 more" in the calm chat, else "+4"); none when all stand open. */
  status_more?: Text
  /** "Show fewer": the word that folds the rest away again once open; with `status_more` in the calm chat. */
  status_less?: Text
  /** The chat's quiet line of its next wakeup ("21:12" · "check the tests"); none without one. */
  wake_line?: WakeLineView
  /** The calm chat (`Focus.calm`): the agent at work, drawn at the chat's end as its face drawing itself; a tap
   *  opens the details (the step it is at, its lines, the turn's trail). None while it does not work. */
  working?: WorkingView
  /** Every card of the session, the newest first (its page "Questions"). */
  questions?: CardRefView[]
  /** Every file of the session's cards, answers, Chats and Artifacts (ids of `ViewModel.files`), the newest
   *  first (its page "Files"). */
  files?: string[]
  /** The hub holds older messages than this device has (history on demand: asking for more reads them). */
  has_more: boolean
  /** The line above the first row when more stands before it ("Earlier messages"): what this device holds before
   *  the window, and whether the hub holds older still. `talk` is a window of the newest [`board::TALK_PAGE`] rows,
   *  a page more for each time the person asked (`Lens::talk_window`); none when it shows everything there is. */
  earlier?: MoreView
  /** Its short id on the board (`/chat/<slug>`). */
  slug: string
  /** What draws it (`SessionRowView::mark`). */
  mark: string
  /** Its team; empty for a chat on no team. */
  team_id: string
  /** Where its menu's "Move to…" can take it: the other teams, and "No team" while it is on one (a chat on no
   *  team: the teams). Empty in a room without teams: there is nowhere else. */
  move_to?: TeamPlaceView[]
  main: boolean
  crowned: boolean
  online: boolean
  /** Where it stands: its team (while there are two or more), itself. */
  path?: string[]
  /** The model it runs on, as the heading says it ("Opus 5.5"); "" when it named none. */
  model_label: string
  /** The model's id, for the tooltip. */
  model: string
  /** The dot beside the name. */
  dot: DotView
  /** Why it is stopped (the red hand). */
  hand?: HandView
  /** The quiet hint ("quiet for 24 min"): connected, working, and nothing from it for a while. None while stopped. */
  quiet?: Text
  /** Above the field: why it does not hear and the step in its terminal, or that his last message waits. */
  note?: LinkNoteView
  /** What the field says while it is empty. */
  compose: Text
  /** The answer its agent is writing now, its words so far (spec 7.5): the shells draw it as the agent's message
   *  growing under its face, until the message itself comes. "" while there is none. */
  live_answer?: string
  /** "<name> is working": his word was the last, a short while ago. */
  answering?: Text
  /** "<name> waits on 4 helpers": its turn is over and helpers it started still run (the calm chat draws it where
   *  the face stands, still); none while it works itself or no helper runs. */
  waits_on_helpers?: Text
  /** What it is at, for its heading: "working", "waits on 4 helpers", "ready"; none while it is not connected. */
  activity?: Text
  /** Its questions that wait for the person (decisions and approvals, never a card to read), as their card rows,
   *  in the order they came: what its page "Questions" lists as open. */
  waiting?: DeskRowView[]
  /** The calm chat (`Focus.calm`): its open cards pinned right above the field: its questions, the newest first, then
   *  its cards to read (never a task it still works on); the shells draw them as small cards swiped sideways. The
   *  questions leave the talk; each goes once answered or read. */
  pinned?: DeskRowView[]
  /** How many of them (`waiting`), and the way to the first of them. */
  open: number
  /** "3 open". */
  open_word?: Text
  /** "3 open questions: go to the first one". */
  open_says?: Text
  /** The talk row of the first of them, where it was asked. */
  first_open?: string
  /** An empty conversation: the card of the agent that joined, and three ways to start. */
  empty?: EmptyTalkView
  /** Its agent's account and how much of it is used (the heading's rings: the week and the 5-hour window). None
   *  without a plan or an e-mail. */
  usage?: UsageView
  /** Its helpers (its agent's subagents), above the status lines; none without. */
  helpers?: HelperListView
  /** In the Archive: the More menu offers "Back from the Archive" (`ArchiveSession` with `on: false`). */
  archived: boolean
  /** The tabs of its side panel (a click on its name opens it), in order: "details" always, then "questions",
   *  "helpers" and "files" where it has some. */
  tabs?: PanelTabView[]
}

/** A tab of a session's side panel. */
export interface PanelTabView {
  /** "details", "questions", "helpers", "files". */
  key: string
  label: Text
  /** What it counts beside its label (the questions that wait, the helpers, the files); 0: no number. */
  count: number
}

/** The dot beside a session's name. */
export interface DotView {
  /** Connected (green). */
  on: boolean
  /** How it hears when it does not hear at once: "oncall", "asleep", "cut", "gone"; else "". */
  link: string
  /** The tooltip: "connected", "disconnected", or the link's words. */
  word: Text
  /** For a screen reader: the word, and what it is at. */
  says: Text
}

/** The note above a field: a receipt, the sentence about the session, the way out. */
export interface LinkNoteView {
  /** "cut", "gone", "asleep", "oncall" (the link's state), or "unheard", "sent", "heard" (a receipt alone). */
  tone: string
  receipt?: Text
  line?: Text
  fix?: FixView
}

export interface EmptyTalkView {
  /** Where it runs, quietly under its name ("Opus 5.5"); "" when it said nothing. */
  meta: string
  text: Text
  /** A tap sends one. */
  starters: Text[]
}

/** One row of a conversation. `kind` says what it is; the fields it needs are filled. */
export interface TalkRow {
  id: string
  from_me: boolean
  text: string
  time_ms: number
  /** His message: its session picked it up (false also when its connector writes no receipts). */
  heard: boolean
  /** Not confirmed by the hub yet (an echo). */
  pending: boolean
  /** File ids (`ViewModel.files`). */
  files?: string[]
  /** "user" (his bubble), "agent" (the agent's words, light markdown), "note" (his note, taped on), "event" (one
   *  quiet line: `event`), "ask" (an open question where it was asked: `row`), "work" (a turn's trail: `work`),
   *  "artifact" (something the session published: `published`), "newer" (written by a newer Trommi). */
  kind: string
  /** Its place in the hub's order (an envelope number); 0 for what has none yet. */
  number: number
  /** The line above it when a new day begins there ("Today", "Yesterday", "Tuesday 13 October"). */
  day?: Text
  /** It goes on a run of the same side: no head, close to the row before. */
  cont: boolean
  /** Typed into the session's terminal (his words, sent for him by the agent's connector), or the agent's answer there. */
  terminal: boolean
  /** The card it is about. */
  card?: CardRefView
  event?: EventView
  /** What he did with a card along with these words (handed back, took back, What??): a quiet line above them. */
  deed?: EventView
  row?: DeskRowView
  work?: WorkView
  /** The Artifact it announces (its object id). */
  published?: string
  /** The agent's details, folded under its words. */
  details?: string
  /** The agent's layout under its words (drawn in the sandboxed frame). */
  html?: string
  /** The cards it carries, a chip each. */
  chips?: CardRefView[]
  /** The calm chat (`Focus.calm`): the card these words answer or are about, quoted at the top of the bubble as a
   *  messenger's reply (his answer with what he did to it, the agent's words on it); none without a card it knows. */
  quote?: QuoteView
  /** `kind` "events" (a phone's chat): the card events of a run, a chip each, in order. */
  events?: EventChipView[]
  /** A selection of the Scribble Board (its one file is the picture). */
  scribble: boolean
  /** A note: when it was written. */
  written_ms?: number
  /** On a card's page, the agent's words that answer his What??: that row's id (drawn as one block with it). */
  answers?: string
  /** On a card's page, his What?? that has no answer yet: "Waiting for the explanation." */
  waits?: Text
  /** The helper (its agent's subagent) that wrote it, beside the agent's name; none for the agent's own words. */
  helper?: string
}

/** A card as a row of a conversation names it. */
export interface CardRefView {
  id: string
  /** Its number ("Nr. 12"); 0 when this device does not hold it. */
  number: number
  title: string
}

/** The card a bubble answers or is about (`TalkRow.quote`): a tap opens it. */
export interface QuoteView {
  /** What he did with it ("You handed it back", "You asked for an explanation"), else "About". */
  label: Text
  card: CardRefView
}

/** One card event of a run on a phone (`TalkRow.events`): a small chip that opens its card. */
export interface EventChipView {
  /** The talk row it was. */
  id: string
  /** The event's kind (`EventView.kind`): its sign. */
  kind: string
  /** What the chip says: the card's title, else what happened. */
  text: string
  /** For a screen reader: what happened ("Trommi revised it: Release notes"). */
  says: Text
  card?: CardRefView
}

/** One quiet line for something that happened to a card. */
export interface EventView {
  /** "asked", "info", "revised", "decided", "trusted", "read", "shredded", "reopened", "done", "handed", "snoozed",
   *  "took_back", "handed_back", "explain". */
  kind: string
  /** "New question", "Answered", … */
  label: Text
  /** The card's title, when the words do not say it already. */
  about?: string
  text?: string
  /** What became of the question is said where it was asked: this line only repeats it. */
  echo: boolean
  /** On a card's page: "revised", the version it brought ("Version 3"); "decided", the version he answered when it
   *  was not the newest ("You chose in version 2"). */
  version?: number
  /** "revised" on a card's page: it came after his hand-back ("Version 3, as you asked"). */
  asked: boolean
  /** "decided", "trusted", "shredded": what he wrote along, one line each (“his note”, On Label: “a note”). */
  said?: Text[]
  /** "decided", "trusted", "shredded": the files he sent along (`ViewModel.files`), shown under the deed. */
  files?: string[]
}

/** The agent at work in the calm chat (`SessionView.working`). */
export interface WorkingView {
  /** "trommi is working": what a screen reader says for the face. */
  says: Text
  /** The details' head, live: "Working · Bash · run the tests", or "Working" between steps. No counts, no time. */
  head: Text
  /** The step it is at ("Bash · run the tests"); "" between steps. */
  now?: string
  /** The trail of the turn it works on (the details list it; the talk does not). */
  trail?: WorkView
  /** "12 steps": the quiet way to the trail, at the right of "… is working"; none without a step. */
  steps?: Text
}

/** A turn's trail: what the agent did between his prompt and its answer, as one block. */
export interface WorkView {
  turn: string
  /** "running", "done", "interrupted", "failed". */
  state: string
  /** "Working", "Worked 2 min", "Interrupted after 12 s", "Stopped by an error after 1 h 5 min". */
  head: Text
  /** "7 steps · 1 helper · 2 with errors"; none without any. */
  sum?: Text
  /** While it runs: the step it is at ("Bash · run the tests"). */
  now?: string
  lines: WorkLineView[]
  /** "and 12 more steps, not listed". */
  rest?: Text
  /** How many steps it took so far, listed or not (helpers and words not counted). */
  steps: number
}

export interface WorkLineView {
  id: string
  /** "step", "helper", or "text" (the agent's words between two steps: `text`). */
  kind: string
  /** "running", "ok", "failed", "interrupted"; "" for words. */
  state: string
  /** What its sign says ("done", "exit code 2"). */
  says?: Text
  /** The bold word: the tool, "Helper <name>", or "Step". */
  tool?: string
  /** What it is about ("title · subject"). */
  what?: string
  /** "3 steps" (a helper), "exit code 2" (a failed command). */
  count?: Text
  /** How long it took ("0.3 s", "2 min"). */
  took?: string
  text?: string
  input?: string
  output?: string
}

/** A status line of a session's running work, as its page shows it (core/client/README.md "Status lines"): only
 *  the lines that show are here. */
export interface StatusLineView {
  id: string
  label: string
  /** The helper (its agent's subagent) whose line it is; none for the agent's own. */
  helper?: string
  /** Who works on it, drawn in its own column before the label: the helper's name, else its agent's own name. */
  who: string
  /** How long the helper it stands for runs ("running 50m", "just started"), or "finished" once it ended; drawn
   *  after `detail`; none for a line no helper's trail stands for. */
  ran?: Text
  /** Its grey words: what the helper it stands for is at while it runs (its newest step's own description,
   *  "Polling until the tests finish"), else what its agent wrote. */
  detail: string
  /** "working", "decision", "later", "done" (for ten minutes), "stale" (greyed and still, for a day). */
  state: string
  /** "working", "waits on you", "put off", "done", "no news for a while". */
  says: Text
  /** When its agent last set it. */
  updated_ms: number
  /** How long ago, short: "now", "28m", "3h", "2d". */
  age: Text
  /** The card it stands for: the one it names, else the card with its session whose line it is. */
  card?: CardRefView
  /** "Card": the word of the way to `card`; none without one. */
  link?: Text
}

/** The chat's quiet line of its session's next wakeup (`SessionView.wake_line`): when, short, in its own column, and
 *  its note beside it; a tap opens the page Wakeups. */
export interface WakeLineView {
  /** "21:12" today, "Tue 07:00" within the week, else "3 Nov 07:00". */
  at: Text
  /** What its agent wrote it for. */
  note: string
  /** "and 2 more" when more wakeups wait behind it; none for one alone. */
  more?: Text
  /** The whole line in words, for a screen reader: "Wakes at 21:12 · check the tests, and 2 more". */
  says: Text
}

/** The answer to his What?? on a card's page (`CardView::explained`). */
export interface ExplainedView {
  /** The id of the talk row with the agent's words. */
  id: string
  /** "The explanation is below ↓": a tap goes there. */
  says: Text
}

/** One card's page (a question, something to read, a request for approval): who it comes from, its words, its
 *  answers or what was said, its history, and the comments under it. Made for the card in view (`Lens::card`). */
export interface CardView {
  id: string
  session_id: string | null
  /** "decision", "info", "request", "note". */
  kind: string
  /** "open", "answered", "closed". */
  state: string
  /** 0 low, 1 normal, 2 high, 3 critical. */
  urgency: number
  title: string
  /** Its whole text (markdown). */
  body: string
  /** Its options, in the tiles' order (the advised last). */
  options: OptionView[]
  /** "You chose: Ship it", when he answered (for a screen reader; drawn from `answer`). */
  you_chose?: Text
  /** How many versions stood before this one. */
  history: number
  starred: boolean
  snoozed_until?: number
  /** File ids (`ViewModel.files`): every attachment that is no page of a picture. */
  files?: string[]
  /** Its number, for its address (`/card/<number>`); 0 when unknown. */
  number: number
  /** "Nr. 12". */
  nr: Text
  /** Where it stands: "catching" (settled elsewhere, its outcome on the way), "open", "decided", "shredded", "done". */
  status: string
  /** Who it comes from, above its title: "Team › Main › Helper" (`FromView::path`). */
  from?: FromView
  /** "Knock! Blocking", while it is open and knocks. */
  knock?: Text
  /** The strip above the title while its session works on it. */
  work?: CardWorkView
  /** When it was asked, or last revised. */
  asked_ms: number
  /** Its place in the walk it is a step of ("3 of 7": the group page's, a chat's or Blitz's), with its neighbours. */
  place?: PlaceView
  /** Small words of the quiet line: "replaces 2 questions", "revised", "3 messages below". */
  notes?: Text[]
  /** The text that stands on the card (markdown): its paragraphs; code and tables stand in the comments. */
  text: string
  /** The comments hold more of the text ("More in the comments ↓"). */
  more: boolean
  /** The agent's layout under the text (drawn in the sandboxed frame). */
  html?: string
  /** The options' own paragraphs, in the tiles' order; empty when none says more than its label. */
  sections?: SectionView[]
  /** Several options may be ticked. */
  multiple: boolean
  /** The answers' column: what can be done, or what was said. */
  answer: AnswerView
  /** Under the answers: whether its session hears, and whether it has his answer. */
  note?: LinkNoteView
  /** What the field under the card says while it is empty. */
  ask: Text
  /** His unsent words (the draft kept with the card). */
  draft?: string
  /** The versions that stood before this one, oldest first. */
  versions?: VersionView[]
  /** What the star says ("Star", "Starred"). */
  star_says: Text
  /** What the card does not hold, before the comments: the whole text or its layout, the links of its options,
   *  that he put it off. */
  lead?: LeadView[]
  /** The comments and what happened to the card, oldest first: the window's rows (`earlier`). */
  talk?: TalkRow[]
  /** The agent's words that answer his newest What?? in `talk`: the page scrolls to them when they come and shows
   *  a line pointing down to them; none while nothing answered it, or a new version came after it. */
  explained?: ExplainedView
  /** The hub holds older comments than `talk` has. */
  has_more: boolean
  /** The cards it replaces. */
  merged_from?: CardRefView[]
  /** The card that replaced it. */
  merged_into?: CardRefView
  /** Its pictures, in order (the stage and the small ones under it), each with the option it shows. */
  pictures?: PictureView[]
  /** The line above the comments when more stand before them ("Load earlier (N)", "Earlier comments"): `talk`
   *  is a window of the newest [`board::COMMENTS_PAGE`] rows, a page more for each time the person asked
   *  (`Lens::comments_window`, `MoreOf::Comments`); none when it shows everything there is. */
  earlier?: MoreView
  /** The way on (not while catching up): the next open question of the team in view, or that there is none. Open,
   *  where its page goes at once when the person answers there (`Action::Answer.onward`, the toast with Undo). */
  after?: AfterView
  /** The page's small fixed words (`CardWordsView`). */
  words: CardWordsView
  /** The card's ⋯ menu beside its title, in order (`CardMenuItem`): what can be done with it besides answering.
   *  Always written, an empty list too, so a shell never meets it missing. */
  menu: CardMenuItem[]
  /** The tint of the paper the question stands on: "full" while it waits for him or is settled, "pale" while its
   *  session works on it (the strip above the title, or the card back with the agent for rework). */
  paper: string
}

/** One entry of a card's ⋯ menu. The shell draws it by its key: `prev`, `next` (the questions before and after it in
 *  its walk), `draw` (the pen on the card), `focus` (the page alone in the window), `chat` (its session's
 *  conversation), `copy` (the card as one line), `wake` (back among the open cards now), `shred` (thrown away unanswered). */
export interface CardMenuItem {
  key: string
  label: Text
  /** The keys that do the same ("Shift ↓"); empty when none. */
  keys: string
  /** Only on the card as it stands now: an older version shown read-only leaves it out. */
  now_only: boolean
}

/** A picture of a card, and the option it belongs to (the web's `pictureKeys`, `picturesOf`): a section that names
 *  it decides; else its file name names one option (by key or label, the longest key winning, two equal ones
 *  naming none); else, with as many pictures as options and three or more, they pair in order. */
export interface PictureView {
  /** The file (`ViewModel.files`). */
  file_id: string
  name: string
  /** Its caption, when it says more than its name. */
  title?: string
  /** The line under it in the full-screen viewer: its caption, else the label of the option it shows, else its
   *  name. */
  caption?: string
  width?: number
  height?: number
  /** The page that belongs to it (an attachment id), shown beside it. */
  page?: string
  /** The option it shows: pointing at that option puts it on the stage. */
  key?: string
  /** Every option a section shows it for (several may share one picture); else its `key`. */
  keys?: string[]
}

/** The way on after an answer on a card's page. */
export interface AfterView {
  /** The next open question: the next in the walk's order after this card's place, else the first that waits. */
  next?: CardRefView
  /** "Next question", or "That was the last open question." */
  says: Text
  /** The short word of the one button that takes its place where the answers stood: "Next", or "All done". */
  go: Text
}

/** The small fixed words of a card's calm page (the iPhone): the mark on the advised option and the three side ways
 *  as one quiet row, Undo after an answer, and the folded talk. */
export interface CardWordsView {
  /** "Recommended", beside the option the agent advises. */
  recommended: Text
  /** "Got it", the answer of an info card. */
  got_it: Text
  /** "Duck it": the agent decides (the duck). */
  duck: Text
  /** "What??": back to the agent to explain. */
  what: Text
  /** "Rework": back to the agent to rework, with the comments. */
  rework: Text
  /** "Undo": takes back the answer that stands. */
  undo: Text
  /** "Later", in the card's menu beside the core's own (`CardView.menu`) while it waits and is not put off. */
  later: Text
  /** The sheet to draw on the card (`CardMenuItem` "draw"): "Cancel", and "Attach" (the drawing goes to the field). */
  draw_cancel: Text
  draw_done: Text
  /** What the title pill does, for a screen reader: "Opens the card's menu". */
  menu_hint: Text
  /** "Talk · 3": the card's talk folded into one row; none when it has none. */
  talk?: Text
}

export interface OptionView {
  key: string
  label: string
  recommended: boolean
  /** He chose it (the answer that stands), or ticked it (his draft, while several may be ticked). */
  chosen: boolean
  /** Its line of detail, as plain words. */
  detail?: string
  /** Choosing it settles the card (its agent marked it final). */
  settles: boolean
  /** His note on it: unsent (his draft) while the card is open, else as he sent it. */
  note?: string
  /** The tooltip ("Recommended · Settles it"). */
  tip?: Text
}

/** An option's own paragraph (a card written as sections). */
export interface SectionView {
  key: string
  label: string
  recommended: boolean
  /** What it says beyond its label (markdown); "" when nothing. */
  text: string
  html?: string
}

/** The strip above a card's title while its session works on it. */
export interface CardWorkView {
  session_id: string
  /** What draws the session. */
  mark: string
  /** "Fernly Website is working on it". */
  says: Text
  /** Its status line ("Launch checklist · 14 of 19 done"), else "Reworking the card". */
  words: Text
  /** When that was said. */
  at_ms: number
}

/** A card's place in the walk it is a step of. */
export interface PlaceView {
  at: number
  of: number
  /** "3 of 7". */
  says: Text
  prev?: string
  next?: string
}

/** The answers' column of a card's page. */
export interface AnswerView {
  /** "options" (the card's options answer; a decision also offers the duck, What?? and the reverse card), "read"
   *  (Got it, What??, the reverse card), "request" (Allow, Deny), "said" (what was said, and the way back),
   *  "revising" (handed back), "working" (an info still worked on), "catching", "loading", "newer". */
  mode: string
  /** The one still tile: what was said ("Ship it", "Read", "Shredded"), "In revision", "Being worked on". */
  label?: Text
  /** Under it: "Your note: …", or what the mode means. */
  detail?: Text
  /** The foot: "You chose", "You handed it back". */
  did?: Text
  at_ms?: number
  /** The way back, when there is one: "reopen" (his answer is taken back: `Action::DecideAgain`), "takeback" (his hand-back is). */
  back?: string
  /** His notes on single options, as sent. */
  option_notes?: NotedView[]
  /** "<name> marked this answer as final: nothing follows from it.", when his answer settled the card. */
  settled?: Text
  /** What its agent said when it closed the card ("Done by the agent"). */
  summary?: string
  /** Its agent closed it after his answer: What?? asks about what it did. */
  finished: boolean
  /** What the options say to a screen reader ("Your answer. One tap answers."), or the mode's one quiet sentence. */
  says?: Text
  /** "said": he left it to the agent (the duck): the duck's row is the one marked; the options' `chosen` say the
   *  others. Always sent. */
  ducked: boolean
  /** "said", his answer still on its way while this device's outbox halts on a refusal (`Lens.outbox_blocked`):
   *  "Not sent yet: <code>". The answer stands here only; the agent has not got it. */
  not_sent?: Text
}

export interface NotedView {
  label: string
  note: Text
}

/** A version of a card that stood before. */
export interface VersionView {
  n: number
  at_ms: number
  title: string
  /** What its agent said changed with the version after it; "" without a word. */
  note?: string
  /** The version as it was, read only ("See version 2", "Open version 2 as it was"): its text as the card shows it
   *  (markdown), and its options. Empty while its body is not on this device (`loaded`). */
  text?: string
  options?: OptionView[]
  /** Its body is on this device (history on demand: an old version may be read from the hub when asked for). */
  loaded: boolean
  /** Its pictures as they were, in order (`CardView.pictures`' rule, with that version's options). */
  pictures?: PictureView[]
  /** Its other files (`ViewModel.files`). */
  files?: string[]
}

/** What the card itself does not hold, at the head of its comments (the web's `cardThread` lead), in this order. */
export interface LeadView {
  /** "whole": the whole text, when the card holds only part of it (`CardView.more`), with the agent's layout;
   *  "with": only the agent's layout, when the card holds all the words; "links": the links written into its
   *  options (a tile says them in plain words); "later": he put it off and it waits on Later. */
  kind: string
  /** "The whole text", "With the text", "Links of the options", "You put it off: it waits on “Later”". */
  label: Text
  /** "whole": the text (markdown, Artifact links by their titles). */
  text?: string
  /** "whole", "with": the agent's layout (`CardView.html`). */
  html?: string
  /** "links": each link with the option it is in. */
  links?: OptionLinkView[]
  /** "later": when he put it off. */
  at_ms?: number
}

/** A link an agent wrote into an option's label or detail. */
export interface OptionLinkView {
  key: string
  /** The option's label without the link; its key when nothing else is left. */
  label: string
  url: string
  /** The address as it is shown: without "https://" and "www.". */
  short: string
}

/** The person's notes (the room's, the same on every team) and the one in the corner (`notes.rs`). */
export interface NotesView {
  /** The ids of the notes that show, the last written first (the ids of `rows`). A sent note held for the toast's
   *  Undo shows nowhere. */
  notes: string[]
  /** Those notes, in the same order. */
  rows?: NoteView[]
  /** The note in the corner: the first of them; none when there is none (the corner then makes a new one). */
  corner?: string
  /** The corner's note holds words or files (the tab's note is drawn written). */
  written: boolean
  /** What the corner's sticky says ("Your note: open it", "New note"). */
  says: Text
  /** The crowned session it goes to; none when no session wears the crown here. */
  to?: NoteToView
  /** What the send says ("Send to Fernly"). */
  send_says?: Text
  /** In the send's place without a crown: "Give a session the crown to send" (opens Settings · Agents). */
  no_crown?: Text
}

/** One note. */
export interface NoteView {
  id: string
  text: string
  files?: NoteFileView[]
  created_ms: number
  updated_ms: number
  /** This device's version is not confirmed by the hub yet. */
  pending: boolean
}

/** A file of a note: a picture as a small thumbnail, any other file by its name. */
export interface NoteFileView {
  /** The attachment (`ViewModel.files`, `Action::OpenFile`). */
  file_id: string
  name: string
  media_type: string
  picture: boolean
}

/** A crowned session a note can go to. */
export interface NoteToView {
  session_id: string
  /** Its name; "Main Session" while it has none. */
  name: string
  /** What draws it (`SessionRowView::mark`). */
  mark: string
  team_id: string
  /** Its team's name ("Personal" for one without). */
  team: string
}

/** Settings: what the list's rows say, and the person's devices (`settings.rs`). What only this device knows (the
 *  theme, the Push level) is the shell's. */
export interface SettingsView {
  /** Hub counts and sizes, read only by an authenticated device of this account. */
  data?: DataView
  /** The account's e-mail, for the row Account; none until the account is read (W3). */
  account: string | null
  /** The person's devices that are in, in the order they joined (the agents are under Agents). */
  devices: DeviceRow[]
  /** The person's removed devices, in the order they joined (the row "Removed" and its page). */
  removed?: DeviceRow[]
  /** The devices that never finished signing in: offered for removal together. */
  unfinished?: string[]
  /** "Remove the 2 devices that never finished signing in". */
  remove_unfinished?: Text
  /** The row Agents: "2 agents · 5 sessions"; none without one. */
  agents?: Text
  /** The page Agents: each program on a computer with its sessions, the most sessions first (the web's
   *  `agentsOf`, `agentsMain`). */
  agent_list?: AgentView[]
  /** The line under the page's title: "2 agents on 1 computer · 5 sessions"; none without an agent. */
  agents_lead?: Text
  /** The row Teams: "3 teams"; none in a room without teams. */
  teams?: Text
  /** The row Devices: "2 devices". */
  devices_says?: Text
  /** The room's key epoch (the version line at the list's foot). */
  key_epoch: number
  /** The room's id (hex) and its hub, for the page that proves the room. */
  room?: string
  hub?: string
  /** What each team holds, for the confirm of its delete (its board counted once read on this device). */
  team_counts?: TeamCountsView[]
  /** The teams being deleted, with how far. */
  deleting?: TeamDeleteView[]
  /** Push on this device, where the platform has push. */
  push?: PushView
  /** The choices of Push on this device, in order (whether there is Push at all: `push`). */
  push_choices: PushChoice[]
  /** Log out of this device. */
  logout: LogoutView
  /** The words every team's delete confirm shares. */
  team_delete: TeamDeleteWords
  /** The page Security. */
  security: SecurityView
  /** The account's ways in, once read (`AccountAction::ReadAccount`). */
  login?: LoginView
}

/** Settings → Data: presentation belongs to the core, never the native HTTP executor. */
export interface DataView {
  state: string
  rows: DataRow[]
  says: Text | null
}

export interface DataRow {
  id: string
  label: Text
  value: Text
}

/** Settings → Security: every key that opens the account's data, and the self test, as groups of rows; every word the
 *  core's (the web's `securityMain`, the iPhone's page Security). */
export interface SecurityView {
  /** The line under the page's title. */
  lead: Text
  /** The groups in order: "key", "devices", "agents", "check". */
  sections: SecuritySection[]
}

/** One group of the page Security: its heading, its rows, the lines under it. */
export interface SecuritySection {
  id: string
  label: Text
  rows: SecurityRow[]
  /** The lines under the group, in order. */
  notes?: Text[]
  /** A line said strongly after the notes ("Nobody else, not the hub either: …"). */
  strong?: Text
}

/** One row of the page Security. */
export interface SecurityRow {
  id: string
  title: Text
  /** "this device" beside the title of this device's row. */
  mark?: Text
  /** The key fingerprint (or the account key's head) shown monospaced at the head of the second line; "" none. */
  fingerprint: string
  /** The rest of the second line ("key epoch 88 · https://hub.trommi.com", "added 1 Oct 2026"); none without one. */
  detail?: Text
  /** What it opens, on the right ("everything", "own session", "runs on this device"). */
  says?: Text
  /** Its drawing: "key", "page", "phone", "laptop", "terminal", "tick". */
  icon: string
  /** Where a tap goes: "" nowhere, "kit" (Account → Emergency Kit), "device" (the device `id`), "agents", "proof". */
  open: string
}

/** What a team holds: what its delete takes away. */
export interface TeamCountsView {
  team_id: string
  sessions: number
  helpers: number
  archived: number
  open_cards: number
  done_cards: number
  /** Agent devices that leave the room with it. */
  agents: number
  /** Shapes on its board, pictures and notes among them; none while the board is not read. */
  board?: number
  pictures?: number
  notes?: number
  files: number
  artifacts: number
  goals: number
  /** Entries of its knowledge. */
  knowledge: number
  /** The confirm's title: "Delete Fernly?". */
  title: Text
  /** The confirm's lines, one per kind it holds that is not none, in order (a count not yet known is left out). */
  lines?: TeamDeleteLine[]
}

/** One line of a team's delete confirm: "3 sessions archived". */
export interface TeamDeleteLine {
  /** "sessions", "helpers", "archived", "agents", "open_cards", "done_cards", "board", "files", "goals", "knowledge". */
  key: string
  n: number
  /** "session" or "sessions". */
  word: Text
  /** What happens to them ("archived", "shredded"); none where nothing more is said. */
  rest?: Text
}

/** The words of a team's delete confirm that every team shares. */
export interface TeamDeleteWords {
  /** "This goes for good, on every device:". */
  lead: Text
  /** "Nothing stands on this team: only its name goes." */
  empty: Text
  /** "The server drops the encrypted data within 30 days. …" */
  small: Text
  /** "Hold to Delete". */
  hold: Text
  cancel: Text
  /** "Deleting…". */
  deleting: Text
}

/** Log out of this device: what the page (web) and the confirm (iPhone) ask and say. */
export interface LogoutView {
  /** "Log out" (the menu row, the page's title). */
  title: Text
  /** "Log out of this device? You can log in again with your passkey or your password." */
  ask: Text
  /** "This is your only device. …", or the demo's line; none otherwise. */
  note?: Text
  /** "Not sent yet: 3 changes from this device …": what is still in this device's outbox and goes with it;
   *  none when nothing waits. */
  unsent?: Text
  /** "Everything Trommi keeps on this device is deleted here; …" */
  meta: Text
  /** "Log Out" (the button). */
  go: Text
  cancel: Text
}

/** One choice of Push on this device. */
export interface PushChoice {
  /** "all", "knocking", "off". */
  level: string
  /** "Yes", "Only knocking", "No". */
  title: Text
  /** "every card"; none for "No". */
  detail?: Text
  /** Its drawing: "bell", "knock", "snooze". */
  icon: string
}

/** An Artifact in the gallery: what an agent published. */
export interface ArtifactView {
  id: string
  title: string
  /** "page", "image", "document", …: what the agent named it; "" when it named nothing. */
  kind: string
  /** The session that published it (its key), and its name. */
  session_id?: string
  session: string
  sent_at: number
  /** Its files (ids of `ViewModel.files`), in its order. */
  files?: string[]
}

/** Push on this device. */
export interface PushView {
  /** "off", "all", "knocking". */
  level: string
  /** A change of the level is on its way. */
  busy: boolean
}

/** A team's delete in progress: units done of the total. */
export interface TeamDeleteView {
  team_id: string
  done: number
  total: number
}

/** The account's ways in, for the page Account: what can be changed and how it is proved. */
export interface LoginView {
  email?: string
  /** The account's id as printed on the kit (a UUID). */
  account: string
  has_password: boolean
  /** The kit is salted with the e-mail ("email") or with the id ("id"). */
  kit_form: string
  passkeys?: PasskeyView[]
}

export interface PasskeyView {
  /** base64url: what `RemovePasskey` names. */
  credential_id: string
  created_at: number
  last_used_at?: number
}

/** An agent: a program on one computer ("Claude Code on MacBook Pro"), with its sessions. */
export interface AgentView {
  /** "Claude Code · MacBook Pro": what Remove names (the action's `key`). */
  key: string
  /** "Claude Code", "Codex", "Agent". */
  program: string
  /** The computer's name; "" when it named none. */
  host: string
  /** "Claude Code on MacBook Pro", "Codex on an unnamed computer". */
  name: Text
  /** One of its sessions is connected. */
  online: boolean
  /** "connected", or "away · last seen" (the shell writes the time of `seen_ms` after it). */
  state: Text
  /** When one of its sessions last did something. */
  seen_ms: number
  /** "2 sessions, 1 helper". */
  count: Text
  /** Every session of it, its helpers too. */
  total: number
  /** Its main sessions (a helper whose main is its own counts under that main), in the list's order. */
  sessions: AgentSessionView[]
  /** The agent devices Remove takes out of the room (never this device, never a person's). */
  devices?: string[]
  /** What Remove does, said before it; none when there is nothing to remove. */
  remove?: Text
  /** Its account and how much of it is used (the connector's `account`); none without a plan or an e-mail. */
  usage?: UsageView
}

/** A main session of an agent on the page Agents. */
export interface AgentSessionView {
  /** Its key (`Screen::Session`). */
  id: string
  slug: string
  name: Text
  /** What draws it (`SessionRowView::mark`). */
  mark: string
  /** "Work team". */
  team: Text
  /** Its folder on the computer; "" when unknown. */
  folder?: string
  helpers: number
  /** "+2 helpers"; none without one. */
  helpers_says?: Text
  online: boolean
  /** "connected", "away". */
  state: Text
}

/** An agent's account: its plan, e-mail and meters. */
export interface UsageView {
  /** "Claude Max", "ChatGPT Pro Lite"; "" without a plan. */
  plan?: string
  email?: string
  /** The 5-hour window and the week, those it said; a window that started again since is left out. */
  meters?: MeterView[]
  /** "1,200 credits", "2 full resets" (Codex). */
  facts?: Text[]
}

export interface MeterView {
  /** "five_hour", "weekly". */
  key: string
  /** "5 h", "week". */
  label: string
  /** "5-hour window", "This week". */
  long: string
  /** Percent used, 0 to 100. */
  used: number
  /** At 80 % or more. */
  hot: boolean
  /** When it starts again (the shell writes the time). */
  reset_ms?: number
}

/** One device of the person. */
export interface DeviceRow {
  id: string
  /** Its name; "Device" while it has named itself nothing. */
  name: string
  this_device: boolean
  /** "human" or "agent". */
  role: string
  /** In the room (not removed). */
  active: boolean
  /** The green dot: in, and connected (this device always is). */
  online: boolean
  /** "this_device", "unfinished" (never finished signing in), "online", "away", "removed". */
  state: string
  /** The state in words ("this device", "never finished signing in"). */
  says: Text
  /** The key fingerprint from the signed member list: it must look the same on every device. */
  fingerprint: string
  /** Its drawing: "phone", "laptop", "terminal". */
  icon: string
  /** When it joined, if known. */
  added_ms?: number
  /** Last contact with the hub, when known. It confers no membership authority. */
  last_seen_ms?: number
  /** Core-formatted last-seen text for an offline device. */
  seen?: Text
}

/** The account's flows as a screen shows them. `step` names the screen; the fields it needs are filled.
 *  `Debug` leaves out the invite link and the Kit words: they open the account. */
export interface AccountView {
  /** "none", "sign_in", "found", "join", "compare_emoji", "keep_kit", "invite", "recover", "working". */
  step: string
  /** The invite in progress (for `ConfirmEmoji`). */
  invite_id?: string
  /** An invite link to show as text or QR code. */
  link?: string
  /** Copyable instructions for this invite, chosen by the core. Contains the invite secret like `link`. */
  connect_prompt?: string
  /** The corresponding Claude Code command. Contains the invite secret; never log it. */
  connect_command?: string
  /** The ways to bring the agent in with this link, one tab each ("claude", "codex", and for a computer approval
   *  "hermes"), the first chosen at the start. Their lines contain the invite secret; never log them. */
  ways?: ConnectWay[]
  /** The six emoji to compare, with their words. */
  emoji?: EmojiView[]
  /** The Emergency Kit words to keep, shown once. */
  kit?: string[]
  /** Self-join asks waiting for "Allow" or "Deny" (for `AnswerAsk`), and those allowed whose session is still
   *  joining (`joining`). */
  asks?: AskView[]
  problem?: Text
  /** The invite this device made and shows (not a self-join's): what it is for, until when, who came; it stays
   *  `joined` once its newcomer is in, until `DropInvite` or the next invite. */
  invite?: InviteView
  /** The account's Emergency Kit was never saved (downloaded, printed or shown, then `KitKept`): the screens show
   *  the kit's page first. The room register `kit` of every human device of the person says it. */
  kit_due: boolean
  /** What the kit's sheet names, once the account was read: its e-mail, its id (as printed), and the address its
   *  QR code holds (`https://<app>/#k1.<hub, base64url>.<id, 32 hex>`: the recovery screen with both filled in; no
   *  word of the kit). */
  kit_email?: string
  kit_account?: string
  kit_address?: string
  /** A word on the start screen about how this device came to be signed out: "Logged out." after the person's
   *  own sign-out, or that another device removed it. Gone with the next step. */
  notice?: Text
  /** A sign-in that was cut off before all its sessions were joined (the app was closed or reloaded meanwhile):
   *  the device is in the room and the app runs on, and these words ask for the way in again (`SignIn`, its
   *  passkey or its kit), which finishes the joins. None otherwise. */
  resume?: Text
}

export interface EmojiView {
  emoji: string
  word: string
}

export interface AskView {
  nonce: string
  /** What the vouching agent says it is: account, name, host (its word, shown as such). */
  says: Text
  /** The asking session's key (its main, which vouches). */
  session_id: string
  /** The name the new session gets. */
  name: string
  /** The computer and the folder it asks from (the agent's word); "" when it named none. */
  host: string
  folder: string
  /** "Claude" or "Codex". */
  kind: string
  /** Who vouches: the asking session's name. */
  vouched_by: string
  /** Allowed: its link is out and the new session is not in yet (no Allow or Deny any more). */
  joining: boolean
}

/** An invite this device made, as its page shows it. */
export interface InviteView {
  invite_id: string
  title: Text
  notice: Text
  computer: boolean
  /** "human" (a device of the person) or "agent". */
  role: string
  /** "open" (waits for the newcomer), "confirm_code" (the emoji to compare), "adding", "joined". */
  state: string
  /** When the link ends unused (ms). */
  expires_ms: number
  /** The newcomer's name once it is known ("" before): the device's own, or the agent's session. */
  newcomer?: string
}

/** One way to bring an agent in with an invite link, as a tab of the invite's checklist: install (when `install` is
 *  some), name it (when `name` is some), paste `line`, compare the emoji. `Debug` leaves out `line` and `alt`: they
 *  carry the invite secret. */
export interface ConnectWay {
  /** "claude", "codex" or "hermes". */
  key: string
  /** The tab: "Claude Code", "Codex", "Hermes". */
  label: Text
  /** The line that installs the connector, once per computer, as the first step; none where `line` installs it
   *  itself (Hermes). */
  install?: string
  /** The first step's title. */
  install_title: Text
  /** A name to give the agent first (`HermesName` sets it). */
  name?: NameAsk
  /** Where to paste `line`. */
  paste: Text
  /** The line to copy. Contains the invite secret. */
  line: string
  /** A shorter second line for the same (the Claude Code slash command), or none. */
  alt?: string
  /** What to know about pasting it. */
  note: Text
  /** The last step: where the six emoji show, and what follows the match. */
  compare: Text
  compare_note: Text
}

/** A name field on an invite's checklist. */
export interface NameAsk {
  title: Text
  /** The name so far ("" for none). */
  value: string
  placeholder: Text
  note: Text
}

/** A team's Scribble Board small: its shapes as `BoardView` has them, without a stroke's times, force and tilt. */
export interface ThumbView {
  team_id: string
  /** `desk/<32 hex>`. */
  board_id: string
  /** "loading" while it is read on this device, else as `BoardView.state`. */
  state: string
  /** How many shapes it holds (once read). */
  count: number
  shapes?: BoardShape[]
}

/** A Scribble Board (spec/v1.md 10): what its items add up to on this device, ready to draw. */
export interface BoardView {
  /** `desk/<32 hex>`. */
  board_id: string
  /** The chat the whiteboard was opened from (the page before it was a chat's, or a card's of that chat): its
   *  "Send to" sends a selection there with one press. None: it was opened from anywhere else. */
  from?: BoardChatView
  /** "Back to Fernly Website": the way back into that chat (with `from`). */
  back?: Text
  /** Every chat a selection can be sent to, of every group: the one it was opened from first, then the sidebar's
   *  order. */
  chats?: BoardChatView[]
  /** "loading" (its snapshot and the items after it are read), "ready", "newer_version" (an item needs a newer
   *  Trommi: what it did is not shown), "withheld" (an item could not be read on this device). */
  state: string
  /** Every shape in drawing order: by layer (`z`), then as they were made; strokes others are still drawing last. */
  shapes?: BoardShape[]
}

/** A chat a selection of the whiteboard can be sent to (`BoardView.chats`, `BoardView.from`). */
export interface BoardChatView {
  /** The session's key (`SessionRowView.id`): where a selection is sent. */
  session_id: string
  /** Its short id: its address (`/chat/<slug>`). */
  slug: string
  name: string
  /** Its face (`SessionRowView.mark`). */
  mark: string
  /** Its agent is connected. */
  online: boolean
  /** "Send to Fernly Website". */
  send: Text
}

/** One shape of a board, in board units (a point is two numbers). The fields its tool does not use are left out.
 *  
 *  - stroke (`pen`, `marker`): `pts` its points, `t` their times (ms since it began), `f` their force (0 to 1),
 *    `az` and `al` the pen's azimuth and altitude in radians (none without tilt), `sim` a force made up from the
 *    speed, `color`, `width`;
 *  - note (`text`, `sticky`, `voice`): `pts` its top left corner, `text`, `size`, `color`, `wrap` (none: no wrap);
 *  - picture (`image`): `pts` two opposite corners, `attachment` its file's id (`ViewModel.files`, `OpenFile`),
 *    `nw` and `nh` its own size in pixels (0: not known), `mime`, `name`. */
export interface BoardShape {
  /** `<sender hex>/<envelope number>/<index>`, or `live:<sender hex>/<stroke hex>` for a stroke still drawn. */
  id: string
  /** The device that drew it (hex). */
  by: string
  tool: string
  z: number
  group?: string
  pts: number[]
  t?: number[]
  f?: number[]
  az?: number[]
  al?: number[]
  sim: boolean
  color?: string
  width?: number
  text?: string
  size?: number
  wrap?: number
  attachment?: string
  nw?: number
  nh?: number
  mime?: string
  name?: string
  /** A stroke someone else is still drawing (its pieces, spec 7.2): its finished form replaces it. */
  live: boolean
  /** Drawn on this device and not yet back from the hub: `<local_id>/<index>` of the `BoardItem` that made it. */
  local?: string
}

export interface FileView {
  name: string
  media_type: string
  size: number
  /** A picture (by its media type or its name's ending): an attachment shows it as a small square thumbnail. */
  picture: boolean
  /** "uploading", "loading", "ready", "failed", "missing", "removed" (the hub deleted it: a full room made room). */
  state: string
  /** The attachment's id once the hub has it (an upload's local id maps to it here). */
  attachment_id?: string
  /** 0..=100 while it moves. */
  percent?: number
  /** Its bytes were handed over (`Effect::FileReady`). */
  loaded: boolean
  /** The Share links this device made for it (`Action::Share`), kept in memory only. */
  shares?: FileShareView[]
  /** What stands in place of a file that cannot be shown ("removed": "Removed (storage full)"). */
  note?: Text
}

/** A Share link of a file: the whole link (its key behind the `#`), for the person to copy. */
export interface FileShareView {
  share_id: string
  link: string
  expires_ms: number
}

/** A passing line at the window's foot: what was done, and the way back. The engine and the actions make them; the
 *  shell draws them and drops one when its time is up or it is pressed. */
export interface Toast {
  id: string
  /** The bold words ("Note sent to Fernly", "Not saved"). */
  text: Text
  /** An action the toast offers ("Undo", "Retry"), sent back as that action. */
  action?: unknown
  /** The quieter words after them ("the board did not take it"). */
  line?: Text
  /** The action's word on its button ("Undo", "Retry"); none: "Undo". */
  label?: Text
  /** Something went wrong: said at once to a screen reader (the web's role "alert"), drawn as a problem. */
  alert: boolean
  /** How long it stands (ms), as the ring on its Undo runs down; none: the shell's own time. */
  ms?: number
  /** A way to a page ("Open"): the screen, and the link's words. */
  go?: ToastGo
}

/** A toast's way to a page. */
export interface ToastGo {
  screen: Screen
  label: Text
}
