// factory.mjs: hand-made values of the Rust core's results (app/web/core/core-api.ts ReceivedEnvelope, GroupSummary,
// RoomRoles, CommitSummary, ReceivedMessage, Sealed) for the model's tests. NO core runs here and nothing is verified
// or encrypted: a `World` only counts (the hub's change numbers, each sender's envelope numbers, made-up hashes) and
// remembers, per object, who owns it and which version is current, so that a test reads as a story. What the core
// would decide (an outcome, a code, `objectAfter`, a register's `current`) is given by the test, or defaults to "it
// counted".
//
// The tests name every id as the wire's base64url text (device(), id16(), bytes()): that is what stands in a body's
// JSON. The factory turns them into the bytes the core hands over (raw()), and hex() gives the model's form.
import { b64u, idToHex, unb64u } from '../../../app/web/core/ids.ts'
import { relink, revivePatch, snapshotForCore } from '../../../app/web/core/board.ts'
import { applyPatch } from '../../../app/web/core/mirror.ts'
import { devregsOf, emptyChange, emptyModel } from '../../../app/web/core/model.ts'
import { loadBinding } from '../stand-in/core.ts'

/** The model is the client core's (the binding's ClientModel); a test reads the page's copy of it (`model`). */
const binding = await loadBinding()
/** The core's model behind each copy. */
const boards = new WeakMap()
/** A copy with a core's model of its own (a model built from the cache: loaded into it). */
export const withDesk = model => { const board = new binding.ClientModel(model.room.my_device_id ?? ''); if (model.cards.size || model.sessions.size) board.load(snapshotForCore(model)); boards.set(model, board); return model }
export const boardOf = model => boards.get(model) ?? (withDesk(model), boards.get(model))
/** The core's patch of everything since the last one, into the copy and into `ch`. */
export function project(model, ch = emptyChange(), now = NOW) {
  const text = boardOf(model).finish(now)
  if (!text) return ch
  const patch = revivePatch(text)
  const got = applyPatch(model, patch)
  relink(model, patch)
  if (patch.devregs) { const d = devregsOf(model); d.clear(); for (const [k, v] of patch.devregs) d.set(k, v) }
  for (const k of ['cards', 'sessions', 'permissions', 'notes', 'published', 'timelines', 'registers', 'invites']) for (const id of got[k]) ch[k].add(id)
  for (const [k, list] of got.items) { let l = ch.items.get(k); if (!l) ch.items.set(k, l = []); l.push(...list) }
  ch.members ||= got.members; ch.alerts ||= got.alerts; ch.stack ||= got.stack; ch.room ||= got.room
  return ch
}
// The builder's calls of before, on the core's model (what a test calls directly).
const hexOf = b => (b == null ? null : [...b].map(x => x.toString(16).padStart(2, '0')).join(''))
export function echoTimelineItem(model, e, ch) { const it = JSON.parse(boardOf(model).echoItem(e.local_id, e.timeline_key, JSON.stringify(e.content), e.recipient_device_id ?? null, e.object_id ?? null, e.now ?? NOW)); project(model, ch); return model.timelines.get(e.timeline_key)?.items.get(e.local_id) ?? it }
export function echoRegisters(model, e, ch) { boardOf(model).echoRegisters(e.local_id, JSON.stringify(e.values)); project(model, ch) }
export function echoAnswer(model, e, ch) { const ok = boardOf(model).echoAnswer(e.local_id, e.object_id, JSON.stringify(e.answer), e.object_state); project(model, ch); return ok }
export function echoNote(model, e, ch) { const key = boardOf(model).echoNote(e.local_id, e.object_id ?? null, JSON.stringify(e.fields), !!e.closed); project(model, ch); return key }
export function confirmEcho(model, local_id, sealed, ch) { boardOf(model).confirmEcho(local_id, hexOf(sealed.envelopeHash), sealed.seq, sealed.objectId ? hexOf(sealed.objectId) : null); project(model, ch) }
export function rollbackEcho(model, local_id, ch) { boardOf(model).rollbackEcho(local_id); project(model, ch) }
export function applyPresence(model, reports, ch, now = NOW) { boardOf(model).applyPresence(JSON.stringify([...reports]), now); project(model, ch, now) }
/** A timeline's window is open (the views read it): it counts headers too. */
export function openWindow(model, key) { boardOf(model).setWindow(key, true, 0, false); project(model) }
/** A timeline of the copy (an empty one when the model holds none). */
export function timelineOf(model, key) {
  const t = model.timelines.get(key)
  if (t) return t
  const at = key.indexOf(':'), id = key.slice(at + 1)
  return { timeline_key: key, timeline_kind: key.slice(0, at), timeline_id: id, object_id: id.slice(id.indexOf('/') + 1), item_count: 0, newest_envelope_number: 0, items: new Map(), loaded_down_to: Infinity, has_more: false, window_open: false }
}

/** n bytes that begin with `tag`, as the wire's base64url. */
export const bytes = (n, ...tag) => { const out = new Uint8Array(n); out.set(tag); return b64u(out) }
/** A device id (32 bytes), a 16-byte id, as base64url; hex() gives the model's form. */
export const device = k => bytes(32, 0xd0, k)
export const id16 = (...tag) => bytes(16, ...tag)
export const hex = idToHex
/** An id of the tests as the core's bytes (null stays null). */
export const raw = id => (id == null ? null : unb64u(id))
const utf8 = text => new TextEncoder().encode(text)
export const NOW = 1_760_000_000_000

/** The model as comparable data: Maps as sorted entries, the builder's own state left out, key order ignored. */
export const canon = v => JSON.stringify(v, (k, x) => {
  if (k === '_builder') return undefined
  if (x instanceof Map) return { $map: [...x].sort((a, b) => String(a[0]).localeCompare(String(b[0]))) }
  if (x instanceof Set) return { $set: [...x].sort() }
  if (typeof x === 'number' && !Number.isFinite(x)) return { $num: String(x) }
  if (x && typeof x === 'object' && !Array.isArray(x)) return Object.fromEntries(Object.keys(x).sort().map(key => [key, x[key]]))
  return x
})

export class World {
  constructor() {
    this.model = emptyModel()
    this.change = 0          // the hub's counter
    this.seq = new Map()     // `${group}/${sender}` -> the sender's last envelope number there
    this.hashes = 0
    this.objects = new Map() // object id -> { owner, current, state }
    this.me = device(1)
    this.phone = device(2)
    this.agent = device(10)
    this.room_group = bytes(32, 0xaa)
    this.session = id16(0x51)
    this.session_group = bytes(32, 0xbb, 1)
    this.model.room.my_device_id = hex(this.me)
    this.model.room.my_role = 'human'
    withDesk(this.model)
    this.board = boardOf(this.model)
    this.clock = NOW
    this.last = emptyChange()
  }
  /** A room of two human devices and one main session with its agent device. */
  groups(extra = [], roles = {}) {
    const humans = roles.humans ?? [this.me, this.phone], agents = roles.agents ?? [this.agent]
    const list = [
      { group: this.room_group, session: null, epoch: roles.epoch ?? 3, leaves: humans },
      { group: this.session_group, session: { session_id: this.session, parent: null }, epoch: 2, leaves: [...humans, ...agents.slice(0, 1)] },
      ...extra,
    ].map(g => this.group(g))
    return this.run(() => this.board.applyGroups(list, { epoch: roles.epoch ?? 3, state: new Uint8Array(32), humans: humans.map(raw), agents: agents.map(raw), recoverySignatureKey: new Uint8Array(32), recoveryHpkeKey: roles.recovery ?? new Uint8Array(32), recoveryAuth: null }))
  }
  /** A GroupSummary; a main session's parent is zeros. */
  group({ group, session, epoch, leaves, disallowed = [], archived = false, pending = false }) {
    return { group: raw(group), session: session && { roomId: new Uint8Array(32), sessionId: raw(session.session_id), parent: session.parent ? raw(session.parent) : new Uint8Array(16) }, epoch, leaves: leaves.map(raw), disallowed: disallowed.map(raw), missingOpener: null, stale: disallowed.length > 0, archived, pending, ownFrom: 0, pastLearned: true }
  }
  /** A CommitSummary (`epoch`: the one it builds on), handed to the model. */
  commit({ group = this.room_group, epoch, committer = this.me, adds = [], removes = [], external = false, agents = null, time = this.clock }, opts = {}) {
    return this.run(() => this.board.applyCommit({ group: raw(group), epoch, committer: raw(committer), external, adds: adds.map(raw), removes: removes.map(raw), cuts: [], agents: agents && agents.map(raw), roomEpoch: null, time }, !!opts.by_recovery, !!opts.removed_me, opts.now ?? this.clock))
  }
  /** One step of a turn's work trail (a ReceivedMessage of kind 'workTrail') under the hub's change number `n`. */
  trail(session, { from = this.agent, turn, number, step }, n) {
    const message = { kind: 'workTrail', from: raw(from), keysTaken: 0, last: false, board: null, turn: raw(turn), number, time: this.clock += 500, payload: utf8(typeof step === 'string' ? step : JSON.stringify(step)) }
    return this.run(() => this.board.applyWorkTrail(hex(session), message.from, message.turn, message.number, message.time, message.payload, n))
  }
  /** A piece of a stroke in progress (a ReceivedMessage of kind 'strokePiece'). */
  piece(board, piece, { from = this.phone } = {}) {
    const message = { kind: 'strokePiece', from: raw(from), keysTaken: 0, last: false, board: raw(board), turn: null, number: 0, time: 0, payload: utf8(piece) }
    return this.run(() => this.board.applyStrokePiece(message.from, message.board, message.payload, this.clock))
  }
  /** What sealing an own envelope gives back. */
  sealed(hash, seq, { group = this.session_group, object_id = null } = {}) {
    return { outboxId: seq, envelopeHash: raw(hash), seq, group: raw(group), objectId: raw(object_id), time: this.clock }
  }
  /** Run a builder call with a fresh change, project, and keep the change as `last`. */
  run(fn) {
    const c = emptyChange()
    const result = fn(c)
    project(this.model, c, this.clock)
    this.last = c
    this.onChange?.(c)       // (a test that follows every change: the cache, the page's copy)
    return result
  }
  hash() { return bytes(32, 0xee, ++this.hashes >> 8, this.hashes & 255) }
  /**
   * One ReceivedEnvelope, as the core would hand it over, from a test's own words (snake_case, ids as base64url).
   * `payload`: the body as an object (the wire's form) or null. Not given: outcome 'applied', the next change number,
   * the sender's next envelope number, a fresh hash.
   */
  envelope({ kind, sender = this.agent, session = this.session, group, recipient = null, time, timeline = null, object = null, register_id = null, payload = {}, bind = null,
    outcome = 'applied', code = null, object_after, register = null, change, seq, hash, push = false, file_ids = [] }) {
    const g = group ?? (session ? this.session_group : this.room_group)
    const chain = `${g}/${sender}`
    const number = seq ?? (this.seq.get(chain) ?? 0) + 1
    if (outcome !== 'refused') this.seq.set(chain, Math.max(this.seq.get(chain) ?? 0, number))
    const envelope_hash = hash ?? this.hash()
    const n = change ?? ++this.change
    let after = object_after
    if (after === undefined && object && (outcome === 'applied' || (outcome === 'chained' && ['pruned', 'newer-version', 'no-key', 'decrypt-failed', 'bad-format'].includes(code)))) {
      const known = this.objects.get(object.object_id) ?? { owner: sender, current: envelope_hash }
      // a version (and a request) becomes the current one; an answer, a take back and a verdict leave it
      if (kind === 'version' || kind === 'request') known.current = envelope_hash
      known.state = object.object_state
      this.objects.set(object.object_id, known)
      after = { object_id: object.object_id, owner: known.owner, object_state: known.state, current_version: known.current }
    }
    // (a test names a timeline in the wire's words: kind chat or board, scope, ref)
    const tl = timeline && { kind: timeline.kind === 'board' ? 'board' : timeline.scope === 'card' ? 'cardChat' : 'sessionChat', id: raw(timeline.ref) }
    const header = {
      group: raw(g), sessionId: raw(session), epoch: 2, sender: raw(sender), seq: number, prev: new Uint8Array(32), recipient: raw(recipient), time: time ?? (this.clock += 1000),
      kind: kind === 'take_back' ? 'takeBack' : kind, push, timeline: tl, registerId: raw(register_id),
      object: object && { objectId: raw(object.object_id), objectType: object.object_type, objectState: object.object_state, urgency: object.urgency, answeredAt: object.answered_at, objectRef: raw(object.object_ref) ?? new Uint8Array(32) }, fileIds: file_ids.map(raw),
      reservedKind: null, reservedBlock: null,
    }
    const flat = { objectId: null, requestId: null, versionHash: null, previousHash: null, requestHash: null, choices: [], expiresAt: 0, allow: false }
    const coreBind = !bind ? null
      : bind.kind === 'answer' ? { ...flat, kind: 'answer', objectId: raw(bind.object_id), versionHash: raw(bind.version_hash), choices: bind.choices }
        : bind.kind === 'request' ? { ...flat, kind: 'request', requestId: raw(bind.request_id), expiresAt: bind.expires_at }
          : bind.kind === 'verdict' ? { ...flat, kind: 'verdict', requestId: raw(bind.request_id), requestHash: raw(bind.request_hash), expiresAt: bind.expires_at, allow: bind.allow }
            : { ...flat, kind: 'takeBack', objectId: raw(bind.object_id), previousHash: raw(bind.previous_hash), versionHash: raw(bind.version_hash) }
    return {
      change: n, envelopeHash: raw(envelope_hash), header, outcome, code, finding: null,
      payload: payload === null || outcome === 'chained' || outcome === 'void' || outcome === 'refused' ? null : utf8(JSON.stringify(payload)),
      bind: outcome === 'applied' || outcome === 'provisional' ? coreBind : null,
      objectAfter: after ? { objectId: raw(after.object_id), objectType: object?.object_type ?? 'card', owner: raw(after.owner), objectState: after.object_state, current: raw(after.current_version), answer: null } : null,
      register: register && { of: null, ...register }, confirmed: null, dropped: null, replayed: false, command: false,
    }
  }
  /** Build an envelope (or take one already built) and hand it to the model. Returns the core's value `r`, the
   *  model's `result`, and the envelope's `hash` (base64url, to name it in a later one), `change` and `time`. */
  take(spec, ctx = { now: this.clock }) {
    const r = spec.header ? spec : this.envelope(spec)
    const result = this.run(() => { const [a] = JSON.parse(this.board.applyEnvelopes([r], ctx.now)); return a.refused == null ? { applied: a.applied } : a })
    return { r, result, hash: b64u(r.envelopeHash), change: r.change, time: r.header.time }
  }

  // ---- the stories' sentences ----
  object(object_id, object_type, object_state = 'open', { urgency = 'normal', answered_at = 0, object_ref = null } = {}) {
    return { object_id, object_type, object_state, urgency, answered_at, object_ref }
  }
  /** A card version from the agent. `previous`: the envelope (hash) it follows. */
  card(object_id, fields, { state = 'open', urgency = 'normal', previous = null, ...rest } = {}) {
    return this.take({ kind: 'version', object: this.object(object_id, 'card', state, { urgency, object_ref: previous }), payload: { schema_version: 2, ...fields, ...(previous ? { previous_version_hash: previous } : {}) }, ...rest })
  }
  /** An answer of this device to the card's current version. */
  answer(object_id, fields, { state = 'answered', sender = this.me, version, ...rest } = {}) {
    const version_hash = version ?? this.objects.get(object_id).current
    const choices = fields.choices ?? []
    return this.take({ kind: 'answer', sender, recipient: this.agent, object: this.object(object_id, 'card', state, { answered_at: this.clock + 1000, object_ref: version_hash }),
      payload: { schema_version: 2, ...fields }, bind: { kind: 'answer', object_id, version_hash, choices }, ...rest })
  }
  takeBack(object_id, previous_hash, { sender = this.me, ...rest } = {}) {
    const version_hash = this.objects.get(object_id).current
    return this.take({ kind: 'take_back', sender, recipient: this.agent, object: this.object(object_id, 'card', 'open', { object_ref: version_hash }), payload: { schema_version: 2 }, bind: { kind: 'take_back', object_id, previous_hash, version_hash }, ...rest })
  }
  /** A Chat message on the session's or a card's timeline. */
  message(fields, { card = null, sender = this.agent, ...rest } = {}) {
    const timeline = card ? { kind: 'chat', scope: 'card', ref: card } : { kind: 'chat', scope: 'session', ref: this.session }
    return this.take({ kind: 'item', sender, recipient: sender === this.agent ? null : this.agent, timeline, payload: { schema_version: 2, content_type: 'message', ...fields }, ...rest })
  }
  /** A register value. In the room group by default for a human sender, in the session group for the agent. */
  register(name, value, { sender = this.me, session = sender === this.agent ? this.session : null, current = true, lamport = 1, ...rest } = {}) {
    return this.take({ kind: 'register', sender, session, register_id: id16(0x7e), payload: { name, value, lamport }, register: { name, current }, ...rest })
  }
}

/** A session record in a copy, made if it is new (a test's own model: the views' tests build one by hand). */
export function sessionOf(model, session_id) {
  let s = model.sessions.get(session_id)
  if (!s) {
    s = { session_id, group_id: null, agent_device_ids: [], agent_device_id: null, agent_session_id: null, device_name: '', is_active: true, stale: false, group_archived: false,
      is_online: false, offline_since: null, link: null, heard_up_to: null, heard_at: null, session_key_epoch: 0, created_by_agent: false, creator_device_id: null, parent_session_id: null,
      profile: null, status_lines: [], agent_alerts: [], registers: new Map(), settings: null, card_ids: [], open_card_ids: [],
      timeline_key: `chat:session/${session_id}`, last_activity_at: 0 }
    model.sessions.set(session_id, s)
  }
  return s
}
