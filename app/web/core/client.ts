// client.ts: the `Client` the core worker runs and the views call (through remote.ts): the model, the human actions
// by the names the views know, timelines, files, invites, sessions, push. Contract with the views: README.md "The
// model", "Human actions", "Change notifications", and worker-protocol.ts `CALLS`.
//
// The division of labour under protocol v1 (spec/v1.md):
//   the core (core-api.ts)   every rule and every key: groups, chains, object state, who may write what
//   engine.ts                owns the device: outbox pump, catch-up in the hub's order, the live stream, upkeep
//   model.ts                 turns what the core accepted into the model
//   this file                connects them: an action is an optimistic echo in the model, a draft sealed by the
//                            engine, and an outbox item until the hub answered; what the engine reports is applied
//                            to the model, projected, told to the views and written to the local cache
//
// Optimistic echoes: the model shows an own send before it is sealed (`pending: true`), the echo is replaced in
// place when the hub's copy comes back, and rolled back when sealing fails or the hub refuses the envelope for
// good (the outbox item is `failed` in that change, then gone). While the pump is halted on an envelope the item
// is `blocked` and `room.outbox_blocked` says why.
//
// The local cache (store-idb.ts `Cache`) holds the model's records, written shortly after each batch (one
// `setMany`) together with the cursor up to which the model holds everything the core took. It is NOT
// authoritative: the core's state is. At a start, a cache at the device's cursor is the model. One behind it (the
// page went away after the core took an item and before the cache was written) is shown, and everything after
// its cursor is read back from the hub through the core before it counts as whole again. One ahead of the device
// is dropped, and the model is built again from the device's groups, the hub's Desk and all of the room's changes.
//
// Nothing here logs. A file key, a share link's secret and a recovery code never go into an error's text.
import { attachmentFromWire, attachmentRef, encodeBodyBytes, encodePiece, encodeRegister, fileIdsOf, fileRefOf, UPDATE_MESSAGE } from './codec.ts'
import type { Fields } from './codec.ts'
import { relink, revivePatch, snapshotForCore } from './board.ts'
import type { ClientModel, Core, Device, Draft, FileRef, OutboxEntry, ReceivedEnvelope, Sealed } from './core-api.ts'
import { Engine } from './engine.ts'
import type { EngineEvents, EngineMeta } from './engine.ts'
import { accountCopiesBytes, HubError } from './hub.ts'
import type { AccountCopies, EnvelopeItem, Hub, StreamEvent } from './hub.ts'
import { b64u, hex, unb64u, unhex } from './ids.ts'
import { applyPatch } from './mirror.ts'
import * as M from './model.ts'
import { mark, timed } from './spent.ts'
import type { Cache } from './store-idb.ts'
import type { Answer, AttachmentRef, Card, Change, Invite, Model, Note, OutboxItem, Session, TimelineItem, Vouch } from './types.ts'

/** The first desk of every account: its id and its register as account creation writes it (spec/v1.md). */
export const DEFAULT_DESK = 'main'
export const FIRST_DESK = Object.freeze({ name: 'Personal', created_at: 0 })

export class ClientError extends Error {
  code: string
  constructor(code: string, message: string) { super(message); this.name = 'ClientError'; this.code = code }
}
const fail = (code: string, message: string): never => { throw new ClientError(code, message) }
type Listener = (data: any) => void
type Sent = { local_id: string; envelope_hash: string; seq: number }

/** How many items of each Chat this browser keeps in its cache; older ones are read again from the hub on scroll. */
export const KEEP_PER_CHAT = 500
/** How many items of a Chat are written to the cache between two trims of it. */
const TRIM_EVERY = 100
/** How long the model's changes are gathered before one write to the cache. Short: a page that goes away meanwhile
 *  finds its cache behind the device and reads the difference back (see `open`). */
const FLUSH_MS = 40
/** A running human device reads what changed on the hub's Desk after every catch-up, shortly after a card or a
 *  permission request came on the stream (`DESK_AFTER_MS`, several fold into one), and else every `DESK_EVERY_MS`;
 *  the whole list at the start, when its groups changed, and every `DESK_WHOLE_MS` (`readDesk`). */
const DESK_EVERY_MS = 5 * 60_000
const DESK_WHOLE_MS = 30 * 60_000
const DESK_AFTER_MS = 1500
/** How many object reads of a Desk reading run at once. */
const DESK_PARALLEL = 6
/** More versions lacking than this: they are read with the full Desk in one answer, not one by one. */
const DESK_ONE_BY_ONE = 40
/** How soon the hub's Desk is read again after a session group was joined (`deskSoon`). */
const DESK_SOON_MS = 250
/** A start from the cache waits this long for the hub's Desk before its first paint (`deskFirst`); past it, the open
 *  cards wait off the Desk until the list answers, and past DESK_HOLD_MAX_MS the last list kept here decides. */
const DESK_FIRST_MS = 300
const DESK_HOLD_MAX_MS = 5000
/** A failed read of the hub's archived sessions (`archivedSoon`) is tried again after this long. */
const ARCHIVED_RETRY_MS = 3000
/** Where the hub's Desk as last read is kept between runs. */
const DESK_KEY = 'client/desk'
/** How many answered cards a reading of the Desk asks the hub about (`readDesk`). */
const ANSWERED_ASKED = 20
const CHUNK = 65_536
const MIB_ = 1 << 20
const DAY = 86_400_000
const INVITE_CONFIRM_MS = 5 * 60_000
const MODEL_KEYS: readonly string[] = ['card/', 'session/', 'perm/', 'note/', 'pub/', 'tlmeta/', 'reg/', 'tl/', 'members', 'devregs', 'room', 'newer', 'alerts']
/** `fn` over `items`, at most DESK_PARALLEL at once, the answers in the items' order. */
async function inParallel<T, R>(items: readonly T[], fn: (item: T) => Promise<R>): Promise<R[]> {
  const out: R[] = new Array(items.length)
  let next = 0
  await Promise.all(Array.from({ length: Math.min(DESK_PARALLEL, items.length) }, async () => { while (next < items.length) { const at = next++; out[at] = await fn(items[at]!) } }))
  return out
}
const randomHex = (bytes: number): string => hex(crypto.getRandomValues(new Uint8Array(bytes)))
const sameBytes = (a: Uint8Array, b: Uint8Array): boolean => a.length === b.length && a.every((v, i) => v === b[i])
const codeText = (numbers: readonly number[]): string => numbers.map(n => String(n).padStart(2, '0')).join('-')
/** The app's origin of an address the views name (`https://app.trommi.com/join`, `https://app.trommi.com`). */
const originOf = (app_url: string): string => { try { return new URL(app_url).origin } catch { return app_url.replace(/\/+$/, '') } }
/** A Note's own fields: everything of the model's note but what the model adds. */
function noteFields(n: Record<string, unknown>): Fields {
  const { object_id: _o, by_device_id: _b, object_version: _v, version_hash: _h, version_hashes: _hs, causal: _c, envelope_number: _n, object_state: _s, pending: _p, unsupported: _u, local_id: _l, _base, previous_version_hash: _pv, lamport: _lp, ...rest } = n
  return rest
}

// ---- files (spec 11): the core encrypts and decrypts piece by piece, the hub stores -------------------------------------

/** Encrypts `data` in pieces of 64 KiB and stores it at the hub. Returns what opens it again. `fileKey` is a secret. */
export async function putEncrypted(core: Core, hub: Hub, data: Uint8Array | Blob): Promise<{ file: FileRef; size: number }> {
  const encryptor = new core.FileEncryptor()
  const size = data instanceof Uint8Array ? data.length : data.size
  const stored: Uint8Array<ArrayBuffer>[] = []
  try {
    for (let at = 0; at < size; at += CHUNK) {
      const piece = data instanceof Uint8Array ? data.subarray(at, at + CHUNK) : new Uint8Array(await data.slice(at, at + CHUNK).arrayBuffer())
      stored.push(encryptor.update(piece) as Uint8Array<ArrayBuffer>)
    }
  } catch (e) { (encryptor as { close?: () => void }).close?.(); throw e }
  const end = encryptor.finish()
  stored.push(end.stored as Uint8Array<ArrayBuffer>)
  // the hub wants the length up front: the pieces are handed over as one Blob, not copied into one array
  await hub.putFile(end.file.fileId, new Blob(stored))
  return { file: end.file, size: end.plainLen }
}
/** Decrypts a stored file as the core says (11.2): what `update` handed out counts only once `finish` succeeded. */
function decrypt(core: Core, file: FileRef, stored: Uint8Array): Uint8Array {
  const decryptor = new core.FileDecryptor(file)
  const plain: Uint8Array[] = []
  let size = 0
  try {
    for (let at = 0; at < stored.length; at += CHUNK) { const p = decryptor.update(stored.subarray(at, at + CHUNK)); plain.push(p); size += p.length }
  } catch (e) { (decryptor as { close?: () => void }).close?.(); throw e }
  const last = decryptor.finish()
  plain.push(last); size += last.length
  const out = new Uint8Array(size)
  let at = 0
  for (const p of plain) { out.set(p, at); at += p.length }
  return out
}
export async function getDecrypted(core: Core, hub: Hub, file: FileRef): Promise<Uint8Array> {
  return decrypt(core, file, (await hub.getFile(file.fileId)).bytes)
}
/** The stored bytes a file is decrypted from: kept in this browser's cache as the hub serves them, still encrypted
 *  (`fetchAttachment`). Small ones only, and at most FILES_KEPT bytes in all, the least recently used dropped. */
const FILE_KEPT_MAX = 4 * MIB_
const FILES_KEPT = 96 * MIB_

// ---- Share links (11.5) ---------------------------------------------------------------------------------------------

const B64_16 = '[A-Za-z0-9_-]{22}', B64_32 = '[A-Za-z0-9_-]{43}'
const SHARE = new RegExp(`/(?:artifact|a)/(${B64_16})#(${B64_32})\\.(${B64_32})\\.(${B64_32})$`)
/** A Share link's parts by its form alone, for a page that has not loaded the core yet (the core checks it for
 *  real in `openShared`). Throws `bad-format`. `share_secret` and `file_key` are secrets. */
export function parseShareLink(link: unknown): { share_id: string; share_secret: string; file_key: string; sha256: string } {
  const m = SHARE.exec(String(link).trim())
  if (!m) return fail('bad-format', 'not a share link')
  return { share_id: hex(unb64u(m[1]!)), share_secret: m[2]!, file_key: m[3]!, sha256: m[4]! }
}
/** The shared file's bytes for whoever holds the link: no room, no sign-in. `core`: the loaded core. */
export async function openShared(core: Core, hub: Hub, link: string): Promise<Uint8Array> {
  const share = core.shareLinkParse(String(link).trim())
  const { bytes } = await hub.getShared(share.shareId, unb64u(share.secret))
  // a Share link names no file id: the core's decryptor reads it from the file's head and checks the hash
  return decrypt(core, { fileId: bytes.subarray(1, 17).slice(), fileKey: share.fileKey, sha256: share.sha256 }, bytes)
}

// ---- the client -----------------------------------------------------------------------------------------------------

export interface ClientOptions {
  core: Core
  hub: Hub
  engine: Engine
  cache: Cache
  /** This device's name and where it runs, written once as its register `device/<id>`. */
  device_name?: string
  device_info?: unknown
  now?: () => number
}
/** The most live sessions of one account that joined by themselves (12.4), links still open counted in; the most
 *  of all accounts together (the account is the asking agent's word, so this one bounds a lying agent); and the most
 *  vouched links open at once on this device (the hub keeps 16 open invites a room: the rest stay for the person). */
const MAX_VOUCHED = 8
const MAX_VOUCHED_ROOM = 24
const MAX_VOUCHED_OPEN = 8
/** An invite as this device keeps it beside what the model shows. */
interface InviteKept { public: Invite; checked_at?: number; done?: boolean; code?: number[]; request_hash?: string; committed?: boolean }

/** A board's id from its timeline id (`desk/<hex>`). */
const boardOf = (timeline_id: string): Uint8Array => unhex(timeline_id.replace(/^desk\//, ''))
/** A snapshot frontier in wire ids, or null when a head has no hash yet. */
function frontierWire(frontier: Record<string, [number, string | null]>): Record<string, [number, Uint8Array]> | null {
  const wire: Record<string, [number, Uint8Array]> = {}
  for (const [writer, [seq, hash]] of Object.entries(frontier)) {
    if (!hash) return null
    wire[b64u(unhex(writer))] = [seq, unhex(hash)]
  }
  return wire
}
/** The files a snapshot's shapes (wire form: `attachment.file_id`, base64url) name: its pictures and posters. */
function snapshotFiles(shapes: readonly Record<string, unknown>[]): Uint8Array[] {
  const strokes = shapes.flatMap(s => {
    if (!s || typeof s !== 'object' || !s['attachment']) return []
    try { return [{ attachment: attachmentFromWire(s['attachment']) }] } catch { return [] }
  })
  return fileIdsOf('board_item', { strokes })
}

export class Client {
  model: Model
  /** The client core's board model (client/src/model): it builds the model; `model` is its copy, patched once per batch. */
  private readonly board: ClientModel
  readonly core: Core
  readonly hub: Hub
  readonly engine: Engine
  readonly room_id: Uint8Array
  readonly my_device_id: string
  stats: Record<string, number> | null = null

  private readonly cache: Cache
  private readonly opts: ClientOptions
  private readonly now: () => number
  private readonly listeners = new Map<string, Set<Listener>>()
  private change: Change = M.emptyChange()
  private alertsSeen = new Set<string>()
  /** Outbox id of a sealed envelope -> its echo and its item in model.outbox. */
  /** `again`: what seals the same item once more, for an envelope the hub held as stale (5.2.8): this device sealed
   *  it before it knew of the removal. Used once. */
  private readonly sending = new Map<number, { local_id: string; item: OutboxItem; again?: (() => Promise<unknown>) | null }>()
  private readonly dirty = new Map<string, unknown>()
  private flushTimer: ReturnType<typeof setTimeout> | null = null
  private flushing: Promise<void> = Promise.resolve()
  private readonly attachmentCache = new Map<string, Uint8Array>()
  private readonly invites = new Map<string, InviteKept>()
  private inviteTimer: ReturnType<typeof setInterval> | null = null
  private readonly checking = new Set<string>()
  /** Self-join (spec/v1.md 12.4): the asks this device answered or is answering, by nonce; invites confirmed on their own. */
  private readonly vouchAnswered = new Set<string>()
  private readonly vouchConfirming = new Set<string>()
  private readonly vouchRetiring = new Set<string>()
  /** Who waits for an invite that was confirmed to be through. */
  private readonly finishing = new Map<string, { resolve(): void; reject(e: unknown): void }>()
  /** A note's newest version as THIS client sealed it (ahead of the model while versions are in flight). */
  /** Notes with a version on its way to the hub and back: its hash and outbox entry, and the edit that waits behind it. */
  private readonly noteBusy = new Map<string, { hash: string; outbox: number; next: { own: Fields; closed: boolean; local_id: string } | null }>()
  private lamport = 0
  private started = false
  private removed = false
  /** Not null: the model is not all the device holds; everything after this change is being read back. */
  private rebuild: number | null = null
  /** The cursor up to which everything the core took is in the model and handed to the cache; -1 while rebuilding. */
  private stamped = -1
  private rebuilding = false
  /** The hub's Desk as last read (`readDesk`): what it lists as open, and its cursor. The hub's list decides what
   *  stands on the Desk (an open card it does not list is settled at the hub: `catching_up`, off the Desk), while
   *  the content of each card is still the core's, verified on this device. */
  private desk: { open: Set<string>; versions: Map<string, number>; at: number } | null = null
  private deskTimer: ReturnType<typeof setTimeout> | null = null
  private deskReading: Promise<void> | null = null
  /** A reading of this run read the versions it lacked (`readDeskNow`). */
  private deskVersionsRead = false
  /** Asked for while a reading ran: one more after it. */
  private deskAgain = false
  /** The next reading is of the whole list (`readDesk`); when the last whole one was. */
  private deskWholeDue = true
  private deskWholeAt = 0
  /** The hub's Desk of THIS run decided what stands open (`knowDesk`); until then the model is the cache's, and a card
   *  closed elsewhere since the last run would still stand. False only on a start from the cache. */
  private deskKnown = true
  /** Past DESK_FIRST_MS without the hub's answer: every open card waits off the Desk (the client core holds them all: `M.setDeskHold`). */
  private deskHeld = false
  private deskWait: Promise<void> | null = null
  private deskWaitDone: (() => void) | null = null
  private deskHoldTimer: ReturnType<typeof setTimeout> | null = null
  /** The cards `readDesk` read the newest Chat page of, with the hand back it was read for. */
  private revisionRead = new Map<string, number>()
  /** Work the room's reading waits for (`readingUntil`): the session groups of a sign-in being joined. */
  private readingHolds = 0
  /** How many session groups this device was in when it last read the hub's Desk for them. */
  private sessionGroups = 0
  /** The cursor the cache was last written with. */
  private written = -2
  private goalsTimer: ReturnType<typeof setTimeout> | null = null
  private readonly offs: (() => void)[] = []

  constructor(opts: ClientOptions) {
    this.opts = opts
    this.core = opts.core
    this.hub = opts.hub
    this.engine = opts.engine
    this.cache = opts.cache
    this.now = opts.now ?? Date.now
    this.room_id = opts.engine.room_id ?? fail('no-room', 'this device has no room')
    this.my_device_id = hex(opts.engine.device_id)
    this.model = M.emptyModel()
    // (an agent that waits for its main session is given one once it is connected: engine.ts `seatAgents`)
    opts.engine.isOnline = device => this.model.members.get(hex(device))?.is_online === true
    this.board = new this.core.ClientModel(this.my_device_id)
    this.listen()
  }
  get is_human(): boolean { return this.engine.is_human }

  /** Fills the model before anything is fetched: from the cache if it was written at the device's cursor, else from
   *  the device's groups alone (the rest follows in `start`). Called once, by room.ts. */
  async open(): Promise<this> {
    const e = this.engine
    const kept = (await this.cache.get('client/at').catch(() => null)) as { cursor: number; device: string } | null
    // The cache belongs to the cursor it was written at. At the device's cursor: it is the model. Behind it (the
    // page went away between "the core took an item" and "the cache was written", or a rebuild was cut short,
    // which is written as -1): what it holds is still true, and everything after its cursor is read back from
    // the hub through the core before the cache counts as whole again. Ahead of the device, or another device's:
    // dropped.
    if (kept && kept.device === this.my_device_id && kept.cursor <= e.cursor) {
      this.model = M.modelFromCache(await this.cache.range(''))
      this.model.room.my_device_id = this.my_device_id
      // (the core builds the model from here on; a mark of an earlier run is the past: `readDesk` sets it again)
      this.board.load(snapshotForCore(this.model))
      // What stands open is what the hub's Desk listed when this device last read it (kept beside the cache): a card
      // it had settled stays off the Desk although this device never read its closing. The Desk of now follows
      // (`deskFirst`, `readDesk`), and only it lets anything else stand.
      if (e.is_human) {
        const last = (await this.cache.get(DESK_KEY).catch(() => null)) as { open?: string[]; versions?: [string, number][]; at?: number } | null
        if (last && Array.isArray(last.open)) this.desk = { open: new Set(last.open), versions: new Map(last.versions ?? []), at: Number(last.at) || 0 }
        this.deskKnown = false
      }
      if (kept.cursor === e.cursor) this.stamped = this.written = kept.cursor
      else this.rebuild = Math.max(0, kept.cursor)
    } else {
      if (kept) await this.dropCache()
      if (e.cursor > 0) this.rebuild = 0
    }
    const room = this.model.room
    Object.assign(room, { room_id: hex(this.room_id), hub_url: this.hub.hub_url, my_device_id: this.my_device_id, my_role: e.is_human ? 'human' : 'agent', last_envelope_number: e.position, connection: 'offline', outbox_blocked: null })
    const ch = M.emptyChange()
    ch.room = true
    this.board.applyGroups(e.groups, e.roles ?? null)
    if (this.desk) this.deskToModel()
    this.sessionGroups = e.groups.filter(g => g.session && !g.archived).length
    for (const [key, value] of await this.cache.range('client/invite/')) { const k = value as InviteKept; this.invites.set(key.slice('client/invite/'.length), k); if (!k.done) this.model.invites.set(k.public.invite_id, k.public) }
    this.fromBoard(ch)
    this.record(ch)
    return this
  }
  private async dropCache(): Promise<void> {
    const gone: [string, undefined][] = []
    for (const prefix of MODEL_KEYS) for (const [key] of await this.cache.range(prefix)) gone.push([key, undefined])
    gone.push([DESK_KEY, undefined])
    if (gone.length) await this.cache.setMany(gone)
  }

  // ---- events

  on(event: string, fn: Listener): () => void {
    let set = this.listeners.get(event)
    if (!set) this.listeners.set(event, set = new Set())
    set.add(fn)
    return () => this.off(event, fn)
  }
  off(event: string, fn: Listener): void { this.listeners.get(event)?.delete(fn) }
  private emit(event: string, data: unknown): void { for (const fn of [...this.listeners.get(event) ?? []]) { try { fn(data) } catch { /* a listener's fault is the listener's */ } } }

  /** Ends a batch: projects, tells the views what changed, hands the change's records to the cache. */
  private publish(): void {
    const ch = this.change
    if (this.model.room.last_envelope_number !== this.engine.position) { this.model.room.last_envelope_number = this.engine.position; ch.room = true }
    const reading = this.readingHolds > 0 || this.rebuild !== null || this.engine.rescanPending
    if (this.model.room.reading !== reading) { this.model.room.reading = reading; ch.room = true; this.board.setReading(reading) }
    this.fromBoard(ch)
    // (the engine moves its cursor only after an item's effects were reported: at any publish the model holds
    // everything up to it, and this change carries what the cache lacks of it)
    if (this.rebuild === null) this.stamped = this.engine.cursor
    if (M.changeIsEmpty(ch)) { if (this.rebuild === null && this.written !== this.stamped) this.touched(); return }
    this.change = M.emptyChange()
    this.record(ch)
    this.emit('change', ch)
    if (ch.alerts) for (const a of this.model.alerts) if (!this.alertsSeen.has(a.alert_id)) { this.alertsSeen.add(a.alert_id); this.emit('alert', a) }
    if (this.alertsSeen.size > 1000) this.alertsSeen = new Set(this.model.alerts.map(a => a.alert_id))
    if (this.is_human && (ch.sessions.size || [...ch.registers].some(k => k.startsWith('desk/') || k.startsWith('session/')))) this.goalsSoon()
    if (this.is_human && (ch.sessions.size || ch.registers.size)) this.vouchSoon()
  }
  /** Chat items written to the cache since each Chat was last trimmed (`trim`). */
  private readonly sinceTrim = new Map<string, number>()
  private trimming: Promise<void> = Promise.resolve()
  private noteWritten(taken: [string, unknown][]): void {
    for (const [key] of taken) {
      if (!key.startsWith('tl/chat:')) continue
      const timeline = key.slice(3, key.lastIndexOf('/'))
      this.sinceTrim.set(timeline, (this.sinceTrim.get(timeline) ?? 0) + 1)
    }
    const due = [...this.sinceTrim].filter(([, n]) => n >= TRIM_EVERY).map(([k]) => k)
    if (!due.length) return
    for (const k of due) this.sinceTrim.delete(k)
    this.trimming = this.trimming.then(async () => { for (const k of due) await this.trim(k).catch(() => {}) })
  }
  /**
   * Keeps the newest `KEEP_PER_CHAT` items of a Chat in this browser's cache and drops the older ones. They are not
   * lost: history is not forward secret (spec 1), the hub keeps a session's Chat, and scrolling up reads it again
   * from the hub through the core (`older`); a settled card's Chat the hub prunes after 30 days (9.4.1), and a device
   * drops what the hub prunes (9.4). The model's open window is left as it is.
   */
  private async trim(timeline_key: string): Promise<void> {
    const prefix = `tl/${timeline_key}/`
    const newest = await this.cache.range(prefix, { reverse: true, limit: KEEP_PER_CHAT + 1 })
    if (newest.length <= KEEP_PER_CHAT) return
    const edge = newest.at(-1)![0]
    const older = await this.cache.range(prefix, { before: edge })
    await this.cache.setMany([...older.map(([k]): [string, undefined] => [k, undefined]), [edge, undefined]])
  }
  /** The cache's cursor is to be written although no record changed. */
  private touched(): void { if (this.flushTimer === null) this.flushTimer = setTimeout(() => { this.flushTimer = null; void this.flush().catch(() => {}) }, FLUSH_MS) }
  /** A change made outside the engine's batches (an action's echo): published at once. */
  private tell(make: (ch: Change) => void): void { make(this.change); this.publish() }
  private record(ch: Change): void {
    for (const [key, value] of M.cacheRecords(this.model, ch)) this.dirty.set(key, value)
    if (this.flushTimer === null) this.flushTimer = setTimeout(() => { this.flushTimer = null; void this.flush().catch(() => {}) }, FLUSH_MS)
  }
  /** Writes what changed to the cache now: one transaction, with the cursor it belongs to. */
  flush(): Promise<void> {
    if (this.flushTimer !== null) { clearTimeout(this.flushTimer); this.flushTimer = null }
    return this.flushing = this.flushing.catch(() => {}).then(async () => {
      if (!this.dirty.size && this.written === (this.rebuild === null ? this.stamped : -1)) return
      const taken = [...this.dirty], stamp = this.rebuild === null ? this.stamped : -1
      this.dirty.clear()
      const entries: [string, unknown | undefined][] = [...taken, ['client/at', { cursor: stamp, device: this.my_device_id }]]
      try { await this.cache.setMany(entries); this.written = stamp; this.noteWritten(taken) } catch (e) {
        // not written: the records stay due (a newer value of one wins), so a later write cannot stamp the cursor
        // over a cache that lacks them
        for (const [key, value] of taken) if (!this.dirty.has(key)) this.dirty.set(key, value)
        throw e
      }
    })
  }

  // ---- what the engine reports -> the model

  private listen(): void {
    const e = this.engine
    const on = <K extends keyof EngineEvents>(event: K, fn: (data: EngineEvents[K]) => void): void => { this.offs.push(e.on(event, fn)) }
    on('envelope', ({ received, how }) => {
      this.board.applyEnvelopes([received], this.now())
      const o = received.header.object
      // a card or a request that came live: what the hub's Desk says of it now, shortly
      if (how === 'ordered' && (o?.objectType === 'card' || o?.objectType === 'request') && this.started && this.is_human && this.engine.connection === 'live') this.deskSoon(DESK_AFTER_MS)
      if (o?.objectType === 'note' && this.noteBusy.get(hex(o.objectId))?.hash === hex(received.envelopeHash)) this.noteBack(hex(o.objectId))
    })
    on('log', ({ item, done }) => {
      const room = item.group.length === 32
      if (done.commit) this.board.applyCommit(done.commit, room && done.commit.external, done.removed && room, this.now())
      const m = done.message
      if (m?.kind === 'workTrail') { const s = e.groups.find(g => sameBytes(g.group, item.group))?.session; if (s && m.from && m.turn) this.board.applyWorkTrail(hex(s.sessionId), m.from, m.turn, m.number, m.time, m.payload, item.change) }
      else if (m?.kind === 'recoveryAuthConflict') this.board.pushAlert('equivocation', 'two different confirmations of the recovery key were sent: neither replaces the one this device holds', item.change, m.from ? hex(m.from) : null, this.now())
      else if (m?.kind === 'newerVersion') this.board.noteNewer('message type', item.change)
    })
    on('relay', m => { if (m.kind === 'strokePiece' && m.from && m.board) this.board.applyStrokePiece(m.from, m.board, m.payload, this.now()) })
    on('groups', ({ groups, roles }) => {
      this.board.applyGroups(groups, roles ?? null)
      // The hub's Desk lists only what lies in the groups this device is in: a device just signed in joins its
      // session groups one by one after it is in the room, and each one joined adds its open cards to the Desk.
      const sessions = groups.filter(g => g.session && !g.archived).length
      if (sessions !== this.sessionGroups) {
        // (a session group joined later than the hub archived it, its Welcome read after the stream's word of the
        //  archiving: held as archived too, as at the start)
        if (sessions > this.sessionGroups && this.started) this.archivedSoon()
        this.sessionGroups = sessions; this.deskWholeDue = true; if (this.started && this.is_human) this.deskSoon()
      }
      const role = e.is_human ? 'human' : 'agent'
      if (this.model.room.my_role !== role) { this.model.room.my_role = role; this.change.room = true }
    })
    on('accepted', ({ entry }) => this.outboxDone(entry, null))
    on('refused', ({ entry, code }) => this.outboxDone(entry, code))
    on('blocked', blocked => {
      const held = blocked ? this.sending.get(blocked.entry.id) : null
      for (const s of this.sending.values()) if (s.item.outbox_state === 'blocked') { s.item.outbox_state = 'sending'; s.item.error = null }
      if (held) { held.item.outbox_state = 'blocked'; held.item.error = blocked!.code }
      this.model.room.outbox_blocked = blocked ? { local_id: held?.local_id ?? '', code: blocked.code, message: blocked.message } : null
      if (blocked) this.board.pushAlert('chain-halted', `the hub refused an envelope (${blocked.code}). Sending is halted; the same envelope is tried again.`, null, null, this.now())
      this.change.outbox = this.change.room = true
    })
    // (a device that was removed stays `removed`: that is the last word on its connection)
    on('connection', connection => {
      if (this.removed) return
      // (a catch-up through, or the stream back: the hub's Desk once more)
      if (connection === 'live' && this.model.room.connection !== 'live' && this.started) void this.readDesk().catch(() => {})
      this.model.room.connection = connection; this.change.room = true
    })
    on('alert', a => { this.board.pushAlert(a.code, a.message ?? '', a.change ?? null, null, this.now()) })
    on('stream', event => this.onStream(event))
    on('invites', ({ open }) => this.invitesDone(open))
    on('rescanned', () => { if (this.rebuilding) { this.rebuilding = false; this.rebuild = null; this.touched() }; void this.reloadArchived().catch(() => {}) })
    on('batch', () => this.publish())
    on('reopened', () => { void this.reconcile().catch(() => {}) })
    on('closed', ({ code }) => {
      if (code === 'removed') {
        this.removed = true
        ;(this.model.room as { connection: string }).connection = 'removed'
        this.change.room = true
        // said once, however this device processed the Commit that removed it (in the room's log, or served by the
        // hub's removal route after a `not-member`): never on the hub's word alone
        this.fromBoard(this.change)
        if (!this.model.alerts.some(x => x.code === 'removed')) this.board.pushAlert('removed', 'this device was removed from the room', null, null, this.now())
        this.publish()
      }
      // the hub no longer takes this device and the device did not confirm it: nothing is wiped, nothing more is
      // asked of the hub on this page; the app says so neutrally (a reload tries again)
      else if (code === 'unreachable') {
        ;(this.model.room as { connection: string }).connection = 'unreachable'
        this.change.room = true
        this.publish()
      }
      // tabs.ts drops this client and stands in line for the device's lock again
      else this.emit('device-closed', null)
    })
  }
  private onStream(event: StreamEvent): void {
    switch (event.event) {
      case 'presence':
        // (an event without `hears` says only that a stream opened or closed: the link report held stands while the
        //  device is online; the hub tells a new stream the reports of now first, and every change after)
        this.board.applyPresence(JSON.stringify([{ device_id: hex(event.device), is_online: event.online, offline_since: event.online ? null : this.now(),
          ...(event.hears === null ? {} : { link: { hears: event.hears ? 'live' : 'oncall', attached: true, working: event.working === true, last_call_at: event.last_call_at } }) }]), this.now())
        // (an agent that came online without a main session gets one: engine.ts seatAgents)
        if (event.online && this.is_human && this.engine.roles?.agents.some(d => hex(d) === hex(event.device))) this.engine.keepSoon(0)
        if (event.lost) this.board.pushAlert('agent-lost', 'An agent lost its connection.', null, hex(event.device), this.now())
        this.publish()
        return
      case 'invite_request': void this.checkInvite(hex(event.invite_id)).catch(() => {}); return
      case 'file_evicted': this.attachmentCache.delete(hex(event.file_id)); void this.cache.delete(`file/${hex(event.file_id)}`).catch(() => {}); return
      case 'archived': void this.engine.archive(event.group).catch(() => {}); return
      default:
    }
  }
  /**
   * A human register this device wrote and the hub has not taken yet, from before the page was loaded again (its
   * envelope waits in the stored outbox): shown as written, pending, as it was before the reload; the hub's copy
   * replaces it as for any write. Without this the model read from the cache showed the value before it until then
   * (an Emergency Kit saved just before a reload came back as due).
   */
  private async restoreRegisters(): Promise<void> {
    const entries = await this.engine.outbox()
    const held = new Set(entries.map(e => e.id))
    for (const [k] of await this.cache.range('client/register-out/').catch(() => [] as [string, unknown][])) {
      const id = Number(k.slice('client/register-out/'.length))
      if (!held.has(id)) void this.cache.delete(k).catch(() => {})
    }
    for (const entry of entries) {
      if (entry.kind !== 'envelope' || this.sending.has(entry.id) || !entry.parts[0]) continue
      const out = await this.cache.get(`client/register-out/${entry.id}`).catch(() => null) as { key?: unknown; value?: unknown } | null
      if (!out || typeof out.key !== 'string') continue
      let info: ReturnType<Core['envelopeHeader']>
      try { info = this.core.envelopeHeader(entry.parts[0]) } catch { continue }
      const local_id = `restored-${entry.id}`, key = out.key
      const item: OutboxItem = { local_id, envelope_kind: 'register', object_id: null, timeline_key: null, recipient_device_id: null, outbox_state: 'sending', error: null, content: { key } } as OutboxItem
      this.sending.set(entry.id, { local_id, item })
      const sealed = { outboxId: entry.id, envelopeHash: info.envelopeHash, seq: info.header.seq, group: info.header.group, objectId: null, time: info.header.time } as Sealed
      this.tell(ch => { this.board.echoRegisters(local_id, JSON.stringify({ [key]: out.value ?? null })); this.model.outbox.push(item); ch.outbox = true; this.confirmEcho(local_id, sealed) })
    }
  }
  /** The hub answered an outbox entry: its item leaves model.outbox; a refusal takes the echo back first. */
  private outboxDone(entry: OutboxEntry, refusal: string | null): void {
    void this.cache.delete(`client/register-out/${entry.id}`).catch(() => {})
    const sent = this.sending.get(entry.id)
    if (!sent) return
    this.sending.delete(entry.id)
    const ch = this.change
    if (refusal === 'stale-session' && sent.again) {
      // the session waited for a Remove when this was sealed: it is repaired now, and the same words go again
      const again = sent.again
      const at = this.model.outbox.indexOf(sent.item)
      if (at >= 0) this.model.outbox.splice(at, 1)
      this.change.outbox = true
      void (async () => { await this.engine.healNow().catch(() => {}); await again() })().catch(e => {
        this.board.pushAlert(this.core.errorCode(e) ?? 'stale-session', 'a message was not sent: its session waits for a repair', null, null, this.now())
        this.publish()
      })
      return
    }
    if (refusal) {
      // the views see the item `failed` in the change that takes the echo back
      sent.item.outbox_state = 'failed'; sent.item.error = refusal
      // a refused Note version: the edit that waited behind it is tried on what stands
      for (const [id, busy] of this.noteBusy) if (busy.outbox === entry.id) this.noteBack(id)
      this.board.rollbackEcho(sent.local_id)
      this.board.pushAlert(refusal, `the hub refused an envelope (${refusal})`, null, null, this.now())
      ch.outbox = true
      this.publish()
    }
    const at = this.model.outbox.indexOf(sent.item)
    if (at >= 0) this.model.outbox.splice(at, 1)
    this.change.outbox = true
  }
  /** After the device was opened again: an echo whose envelope is not in the stored outbox was never sealed. */
  private async reconcile(): Promise<void> {
    const stored = new Set((await this.engine.outbox()).map(e => e.id))
    this.tell(ch => {
      for (const [id, sent] of this.sending) {
        if (stored.has(id)) continue
        this.sending.delete(id)
        this.board.rollbackEcho(sent.local_id)
        const at = this.model.outbox.indexOf(sent.item)
        if (at >= 0) this.model.outbox.splice(at, 1)
        ch.outbox = true
      }
    })
  }

  // ---- life

  /** Starts the engine: the outbox is sent, the hub's order taken, the stream opened (`stream: false`: none). */
  async start({ stream = true }: { stream?: boolean; process_instance?: string | null } = {}): Promise<void> {
    if (this.started) return
    this.started = true
    mark(`client start (${this.rebuild !== null ? `rebuild from ${this.rebuild}` : 'from the cache'})`)
    // From the cache, the Desk stands already: the hub's Desk is read alongside the catch-up and the stream, not
    // before them (the client core holds back by the Desk's own cursor, whichever comes first; the catch-up's end reads
    // it again only when this one is through). Without a cache it is what is shown first, and waited for.
    if (this.rebuild !== null) { await this.fromDesk().catch(() => {}); mark('desk read') }
    else if (this.is_human && !this.deskReading) void this.readDesk().catch(() => {}).finally(() => mark('desk read'))
    if (!this.started) return
    await this.restoreRegisters().catch(() => {})
    if (!this.started) return
    await this.engine.start({ stream })
    if (!this.started) return     // stopped meanwhile: no timer is left behind
    if (this.rebuild !== null) { this.rebuilding = true; await this.engine.wantRescan(this.rebuild) }
    this.inviteTimer = setInterval(() => this.watchInvites(), 1000)
    void this.nameDevice().catch(() => {})
    void this.ensureDesk().catch(() => {})
    void this.learnArchived().catch(() => {})
  }
  /** The sessions the hub holds as archived that this device still holds as live (archived while it was away): it
   *  holds them as archived too, so that it loads them only when one is opened (loadArchived). One small request. */
  /** `learnArchived` once more, after what runs now; a failed read is tried again a while later. */
  private archivedSoon(): void {
    if (this.archivedRun) { this.archivedAgain = true; return }
    this.archivedRun = this.learnArchived().catch(() => {
      if (this.started) setTimeout(() => { if (this.started) this.archivedSoon() }, ARCHIVED_RETRY_MS)
    }).finally(() => {
      this.archivedRun = null
      if (this.archivedAgain) { this.archivedAgain = false; this.archivedSoon() }
    })
  }
  private archivedRun: Promise<void> | null = null
  private archivedAgain = false
  private async learnArchived(): Promise<void> {
    for (const row of await this.hub.roomGroups({ archived: true })) {
      const held = this.engine.groups.find(g => g.session && sameBytes(g.group, row.group))
      if (!row.live && held && !held.archived) await this.engine.archive(row.group)
    }
  }
  /** The archived sessions loaded in this run (session ids, hex). */
  private archivedLoaded = new Set<string>()
  /**
   * An archived session is loaded when it is opened (hub-api.md point 46): a catch-up leaves its envelopes out, so
   * its cards, registers (its name, its status lines) and the rest of its content are read now, in one pass over
   * its group alone (`GET /v1/changes?group=`), through the core. Once a run; a live session has nothing to load.
   */
  async loadArchived(session_id: string): Promise<number> {
    const group = this.engine.groups.find(g => g.session && hex(g.session.sessionId) === session_id)
    if (!group?.archived || this.archivedLoaded.has(session_id)) return 0
    this.archivedLoaded.add(session_id)
    const items: EnvelopeItem[] = []
    try {
      for (let after = 0, pages = 0; pages < 1000; pages++) {
        const page = await this.hub.changes(after, 1000, { group: group.group })
        for (const item of page.items) if (item.kind === 'envelope') items.push({ ...item, cut: false })
        if (!page.more || page.change === after) break
        after = page.change
      }
    } catch (e) { this.archivedLoaded.delete(session_id); throw e }
    const received = await this.engine.receiveGroup(items)
    this.tell(() => { this.board.applyEnvelopes(received.filter(r => r.outcome !== 'refused'), this.now()) })
    await this.engine.noteArchivedOpened(session_id)
    return received.length
  }
  /** After a rescan (it reads the live sessions alone, and a model built again from nothing lost what an opened
   *  archived session brought): the archived sessions this device opened are loaded again. */
  private async reloadArchived(): Promise<void> {
    for (const sid of await this.engine.openedArchived()) {
      this.archivedLoaded.delete(sid)
      await this.loadArchived(sid).catch(() => {})
    }
  }
  /**
   * Every account has at least one desk, from its creation on: the first is `desk/main`, "Personal". A human device
   * writes it once the room is taken up to the hub's head and no desk is there (a new account, a device that joined
   * or recovered, an account from before, a list that two devices emptied at once). The id and the value are fixed,
   * so devices that do it at the same moment write the same register: one desk, never two.
   */
  async ensureDesk(): Promise<void> {
    if (!this.is_human || [...this.model.human.desks.values()].some(v => v)) return
    await this.setDesk(DEFAULT_DESK, FIRST_DESK)
  }
  /** Takes a desk away; never the last one (`last-desk`). */
  async removeDesk(desk_id: string): Promise<Sent> {
    const live = [...this.model.human.desks].filter(([, v]) => v).map(([id]) => id)
    if (live.length <= 1 && (live[0] === desk_id || !live.length)) fail('last-desk', 'the last desk stays: every account has one')
    return this.setRegisters({ [`desk/${desk_id}`]: null })
  }
  /** Stops everything, writes the cache and closes the device (its store and lock with it). */
  async stop(): Promise<void> {
    this.started = false
    if (this.inviteTimer !== null) { clearInterval(this.inviteTimer); this.inviteTimer = null }
    if (this.goalsTimer !== null) { clearTimeout(this.goalsTimer); this.goalsTimer = null }
    if (this.deskTimer !== null) { clearTimeout(this.deskTimer); this.deskTimer = null }
    if (this.deskHoldTimer !== null) { clearTimeout(this.deskHoldTimer); this.deskHoldTimer = null }
    if (this.wakeTimer !== null) { clearTimeout(this.wakeTimer); this.wakeTimer = null }
    this.deskWaitDone?.()
    await this.engine.stop()
    for (const off of this.offs.splice(0)) off()
    await this.flush().catch(() => {})
  }
  catchUp(): Promise<void> { return this.engine.catchUp() }
  /** Waits until the outbox is empty and the hub's copies came back. */
  async settle({ timeout_ms = 10_000 }: { timeout_ms?: number } = {}): Promise<void> { await this.engine.settle(timeout_ms) }

  /** A model without a usable cache: the open objects and the registers at once, from the hub's Desk, each read
   *  back through the core (it holds their chains already). The rest comes with the rescan. */
  private async fromDesk(): Promise<void> {
    const desk = await this.hub.desk({ active: true })
    // (what it lists as open decides the Desk from here on, as `readDesk`: an old question read again by the
    //  rebuild below never stands on the Desk because its answer comes later in the history)
    if (!desk.truncated) {
      const listed = [...desk.cards, ...desk.permission_requests]
      this.desk = { open: new Set(listed.map(o => hex(o.object_id))), versions: new Map(listed.map(o => [hex(o.object_id), o.version?.change ?? 0])), at: desk.change }
      this.keepDesk()
      this.deskToModel()
    }
    const items: EnvelopeItem[] = [...desk.registers]
    for (const list of [desk.cards, desk.notes, desk.permission_requests, desk.artifacts]) for (const o of list) if (o.version) items.push(o.version)
    items.sort((a, b) => a.change - b.change)
    const received = await this.engine.receivePage(items)
    this.tell(() => { this.board.applyEnvelopes(received.filter(r => r.outcome !== 'refused'), this.now()) })
    if (!desk.truncated) this.knowDesk()
  }
  /**
   * The first paint of a start from the cache waits for the hub's Desk (the person's rule: the Desk is the hub's open
   * list, and nothing it does not list stands as open, not even for a moment). Starts the reading at once (before
   * `start`) and resolves when its list decided what stands, or after `wait_ms`: then every open card waits off the
   * Desk (`room.desk_waiting`, a quiet "Catching up") until the list answers, at most DESK_HOLD_MAX_MS, after which
   * the list last read on this device decides (no hub: the device is offline). Nothing to wait for: at once.
   */
  async deskFirst(wait_ms: number = DESK_FIRST_MS): Promise<void> {
    if (!this.is_human || this.deskKnown) return
    if (!this.deskWait) {
      this.deskWait = new Promise(resolve => { this.deskWaitDone = resolve })
      void this.readDesk().catch(() => this.knowDesk())
    }
    let timer: ReturnType<typeof setTimeout> | null = null
    const late = await Promise.race([this.deskWait.then(() => false), new Promise<boolean>(resolve => { timer = setTimeout(() => resolve(true), Math.max(0, wait_ms)) })])
    if (timer !== null) clearTimeout(timer)
    if (!late || this.deskKnown || this.deskHeld) return
    this.deskHeld = true
    this.deskHoldTimer = setTimeout(() => { this.deskHoldTimer = null; this.knowDesk() }, DESK_HOLD_MAX_MS)
    this.tell(ch => { this.model.room.desk_waiting = true; ch.room = true; ch.stack = true; this.board.setDeskHold(true) })
  }
  /** The hub's Desk of this run decided (or none comes: the last list kept here does). */
  private knowDesk(): void {
    if (this.deskKnown) return
    this.deskKnown = true
    if (this.deskHoldTimer !== null) { clearTimeout(this.deskHoldTimer); this.deskHoldTimer = null }
    const held = this.deskHeld
    this.deskHeld = false
    this.tell(ch => { if (held) { this.model.room.desk_waiting = false; ch.room = true; this.board.setDeskHold(false) } this.deskToModel(); ch.stack = true })
    this.deskWaitDone?.()
  }
  /** Keeps the hub's Desk as read, for the next start (`open`). */
  private keepDesk(): void {
    const d = this.desk
    if (d) void this.cache.set(DESK_KEY, { open: [...d.open], versions: [...d.versions], at: d.at }).catch(() => {})
  }
  /** Reads the hub's Desk again shortly (several groups joined at once, several cards that came, ask once). */
  private deskSoon(delay = DESK_SOON_MS): void {
    if (this.deskTimer !== null) clearTimeout(this.deskTimer)
    this.deskTimer = setTimeout(() => { this.deskTimer = null; void this.readDesk().catch(() => {}) }, delay)
  }
  /** The room counts as being read (`room.reading`) until `work` settles: room.ts hands it the session joins of a
   *  sign-in, which the Desk waits for. */
  readingUntil(work: Promise<unknown>): void {
    this.readingHolds++
    this.publishSoon()
    void work.catch(() => {}).finally(() => { this.readingHolds--; this.publishSoon() })
  }
  private publishSoon(): void { this.change.room = true; queueMicrotask(() => this.publish()) }
  /**
   * The hub's Desk decides what stands on this device's Desk (the person's choice: one list, the same on every
   * device). The hub keeps, per card and permission request, whether it is open, from the envelopes' headers
   * (9.2.1, the same replay every device makes); a device that is behind, or that missed or misread an envelope,
   * would otherwise show what the hub settled long ago, or miss what it reopened. So, at the start, after every
   * catch-up and every minute while running:
   * - a card or request this device holds as open that the hub does not list, and that has nothing newer here than
   *   the Desk's cursor, is held back (`catching_up`): not on the Desk, its page says it was settled elsewhere;
   * - one the hub lists as open whose newest version this device does not hold (its version's change is above
   *   what it took) is taken from the Desk's own copy of that version, read through the core out of the hub's
   *   order (verified, provisional until its chain reaches it), and a hand back older than that version is over.
   * A hub that leaves a card out can hide it, as it can today by withholding its envelopes; it cannot make one up.
   */
  private readDesk(): Promise<void> {
    if (!this.is_human) return Promise.resolve()
    if (this.deskReading) { this.deskAgain = true; return this.deskReading }
    return this.deskReading = (async () => {
      do { this.deskAgain = false; await this.readDeskNow() } while (this.deskAgain && this.started)
    })().finally(() => {
      this.deskReading = null
      if (this.started && this.deskTimer === null) this.deskTimer = setTimeout(() => { this.deskTimer = null; void this.readDesk().catch(() => {}) }, DESK_EVERY_MS)
    })
  }
  /**
   * One reading: the whole list (ids and versions, `GET /v1/desk?brief=1`) at the start, after the groups changed
   * and now and then; otherwise only what changed since the last one's cursor (`&since=`), which is nothing most
   * of the time. The versions this device lacks are read by their object route, a few at once.
   */
  private async readDeskNow(): Promise<void> {
    const whole = this.deskWholeDue || !this.desk || this.now() - this.deskWholeAt >= DESK_WHOLE_MS
    this.deskWholeDue = false
    let brief
    try { brief = await timed('desk', () => this.hub.deskBrief(whole ? null : this.desk!.at, { active: true })) } catch (e) { if (whole) this.deskWholeDue = true; this.knowDesk(); throw e }
    if (brief.truncated) { if (!whole) { this.deskWholeDue = true; this.deskAgain = true } else this.knowDesk(); return }
    const listed = [...brief.cards.map(o => ({ ...o, route: 'cards' as const })), ...brief.permission_requests.map(o => ({ ...o, route: 'permission-requests' as const }))]
    if (whole || !this.desk) { this.desk = { open: new Set(), versions: new Map(), at: 0 }; this.deskWholeAt = this.now() }
    const d = this.desk
    for (const gone of brief.settled ?? []) { d.open.delete(hex(gone)); d.versions.delete(hex(gone)) }
    for (const o of listed) { d.open.add(hex(o.object_id)); d.versions.set(hex(o.object_id), o.version_change) }
    d.at = brief.change
    this.keepDesk()
    // (the list decides what stands at once; the versions this device lacks follow below)
    this.knowDesk()
    // what this reading is about: everything, or what changed since the last
    const touched = whole ? null : new Set([...listed.map(o => hex(o.object_id)), ...(brief.settled ?? []).map(hex)])
    // the newest versions this device lacks, read through the core
    const lacking = listed.filter(o => {
      const id = hex(o.object_id), c = this.model.cards.get(id) ?? this.model.permissions.get(id)
      return !c || (c.envelope_number ?? 0) < o.version_change
    })
    // (many lacking, as on a new device that has not read the room yet: the full Desk, which holds them all, in one
    //  answer, so that a sign-in shows the true Desk at once; later readings while the room is still being read (its
    //  catch-up behind the Desk, a rescan due, the session joins of a sign-in) leave them to that, which brings them)
    const first = !this.deskVersionsRead
    this.deskVersionsRead = true
    const reading = this.engine.position < brief.change || this.engine.rescanPending || this.readingHolds > 0
    const versions = lacking.length > DESK_ONE_BY_ONE && !first && reading ? []
      : lacking.length > DESK_ONE_BY_ONE
      ? await this.hub.desk({ active: true }).then(full => { const want = new Set(lacking.map(o => `${hex(o.object_id)}/${o.version_change}`)); return [...full.cards, ...full.permission_requests].flatMap(o => (o.version && want.has(`${hex(o.object_id)}/${o.version.change}`) ? [o.version] : [])) }).catch(() => [])
      : (await inParallel(lacking, async o => {
          const got = await this.hub.objectEnvelopes(o.route, o.object_id, { after: o.version_change - 1, limit: 1 }).catch(() => null)
          return got?.items.filter(i => i.change === o.version_change) ?? []
        })).flat()
    const missing: EnvelopeItem[] = [...versions]
    // a card answered here and not yet closed by its agent (it counts as "an answer waits"): the hub's state of it,
    // and what came after this device's last word on it (its closing, a newer version), the few there are; a
    // reading of what changed asks only of those that changed
    const answered = [...this.model.cards.values()].filter(c => c.object_state === 'answered' && !d.open.has(c.object_id) && (!touched || touched.has(c.object_id))).slice(0, ANSWERED_ASKED)
    for (const got of await inParallel(answered, c => this.hub.objectEnvelopes('cards', unhex(c.object_id), { after: c.state_envelope_number, limit: 20 }).then(got => ({ got, c })).catch(() => null))) {
      if (got && got.got.head_change > got.c.state_envelope_number) missing.push(...got.got.items)
    }
    const received = missing.length ? await this.engine.receivePage(missing.sort((x, y) => x.change - y.change)).catch(() => []) : []
    // a card the hub lists as open that this device holds as handed back: the newest page of its Chat says whether
    // the agent put it before the human again since (a present this device missed, or one an older app read before
    // the hand back it answered); once per hand back and run, the few there are
    const handed = [...d.open].map(id => this.model.cards.get(id)).filter((c): c is Card => !!c?.in_revision && this.revisionRead.get(c.object_id) !== c.in_revision.envelope_number).slice(0, ANSWERED_ASKED)
    for (const c of handed) this.revisionRead.set(c.object_id, c.in_revision!.envelope_number)
    const pages = await inParallel(handed, c => this.hub.chatItems('card', unhex(c.object_id), { limit: 20 }).catch(() => null))
    const deeds: ReceivedEnvelope[] = []
    for (const got of pages) if (got?.items.length) deeds.push(...await this.engine.receivePage(got.items).catch(() => []))
    this.tell(ch => {
      this.board.applyEnvelopes(received.filter(r => r.outcome !== 'refused'), this.now())
      if (deeds.length) this.board.applyCardDeeds(deeds)
      this.deskToModel()
      ch.stack = true
    })
  }
  /** The hub's Desk as read decides what stands (the client core's Desk: client/src/desk.rs, `M.project`): an open
   *  card or request it does not list, with nothing of it here newer than its change, is held back (`catching_up`);
   *  a hand back older than the version it lists is over. */
  /** What the core's model changed since the last batch, put into the copy (`model`) and into `ch`. */
  private fromBoard(ch: Change): void {
    const text = this.board.finish(this.now())
    if (!text) return
    const patch = revivePatch(text)
    const got = applyPatch(this.model, patch)
    relink(this.model, patch)
    if (patch.devregs) { const d = M.devregsOf(this.model); d.clear(); for (const [k, v] of patch.devregs) d.set(k, v) }
    for (const k of ['cards', 'sessions', 'permissions', 'notes', 'published', 'timelines', 'registers', 'invites'] as const) for (const id of got[k]) ch[k].add(id)
    for (const [k, list] of got.items) for (const it of list) M.addItem(ch, k, it)
    ch.members ||= got.members; ch.alerts ||= got.alerts; ch.stack ||= got.stack; ch.room ||= got.room
    if (patch.session_list) ch.status = true
    if (patch.wake_at !== undefined) this.wakeAt(patch.wake_at)
  }
  /** When the core's model changes by the clock alone (a snooze ends, a request runs out, a session falls asleep, a
   *  red hand after its grace): a batch then, so that the core decides it, not the page. */
  private wakeTimer: ReturnType<typeof setTimeout> | null = null
  private wakeTime = 0
  private wakeAt(at: number): void {
    if (at === this.wakeTime) return
    if (this.wakeTimer !== null) { clearTimeout(this.wakeTimer); this.wakeTimer = null }
    this.wakeTime = at
    if (!at) return
    this.wakeTimer = setTimeout(() => { this.wakeTimer = null; this.wakeTime = 0; this.publish() }, Math.min(2 ** 31 - 1, Math.max(0, at - this.now()) + 50))
  }
  /** An own send was sealed: the core's model learns its hash (the hub's copy will name it), number and object id. */
  private confirmEcho(local_id: string, sealed: Sealed): void {
    this.board.confirmEcho(local_id, hex(sealed.envelopeHash), sealed.seq, sealed.objectId ? hex(sealed.objectId) : null)
  }
  private deskToModel(): void {
    const d = this.desk
    if (!d) return
    this.board.setDeskListing(d.at, [...d.open].map(id => ({ objectId: id, versionChange: d.versions.get(id) ?? 0 })))
  }
  private async nameDevice(): Promise<void> {
    const name = this.opts.device_name
    if (!name || !this.is_human) return
    const me = this.model.members.get(this.my_device_id)
    if (me?.device_name === name) return
    const info = (this.opts.device_info && typeof this.opts.device_info === 'object' ? this.opts.device_info : {}) as Fields
    await this.setRegisters({ [`device/${this.my_device_id}`]: { ...info, device_name: name } })
  }

  // ---- sending: echo, seal, outbox

  /** Seals drafts of one echo and puts their items into model.outbox. A failure takes the echo back. */
  private async send(local_id: string, drafts: { draft: Draft; recipient?: string | null; files?: Uint8Array[]; item?: Partial<OutboxItem> }[], after?: (sealed: Sealed) => void, retried = false): Promise<Sent> {
    let last: Sealed | null = null
    try {
      for (const d of drafts) {
        const sealed = last = await this.sealHealed(d.draft, d.recipient ? unhex(d.recipient) : null, d.files ?? [])
        const item: OutboxItem = { local_id, envelope_kind: d.draft.kind, object_id: null, timeline_key: null, recipient_device_id: d.recipient ?? null, outbox_state: 'sending', error: null, ...d.item }
        // (a single message may go once more if the hub held its session as stale: its words are sealed anew)
        const again = drafts.length === 1 && !retried && (d.draft.kind === 'sessionChat' || d.draft.kind === 'cardChat') ? () => this.send(local_id, drafts, after, true) : null
        this.sending.set(sealed.outboxId, { local_id, item, again })
        this.tell(ch => { this.model.outbox.push(item); ch.outbox = true; this.confirmEcho(local_id, sealed); after?.(sealed) })
      }
    } catch (e) {
      // what was sealed stays sealed (its number is used); what the echo showed beyond that is taken back
      if (!last) this.tell(() => this.board.rollbackEcho(local_id))
      throw e
    }
    return { local_id, envelope_hash: hex(last!.envelopeHash), seq: last!.seq }
  }
  /** Seals into the outbox. A session the core holds as stale (5.2.8: a leaf the room no longer allows, or a helper
   *  session without its opener) takes nothing until a human device repairs it: this one repairs it now (the
   *  engine's heal) and seals once more. */
  private async sealHealed(draft: Draft, recipient: Uint8Array | null, files: Uint8Array[]): Promise<Sealed> {
    try { return await this.engine.seal(draft, recipient, files) } catch (e) {
      if (this.core.errorCode(e) !== 'stale-session' || !this.is_human) throw e
      await this.engine.healNow().catch(() => {})
      return await this.engine.seal(draft, recipient, files)
    }
  }
  private needHuman(): void { if (!this.is_human) fail('forbidden', 'only a human device can do this') }
  private card(object_id: string): Card { return this.model.cards.get(object_id) ?? fail('not-found', `no card ${object_id}`) }
  private sessionGroup(session_id: string): Uint8Array { return this.core.sessionGroupId(this.room_id, unhex(session_id)) }
  /** The session an agent device speaks in. */
  sessionOfAgent(agent_device_id: string): string | null {
    for (const s of this.model.sessions.values()) if (s.is_active && s.agent_device_id === agent_device_id) return s.session_id
    for (const s of this.model.sessions.values()) if (s.agent_device_ids.includes(agent_device_id)) return s.session_id
    return null
  }

  // ---- human actions (README "Human actions")

  /** A Chat message to a session's agent, or on a card. */
  async sendMessage({ agent_device_id = null, object_id = null, session_id = null, ...fields }: { agent_device_id?: string | null; object_id?: string | null; session_id?: string | null; [field: string]: unknown }): Promise<Sent> {
    const card = object_id ? this.card(object_id) : null
    const sid = card?.session_id ?? session_id ?? (agent_device_id ? this.sessionOfAgent(agent_device_id) : null)
    if (!sid) return fail('bad-argument', 'a message names a session (with an agent) or a card')
    const recipient = this.is_human ? (card ? M.holderOf(this.model, card) : null) ?? M.recipientOf(this.model, sid) : null
    const content = { content_type: 'message', ...fields }
    const payload = encodeBodyBytes('message', content)
    const timeline_key = M.timelineKey('chat', object_id ? `card/${object_id}` : `session/${sid}`)
    const local_id = `local-${randomHex(8)}`
    this.tell(() => { this.board.echoItem(local_id, timeline_key, JSON.stringify(content), recipient ?? null, object_id ?? null, this.now()) })
    const draft: Draft = object_id ? { kind: 'cardChat', session: unhex(sid), card: unhex(object_id), payload } : { kind: 'sessionChat', session: unhex(sid), payload }
    const sent = await this.send(local_id, [{ draft, recipient, files: fileIdsOf('message', content), item: { timeline_key, object_id, content } }])
    // handed back (the reverse card) or "What??": the card goes to its agent and comes back up awake
    if (object_id && (fields['hand_back'] || fields['explain'])) this.wakeCard(object_id)
    return sent
  }
  /** A card put off (Later) that he acts on is woken, as the wake action does: its snooze ends now. */
  private wakeCard(object_id: string): void {
    if (!this.is_human) return
    const s = this.model.human.snoozes.get(object_id)
    if (!s || (s.until != null && s.until <= this.now())) return
    void this.setRegisters({ [`snooze/${object_id}`]: { until: this.now() } }).catch(() => {})
  }

  async answer({ object_id, choices = [], note, option_notes, attachments, marks, trusted, answer_action = 'answer' }: { object_id: string; choices?: string[]; note?: string | null | undefined; option_notes?: Record<string, string> | undefined; attachments?: AttachmentRef[] | undefined; marks?: unknown[] | undefined; trusted?: boolean | undefined; answer_action?: string }): Promise<Sent> {
    this.needHuman()
    const card = this.card(object_id)
    if (card.object_state !== 'open') fail('card-closed', 'the card is not open')
    // a card of a newer version is never answered by what this version guesses it means
    if (!M.cardSupported(card)) fail('needs-update', UPDATE_MESSAGE)
    if (card.content_state !== 'ok') fail('card-pruned', 'this device holds only the header of this card; answer it on a device that shows it')
    if (!card.session_id) fail('not-found', 'the card has no session')
    // when it was answered stands in the body (v1.md 9), not in the header the hub reads
    const answered_at = this.now()
    const content: Fields = { answer_action, choices, note, option_notes, attachments, marks, trusted, answered_at }
    // An answer whose every choice is an option the agent marked final settles the card in the same step, as read
    // and shred close it. Anything said beside the choice is for the agent to read, so the card stays with it; so
    // does a trusted answer, where the agent still chooses.
    const plain = !String(note ?? '').trim() && !Object.values(option_notes ?? {}).some(v => String(v ?? '').trim()) && !attachments?.length && !marks?.length
    const settles = answer_action === 'answer' && !trusted && plain && M.choicesFinal(card, choices)
    const state = answer_action === 'answer' && !settles ? 'answered' : 'closed'
    const recipient = M.holderOf(this.model, card)
    const echo: Omit<Answer, 'pending'> = { answer_action, choices, note: note ?? null, option_notes: option_notes ?? {}, attachments: attachments ?? [], marks: marks ?? [], trusted: !!trusted,
      bound_version_hash: card.version_hash, bound_object_version: card.object_version, envelope_number: null, envelope_hash: null, by_device_id: this.my_device_id, answered_at, taken_back_at: null, taken_back_sent_at: null }
    const local_id = `local-${randomHex(8)}`
    this.tell(() => { this.board.echoAnswer(local_id, object_id, JSON.stringify(echo), state) })
    const draft: Draft = { kind: 'answer', session: unhex(card.session_id!), objectId: unhex(object_id), choices, closes: state === 'closed', payload: encodeBodyBytes('answer', content) }
    const sent = await this.send(local_id, [{ draft, recipient, files: fileIdsOf('answer', content), item: { object_id, content } }])
    // answered while put off: awake, so a reworked or reopened card is back on the Desk
    this.wakeCard(object_id)
    return sent
  }
  trust({ object_id, note }: { object_id: string; note?: string | null }): Promise<Sent> {
    const card = this.card(object_id)
    const recommended = card.recommended == null ? [] : Array.isArray(card.recommended) ? card.recommended : [card.recommended]
    return this.answer({ object_id, choices: recommended, note, trusted: true })
  }
  markRead({ object_id }: { object_id: string }): Promise<Sent> { return this.answer({ object_id, answer_action: 'read' }) }
  shred({ object_id, note }: { object_id: string; note?: string | null }): Promise<Sent> { return this.answer({ object_id, answer_action: 'shred', note }) }

  /** Takes the answer in force back: the card is open again. */
  async decideAgain({ object_id }: { object_id: string }): Promise<Sent> {
    this.needHuman()
    const card = this.card(object_id)
    if (!card.answer?.envelope_hash || !card.session_id) fail('decision-mismatch', 'no answer in force to take back')
    const draft: Draft = { kind: 'takeBack', session: unhex(card.session_id!), objectId: unhex(object_id), payload: encodeBodyBytes('take_back', {}) }
    return this.send(`local-${randomHex(8)}`, [{ draft, recipient: M.holderOf(this.model, card), item: { object_id } }])
  }

  async verdict({ object_id, allow }: { object_id: string; allow: boolean }): Promise<Sent> {
    this.needHuman()
    const p = this.model.permissions.get(object_id) ?? fail('not-found', 'no such permission request')
    if (!p.session_id) fail('not-found', 'the request has no session')
    const draft: Draft = { kind: 'verdict', session: unhex(p.session_id!), requestId: unhex(object_id), allow: !!allow, payload: encodeBodyBytes('verdict', {}) }
    return this.send(`local-${randomHex(8)}`, [{ draft, recipient: M.holderOf(this.model, p), item: { object_id } }])
  }

  /**
   * Before this device writes a board's snapshot register: tells the hub what the snapshot will cover and which
   * files its shapes name (10.9), so that pruning is held back for it from now on. Returns false when the frontier
   * cannot be declared (a head without its hash yet, a frontier behind what the hub already pruned: `replay`): then no
   * snapshot is written.
   * `frontier` in the model's ids (`{ <writer hex>: [seq, hash hex] }`), `shapes` the snapshot's (wire form).
   */
  async declareBoardFrontier(timeline_id: string, frontier: Record<string, [number, string | null]>, shapes: readonly Record<string, unknown>[]): Promise<boolean> {
    this.needHuman()
    const wire = frontierWire(frontier)
    if (!wire) return false
    try { await this.hub.postBoardFrontier(boardOf(timeline_id), wire, snapshotFiles(shapes)) }
    catch (e) {
      // (`replay`: the board has items pruned beyond this frontier; a later capture, after the newest snapshot, covers them)
      if ((e as { code?: string }).code === 'replay') return false
      throw e
    }
    return true
  }

  /**
   * After this device wrote a board's snapshot register (`sent`, from `setRegisters`): once the hub took that
   * envelope, binds the declared frontier to it (10.9), and the hub prunes the board's items behind every device's
   * snapshot. Returns how many item bodies the hub pruned, or null when the register was refused or not sent within
   * a minute (the declaration keeps holding pruning back; the next snapshot binds again).
   */
  async postBoardFrontier(timeline_id: string, sent: Sent, frontier: Record<string, [number, string | null]>, shapes: readonly Record<string, unknown>[]): Promise<number | null> {
    this.needHuman()
    const item = this.model.outbox.find(i => i.local_id === sent.local_id)
    for (const until = this.now() + 60_000; item && this.model.outbox.includes(item);) {
      if (this.now() > until) return null
      await new Promise(r => setTimeout(r, 250))
    }
    if (item?.outbox_state === 'failed') return null
    const wire = frontierWire(frontier)
    if (!wire) return null
    const { pruned } = await this.hub.postBoardFrontier(boardOf(timeline_id), wire, snapshotFiles(shapes), [sent.seq, unhex(sent.envelope_hash)])
    return pruned
  }

  /**
   * Registers, one envelope per name. Without `session_id`: the room's (shared by human devices, shown at once), and
   * this device's own `device/<id>`. With it: a register of that session's group (`goals`), no echo.
   */
  async setRegisters(values: Record<string, unknown>, { session_id = null }: { own_device?: boolean; session_id?: string | null } = {}): Promise<Sent> {
    const keys = Object.keys(values)
    if (!keys.length) return fail('bad-argument', 'no register to set')
    for (const k of keys) if (k.startsWith('device/') && k !== `device/${this.my_device_id}`) fail('forbidden', 'a device writes only its own device register')
    const group = session_id ? this.sessionGroup(session_id) : this.core.roomGroupId(this.room_id)
    if (!session_id) this.needHuman()
    // one echo and one envelope per name: a name whose sealing fails is taken back alone
    let last: Sent | null = null
    for (const key of keys) {
      const local_id = `local-${randomHex(8)}`
      if (!session_id && !key.startsWith('device/')) this.tell(() => this.board.echoRegisters(local_id, JSON.stringify({ [key]: values[key] ?? null })))
      const { name, value } = encodeRegister(key, values[key] ?? null)
      last = await this.send(local_id, [{ draft: { kind: 'register', group, name, value }, files: fileIdsOf('register', { value: values[key] }), item: { content: { key } } }], sealed => {
        // (kept beside the outbox entry: a page loaded again before the hub took it shows the write, not the value
        //  before it, until the hub's copy comes back; restoreRegisters)
        if (!session_id && !key.startsWith('device/')) void this.cache.set(`client/register-out/${sealed.outboxId}`, { key, value: values[key] ?? null }, { durable: true }).catch(() => {})
      })
      if (key.startsWith('session/') && (values[key] as { archived?: unknown } | null)?.archived === true) {
        const sid = key.slice('session/'.length), vouched = (values[key] as { vouched?: unknown }).vouched ?? this.model.sessions.get(sid)?.settings?.['vouched']
        void this.archive(sid).then(() => this.retireVouched(sid, vouched)).catch(() => {})
      }
    }
    return last!
  }
  /** An archived session is over for good (5.2.10): the hub takes nothing more for its group, and neither does this device. */
  private async archive(session_id: string): Promise<void> {
    const group = this.engine.groups.find(g => g.session && hex(g.session.sessionId) === session_id)
    if (!group || group.archived) return
    await this.hub.archiveGroup(group.group)
    await this.engine.archive(group.group)
  }
  setDraft(object_id: string, draft: unknown): Promise<Sent> { return this.setRegisters({ [`draft/${object_id}`]: draft ?? null }) }
  snooze(object_id: string, until: number | null | undefined): Promise<Sent> { return this.setRegisters({ [`snooze/${object_id}`]: until == null ? null : { until } }) }
  /** Star a card (null: take the star off). His own mark, kept end-to-end encrypted in the room like a snooze. */
  star(object_id: string, on: boolean): Promise<Sent> { return this.setRegisters({ [`star/${object_id}`]: on ? { at: Date.now() } : null }) }
  duck(object_id: string, value: unknown): Promise<Sent> { return this.setRegisters({ [`duck/${object_id}`]: value ?? null }) }
  setCrown(value: unknown): Promise<Sent> { return this.setRegisters({ crown: value ?? null }) }
  setDesk(desk_id: string, value: unknown): Promise<Sent> {
    if (value == null) return this.removeDesk(desk_id)
    return this.setRegisters({ [`desk/${desk_id}`]: value })
  }

  /** A note's version this device confirmed last (what came back from the hub), under any echo in front of it. */
  private noteHead(object_id: string): { object_version: number; version_hash: string | null; content: Fields } | null {
    const m = this.model.notes.get(object_id)
    const base = m?.pending ? m._base : m
    return base ? { object_version: base.object_version, version_hash: base.version_hash, content: noteFields(base) } : null
  }
  /**
   * A note's version. The core takes a later version only on one it holds, and it holds an own version once that
   * came back from the hub. So while a version of a note is on its way, a further edit is shown at once and kept
   * here; only the newest is kept, and it is sealed when the one before it is back. (An edit that waits like that
   * is in memory only: it is lost if the app ends before the earlier version returned.)
   */
  private async writeNote(object_id: string | null, fields: Fields, closed: boolean): Promise<string> {
    this.needHuman()
    if (object_id && this.model.notes.get(object_id)?.unsupported) fail('needs-update', UPDATE_MESSAGE)
    if (object_id && !this.model.notes.get(object_id)) fail('not-found', 'no such note')
    const { object_state: _state, ...own } = fields
    const local_id = `local-${randomHex(8)}`
    const busy = object_id ? this.noteBusy.get(object_id) : undefined
    if (busy) {
      this.tell(() => { if (busy.next) this.board.rollbackEcho(busy.next.local_id); this.board.echoNote(local_id, object_id ?? null, JSON.stringify(closed ? {} : own), closed) })
      busy.next = { own: { ...(busy.next?.own ?? {}), ...own }, closed, local_id }
      return object_id!
    }
    this.tell(() => { this.board.echoNote(local_id, object_id ?? null, JSON.stringify(closed ? {} : own), closed) })
    return this.sealNote(object_id, own, closed, local_id)
  }
  private async sealNote(object_id: string | null, own: Fields, closed: boolean, local_id: string): Promise<string> {
    const head = object_id ? this.noteHead(object_id) : null
    // one above every lamport this device has seen on a note (9.3.2 chooses the current version by it)
    for (const n of this.model.notes.values()) this.lamport = Math.max(this.lamport, n.causal?.lamport ?? 0, n._base?.causal?.lamport ?? 0)
    const content: Fields = { ...(head?.content ?? {}), ...own, object_version: (head?.object_version ?? 0) + 1, lamport: ++this.lamport }
    // every version names the one before it; a first version names zeros (spec 9, `object_ref`)
    content['previous_version_hash'] = head?.version_hash ?? '0'.repeat(64)
    for (const k of Object.keys(content)) if (content[k] === undefined) delete content[k]
    const payload = encodeBodyBytes('note', content)
    let id = object_id
    const draft: Draft = object_id ? { kind: 'noteVersion', objectId: unhex(object_id), closed, payload } : { kind: 'noteFirst', payload }
    await this.send(local_id, [{ draft, files: fileIdsOf('note', content), item: { object_id, content } }], sealed => {
      id = sealed.objectId ? hex(sealed.objectId) : id
      this.noteBusy.set(id!, { hash: hex(sealed.envelopeHash), outbox: sealed.outboxId, next: null })
    })
    return id!
  }
  /** A note's version is back from the hub, or was refused: the edit that waited behind it is sealed now. */
  private noteBack(object_id: string): void {
    const busy = this.noteBusy.get(object_id)
    if (!busy) return
    this.noteBusy.delete(object_id)
    const next = busy.next
    if (next) void this.sealNote(object_id, next.own, next.closed, next.local_id).catch(() => {})
  }
  /** A new note or a new version of one: in model.notes at once (a new one first under its local id). Resolves to its object id. */
  saveNote({ object_id = null, ...fields }: { object_id?: string | null; [field: string]: unknown }): Promise<string> {
    return this.writeNote(object_id, fields, fields['object_state'] === 'closed')
  }
  /** A closed version. */
  async deleteNote(object_id: string): Promise<string> { return this.writeNote(object_id, {}, true) }

  /**
   * Scribble Board items: strokes, erase, move, send_away on `desk/<board>`. `selection_sent` (the views' "send what
   * I selected to an agent") is two writes: a Chat message with the picture to that session, and
   * `send_away` of the shapes on the board they came from.
   */
  async sendStrokes({ timeline_id, content_type = 'strokes', recipient_device_id = null, ...fields }: { timeline_id: string; content_type?: string; recipient_device_id?: string | null; [field: string]: unknown }): Promise<Sent> {
    this.needHuman()
    const scope = timeline_id.slice(0, timeline_id.indexOf('/')), id = timeline_id.slice(timeline_id.indexOf('/') + 1)
    if (content_type === 'selection_sent') {
      const { stroke_ids, board, ...message } = fields
      const session_id = this.model.sessions.has(id) ? id : this.sessionOfAgent(recipient_device_id ?? id)
      const sent = await this.sendMessage({ session_id, ...message })
      if (Array.isArray(stroke_ids) && stroke_ids.length && typeof board === 'string') await this.sendStrokes({ timeline_id: board, content_type: 'send_away', stroke_ids })
      return sent
    }
    if (scope !== 'desk') return fail('bad-argument', 'a board item goes to desk/<board>')
    const content = { content_type, ...fields }
    const payload = encodeBodyBytes('board_item', content)
    const timeline_key = M.timelineKey('scribble', timeline_id)
    const local_id = `local-${randomHex(8)}`
    this.tell(() => { this.board.echoItem(local_id, timeline_key, JSON.stringify(content), null, null, this.now()) })
    return this.send(local_id, [{ draft: { kind: 'boardItem', board: unhex(id), payload }, files: fileIdsOf('board_item', content), item: { timeline_key, content } }])
  }
  /** The points of a stroke still being drawn (7.2), to the other human devices that are connected: not stored,
   *  not echoed. `piece`: { stroke (32 hex), number (from 1), tool, color, width, points (packed, scribble.ts) }. */
  async sendStrokePiece({ timeline_id, ...piece }: { timeline_id: string; stroke: string; number: number; tool: string; color?: string | null; width?: number | null; points: string }): Promise<void> {
    this.needHuman()
    const board = unhex(timeline_id.slice(timeline_id.indexOf('/') + 1))
    const bytes = new TextEncoder().encode(encodePiece(piece))
    await this.engine.do(d => d.sendStrokePiece(board, bytes))
  }

  // ---- timelines: lazy, newest first

  private async cached(timeline_key: string, opts: { before?: number; after?: number; limit?: number; reverse?: boolean }): Promise<TimelineItem[]> {
    const prefix = `tl/${timeline_key}/`
    const rows = await this.cache.range(prefix, {
      ...(opts.before !== undefined && Number.isFinite(opts.before) ? { before: M.cacheItemKey(timeline_key, opts.before) } : {}),
      ...(opts.after !== undefined ? { after: M.cacheItemKey(timeline_key, opts.after) } : {}),
      ...(opts.limit !== undefined ? { limit: opts.limit } : {}), reverse: opts.reverse ?? false,
    })
    return rows.map(([, v]) => v as TimelineItem)
  }
  /** One page of a Chat or a board from the hub, read by the core out of the hub's order. */
  private async page(timeline_key: string, opts: { before?: number; after?: number; limit: number }): Promise<{ received: ReceivedEnvelope[]; more: boolean; last: number }> {
    const p = M.parseTimelineKey(timeline_key), ref = unhex(p.scope_id)
    const got = p.timeline_kind === 'scribble'
      ? await this.hub.boardItems(ref, { after_change: opts.after ?? 0, limit: opts.limit })
      : await this.hub.chatItems(p.scope as 'session' | 'card', ref, { ...(opts.before !== undefined && Number.isFinite(opts.before) ? { before: opts.before } : {}), limit: opts.limit })
    // `last`: how far the hub's page went, whatever of it could be read
    return { received: (await this.engine.receivePage(got.items)).filter(r => r.outcome !== 'refused'), more: got.more, last: got.items.at(-1)?.change ?? 0 }
  }
  /** Older items of a timeline, newest first: from the cache where it holds them, else the hub's page. `scratch`: the
   *  page is read into a model of its own (a read that must not grow the window), else into the client's. */
  private async older(timeline_key: string, before: number, limit: number, scratch: boolean): Promise<{ items: TimelineItem[]; more: boolean }> {
    await this.flush().catch(() => {})
    const local = await this.cached(timeline_key, { before, limit, reverse: true })
    if (local.length >= limit) return { items: local, more: true }
    let more = false
    try {
      const page = await this.page(timeline_key, { before, limit })
      more = page.more
      const got = JSON.parse(this.board.applyPage(timeline_key, page.received, scratch, this.now())) as TimelineItem[]
      const merged = new Map<number, TimelineItem>(local.map(i => [i.envelope_number!, i]))
      for (const item of got) { const n = item.envelope_number!; merged.set(n, merged.get(n)?.content && !item.content ? merged.get(n)! : item) }
      return { items: [...merged.values()].sort((a, b) => b.envelope_number! - a.envelope_number!).slice(0, limit), more }
    } catch (e) {
      // offline: what the cache holds is what there is
      if (!(e instanceof HubError) || !e.transient) throw e
      return { items: local, more: local.length > 0 }
    }
  }
  /** Loads the next older page of a timeline into the window. */
  async loadTimeline(timeline_key: string, { limit = 50, since_ms }: { limit?: number; since_ms?: number } = {}): Promise<{ loaded: number; has_more: boolean }> {
    // an archived session's page opened: the session itself first (its cards, its name), which no catch-up brought
    if (timeline_key.startsWith('chat:session/')) await this.loadArchived(timeline_key.slice('chat:session/'.length)).catch(() => {})
    // A Chat's first window: its last `since_ms` (24 hours) or `limit` items, whichever holds more, at most
    // `KEEP_PER_CHAT`; what lies before it is read when the person scrolls up.
    if (since_ms !== undefined && !timeline_key.startsWith('scribble:')) {
      const from = this.now() - since_ms
      let loaded = 0, got = await this.loadTimeline(timeline_key, { limit })
      loaded += got.loaded
      const oldest = (): number => { const t = this.model.timelines.get(timeline_key); return t ? t.items.get(t.loaded_down_to)?.sent_at ?? 0 : 0 }
      while (got.has_more && got.loaded > 0 && loaded < KEEP_PER_CHAT && oldest() > from) { got = await this.loadTimeline(timeline_key, { limit: Math.min(200, KEEP_PER_CHAT - loaded) }); loaded += got.loaded }
      return { loaded, has_more: got.has_more }
    }
    // a board has no pages towards the past (its route reads forwards from a change): all of it, once
    if (timeline_key.startsWith('scribble:')) return { loaded: (await this.loadTimelineAfter(timeline_key, 0)).loaded, has_more: false }
    const down = this.model.timelines.get(timeline_key)?.loaded_down_to ?? Infinity
    const { items, more } = await this.older(timeline_key, down, limit, false)
    const has_more = more && items.length > 0
    this.tell(() => { this.board.putItems(timeline_key, JSON.stringify(items), items.length ? items.at(-1)!.envelope_number! : null, has_more) })
    return { loaded: items.length, has_more }
  }
  /** A windowed read for scrolling: items before an envelope number, oldest first. Does not grow the window. */
  async timelineWindow(timeline_key: string, { before_envelope_number = Infinity, limit = 50 }: { before_envelope_number?: number; limit?: number } = {}): Promise<TimelineItem[]> {
    const { items } = await this.older(timeline_key, before_envelope_number, limit, true)
    return items.reverse()
  }
  /** A board's tail after a snapshot: every item after an envelope number, oldest first, all pages. */
  async loadTimelineAfter(timeline_key: string, after_envelope_number: number): Promise<{ loaded: number; items: TimelineItem[]; has_more: false }> {
    const out = new Map<number, TimelineItem>()
    await this.flush().catch(() => {})
    for (const it of await this.cached(timeline_key, { after: after_envelope_number })) out.set(it.envelope_number!, it)
    try {
      for (let after = after_envelope_number; ;) {
        const page = await this.page(timeline_key, { after, limit: 500 })
        for (const item of JSON.parse(this.board.applyPage(timeline_key, page.received, false, this.now())) as TimelineItem[]) {
          if (!(out.get(item.envelope_number!)?.content && !item.content)) out.set(item.envelope_number!, item)
        }
        if (!page.more || page.last <= after) break
        after = page.last
      }
    } catch (e) {
      // not reached: what the cache holds is given if it holds anything, else the caller hears that nothing was read
      if (!(e instanceof HubError) || !e.transient || !out.size) throw e
    }
    const items = [...out.values()].sort((a, b) => a.envelope_number! - b.envelope_number!)
    this.tell(() => { this.board.putItems(timeline_key, JSON.stringify(items), null, null) })
    return { loaded: items.length, items: items.map(it => this.model.timelines.get(timeline_key)?.items.get(it.envelope_number!) ?? it), has_more: false }
  }

  // ---- attachments and Share links

  /** Encrypts and stores a file; the reference goes into the body that names it. Its plaintext is kept in memory. */
  async uploadAttachment(data: Uint8Array | ArrayBuffer | Blob, meta: Record<string, unknown> = {}): Promise<AttachmentRef> {
    const bytes = data instanceof ArrayBuffer ? new Uint8Array(data) : data
    const { file, size } = await putEncrypted(this.core, this.hub, bytes)
    const ref = attachmentRef(file, { ...meta, total_size: size })
    if (bytes instanceof Uint8Array) this.remember(ref.attachment_id, bytes)
    return ref
  }
  private remember(id: string, bytes: Uint8Array): void {
    if (this.attachmentCache.size > 64) this.attachmentCache.delete(this.attachmentCache.keys().next().value!)
    this.attachmentCache.set(id, bytes)
  }
  async fetchAttachment(ref: AttachmentRef): Promise<Uint8Array> {
    const hit = this.attachmentCache.get(ref.attachment_id)
    if (hit) return hit
    const file = fileRefOf(ref)
    // the bytes as the hub stored them, kept in this browser's cache: decrypted and checked against the file's
    // hash as if they had just come (a kept copy that fails is dropped and fetched again)
    const kept = await this.cache.get(`file/${ref.attachment_id}`).catch(() => undefined)
    if (kept instanceof Uint8Array) {
      try { const bytes = decrypt(this.core, file, kept); this.remember(ref.attachment_id, bytes); void this.keptFile(ref.attachment_id, null); return bytes } catch { void this.cache.delete(`file/${ref.attachment_id}`).catch(() => {}) }
    }
    const stored = (await this.hub.getFile(file.fileId)).bytes
    const bytes = decrypt(this.core, file, stored)
    this.remember(ref.attachment_id, bytes)
    if (stored.length <= FILE_KEPT_MAX) void this.keptFile(ref.attachment_id, stored).catch(() => {})
    return bytes
  }
  /** Keeps a file's stored bytes in the cache (`stored`), or marks a kept one as just used (null); drops the least
   *  recently used ones beyond FILES_KEPT. One list of what is kept, in the order of use. */
  private keptFile(id: string, stored: Uint8Array | null): Promise<void> {
    return this.filesKept = this.filesKept.catch(() => {}).then(async () => {
      const list = ((await this.cache.get('client/files').catch(() => null)) ?? []) as [string, number][]
      const at = list.findIndex(([k]) => k === id)
      // (a use of one kept: moved to the end now and then, not written on every picture shown)
      if (!stored && (at < 0 || Math.random() < 0.9)) return
      const size = stored ? stored.length : list[at]![1]
      if (at >= 0) list.splice(at, 1)
      list.push([id, size])
      const drop: string[] = []
      for (let total = list.reduce((n, [, b]) => n + b, 0); total > FILES_KEPT && list.length > 1;) { const [k, b] = list.shift()!; drop.push(k); total -= b }
      await this.cache.setMany([...(stored ? [[`file/${id}`, stored] as [string, unknown]] : []), ...drop.map((k): [string, undefined] => [`file/${k}`, undefined]), ['client/files', list]])
    })
  }
  private filesKept: Promise<void> = Promise.resolve()
  async attachmentBlob(ref: AttachmentRef): Promise<Blob> { return new Blob([await this.fetchAttachment(ref) as Uint8Array<ArrayBuffer>], { type: ref.media_type ?? 'application/octet-stream' }) }

  /** A link for someone outside the room to one file of an open Artifact (11.5): the hub learns the hash of the
   *  link's secret, never the key. `keep_link`: the link is kept in this device's cache for the Links page. */
  async shareAttachment(ref: AttachmentRef, { expires_at = this.now() + 30 * DAY - 60_000, app_url = 'https://app.trommi.com', keep_link = false }: { expires_at?: number; app_url?: string; keep_link?: boolean } = {}): Promise<{ share_id: string; link: string; expires_at: number }> {
    const share = this.core.shareLinkCreate(originOf(app_url), fileRefOf(ref))
    const made = await this.hub.postShare({ share_id: share.shareId, secret_hash: share.secretHash, file_id: unhex(ref.attachment_id), expires_at })
    const share_id = hex(share.shareId)
    const shares = await this.shares()
    shares[share_id] = { attachment_id: ref.attachment_id, expires_at: made.expires_at, ...(keep_link ? { link: share.text, created_at: this.now() } : {}) }
    for (const [id, s] of Object.entries(shares)) if (s.expires_at < this.now()) delete shares[id]
    await this.cache.set('client/shares', shares)
    return { share_id, link: share.text, expires_at: made.expires_at }
  }
  private async shares(): Promise<Record<string, { attachment_id: string; expires_at: number; link?: string; created_at?: number }>> {
    return ((await this.cache.get('client/shares')) ?? {}) as Record<string, { attachment_id: string; expires_at: number; link?: string; created_at?: number }>
  }
  /** The open shares this device made, newest first; `link` only where it was kept. */
  async myShares(): Promise<{ share_id: string; attachment_id: string; expires_at: number; link?: string; created_at?: number }[]> {
    return Object.entries(await this.shares()).filter(([, s]) => s.expires_at > this.now()).map(([share_id, s]) => ({ share_id, ...s })).sort((a, b) => (b.created_at ?? 0) - (a.created_at ?? 0))
  }
  async revokeShare(share_id: string, _opts: { attachment_id?: string | null } = {}): Promise<void> {
    await this.hub.deleteShare(unhex(share_id))
    const shares = await this.shares()
    delete shares[share_id]
    await this.cache.set('client/shares', shares)
  }

  // ---- invites (12.1): the inviter's side

  private setInvite(invite_id: string, fields: Partial<Invite> & { done?: boolean }): void {
    const kept = this.invites.get(invite_id)
    if (!kept) return
    const { done, ...shown } = fields
    Object.assign(kept.public, shown)
    if (done) kept.done = true
    void this.cache.set(`client/invite/${invite_id}`, kept).catch(() => {})
    this.tell(ch => ch.invites.add(invite_id))
  }
  /**
   * An invite link for a human device or an agent. Every invite is bound to the device the human confirms: both
   * sides show the same six emoji, and `confirmInvite` adds the newcomer or burns the link. `takeover` with
   * `session_id`: the agent that joins continues that session and the device that held it is removed (5.3, 13.5).
   */
  async createInvite({ device_role = 'human', app_url = 'https://app.trommi.com/join', label = null, session_id = null, with_history, takeover = false, desk = null, vouch = null }: { device_role?: 'human' | 'agent'; app_url?: string; ttl_ms?: number; label?: string | null; session_id?: string | null; with_history?: boolean; takeover?: boolean; desk?: string | null; vouch?: Vouch | null } = {}): Promise<Invite> {
    this.needHuman()
    if (takeover && (device_role !== 'agent' || !session_id || !this.model.sessions.get(session_id)?.group_id)) fail('bad-argument', 'a takeover invite names an existing session and is for an agent')
    const opened = await this.engine.do(d => d.inviteOpen(device_role, takeover ? unhex(session_id!) : null, originOf(app_url), this.hub.hub_url, this.now()))
    await this.hub.postInvite(opened.offer, opened.signature, opened.mac)
    const invite_id = hex(opened.inviteId)
    const pub: Invite = { invite_id, device_role, link: opened.link, label: label ?? '', session_id: takeover ? session_id : null, with_history: takeover ? with_history !== false : !!with_history, takeover: !!takeover,
      desk: device_role === 'agent' && !takeover && typeof desk === 'string' && desk ? desk.slice(0, 64) : null, confirm_code: !vouch, expires_at: opened.expiresAt, check_code: null, invite_state: 'open', newcomer: null, error: null, ...(vouch && device_role === 'agent' && !takeover ? { vouch } : {}) }
    this.invites.set(invite_id, { public: pub })
    this.model.invites.set(invite_id, pub)
    this.setInvite(invite_id, {})
    // One agent link at a time: a new one for a new session ends this device's earlier one nobody answered yet (it
    // goes at the hub, so the link does nothing any more). One a connector already answered is left to finish.
    if (device_role === 'agent' && !takeover && !vouch) {
      for (const [id, kept] of this.invites) {
        if (id === invite_id || kept.done || kept.public.device_role !== 'agent' || kept.public['takeover'] || kept.public['vouch'] || kept.public.invite_state !== 'open') continue
        this.setInvite(id, { invite_state: 'failed', error: 'invite-replaced', done: true })
        void this.hub.deleteInvite(unhex(id)).catch(() => {})
      }
    }
    return pub
  }
  /** Once a second while invites are open: expiry, and a look for the Request (the stream's event comes first when it comes). */
  private watchInvites(): void {
    for (const [id, kept] of this.invites) {
      if (kept.done) continue
      const state = kept.public.invite_state
      if (state === 'open' && this.now() > kept.public.expires_at) this.setInvite(id, { invite_state: 'expired', done: true })
      else if (state === 'confirm_code' && this.now() > (kept.checked_at ?? 0) + INVITE_CONFIRM_MS) this.setInvite(id, { invite_state: 'expired', error: 'invite-expired', done: true })
      else if (state === 'open') void this.checkInvite(id).catch(() => {})
      else if (state === 'confirm_code' && kept.public['vouch']) this.vouchConfirm(id, kept)
    }
  }
  private async checkInvite(invite_id: string): Promise<void> {
    const kept = this.invites.get(invite_id)
    if (!kept || kept.done || kept.public.invite_state !== 'open' || this.checking.has(invite_id)) return
    this.checking.add(invite_id)
    try {
      const id = unhex(invite_id)
      const { requests } = await this.hub.getInvite(id)
      for (const request of requests ?? []) {
        if (kept.done) return // (replaced by a newer link while the hub was asked)
        let accepted
        try { accepted = await this.engine.do(d => d.inviteAccept(id, request, this.now())) }
        catch (e) { if (this.core.errorCode(e) === 'invite-expired') { this.setInvite(invite_id, { invite_state: 'expired', error: 'invite-expired', done: true }); return } continue }
        // a lost answer here is tried again at the next look: accepting the same Request again gives the same Reveal
        await this.hub.putReveal(id, accepted.reveal, accepted.signature)
        kept.checked_at = this.now()
        // what the person's confirmation names, kept so that it survives a restart (neither is a secret)
        kept.code = [...accepted.code.numbers]; kept.request_hash = b64u(accepted.requestHash)
        this.setInvite(invite_id, { check_code: codeText(kept.code), newcomer: { device_id: hex(accepted.newDevice), device_name: '' }, invite_state: 'confirm_code' })
        return
      }
    } finally { this.checking.delete(invite_id) }
  }
  /** The human compared the six emoji: `true` adds the newcomer, `false` burns the invite. Resolves once the
   *  newcomer is in and everything that follows is done (keys handed over, sessions joined or founded or taken over). */
  async confirmInvite(invite_id: string, matches: unknown): Promise<void> {
    if (typeof matches !== 'boolean') fail('bad-argument', 'confirmInvite takes true (the codes match) or false (they do not)')
    const kept = this.invites.get(invite_id)
    if (!kept || kept.public.invite_state !== 'confirm_code' || !kept.code || !kept.request_hash) return fail('bad-invite', 'this invite waits for no code')
    const id = unhex(invite_id), code = Uint8Array.from(kept.code), request_hash = unb64u(kept.request_hash)
    if (!matches) {
      await this.engine.do(d => d.inviteConfirm(id, code, request_hash, false, this.now())).catch(() => {})
      this.setInvite(invite_id, { invite_state: 'failed', error: 'code-mismatch', done: true })
      void this.hub.deleteInvite(id).catch(() => {})
      return fail('code-mismatch', 'the check codes do not match: nobody was added, the invite is spent')
    }
    // The core commits the newcomer in the write that finishes the invite: a confirmation is acted on once. What
    // follows is the engine's, step by step as the core lists it (engine.ts `invited`), also after a restart.
    try {
      if (kept.public['takeover'] && kept.public['with_history'] === false) await this.engine.withoutHistory(id)
      await this.engine.do(d => d.inviteConfirm(id, code, request_hash, true, this.now()))
    } catch (err) {
      this.setInvite(invite_id, { invite_state: 'failed', error: String(this.core.errorCode(err) ?? 'failed'), done: true })
      throw err
    }
    kept.committed = true
    this.setInvite(invite_id, { invite_state: 'adding' })
    this.engine.keepSoon(0)
    await new Promise<void>((resolve, reject) => { this.finishing.set(invite_id, { resolve, reject }) })
  }
  /** After each look at the invites' steps: an invite that was committed, has nothing left to do and whose device
   *  is in, is `joined`; one whose device is not in (it was removed again) has failed. */
  private invitesDone(open: readonly string[]): void {
    const e = this.engine
    for (const [invite_id, kept] of this.invites) {
      if (kept.done || kept.public.invite_state !== 'adding' || open.includes(invite_id)) continue
      const device = kept.public.newcomer?.device_id, human = kept.public.device_role === 'human'
      const inside = ((human ? e.roles?.humans : e.roles?.agents) ?? []).some(d => hex(d) === device)
      const waiter = this.finishing.get(invite_id)
      this.finishing.delete(invite_id)
      if (!inside) { this.setInvite(invite_id, { invite_state: 'failed', error: 'removed-sender', done: true }); waiter?.reject(new ClientError('removed-sender', 'the newcomer is not in the room')); continue }
      if (!human && !kept.public['takeover']) {
        // the new agent's session: the name the human chose when inviting, and the desk the invite was made on
        const session = e.groups.find(g => g.session && !g.archived && g.leaves.some(l => hex(l) === device))?.session
        if (session) {
          const session_id = kept.public['session_id'] = hex(session.sessionId)
          const vouch = kept.public['vouch'] as Vouch | undefined
          const place = { ...(kept.public.label ? { name: kept.public.label } : {}), ...(kept.public['desk'] ? { desk: kept.public['desk'] } : {}),
            ...(vouch ? { vouched: { by: vouch.session_id, by_name: vouch.by_name, account: vouch.account, account_label: vouch.account_label } } : {}) }
          if (Object.keys(place).length) void this.setRegisters({ [`session/${session_id}`]: { ...(this.model.human.session_settings.get(session_id) ?? {}), ...place } }).catch(() => {})
        }
      }
      this.setInvite(invite_id, { invite_state: 'joined', done: true })
      waiter?.resolve()
    }
  }

  // ---- self-join (12.4): a session of an agent's own machine and account, let in without the emoji

  private vouchTimer: ReturnType<typeof setTimeout> | null = null
  private vouchSoon(): void {
    if (this.vouchTimer !== null || !this.started) return
    this.vouchTimer = setTimeout(() => { this.vouchTimer = null; void this.answerAsks().catch(() => {}) }, 300)
  }
  /** Each live main session whose agent asks to let a new session in (`join_ask/<nonce>`) gets a link for it in
   *  `join_link/<nonce>`, or `{ refused }`: `cap` when that account has MAX_VOUCHED live sessions that joined so or
   *  the room MAX_VOUCHED_ROOM (open vouched links counted in), `busy` while MAX_VOUCHED_OPEN are open here or no
   *  invite can be opened. */
  private async answerAsks(): Promise<void> {
    if (!this.started || this.model.room.connection !== 'live') return
    // an archived session that joined by itself whose device is still in (archived on another device, an older app,
    // or the removal did not go through): its device goes now
    for (const x of this.model.sessions.values()) if ((x.settings?.archived || x.group_archived) && x.settings?.['vouched']) void this.retireVouched(x.session_id, x.settings['vouched']).catch(() => {})
    // an ask past the bounds is refused at once; one within them waits for the person's tap (answerVouch)
    for (const { s, nonce, ask } of this.pendingAsks()) {
      if (this.vouchAnswered.has(nonce)) continue
      const refused = this.vouchRefusal(ask)
      if (!refused) continue
      this.vouchAnswered.add(nonce)
      await this.setRegisters({ [`join_link/${nonce}`]: { refused } }, { session_id: s.session_id })
    }
  }
  /** The asks of live main sessions' agents nobody answered yet. */
  private pendingAsks(): { s: Session; nonce: string; ask: Record<string, unknown> }[] {
    const out: { s: Session; nonce: string; ask: Record<string, unknown> }[] = []
    for (const s of this.model.sessions.values()) {
      if (!s.is_active || s.stale || s.parent_session_id || !s.group_id || s.settings?.archived) continue
      for (const [key, reg] of s.registers) {
        if (!key.startsWith('join_ask/')) continue
        const nonce = key.slice('join_ask/'.length), ask = reg.value as Record<string, unknown> | null
        if (!ask || typeof ask !== 'object' || s.registers.get(`join_link/${nonce}`)?.value != null) continue
        out.push({ s, nonce, ask })
      }
    }
    return out
  }
  /** Why an ask is refused without asking the person: `bad-ask` (no account), `cap` (8 live self-joined sessions of
   *  that account or 24 in the room, this device's open links counted in), `busy` (8 links open here); else null. */
  private vouchRefusal(ask: Record<string, unknown>): string | null {
    const account = String(ask['account'] ?? '')
    const open = [...this.invites.values()].filter(k => !k.done && k.public['vouch']).map(k => (k.public['vouch'] as Vouch).account)
    const accounts = [...this.model.sessions.values()].filter(x => x.is_active && !x.settings?.archived && x.settings?.['vouched']).map(x => (x.settings!['vouched'] as { account?: unknown }).account).concat(open)
    return !account ? 'bad-ask' : accounts.filter(x => x === account).length >= MAX_VOUCHED || accounts.length >= MAX_VOUCHED_ROOM ? 'cap' : open.length >= MAX_VOUCHED_OPEN ? 'busy' : null
  }
  /**
   * The person's tap on an agent's ask to let in a new session of its machine (12.4): `true` (Allow) answers it with
   * an invite link for that session, whose Request is then confirmed when the ask names its hash (the tap stands for
   * the emoji); `false` (Deny) refuses it (`denied`). An ask answered meanwhile (here or on another device) is left.
   */
  async answerVouch(nonce: string, allow: unknown): Promise<void> {
    this.needHuman()
    if (typeof allow !== 'boolean') fail('bad-argument', 'answerVouch takes true (allow) or false (deny)')
    const found = this.pendingAsks().find(x => x.nonce === nonce)
    if (!found || this.vouchAnswered.has(nonce)) return
    const { s, ask } = found
    this.vouchAnswered.add(nonce)
    const answer = async (value: unknown) => { await this.setRegisters({ [`join_link/${nonce}`]: value }, { session_id: s.session_id }) }
    if (!allow) return answer({ refused: 'denied' })
    const refused = this.vouchRefusal(ask)
    if (refused) return answer({ refused })
    const vouch: Vouch = { nonce, session_id: s.session_id, account: String(ask['account']), account_label: String(ask['account_label'] ?? ''), by_name: String(s.settings?.name ?? ask['vouched_by'] ?? '') }
    let invite: Invite
    try { invite = await this.createInvite({ device_role: 'agent', label: (String(ask['name'] ?? '') || String(ask['folder'] ?? '').replace(/[\\/]+$/, '').split(/[\\/]/).pop() || '').slice(0, 120) || null, desk: s.settings?.desk ?? null, vouch }) }
    catch { return answer({ refused: 'busy' }) }
    await answer({ link: invite.link, invite_id: invite.invite_id })
  }
  /** A vouched invite whose Request came: confirmed on its own when the ask names that Request's hash and its
   *  voucher still stands, burned otherwise. */
  private vouchConfirm(invite_id: string, kept: InviteKept): void {
    const vouch = kept.public['vouch'] as Vouch, hash = this.model.sessions.get(vouch.session_id)?.registers.get(`join_ask/${vouch.nonce}`)?.value as { request_hash?: unknown } | null | undefined
    const named = typeof hash?.request_hash === 'string' ? hash.request_hash : null
    if (!named || this.vouchConfirming.has(invite_id)) return
    this.vouchConfirming.add(invite_id)
    // the voucher must still be a live session with its agent in: a removed or archived one vouches for nobody
    const voucher = this.model.sessions.get(vouch.session_id), standing = !!voucher?.is_active && !voucher.settings?.archived
    void this.confirmInvite(invite_id, standing && named === kept.request_hash).catch(() => {}).finally(() => this.vouchConfirming.delete(invite_id))
  }
  /** An archived session that joined by itself takes its device along: it was made for this session alone. */
  private async retireVouched(session_id: string, vouched: unknown): Promise<void> {
    if (!vouched || typeof vouched !== 'object') return
    const device = this.model.sessions.get(session_id)?.agent_device_id
    if (!device || this.vouchRetiring.has(device) || !(this.engine.roles?.agents ?? []).some(a => hex(a) === device)) return
    this.vouchRetiring.add(device)
    try { await this.removeDevices([device]) } finally { this.vouchRetiring.delete(device) }
  }

  // ---- removal (5.2.8)

  /** Removes devices: human devices by one room Commit with their Cuts, agent devices from `agents`; then every
   *  session group that still holds one gets its Remove (5.2.8). Any human device finishes what a crash leaves. */
  async removeDevices(device_ids: string[]): Promise<{ key_epoch: number }> {
    this.needHuman()
    const e = this.engine, room = this.core.roomGroupId(this.room_id)
    const humans = device_ids.filter(id => this.model.members.get(id)?.device_role === 'human').map(unhex)
    const agents = device_ids.filter(id => (e.roles?.agents ?? []).some(a => hex(a) === id)).map(unhex)
    if (humans.length) await e.land(async d => { const cuts = []; for (const h of humans) cuts.push(await d.cutOf(room, h)); return d.removeHumanDevices(cuts, this.now()) })
    if (agents.length) await e.land(d => d.removeAgents(agents, this.now()))
    await e.healNow()
    return { key_epoch: this.model.room.key_epoch }
  }
  /** Log out. The core lets no device commit its own removal, so nothing is removed here: what is still in the
   *  outbox goes out if it can, the client stops, and the caller wipes this device's storage. The device stays a
   *  member (it shows under Devices) until another human device removes it; `removed` says so. */
  async leaveRoom(): Promise<{ key_epoch: number; humans_left: number; removed: false }> {
    this.needHuman()
    try {
      await this.settle({ timeout_ms: 3000 }).catch(() => {})
      const humans_left = [...this.model.members.values()].filter(m => m.is_active && m.device_role === 'human' && m.device_id !== this.my_device_id).length
      return { key_epoch: this.model.room.key_epoch, humans_left, removed: false }
    } finally {
      await this.stop().catch(() => {})
      // the token ends at the hub now, not when it runs out (spec/hub-api.md "Signing out")
      await this.hub.signOut().catch(() => {})
    }
  }

  /** Each live main session on a desk carries that desk's goals for its agent (9.3.4): written when they differ. */
  private goalsSoon(): void {
    if (this.goalsTimer !== null || !this.started) return
    this.goalsTimer = setTimeout(() => { this.goalsTimer = null; void this.syncGoals().catch(() => {}) }, 1500)
  }
  private async syncGoals(): Promise<void> {
    if (!this.started || this.model.room.connection !== 'live') return
    for (const s of this.model.sessions.values()) {
      if (!s.is_active || s.stale || s.parent_session_id || !s.group_id || s.settings?.archived) continue
      const desk_id = s.settings?.desk ?? null
      const desk = desk_id ? this.model.human.desks.get(desk_id) : null
      const goals = M.cleanGoals(desk?.goals)
      const wanted = goals && desk_id ? { desk_id, desk_name: String(desk?.name ?? ''), goals } : null
      const held = s.registers.get('goals')?.value ?? null
      if (JSON.stringify(held) !== JSON.stringify(wanted)) await this.setRegisters({ goals: wanted }, { session_id: s.session_id }).catch(() => {})
    }
  }

  // ---- recovery code (8.6), for account.ts, which holds no key itself

  /** Replaces the recovery code (8.6): the core makes the new one, `account` seals it into the account's new
   *  copies, and one request carries the room Commit, its RecoveryLink and those copies, all or nothing.
   *  `wrong-recovery` (a `code` that is not the one in force) passes through. */
  async replaceRecoveryCode({ code, account }: { code: Uint8Array; account: (new_code: Uint8Array) => Record<string, unknown> | null | Promise<Record<string, unknown> | null> }): Promise<void> {
    this.needHuman()
    const e = this.engine, held = code.slice()
    try {
      const fresh = await e.do(d => d.newRecoveryCode(held))
      let copies: Uint8Array
      try { copies = accountCopiesBytes((await account(fresh)) as AccountCopies | null) } finally { fresh.fill(0) }
      await e.land(d => d.replaceCode(held, copies, this.now()))
    } finally { held.fill(0) }
  }

  /** Resolves only if `code` is the room's recovery code in force, by this device's own verified room state;
   *  `wrong-recovery` otherwise. Sends nothing and leaves nothing behind: the code's signature key is read from a
   *  sign-in it would make (the HubAuth names the key that signs, spec 12.3) and compared with the room's. */
  async checkRecoveryCode(code: Uint8Array): Promise<void> {
    const held = code.slice()
    try {
      const key = this.engine.roles?.recoverySignatureKey
      const { auth } = this.core.recoverySignIn(held, this.room_id, this.hub.hub_url, new Uint8Array(32))
      // HubAuth: room, hub, device, challenge; the device is the 32 bytes before the last 32
      if (!key || !sameBytes(auth.subarray(auth.length - 64, auth.length - 32), key)) fail('wrong-recovery', 'that is not this account\'s recovery code')
    } catch (e) {
      if (e instanceof ClientError) throw e
      fail('wrong-recovery', 'that is not this account\'s recovery code')
    } finally { held.fill(0) }
  }

  // ---- push (15)

  /** Registers this device's Web Push subscription (`subscription.toJSON()`) at a level, or removes it. */
  async pushSubscribe(subscription: unknown, remove = false, level: string | null = null): Promise<void> {
    const s = subscription as { endpoint?: string; keys?: { p256dh?: string; auth?: string } } | null
    if (!s?.endpoint) return fail('bad-argument', 'a push subscription has an endpoint')
    if (remove) { await this.hub.pushRemove(s.endpoint); return }
    if (!s.keys?.p256dh || !s.keys.auth) return fail('bad-argument', 'a push subscription has its keys')
    await this.hub.pushRegister({ web_push: { endpoint: s.endpoint, keys: { p256dh: unb64u(s.keys.p256dh), auth: unb64u(s.keys.auth) } }, level: level === 'knocking' ? 'knocking' : 'all' })
  }
  /** This device's push registrations, as the views read them: `devices[<device id>]`, and the hub's Web Push key. */
  async pushStates(): Promise<{ devices: Record<string, { level: string; endpoints: string[] }>; vapid_public_key: string; apns: boolean }> {
    const state = await this.hub.pushState()
    const devices: Record<string, { level: string; endpoints: string[] }> = {}
    if (state.subscriptions.length) devices[this.my_device_id] = { level: state.subscriptions.some(s => s.level === 'all') ? 'all' : 'knocking', endpoints: state.subscriptions.map(s => s.endpoint) }
    return { devices, vapid_public_key: b64u(state.vapid_public_key), apns: state.apns }
  }
}

export type { EngineMeta }
/** The engine's records in the app's cache, under a prefix of their own. */
export function engineMeta(cache: Cache): EngineMeta {
  return { get: key => cache.get(`engine/${key}`), set: (key, value) => cache.set(`engine/${key}`, value, { durable: true }), delete: key => cache.delete(`engine/${key}`) }
}
/** Whether a register key is one a human device writes into the room group, or one an agent writes into its session. */
const AGENT_KEY = /^(profile|heard|status_line\/.+|alert\/.+)$/
export const isAgentRegisterKey = (key: string): boolean => AGENT_KEY.test(key)
export const isHumanRegisterKey = (key: string): boolean => !isAgentRegisterKey(key) && key !== 'heads' && !key.startsWith('device/')
