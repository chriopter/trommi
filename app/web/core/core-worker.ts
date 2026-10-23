// core-worker.ts: the app core (core/client, through the WASM binding) in a Web Worker, with its executor (shell.ts).
// The pages (public/core.mjs) send events and get renders; nothing of the room is decided on a page.
//
// One core for all windows: where the browser has SharedWorker (desktop browsers), every tab of this profile
// connects to this one worker, and its one core holds the device for all of them. Each tab is a window of the core
// (core/client/src/app/windows.rs): its events name it, its renders go to it alone. Where there is none (Chrome on
// Android), each tab starts this file as a dedicated worker, and one tab at a time holds the device (below).
//
// One slot (`trommi`): the device's state in IndexedDB (store-idb.ts, a Web Lock held for as long as this worker
// owns it), the core's cache in a database of its own (`trommi:app:cache`). One worker owns the slot: another that
// starts (a tab without SharedWorker, a tab of a newer build whose worker is another file) says `busy`, and with
// `take` asks the owner (over a BroadcastChannel) to give it up; the owner stops its core, frees the lock and tells
// its pages `taken`. The demo (`config.demo`) keeps nothing and owns nothing, and runs in a dedicated worker.
//
// Passkeys and push are a page's (WebAuthn and the push manager live there): such a platform op goes to the window
// that acted last and its answer comes back. Renders go to their window; files and logs to every window.
import { CoreShell, memorySpaces } from './shell.ts'
import type { CacheSpace, DeviceSpace } from './shell.ts'
import { IdbStore, deviceOwner, loadCore } from './core-wasm.ts'
import { openCache, openDeviceStore } from './store-idb.ts'
import type { DeviceStore } from './store-idb.ts'

type Json = Record<string, unknown>
/** What a page sends. */
export type ToCore =
  | { t: 'start'; slot: string; config: Json; take?: boolean }
  | { t: 'event'; event: Json; blobs?: Uint8Array[] }
  | { t: 'platform'; id: number; answer?: Json; blobs?: Uint8Array[]; error?: string }
  | { t: 'close' }
  | { t: 'owner_alive'; nonce: string }
/** What the worker sends. */
export type FromCore =
  | { t: 'ready' }
  | { t: 'owner_probe'; nonce: string }
  | { t: 'busy' }
  | { t: 'taken' }
  | { t: 'started' }
  | { t: 'failed'; error: string }
  | { t: 'render'; delta: unknown }
  | { t: 'file'; id: string; media_type: string; bytes: Uint8Array }
  | { t: 'file_gone'; id: string }
  | { t: 'save'; name: string; media_type: string; bytes: Uint8Array }
  | { t: 'open_url'; url: string }
  | { t: 'notify'; notice: unknown }
  | { t: 'log'; level: string; text: string }
  | { t: 'platform'; id: number; op: Json; blobs: Uint8Array[] }

interface Port {
  postMessage(m: FromCore): void
  onmessage: ((e: MessageEvent<ToCore>) => void) | null
  start?(): void
  addEventListener?(type: 'close', fn: () => void): void
}

/** One page connected to this worker: it asked for the core (`start`), and its window of the core once attached. */
interface Conn { port: Port; wants: boolean; window: number | null }

const conns = new Set<Conn>()

/** The core: none, starting, running, or `busy` (another worker holds the device). */
let phase: 'idle' | 'starting' | 'running' | 'busy' = 'idle'
let shell: CoreShell | null = null
/** Events that came while the core started: handed in, in order, once it is up. */
let early: { conn: Conn; event: Json; blobs: Uint8Array[] }[] = []
let store: DeviceStore | null = null
let owner: ReturnType<typeof deviceOwner<DeviceStore>> | null = null
/** Counts starts and stops: the work of a start that was stopped meanwhile is dropped. */
let run = 0
/** A stop in progress (the lock being freed): a new start waits for it. */
let stopping: Promise<void> = Promise.resolve()
/** The next window number of the running core (each core counts from 0). */
let windows = 0
/** The page whose window did something last (an action): a platform op, a download, a link to open go there. */
let lastConn: Conn | null = null
/** That page went before its window's prompts were answered. */
let actorLeft = false
const asked = new Map<number, { conn: Conn; ok: (r: { answer: Json; blobs: Uint8Array[] }) => void; bad: (e: Error) => void }>()
let asks = 0

const send = (conn: Conn | null, m: FromCore): void => { try { conn?.port.postMessage(m) } catch {} }
/** To every page that asked for the core (`attached`: only those that are windows of it). */
const everyone = (m: FromCore, attached = true): void => { for (const c of conns) if (attached ? c.window !== null : c.wants) send(c, m) }
const byWindow = (w: number): Conn | null => { for (const c of conns) if (c.window === w) return c; return null }
/** Where a page's service goes: the window that did something last, else any window; not another one when that
 *  window went (`any` false: a passkey or push prompt is never shown in a tab that did not ask). */
const actor = (any: boolean): Conn | null => {
  if (lastConn && conns.has(lastConn) && lastConn.window !== null) return lastConn
  if (actorLeft && !any) return null
  return [...conns].find(c => c.window !== null) ?? null
}

/** The slot's device state as the shell takes it: the binding's store, its values wrapped at rest. */
function deviceSpace(s: DeviceStore): DeviceSpace {
  return {
    load: () => s.load(),
    apply: async (expectedRevision, put, remove) => {
      try { await s.apply({ expectedRevision, put, delete: remove }) } catch (e) {
        const name = (e as { name?: string })?.name
        throw Object.assign(new Error('the device store refused a write'), { kind: name === 'StoreConflict' ? 'conflict' : name === 'QuotaExceededError' ? 'full' : 'failed' })
      }
    },
  }
}
/** The core's cache: string keys, bytes. */
async function cacheSpace(name: string): Promise<CacheSpace> {
  const c = await openCache(name)
  const bytes = (v: unknown): Uint8Array | null => (v instanceof Uint8Array ? v : null)
  return {
    get: keys => Promise.all(keys.map(k => c.get(k).then(bytes))),
    scan: async prefix => (await c.range(prefix)).flatMap(([key, value]) => { const v = bytes(value); return v ? [{ key, value: v }] : [] }),
    keys: prefix => c.keys(prefix),
    put: (put, remove, durable = false) => c.setMany([...remove.map((k): [string, undefined] => [k, undefined]), ...put.map((e): [string, Uint8Array] => [e.key, e.value])], { durable }),
  }
}

/** A page joins the running core: it becomes a window, which the core renders whole. */
function attach(conn: Conn): void {
  if (!shell || conn.window !== null) return
  conn.window = windows++
  send(conn, { t: 'started' })
  shell.send({ type: 'window_opened', window: conn.window })
}

async function start(m: Extract<ToCore, { t: 'start' }>): Promise<void> {
  // (this start's number before its first wait: a stop meanwhile makes it stale, and a stale start lets go of
  //  whatever it took and touches nothing else)
  const mine = ++run
  phase = 'starting'
  let o: ReturnType<typeof deviceOwner<DeviceStore>> | null = null
  let opened: DeviceStore | null = null
  const stale = async (): Promise<void> => { o?.close(); await opened?.close().catch(() => {}) }
  try {
    await stopping
    if (mine !== run) return
    const core = await loadCore()
    if (mine !== run) return
    let spaces: { device: DeviceSpace; cache: CacheSpace }
    // (the demo, and a page with no slot (a Share link's page): nothing kept, nothing owned)
    if (m.config['demo'] != null || !m.slot) spaces = memorySpaces()
    else {
      // One owner: the lock, or `busy`. With `take` the owner is asked to give it up, and this waits for it.
      o = deviceOwner(m.slot, steal => openDeviceStore({ name: m.slot, steal, onLost: () => { if (mine === run) void giveUp() }, IdbStore }), nonce => everyone({ t: 'owner_probe', nonce }, false), () => { if (mine === run) void giveUp() })
      owner = o
      opened = await o.open(Boolean(m.take))
      if (mine !== run) return await stale()
      if (!opened) { o.close(); owner = null; phase = 'busy'; early = []; everyone({ t: 'busy' }, false); return }
      store = opened
      spaces = { device: deviceSpace(opened), cache: await cacheSpace(`${m.slot}:app`) }
      if (mine !== run) return await stale()
    }
    windows = 0
    shell = new CoreShell({
      core: new core.AppCore(), ...spaces,
      host: {
        render: (delta, window) => send(byWindow(window), { t: 'render', delta }),
        fileReady: (id, media_type, bytes) => everyone({ t: 'file', id, media_type, bytes }),
        fileGone: id => everyone({ t: 'file_gone', id }),
        saveFile: (name, media_type, bytes) => send(actor(true), { t: 'save', name, media_type, bytes }),
        openUrl: url => send(actor(true), { t: 'open_url', url }),
        notify: notice => send(actor(true), { t: 'notify', notice }),
        log: (level, text) => everyone({ t: 'log', level, text }),
        platform: (op, blobs) => new Promise((ok, bad) => {
          // (a passkey or push prompt only in the window that asked; gone: refused, never another tab's)
          const conn = actor(false)
          if (!conn) { ok({ answer: { answer: 'refused' }, blobs: [] }); return }
          const id = ++asks
          asked.set(id, { conn, ok, bad })
          send(conn, { t: 'platform', id, op, blobs })
        }),
      },
    })
    phase = 'running'
    // (every page that asked meanwhile is a window now, in the order it came, the first window 0 before the start
    //  renders it; then what each sent)
    for (const c of conns) if (c.wants) c.window = windows++
    shell.send({ type: 'start', config: { ...m.config, slot: m.slot } })
    for (const c of conns) if (c.window !== null) { send(c, { t: 'started' }); if (c.window > 0) shell.send({ type: 'window_opened', window: c.window }) }
    for (const e of early) if (e.conn.window !== null) shell.send({ ...e.event, window: e.conn.window }, e.blobs)
    early = []
  } catch (e) {
    // (a start that failed lets go of the device it took; only the current one says so)
    if (mine !== run) return await stale()
    run++
    phase = 'idle'
    shell?.close(); shell = null; owner = null; store = null; early = []
    await stale()
    everyone({ t: 'failed', error: String((e as Error)?.message ?? e) }, false)
  }
}

/** Another worker takes the slot (or the last page went): this core stops, the lock is freed, the pages hear
 *  `taken` (`tell`). A later start begins anew. */
function giveUp(tell = true): Promise<void> {
  if (phase === 'idle') return stopping
  run++
  phase = 'idle'
  early = []
  const s = shell, o = owner, st = store
  shell = null; owner = null; store = null
  s?.send({ type: 'background' })
  s?.close()
  o?.close()
  for (const a of asked.values()) a.bad(new Error('the core stopped'))
  asked.clear()
  if (tell) everyone({ t: 'taken' }, false)
  for (const c of conns) { c.window = null; c.wants = false }
  stopping = (st?.close() ?? Promise.resolve()).catch(() => {})
  return stopping
}

/** A page went: its window closes; with the last page the core stops and frees the device. */
function leave(conn: Conn): void {
  if (!conns.delete(conn)) return
  if (conn.window !== null && shell) shell.send({ type: 'window_closed', window: conn.window })
  for (const [id, a] of asked) if (a.conn === conn) { asked.delete(id); a.ok({ answer: { answer: 'refused' }, blobs: [] }) }
  early = early.filter(e => e.conn !== conn)
  if (lastConn === conn) { lastConn = null; actorLeft = true }
  if (![...conns].some(c => c.wants)) void giveUp(false)
}

function receive(conn: Conn, m: ToCore): void {
  switch (m.t) {
    case 'start':
      conn.wants = true
      // (a page while the core runs is one more window of it; while another worker holds the device, it asks
      //  again only with `take`)
      if (phase === 'running') { attach(conn); return }
      if (phase === 'starting') return
      if (phase === 'busy' && !m.take) { send(conn, { t: 'busy' }); return }
      void start(m)
      return
    case 'event': {
      const blobs = m.blobs ?? []
      if (m.event['type'] === 'action') { lastConn = conn; actorLeft = false }
      if (shell && conn.window !== null) shell.send({ ...m.event, window: conn.window }, blobs)
      else if (phase === 'starting' && conn.wants) early.push({ conn, event: m.event, blobs })
      return
    }
    case 'owner_alive':
      owner?.respond(m.nonce)
      return
    case 'platform': {
      const a = asked.get(m.id)
      asked.delete(m.id)
      if (m.error != null) a?.bad(new Error(m.error))
      else a?.ok({ answer: m.answer ?? { answer: 'refused' }, blobs: m.blobs ?? [] })
      return
    }
    case 'close':
      leave(conn)
      return
  }
}

function connect(port: Port): void {
  const conn: Conn = { port, wants: false, window: null }
  conns.add(conn)
  port.onmessage = ({ data }) => receive(conn, data)
  // (a tab that ends without a word: where the browser tells a port's end, its window closes)
  port.addEventListener?.('close', () => leave(conn))
  port.start?.()
  send(conn, { t: 'ready' })
}

// A SharedWorker hears each page connect; a dedicated worker has its one page on its own scope.
const scope = globalThis as unknown as Port & { onconnect?: ((e: MessageEvent) => void) | null }
if ('onconnect' in scope) scope.onconnect = e => { const p = e.ports[0]; if (p) connect(p as unknown as Port) }
else connect(scope)
addEventListener('unhandledrejection', event => { event.preventDefault(); everyone({ t: 'log', level: 'error', text: String(event.reason?.message ?? event.reason) }, false) })
// (the core's module is fetched and compiled at once, while the page still sets itself up)
loadCore().catch(() => {})
