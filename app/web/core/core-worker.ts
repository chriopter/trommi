// core-worker.ts: the app core (core/client, through the WASM binding) in a dedicated Web Worker, with its executor
// (shell.ts). The page (public/core.mjs) sends events and gets renders; nothing of the room is decided on the page.
//
// One slot (`trommi`): the device's state in IndexedDB (store-idb.ts, a Web Lock held for as long as this worker
// owns it), the core's cache in a database of its own (`trommi:app:cache`). One tab owns the slot: another tab that
// starts says `busy`, and with `take` asks the owner (over a BroadcastChannel) to give it up; the owner stops its
// core, frees the lock and tells its page `taken`. The demo (`config.demo`) keeps nothing and owns nothing.
//
// Passkeys and push are the page's (WebAuthn and the push manager live there): such a platform op goes to the page
// and its answer comes back. What the core hands out (renders, files, links, logs) goes to the page as it comes.
import { CoreShell, memorySpaces } from './shell.ts'
import type { CacheSpace, DeviceSpace } from './shell.ts'
import { IdbStore, loadCore } from './core-wasm.ts'
import { openCache, openDeviceStore } from './store-idb.ts'
import type { DeviceStore } from './store-idb.ts'

type Json = Record<string, unknown>
/** What the page sends. */
export type ToCore =
  | { t: 'start'; slot: string; config: Json; take?: boolean }
  | { t: 'event'; event: Json; blobs?: Uint8Array[] }
  | { t: 'platform'; id: number; answer?: Json; blobs?: Uint8Array[]; error?: string }
  | { t: 'close' }
/** What the worker sends. */
export type FromCore =
  | { t: 'ready' }
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

const port = globalThis as unknown as { postMessage(m: FromCore): void; onmessage: ((e: MessageEvent<ToCore>) => void) | null }
const post = (m: FromCore): void => port.postMessage(m)

let shell: CoreShell | null = null
/** Events the page sent before the core was up: handed in, in order, once it is. */
let early: { event: Json; blobs: Uint8Array[] }[] | null = []
let store: DeviceStore | null = null
let channel: BroadcastChannel | null = null
let stopped = false
const asked = new Map<number, { ok: (r: { answer: Json; blobs: Uint8Array[] }) => void; bad: (e: Error) => void }>()
let asks = 0

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
    put: (put, remove) => c.setMany([...remove.map((k): [string, undefined] => [k, undefined]), ...put.map((e): [string, Uint8Array] => [e.key, e.value])]),
  }
}

async function start(m: Extract<ToCore, { t: 'start' }>): Promise<void> {
  const core = await loadCore()
  let spaces: { device: DeviceSpace; cache: CacheSpace }
  // (the demo, and a page with no slot (a Share link's page): nothing kept, nothing owned)
  if (m.config['demo'] != null || !m.slot) spaces = memorySpaces()
  else {
    // One owner: the lock, or `busy`. With `take` the owner is asked to give it up, and this waits for it.
    channel = new BroadcastChannel(`trommi-core:${m.slot}`)
    if (m.take) channel.postMessage({ give: true })
    store = openDeviceStore({ name: m.slot, wait: Boolean(m.take), IdbStore })
    try { await store.load() } catch (e) {
      store = null
      if ((e as { name?: string })?.name === 'StoreConflict') { channel.close(); channel = null; post({ t: 'busy' }); return }
      throw e
    }
    channel.onmessage = ({ data }) => { if (data?.give) void giveUp() }
    spaces = { device: deviceSpace(store), cache: await cacheSpace(`${m.slot}:app`) }
  }
  shell = new CoreShell({
    core: new core.AppCore(), ...spaces,
    host: {
      render: delta => post({ t: 'render', delta }),
      fileReady: (id, media_type, bytes) => post({ t: 'file', id, media_type, bytes }),
      fileGone: id => post({ t: 'file_gone', id }),
      saveFile: (name, media_type, bytes) => post({ t: 'save', name, media_type, bytes }),
      openUrl: url => post({ t: 'open_url', url }),
      notify: notice => post({ t: 'notify', notice }),
      log: (level, text) => post({ t: 'log', level, text }),
      platform: (op, blobs) => new Promise((ok, bad) => {
        const id = ++asks
        asked.set(id, { ok, bad })
        post({ t: 'platform', id, op, blobs })
      }),
    },
  })
  shell.send({ type: 'start', config: { ...m.config, slot: m.slot } })
  for (const e of early ?? []) shell.send(e.event, e.blobs)
  early = null
  post({ t: 'started' })
}

/** Another tab takes the slot (or the page closes it): this core stops, the lock is freed, the page hears `taken`. */
async function giveUp(): Promise<void> {
  if (stopped) return
  stopped = true
  shell?.send({ type: 'background' })
  shell?.close()
  shell = null
  channel?.close()
  await store?.close().catch(() => {})
  store = null
  post({ t: 'taken' })
}

port.onmessage = ({ data: m }) => {
  switch (m.t) {
    case 'start':
      start(m).catch(e => post({ t: 'failed', error: String((e as Error)?.message ?? e) }))
      return
    case 'event':
      if (shell) shell.send(m.event, m.blobs ?? [])
      else early?.push({ event: m.event, blobs: m.blobs ?? [] })
      return
    case 'platform': {
      const a = asked.get(m.id)
      asked.delete(m.id)
      if (m.error != null) a?.bad(new Error(m.error))
      else a?.ok({ answer: m.answer ?? { answer: 'refused' }, blobs: m.blobs ?? [] })
      return
    }
    case 'close':
      void giveUp()
      return
  }
}
addEventListener('unhandledrejection', event => { event.preventDefault(); post({ t: 'log', level: 'error', text: String(event.reason?.message ?? event.reason) }) })
// (the core's module is fetched and compiled at once, while the page still sets itself up)
loadCore().catch(() => {})
post({ t: 'ready' })
