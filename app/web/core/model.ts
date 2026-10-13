// model.ts: what the views read of the board model besides the records themselves (core/protocol/README.md "The model" is the
// contract; the shapes are types.ts). The model is BUILT by the client core (core/client/src/model, the binding's
// `ClientModel`): what the Rust core accepted goes in there, and client.ts puts its patch into the page's copy once per
// batch (board.ts, mirror.ts). What stays here is read-only: small rules the views share (a card's link to its agent,
// the receipt, who holds an object, an item's state), the stack of one desk, and the records of the app's local cache.
import { BOARD_CONTENT_TYPES } from './codec.ts'
import { applyPatch } from './mirror.ts'
import { emptyModel, emptyChange } from './model-shape.ts'
import type {
  Alert, Body, Card, CardOption, Change, ContentState, HumanRegister, ItemState, Link, Member, Model, Note,
  PermissionRequest, Published, Session, Timeline, TimelineItem,
} from './types.ts'

export { emptyModel, emptyChange }
type Obj = Record<string, any>
const isObject = (v: unknown): v is Obj => !!v && typeof v === 'object' && !Array.isArray(v)

// ---- small things the client and the views' seam use ---------------------------------------------------------------
/** Whether this version can show and act on a card (false: a placeholder, no answer from here). */
export const cardSupported = (card: { unsupported?: unknown } | null | undefined): boolean => !card?.unsupported
/** change.items: Map<timeline_key, TimelineItem[]> added or replaced in this batch. */
export function addItem(change: Change, key: string, item: TimelineItem): void { let l = change.items.get(key); if (!l) change.items.set(key, l = []); l.push(item) }
const CHANGE_SETS = ['cards', 'sessions', 'permissions', 'notes', 'published', 'timelines', 'registers', 'invites'] as const
export function changeIsEmpty(c: Change): boolean {
  return c.items.size === 0 && !c.members && !c.alerts && !c.outbox && !c.stack && !c.room && !c.status && CHANGE_SETS.every(k => c[k].size === 0)
}
export const timelineKey = (timeline_kind: string, timeline_id: string): string => `${timeline_kind}:${timeline_id}`
export interface TimelineKeyParts { timeline_kind: string; timeline_id: string; scope: string; scope_id: string }
export function parseTimelineKey(key: string): TimelineKeyParts {
  const at = key.indexOf(':')
  const timeline_kind = key.slice(0, at), timeline_id = key.slice(at + 1)
  const slash = timeline_id.indexOf('/')
  return { timeline_kind, timeline_id, scope: timeline_id.slice(0, slash), scope_id: timeline_id.slice(slash + 1) }
}

export const GOALS_LINES = 20, GOALS_LINE_MAX = 200
/** A desk's goals as they are kept: at most GOALS_LINES lines of at most GOALS_LINE_MAX characters, no blank lines. */
export function cleanGoals(text: unknown): string {
  return String(text ?? '').split(/\r?\n/).map(l => l.trim().slice(0, GOALS_LINE_MAX)).filter(Boolean).slice(0, GOALS_LINES).join('\n')
}
/** Every choice is an option the agent marked final (and there is at least one): the answer alone ends the matter. */
export function choicesFinal(card: { options?: (CardOption | null | undefined)[] | null } | null | undefined, choices: unknown): boolean {
  const final = new Set((card?.options ?? []).filter(o => o?.final === true).map(o => o!.key))
  return Array.isArray(choices) && choices.length > 0 && choices.every(k => final.has(k))
}

// ---- the link: whether an agent's session can hear the human and speak to him ----------------------------
//
// More than is_online ("its connector has a stream"). The connector reports it to the hub (spec 13.7), the hub
// serves it and pushes every change (stream event `presence`):
//   link = { hears: 'live' | 'oncall', attached, last_call_at, working, cut_since?, exit?: { reason, claude } }
//     hears        'live': Claude Code shows board events at once; 'oncall': events wait in the connector and go out
//                  with the agent's next tool call
//     attached     an agent's MCP session stands behind the key (false: the process holds the key and nobody listens)
//     last_call_at the agent's last tool call through the connector: the one sign that the AGENT is alive
//     cut_since    a Claude Code session of the folder lives while its connector is gone (seen by this connector)
//     exit         the connector's last word before its stream closed: why, and whether its Claude Code process lived
//                  on ('alive'), ended ('gone') or was still being looked at ('checking')
// The five states, from the hub's two facts (is_online, offline_since) and the report (decided by the client core,
// core/client/src/status.rs `link_state`):
//   gone     no stream, and no word that Claude Code lives on
//   cut      no stream but its Claude Code lives (exit.claude 'alive'); or a stream, and a session of its folder lost
//            its connector (cut_since); or a stream with nobody attached
//   live     hears at once (also: an agent that reports nothing)
//   oncall   hears on its next tool call
//   asleep   oncall, and no tool call for ASLEEP_MS
/** No tool call for this long, in a session that hears only on its next one: it is not listening. */
export const ASLEEP_MS = 10 * 60_000
const HEARS: readonly unknown[] = ['live', 'oncall'], CLAUDE: readonly unknown[] = ['alive', 'gone', 'checking']
const stamp = (v: unknown): number | null => (typeof v === 'number' && Number.isSafeInteger(v) && v > 0 ? v : null)
/** A link report as the model keeps it: known fields of the right type only, null when there is none. */
export function cleanLink(report: unknown): Link | null {
  if (!isObject(report) || !HEARS.includes(report['hears'])) return null
  const l = report
  const exit = isObject(l['exit']) ? { reason: String(l['exit']['reason'] ?? '').slice(0, 40), claude: CLAUDE.includes(l['exit']['claude']) ? l['exit']['claude'] : 'gone' } : null
  return { hears: l['hears'], attached: l['attached'] !== false, last_call_at: stamp(l['last_call_at']), working: l['working'] === true, since: stamp(l['since']), cut_since: stamp(l['cut_since']), exit }
}
// ---- the receipt: what the agent has been handed ------------------------------------------------------------
// The connector writes the session register `heard` = { up_to, at } when it really hands the human's commands to the
// agent: every command of this session up to envelope number up_to is with the agent. heard_up_to null: nothing is known.
/** Whether the session's agent was handed the envelope: true, false, or null when its connector writes no receipts. */
export const heardBy = (session: { heard_up_to?: number | null } | null | undefined, envelope_number: unknown): boolean | null =>
  (session?.heard_up_to == null || !Number.isInteger(envelope_number) ? null : (envelope_number as number) <= session.heard_up_to)
/** What the agent has to hear of a card: the human's answer in force, else the message that handed it back. Null: nothing. */
export const cardWaitsOn = (card: Pick<Card, 'answer' | 'in_revision'> | null | undefined): number | null =>
  (card?.answer && !card.answer.pending ? card.answer.envelope_number : card?.in_revision?.envelope_number) ?? null
/** Whether the card's agent has the human's last word on it (answer or hand-back): true, false, null (nothing to hear, or not known). */
export function cardHeard(model: Model, card: Card): boolean | null {
  const n = cardWaitsOn(card)
  return n == null ? null : heardBy(card.session_id ? model.sessions.get(card.session_id) : undefined, n)
}

const fingerprintOf = (device_id: string): string => device_id.slice(0, 16).match(/.{4}/g)!.join(' ')
/** A key's fingerprint as the app shows it: the first 8 bytes of its hex, in groups of four. */
export const keyFingerprint = (key: Uint8Array): string => fingerprintOf([...key].map(b => b.toString(16).padStart(2, '0')).join(''))

/** The longest a permission request waits for its verdict: a connector's hook waits at most an hour
 *  (TROMMI_PERMISSION_MS), and two minutes more for the clocks. */
export const REQUEST_LIFE_MAX_MS = 62 * 60_000
/** A permission request whose body was not read here (a cache of before has no mark: it was read). */
export const unreadRequest = (p: { content_state?: ContentState }): boolean => p.content_state != null && p.content_state !== 'ok'

/** Who holds an object now (whom a human addresses an answer or a verdict to): its owner while that device is a
 *  leaf of the session's group, else the session's agent device (a helper session: its opener), as spec 9.2 says. */
export function holderOf(model: Model, obj: { agent_device_id: string; session_id?: string | null } | null | undefined): string | null {
  if (!obj) return null
  const s = obj.session_id ? model.sessions.get(obj.session_id) : undefined
  if (!s || !s.agent_device_ids.length || s.agent_device_ids.includes(obj.agent_device_id)) return obj.agent_device_id
  return recipientOf(model, s.session_id)
}
/** Whom a human addresses a Chat message of a session to: its agent device; in a helper session its opener (9.2). */
export function recipientOf(model: Model, session_id: string): string | null {
  const s = model.sessions.get(session_id)
  return !s ? null : s.parent_session_id ? s.creator_device_id : s.agent_device_id
}
/** A helper session's main session, if this device knows it. */
export const parentSessionOf = (model: Model, s: Session | null | undefined): string | null => (s?.parent_session_id && model.sessions.has(s.parent_session_id) ? s.parent_session_id : null)

const KEPT_STATES: readonly unknown[] = ['pruned', 'undecryptable', 'newer_schema']
/** A timeline item's content type this version knows (an item without one counts as a message). */
export const contentTypeKnown = (content: Body | null | undefined): boolean => content?.['content_type'] == null || content['content_type'] === 'message' || content['content_type'] === 'stroke_piece' || BOARD_CONTENT_TYPES.includes(content['content_type'] as string)
/** A timeline item's state from its body and content_state: loaded, unsupported (a content type of a newer version),
 *  newer_schema, pruned, undecryptable, or header (the body not here yet). */
export const itemStateOf = (content: Body | null | undefined, content_state: ContentState | string | null | undefined): ItemState => (content ? (content_state !== 'ok' ? content_state as ItemState : contentTypeKnown(content) ? 'loaded' : 'unsupported')
  : KEPT_STATES.includes(content_state) ? content_state as ItemState : 'header')
/** item_state values that mean "a newer client wrote this": the views show the update placeholder. */
export const itemNeedsUpdate = (item: Pick<TimelineItem, 'item_state'> | null | undefined): boolean => item?.item_state === 'unsupported' || item?.item_state === 'newer_schema'

/** The stack of one desk (desk_id), or of all (none). */
export function stackOf(model: Model, { desk_id }: { desk_id?: string | null } = {}): string[] {
  if (desk_id == null) return model.stack
  return model.stack.filter(id => (model.sessions.get(model.cards.get(id)?.session_id as string)?.settings?.desk ?? null) === desk_id)
}

// ---- the app's local cache: records <-> model (README "Incremental persistence") ------------------------------------
//
// One record per card, session, permission request, note, Artifact, room register and timeline (its counters), one
// per timeline item, and four small ones (room, members, device registers, alerts). Every value is plain JSON-able
// data. cacheRecords gives exactly the records a change names, for one Cache.setMany; modelFromCache builds the
// model a warm start shows before the hub answers. Timeline items are not read back into the model: their windows
// are read on demand (cacheItemKey, a range over `tl/<timeline_key>/`). Not cached: invites and the outbox (the
// engine's), pending echoes, live strokes, the projections.

const pad = (n: number): string => String(n).padStart(12, '0')
/** The cache key of a timeline item: ordered by envelope number within its timeline. */
export const cacheItemKey = (timeline_key: string, envelope_number: number): string => `tl/${timeline_key}/${pad(envelope_number)}`
/** A card at rest, without what it holds twice: the newest version's content is the card's own fields again, and
 *  `answer` is the last of `answers`. expandCard gives the same card back. */
const AT_REST = '$card'
export function compactCard(c: Card): Record<string, unknown> {
  let out: Record<string, unknown> = c
  const lv = c.versions.at(-1)
  if (lv?.content && typeof lv.content === 'object') {
    const own: string[] = [], rest: Record<string, unknown> = {}
    for (const [k, v] of Object.entries(lv.content)) { if (k in c && JSON.stringify(c[k]) === JSON.stringify(v)) own.push(k); else rest[k] = v }
    if (own.length) out = { ...c, versions: [...c.versions.slice(0, -1), { ...lv, content: { [AT_REST]: own, rest, keys: Object.keys(lv.content) } }] }
  }
  if (c.answer && !c.answer.pending && c.answers.at(-1) === c.answer) out = { ...out, answer: AT_REST }
  else if (c.answer?.pending) out = { ...out, answer: null }
  return out
}
export function expandCard(stored: unknown): Card {
  const v = structuredClone(stored) as Obj
  const lv = v['versions']?.at(-1)
  if (lv?.content?.[AT_REST]) {
    const { [AT_REST]: own, rest, keys } = lv.content
    const content: Record<string, unknown> = {}
    for (const k of keys as string[]) content[k] = own.includes(k) ? v[k] : rest[k]
    lv.content = content
  }
  if (v['answer'] === AT_REST) v['answer'] = v['answers']?.at(-1) ?? null
  return v as Card
}
const storedTimeline = (t: Timeline): Record<string, unknown> => { const { items: _items, ...rest } = t; return { ...rest, loaded_down_to: null, window_open: false } }
const storedNote = (n: Note): Note | undefined => { const kept = n.pending ? n._base ?? undefined : n; if (!kept) return undefined; const { _base: _b, ...rest } = kept; return rest as Note }
const ROOM_KEPT = ['room_id', 'hub_url', 'my_device_id', 'my_role', 'recovery_key', 'key_epoch', 'last_envelope_number'] as const

/** Each device's own register `device/<id>` as the core last told it (its name before it is a member): kept beside the
 *  model for the cache. */
export function devregsOf(model: Model): Map<string, Obj> {
  const m = model as Model & { _devregs?: Map<string, Obj> }
  // (not enumerable: it travels nowhere and no copy compares it)
  if (!m._devregs) Object.defineProperty(m, '_devregs', { value: new Map(), enumerable: false, writable: true })
  return m._devregs!
}
/** The records a change names, as entries for one Cache.setMany (undefined deletes). */
export function cacheRecords(model: Model, change: Change): [key: string, value: unknown | undefined][] {
  const out = new Map<string, unknown>()
  for (const id of change.cards) { const c = model.cards.get(id); out.set(`card/${id}`, c ? compactCard(c) : undefined) }
  for (const id of change.sessions) { const s = model.sessions.get(id); out.set(`session/${id}`, s ? { ...s, registers: [...s.registers] } : undefined) }
  for (const id of change.permissions) out.set(`perm/${id}`, model.permissions.get(id))
  for (const id of change.notes) { const n = model.notes.get(id); out.set(`note/${id}`, n ? storedNote(n) : undefined) }
  for (const id of change.published) out.set(`pub/${id}`, model.published.get(id))
  for (const key of change.timelines) { const t = model.timelines.get(key); if (t) out.set(`tlmeta/${key}`, storedTimeline(t)) }
  for (const key of change.registers) {
    const raw = model.human.raw.get(key)
    if (raw && !raw.pending) out.set(`reg/${key}`, raw)
    else if (key.startsWith('device/')) out.set('devregs', [...devregsOf(model)])
  }
  for (const [key, items] of change.items) for (const it of items) if (it.envelope_number != null && !it.pending) out.set(cacheItemKey(key, it.envelope_number), it)
  if (change.members) { out.set('members', [...model.members.values()]); out.set('devregs', [...devregsOf(model)]) }
  if (change.room) { out.set('room', Object.fromEntries(ROOM_KEPT.map(k => [k, model.room[k]]))); out.set('newer', model.newer) }
  if (change.alerts) out.set('alerts', model.alerts)
  return [...out]
}
/** Every record of a model (a first write, or a rewrite after the model was built anew). */
export function cacheAll(model: Model): [key: string, value: unknown | undefined][] {
  const all = emptyChange()
  for (const id of model.cards.keys()) all.cards.add(id)
  for (const id of model.sessions.keys()) all.sessions.add(id)
  for (const id of model.permissions.keys()) all.permissions.add(id)
  for (const id of model.notes.keys()) all.notes.add(id)
  for (const id of model.published.keys()) all.published.add(id)
  for (const key of model.timelines.keys()) all.timelines.add(key)
  for (const key of model.human.raw.keys()) all.registers.add(key)
  all.members = all.room = all.alerts = true
  return cacheRecords(model, all)
}
/** The model a cache holds (entries as Cache.range gives them; values as they came back from JSON or IndexedDB).
 *  Timeline windows are empty and nothing is connected; call project() on it. */
export function modelFromCache(entries: Iterable<readonly [string, unknown]>): Model {
  const m = emptyModel()
  const registers: [string, HumanRegister][] = []
  for (const [key, raw] of entries) {
    if (raw === undefined || raw === null) continue
    const at = key.indexOf('/'), kind = at < 0 ? key : key.slice(0, at), id = key.slice(at + 1)
    const v = raw as Obj
    switch (kind) {
      case 'card': m.cards.set(id, expandCard(v)); break
      case 'session': { const s = structuredClone(v); m.sessions.set(id, { ...s, registers: new Map(s['registers'] ?? []) } as Session); break }
      case 'perm': m.permissions.set(id, structuredClone(v) as PermissionRequest); break
      case 'note': m.notes.set(id, structuredClone(v) as Note); break
      case 'pub': m.published.set(id, structuredClone(v) as Published); break
      case 'tlmeta': m.timelines.set(id, { ...structuredClone(v), items: new Map(), loaded_down_to: Infinity, window_open: false } as Timeline); break
      case 'reg': registers.push([id, structuredClone(v) as HumanRegister]); break
      case 'members': for (const x of v as Member[]) m.members.set(x.device_id, { ...x, added_at: x.added_at ?? null }); break
      case 'devregs': for (const [device, value] of v as [string, Obj][]) devregsOf(m).set(device, value); break
      case 'room': Object.assign(m.room, v); break
      case 'newer': m.newer = structuredClone(v) as Model['newer']; break
      case 'alerts': m.alerts = structuredClone(v) as Alert[]; break
    }
  }
  // (the registers as the page's copy keeps them: raw, and the maps the views read)
  applyPatch(m, { room: m.room, human: registers.map(([key, raw]) => ({ key, raw })), change: { cards: [], sessions: [], permissions: [], notes: [], published: [], timelines: [], registers: [], invites: [], members: false, alerts: false, outbox: false, stack: false, room: false, items: [] } })
  return m
}
