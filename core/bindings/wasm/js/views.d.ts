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
  /** The desk in view: a desk's id, "all" for every desk at once, none for the first desk. */
  desk?: string
}

export type Screen =
  | { screen: 'desk' }
  | { screen: 'session'; session_id: string }
  | { screen: 'card'; card_id: string }
  | { screen: 'notes' }
  /** The page Off your mind, the complete list, with the search's words (empty: all). */
  | { screen: 'off'; query: string }
  | { screen: 'scribble'; board_id: string }
  | { screen: 'settings' }

/** Everything the shell draws. Serialised, this is what the conformance scripts compare. */
export interface ViewModel {
  /** Counts the deltas folded in; a delta whose `base` is not this is out of order (a shell bug). */
  rev: number
  phase: Phase
  /** One line on where the app stands when it is not plainly live ("Catching up", "Offline: retry in 30 s"). */
  status?: Text
  sidebar: SidebarView
  desk: DeskView
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
  | { part: 'desk'; view: DeskView }
  | { part: 'session'; id: string; view: SessionView | null }
  | { part: 'card'; id: string; view: CardView | null }
  /** The page Off your mind; none when it is left. */
  | { part: 'off'; view: OffListView | null }
  | { part: 'notes'; view: NotesView }
  | { part: 'settings'; view: SettingsView }
  | { part: 'account'; view: AccountView }
  | { part: 'file'; id: string; view: FileView | null }
  | { part: 'toasts'; toasts: Toast[] }

/** Edits of a long list, by position: at `at`, `gone` rows leave and `rows` come (the Desk's stack ops today). */
export interface RowEdit<T> {
  at: number
  gone: number
  rows: T[]
}

export type Rows<T> =
  | { rows: 'whole'; items: T[] }
  | { rows: 'edits'; edits: RowEdit<T>[] }

/** The sidebar (the phone: the menu's desks, the list of chats, the tab pill's numbers): the desks, and one row per
 *  session of the desk in view, in the order they are drawn. */
export interface SidebarView {
  /** The desks, in the menu's order. A room without desks has one: "Personal". */
  desks: DeskTab[]
  /** The sessions of the desk in view, one row each, in drawing order: a main, then its helpers. */
  sessions: SessionRowView[]
  /** The number on the app icon and the tab: the open questions of the desk in view (no infos, nothing that is
   *  with its agent). Never drawn red. */
  badge: number
  /** The desk in view: its id, or "all". */
  desk: string
  /** "All Desks", the row above the desks: there with two desks or more. */
  all?: DeskTab
  /** Where the person is, for the head: the desk's name, or "All desks". */
  place: Text
  /** The open questions of the desk in view (its lamp is lit while there are some). */
  waiting: number
  /** How many of them knock. */
  knocking: number
  /** How many rows of the desk in view are stopped (the red hand). */
  stopped: number
  /** How many of them are at work. */
  working: number
  /** How many of his answers the sessions of the desk in view have not picked up. */
  unheard: number
}

export interface DeskTab {
  id: string
  name: Text
  /** The open questions of the desk's sessions. */
  waiting: number
  selected: boolean
  /** One of them knocks. */
  knocks: boolean
}

/** One session in the sidebar. */
export interface SessionRowView {
  /** The session's key in the model (`Screen::Session`, the actions' `session_id`, `ViewModel.sessions`). */
  id: string
  name: Text
  /** "live", "oncall", "asleep", "cut", "gone" (core/client/src/status.rs); "" for a helper under its main (it
   *  runs inside its main's process: the main's link says it for both). */
  link: string
  /** The red hand. */
  blocked: boolean
  /** How many of his answers it has not picked up for two minutes; a main counts its helpers' too. */
  unheard: number
  /** 0 for a row of its own, 1 for a helper under its main. */
  depth: number
  /** Its short id on the board: its address (`/chat/<slug>`). */
  slug: string
  /** What draws it: the icon the person or the agent chose ("draw:leaf"), else its slug. */
  mark: string
  /** The desk it stands on. */
  desk_id: string
  online: boolean
  /** Connected and at work (a status line of it shows "working"). */
  working: boolean
  /** A main session: one that says so, or one with helpers. */
  main: boolean
  /** The crowned session of its desk (the one a note goes to). */
  crowned: boolean
  /** Its main's id, for a helper drawn under it. */
  parent?: string
  /** Its helpers' ids, in drawing order. */
  subs?: string[]
  /** Its open questions: what waits for the person on the Desk (no infos, nothing that is with its agent). */
  open: number
  /** One of them knocks. */
  knocks: boolean
  /** Why it is stopped, in words. */
  hand?: HandView
  /** The small line under its name: its link when it does not hear at once, else the answers it has not picked up. */
  cap?: CapView
  /** A helper whose main is not listening, cut off or gone: drawn hushed. */
  hushed: boolean
  /** Not connected, and none of its helpers is: it stands under "Disconnected". */
  away: boolean
  /** The line drawn above this row: "Disconnected" over the first row that is away, a desk's name over its first
   *  row on All desks. */
  heading?: HeadingView
  /** The part of the list a main row may be dragged within (its desk, connected or not); "" for a helper. */
  group: string
  /** What its badge says to a screen reader ("Working, 2 questions open"); none without a badge. */
  says?: Text
  /** A main with helpers, drawn folded: what the row then shows for all of them together. */
  folded?: FoldView
}

/** A main and its helpers as one folded row. */
export interface FoldView {
  open: number
  knocks: boolean
  /** One of them is connected. */
  online: boolean
  working: boolean
  /** The first of them that is stopped. */
  hand?: HandView
  says?: Text
  /** The helpers that are stopped: their edges show it while the row is folded. */
  stopped?: string[]
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

/** A line above a row of the sidebar. */
export interface HeadingView {
  /** "away" ("Disconnected") or "desk" (a desk's name, on All desks). */
  kind: string
  text: Text
  /** For "desk": the desk, and its open questions. */
  desk_id?: string
  waiting: number
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

/** The Desk of the desk in view: its head, and the stack of what waits for the person as rows. */
export interface DeskView {
  /** Card and request ids, in the Desk's order: the ids of `rows`. */
  stack: string[]
  /** Off your mind, of the desk in view (`off.rs`, from the core's `off_mind`): the cards with their agents,
   *  starred ones too, the one that moved last first (the complete list's top part). */
  off_your_mind: string[]
  /** The put-off cards, the last put off first. */
  later: string[]
  /** The cards closed last (done, withdrawn, shredded), the newest first: the core's window of them
   *  (`off_mind::RECENT` for the whole room), not every closed card; the complete list has them all. */
  done: string[]
  /** The two stacks docked at the Desk's foot; none while a stack is empty. A card has one home: a starred card
   *  is in `starred` only, also while it is being worked on. */
  starred?: DockView
  running?: DockView
  /** The section Off your mind under the Desk: its first rows and how many; none while nothing is put off or
   *  closed. */
  off?: OffHintView
  /** What stands in the list's place when nothing is open at all ("Questions land here."). */
  empty?: Text
  /** "invite" (no session stands here yet: invite the first agent), "reading" (none yet, and the room is still
   *  being read), "desk". */
  mode: string
  /** The one big line at the top; none for "invite". */
  hello?: Text
  /** How far the reading of the room's past is ("Reading your history: 12,400 of 27,000"), while it is read. */
  reading?: Text
  /** The desk's goals; none on All desks. */
  goals?: GoalsView
  /** What waits, in the Desk's order: the questions and what is only to read. */
  rows?: DeskRowView[]
  /** The open questions (no infos): the number in the window's title. */
  waiting: number
  /** What Blitz walks through and the duck closes ("Blitz: 3 open cards"); none when nothing waits. */
  blitz?: Text
  /** The sessions that are cut off, above the questions: one line each, with the step in its terminal. */
  slip?: SlipView[]
}

/** One of the two stacks docked at the Desk's foot (the owner's pick of 13 October 2026: two small tabs, the
 *  titles shown only when a stack is fanned out). */
export interface DockView {
  /** "star" or "run". */
  key: string
  /** How many cards it holds: the number on its tab (`rows.len()`). */
  count: number
  /** What the tab says to a screen reader ("3 starred", "1 running"). */
  says: Text
  /** Its cards in order, fanned out: the first is the top card, the one the tab shows. */
  rows: DockRowView[]
  /** The way on to the complete list at the end of the fan ("All running and done"); the running stack only. */
  all?: Text
}

/** A small card of a docked stack. */
export interface DockRowView {
  /** The card (`Screen::Card`). */
  id: string
  title: string
  from: FromView
  /** The quiet line under the title: "Fernly · running" (a starred card being worked on), "Fernly", "Fernly ·
   *  Reworking the copy" (its status line), "Fernly · on it". */
  line: Text
  /** It is being worked on (the drawing's ring turns). */
  running: boolean
  /** When it last moved (its status line's update, else when it went to its agent); when it was starred, for a
   *  starred card. */
  at_ms: number
}

/** The section Off your mind under the Desk: a short hinted list that opens the complete list (`Screen::Off`). */
export interface OffHintView {
  /** The first rows, put off first, then the closed ones, the newest first (at most three; the shell fades them). */
  rows: OffRowView[]
  /** "2 put off · 14 done". */
  says: Text
  put_off: number
  /** Every closed card of the desk in view, counted (not only the ones named). */
  closed: number
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

/** The page Off your mind: every card off the Desk of the desk in view. What runs as live cards, the one that moved
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

/** A desk's goals: a note of the person's, at most twenty lines. */
export interface GoalsView {
  desk_id: string
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

/** A session that is cut off, as the Desk says it. */
export interface SlipView {
  session_id: string
  text: Text
  fix: FixView
}

/** One row of the Desk: a question, a request for approval, or something to read. */
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
  /** Where it stands, for the line above a card's title ("Desk › Main › Helper"): its desk while there are two
   *  or more, its main for a helper, itself. */
  path: string[]
  /** Its desk's name, on All desks only: the small chip before the row's words. */
  desk?: string
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
  status_lines: StatusLineView[]
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
  desk_id: string
  main: boolean
  crowned: boolean
  online: boolean
  /** Where it stands: its desk (while there are two or more), its main (for a helper), itself. */
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
  /** "<name> is working": his word was the last, a short while ago. */
  answering?: Text
  /** Its questions that wait for the person (its infos too), and the way to the first of them. */
  open: number
  /** "3 open". */
  open_word?: Text
  /** "3 open questions: go to the first one". */
  open_says?: Text
  /** The talk row of the first of them, where it was asked. */
  first_open?: string
  /** An empty conversation: the card of the agent that joined, and three ways to start. */
  empty?: EmptyTalkView
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
  /** A selection of the Scribble Board (its one file is the picture). */
  scribble: boolean
  /** A note: when it was written. */
  written_ms?: number
  /** On a card's page, the agent's words that answer his What??: that row's id (drawn as one block with it). */
  answers?: string
  /** On a card's page, his What?? that has no answer yet: "Waiting for the explanation." */
  waits?: Text
}

/** A card as a row of a conversation names it. */
export interface CardRefView {
  id: string
  /** Its number ("Nr. 12"); 0 when this device does not hold it. */
  number: number
  title: string
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
  detail: string
  /** "working", "decision", "later", "done" (for ten minutes), "stale" (greyed and still, for a day). */
  state: string
  /** "working", "waits on you", "put off", "done", "no news for a while". */
  says: Text
  /** When its agent last set it. */
  updated_ms: number
  /** The card it stands for: the one it names, else the card with its session whose line it is. */
  card?: CardRefView
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
  /** Who it comes from, above its title: "Desk › Main › Helper" (`FromView::path`). */
  from?: FromView
  /** "Knock! Blocking", while it is open and knocks. */
  knock?: Text
  /** The strip above the title while its session works on it. */
  work?: CardWorkView
  /** When it was asked, or last revised. */
  asked_ms: number
  /** Its place among what waits on the Desk in view ("3 of 7"), with its neighbours. */
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
  /** The comments and what happened to the card, oldest first. */
  talk?: TalkRow[]
  /** The hub holds older comments than `talk` has. */
  has_more: boolean
  /** The cards it replaces. */
  merged_from?: CardRefView[]
  /** The card that replaced it. */
  merged_into?: CardRefView
  /** Its pictures, in order (the stage and the small ones under it), each with the option it shows. */
  pictures?: PictureView[]
  /** Once it is settled (not open, not catching up): the way on that the page offers right after the person
   *  answered here (the shell shows it then): the next open question of the desk in view, or that there is none. */
  after?: AfterView
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
  /** The next open question: the next in the Desk's order after this card's place, else the first that waits. */
  next?: CardRefView
  /** "Next question", or "That was the last open question." */
  says: Text
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

/** A card's place among what waits on the Desk in view. */
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
}

/** The person's notes (the room's, the same on every desk) and the one in the corner (`notes.rs`). */
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
  /** All desks, and it goes to another desk's crown than the desk chosen last: the send names it. */
  far: boolean
  /** All desks with two crowned desks or more: the person picks one (each desk's crown, in the desks' order). */
  choose?: NoteToView[]
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
  desk_id: string
  /** Its desk's name ("Personal" for one without), for the choice on All desks. */
  desk: string
}

/** Settings: what the list's rows say, and the person's devices (`settings.rs`). What only this device knows (the
 *  theme, the Push level) is the shell's. */
export interface SettingsView {
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
  /** The row Desks: "3 desks"; none in a room without desks. */
  desks?: Text
  /** The row Devices: "2 devices". */
  devices_says?: Text
  /** The room's key epoch (the version line at the list's foot). */
  key_epoch: number
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
  /** The six emoji to compare, with their words. */
  emoji?: EmojiView[]
  /** The Emergency Kit words to keep, shown once. */
  kit?: string[]
  /** Self-join asks waiting for "Allow" or "Deny" (for `AnswerAsk`). */
  asks?: AskView[]
  problem?: Text
}

export interface EmojiView {
  emoji: string
  word: string
}

export interface AskView {
  nonce: string
  /** What the vouching agent says it is: account, name, host (its word, shown as such). */
  says: Text
}

export interface FileView {
  name: string
  media_type: string
  size: number
  /** "uploading", "ready", "failed", "missing". */
  state: string
  /** The attachment's id once the hub has it (an upload's local id maps to it here). */
  attachment_id?: string
  /** 0..=100 while it moves. */
  percent?: number
  /** Its bytes were handed over (`Effect::FileReady`). */
  loaded: boolean
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
