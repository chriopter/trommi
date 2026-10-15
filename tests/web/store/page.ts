// page.ts: the test page of the web app's storage (store-idb.ts), bundled by
// run.mjs and loaded into headless Chromium. What needs one page only runs here in one go (`T.single()`); what needs
// several tabs, a quota or a reload is cut into steps that run.mjs calls over the DevTools protocol.
//
// Real here: IndexedDB, Web Locks, BroadcastChannel and WebCrypto of the browser; the binding's IdbStore; and, in the
// tests named "core:", the Rust core's Device from the WASM build run.mjs serves under /pkg/. NOT real: the client.
import { deleteStores, dropIfEmpty, openCache, openDeviceStore } from '../../../app/web/core/store-idb.ts'
import type { Cache, DeviceStore, IdbStoreClass } from '../../../app/web/core/store-idb.ts'
import type { StoredState, StoreEntry, StoreWrite } from '../../../app/web/core/core-api.ts'
import type * as Binding from '../../../core/bindings/wasm/js/trommi-core.js'
const hex = (b: Uint8Array): string => Array.from(b, x => x.toString(16).padStart(2, '0')).join('')
const keyOf = (n: number): Uint8Array => new Uint8Array([n >>> 24, n >>> 16, n >>> 8, n])
import type { FakeClient } from './fake-client.ts'

interface Result { name: string; ok: boolean; detail: string }

// The binding as run.mjs serves it (never bundled: the glue fetches its .wasm from beside itself).
const pkg = '/pkg/'
const core = await import(`${pkg}trommi-core.js`) as typeof Binding
const IdbStore = (await import(`${pkg}idb-store.js`) as { IdbStore: IdbStoreClass }).IdbStore
await core.init()

const utf8 = new TextEncoder(), text = new TextDecoder()
const sleep = (ms: number) => new Promise(r => setTimeout(r, ms))
const bytes = (...b: number[]) => new Uint8Array(b)
const put = (key: Uint8Array, value: Uint8Array | string): StoreEntry => ({ key, value: typeof value === 'string' ? utf8.encode(value) : value })
const write = (expectedRevision: number, puts: StoreEntry[], deletes: Uint8Array[] = []): StoreWrite => ({ expectedRevision, put: puts, delete: deletes })
/** What a store holds, as text: "key=value" sorted by key. */
const shown = (entries: StoreEntry[]) => entries.map(e => `${hex(e.key)}=${text.decode(e.value)}`).sort().join(' ')
let names = 0
const fresh = (what: string) => `t-${what}-${Date.now().toString(36)}-${names++}`

function check(cond: unknown, what: string): asserts cond { if (!cond) throw new Error(what) }
function same(got: unknown, want: unknown, what: string): void {
  const g = JSON.stringify(got), w = JSON.stringify(want)
  if (g !== w) throw new Error(`${what}: got ${g}, expected ${w}`)
}
/** The error a promise or a function ends with; fails when there is none. */
async function refused(run: () => unknown, what: string): Promise<{ code?: string; name?: string; message: string }> {
  try { await run() } catch (e) { return e as { code?: string; name?: string; message: string } }
  throw new Error(`${what}: was not refused`)
}
async function until(cond: () => unknown, what: string, ms = 8000): Promise<void> {
  const t = Date.now()
  while (!await cond()) { if (Date.now() - t > ms) throw new Error(`timed out waiting for ${what}`); await sleep(20) }
}
const deviceStore = (name: string, wait = false) => openDeviceStore({ name, wait, IdbStore })
/** A loaded store, without waiting for the lock: after `await close()` of the one before, the state is free. */
async function opened(name: string): Promise<{ store: DeviceStore; revision: number; entries: StoreEntry[] }> {
  const store = deviceStore(name)
  return { store, ...await store.load() }
}
/** A database read or changed with plain IndexedDB: not through store-idb.ts, not through the binding, and without the lock. */
function plain<T>(name: string, stores: string[], mode: IDBTransactionMode, run: (tx: IDBTransaction) => T): Promise<T> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(name)
    req.onerror = () => reject(req.error)
    req.onsuccess = () => {
      const db = req.result
      const tx = db.transaction(stores, mode)
      const out = run(tx)
      tx.oncomplete = () => { db.close(); resolve(out) }
      tx.onabort = () => { db.close(); reject(tx.error) }
    }
  })
}
async function raw(name: string, store: string): Promise<{ keys: IDBValidKey[]; values: unknown[] }> {
  const [keys, values] = await plain(name, [store], 'readonly', tx => [tx.objectStore(store).getAllKeys(), tx.objectStore(store).getAll()] as const)
  return { keys: keys.result, values: values.result }
}
const contains = (hay: Uint8Array, needle: Uint8Array): boolean => {
  outer: for (let i = 0; i + needle.length <= hay.length; i++) { for (let j = 0; j < needle.length; j++) if (hay[i + j] !== needle[j]) continue outer; return true }
  return false
}
const lockHeld = async (name: string) => (await navigator.locks.query()).held?.some(l => l.name === `trommi-core:${name}`) ?? false
/** The lock is free NOW: whatever closed before has resolved only once the browser let it go. */
const freed = async (name: string) => same(await lockHeld(name), false, 'the lock')
const databases = async (name: string) => (await indexedDB.databases()).map(d => d.name).filter(n => n?.startsWith(name)).sort()

// ---------------------------------------------------------------------------------------------------------------------
// One page

const tests: [string, () => Promise<void>][] = [
  ['store: an empty store loads as revision 0 with no entries and makes no wrapping key; load() twice gives the same state; the lock is held from load() to close()', async () => {
    const name = fresh('empty')
    const store = deviceStore(name)
    same(await lockHeld(name), false, 'the lock before load()')
    const state = await store.load()
    same([state.revision, state.entries.length, await lockHeld(name)], [0, 0, true], 'empty store')
    check(await store.load() === state, 'a second load() read again')
    same(await databases(name), [name], 'a look into an empty store leaves no key behind')
    const other = deviceStore(name)
    same((await refused(() => other.load(), 'a second owner')).name, 'StoreConflict', 'the second owner')
    await store.close()
    same(await lockHeld(name), false, 'the lock once close() resolved')
    await refused(() => store.load(), 'load() on a closed store')
    const again = await opened(name)
    await again.store.close()
  }],
  ['store: writes are applied in order and are there after reopening', async () => {
    const name = fresh('order')
    const a = await opened(name)
    await a.store.apply(write(0, [put(bytes(1, 1), 'one'), put(bytes(1, 2), 'two')]))
    await a.store.apply(write(1, [put(bytes(1, 1), 'ONE'), put(bytes(2), 'three')], [bytes(1, 2)]))
    // deletes come first: a key both deleted and put ends up put; of two puts of one key the later counts
    await a.store.apply(write(2, [put(bytes(9), 'early'), put(bytes(9), 'late'), put(bytes(2), 'again')], [bytes(2), bytes(7, 7)]))
    await a.store.close()
    const b = await opened(name)
    same([b.revision, shown(b.entries)], [3, '0101=ONE 02=again 09=late'], 'after reopen')
    // an empty value and an empty write are a value and a revision like any other
    await b.store.apply(write(3, [put(bytes(3), new Uint8Array(0))]))
    await b.store.apply(write(4, []))
    await b.store.close()
    const c = await opened(name)
    same([c.revision, shown(c.entries)], [5, '0101=ONE 02=again 03= 09=late'], 'after the second reopen')
    await c.store.close()
  }],
  ['store: a write that names the wrong revision is refused with StoreConflict and nothing of it is stored', async () => {
    const name = fresh('conflict')
    const a = await opened(name)
    same((await refused(() => a.store.apply(write(5, [put(bytes(1), 'no')])), 'expected 5 at revision 0')).name, 'StoreConflict', 'refusal')
    await a.store.apply(write(0, [put(bytes(1), 'yes')]))
    same((await refused(() => a.store.apply(write(0, [put(bytes(1), 'twice'), put(bytes(2), 'more')])), 'expected 0 at revision 1')).name, 'StoreConflict', 'second refusal')
    await a.store.close()
    const b = await opened(name)
    same([b.revision, shown(b.entries)], [1, '01=yes'], 'stored')
    await b.store.close()
    same((await refused(() => deviceStore(fresh('unloaded')).apply(write(0, [])), 'a write before load()')).code, 'storage-failed', 'before load')
  }],
  ['store: another owner wrote behind this one\'s back (past the lock): the next write is refused whole', async () => {
    const name = fresh('behind')
    const a = await opened(name)
    await a.store.apply(write(0, [put(bytes(1), 'mine')]))
    // what a second owner would do if the lock had failed: one more write, the revision counted up
    await plain(name, ['meta'], 'readwrite', tx => { tx.objectStore('meta').put(2, 'revision') })
    same((await refused(() => a.store.apply(write(1, [put(bytes(1), 'overwritten'), put(bytes(2), 'new')])), 'the write behind another owner')).name, 'StoreConflict', 'refusal')
    await a.store.close()
    const b = await opened(name)
    same([b.revision, shown(b.entries)], [2, '01=mine'], 'what is stored')
    await b.store.close()
  }],
  ['store: values are wrapped at rest (no plaintext in IndexedDB), a fresh nonce per write, bound to their key, under a non-extractable key', async () => {
    const name = fresh('wrapped')
    const secret = crypto.getRandomValues(new Uint8Array(64))
    const a = await opened(name)
    await a.store.apply(write(0, [put(bytes(1), secret), put(bytes(2), secret), put(bytes(3), 'other')]))
    const once = (await raw(name, 'entries')).values as Uint8Array[]
    await a.store.apply(write(1, [put(bytes(1), secret)]))   // the same value under the same key again
    await a.store.close()
    const device = await raw(name, 'entries')
    same(device.keys.map(k => hex(new Uint8Array(k as ArrayBuffer))), ['01', '02', '03'], 'the raw keys are the entries\' key bytes')
    const stored = device.values as Uint8Array[]
    for (const v of stored) check(v instanceof Uint8Array && !contains(v, secret.subarray(0, 16)) && !contains(v, utf8.encode('other')), 'a stored value shows its plaintext')
    same([stored[0]!.length, stored[0]![0]], [1 + 12 + 64 + 16, 1], 'form byte, nonce, ciphertext, tag')
    const nonces = [once[0]!, once[1]!, once[2]!, stored[0]!].map(v => hex(v.subarray(1, 13)))
    same(new Set(nonces).size, 4, `the nonces of four writes: ${nonces}`)
    const keys = await raw(`${name}:wrap`, 'wrap')
    const key = keys.values[0] as CryptoKey
    check(keys.values.length === 1 && key instanceof CryptoKey && key.extractable === false && key.algorithm.name === 'AES-GCM', 'the wrapping key is not a single non-extractable AES-GCM key')
    await refused(() => crypto.subtle.exportKey('raw', key), 'exporting the wrapping key')
    // the key is kept, not made again: a second store on the name reads what the first wrote
    const b = await opened(name)
    same(b.entries.map(e => hex(e.value)).sort(), [hex(secret), hex(secret), hex(utf8.encode('other'))].sort(), 'unwrapped on load')
    await b.store.close()
    // a stored value moved under another key does not open (the key is the additional data)
    await plain(name, ['entries'], 'readwrite', tx => { tx.objectStore('entries').put(stored[0], new Uint8Array([3]).buffer) })
    await freed(name)
    const c = deviceStore(name)
    const e = await refused(() => c.load(), 'loading a moved value')
    same(e.code, 'storage-failed', 'a moved value')
    await freed(name)   // (the failed load gave the lock back)
    check(!e.message.includes(hex(secret).slice(0, 8)), 'the error shows stored bytes')
  }],
  ['store: entries whose wrapping key is gone do not open, and no new key is made over them', async () => {
    const name = fresh('keyless')
    const a = await opened(name)
    await a.store.apply(write(0, [put(bytes(1), 'sealed')]))
    await a.store.close()
    await new Promise<void>((resolve, reject) => { const r = indexedDB.deleteDatabase(`${name}:wrap`); r.onsuccess = () => resolve(); r.onerror = () => reject(r.error) })
    await freed(name)
    same((await refused(() => deviceStore(name).load(), 'loading without the key')).code, 'storage-failed', 'refusal')
    same((await raw(`${name}:wrap`, 'wrap')).values.length, 0, 'no key was made')
    await freed(name)
  }],
  ['store: close() while load() waits for the lock: that load() is refused at once and never takes the lock', async () => {
    const name = fresh('line')
    const a = await opened(name)
    const b = deviceStore(name, true)
    let outcome = 'waiting'
    const loading = b.load().then(() => { outcome = 'loaded' }, () => { outcome = 'refused' })
    await sleep(150)
    same([outcome, (await navigator.locks.query()).pending?.filter(l => l.name === `trommi-core:${name}`).length], ['waiting', 1], 'the second store while the first owns')
    await b.close()
    await loading
    same([outcome, (await navigator.locks.query()).pending?.filter(l => l.name === `trommi-core:${name}`).length, await lockHeld(name)], ['refused', 0, true], 'out of the line, while the first still owns')
    await a.store.close()
    const c = deviceStore(name, true)
    same((await c.load()).revision, 0, 'a waiting load() with nobody in front')
    await c.close()
  }],
  ['store: the lock is stolen from an owner: its next write is refused with StoreConflict and nothing of it is stored', async () => {
    const name = fresh('stolen')
    const a = await opened(name)
    await a.store.apply(write(0, [put(bytes(1), 'mine')]))
    let giveBack = () => {}
    const thief = navigator.locks.request(`trommi-core:${name}`, { steal: true }, () => new Promise<void>(r => { giveBack = r }))
    await until(async () => (await navigator.locks.query()).held?.filter(l => l.name === `trommi-core:${name}`).length === 1, 'the theft')
    await sleep(50)
    same((await refused(() => a.store.apply(write(1, [put(bytes(2), 'after the theft')])), 'a write after the lock was stolen')).name, 'StoreConflict', 'refusal')
    await a.store.close()
    giveBack(); await thief
    const b = await opened(name)
    same([b.revision, shown(b.entries)], [1, '01=mine'], 'what is stored')
    await b.store.close()
  }],
  ['store: checked takeover fences the old owner before the new owner writes', async () => {
    const name = fresh('takeover')
    let lost = 0
    const a = openDeviceStore({ name, IdbStore, onLost: () => { lost++ } })
    await a.load(); await a.apply(write(0, [put(bytes(1), 'first')]))
    const b = openDeviceStore({ name, IdbStore, steal: true })
    const loaded = await b.load()
    same([lost, loaded.revision, shown(loaded.entries)], [1, 1, '01=first'], 'the previous owner was fenced')
    same((await refused(() => a.apply(write(1, [put(bytes(2), 'stale')])), 'stale write')).name, 'StoreConflict', 'late write refused')
    await b.apply(write(1, [put(bytes(3), 'new owner')]))
    await a.close(); await b.close()
    const c = await opened(name)
    same([c.revision, shown(c.entries)], [2, '01=first 03=new owner'], 'only the current owner wrote')
    await c.store.close()
  }],
  ['store: deleteStores is refused while a device has the state open; then it removes the state, the cache and the wrapping key; the next device gets a new key', async () => {
    const name = fresh('deleted')
    const a = await opened(name)
    await a.store.apply(write(0, [put(bytes(1), 'gone soon')]))
    const cache = await openCache(name)
    await cache.set('x', 1)
    same(await databases(name), [name, `${name}:cache`, `${name}:wrap`], 'before')
    same((await refused(() => deleteStores({ name, IdbStore }), 'deleting an open state')).name, 'StoreConflict', 'while open')
    same([await databases(name), await cache.get('x')], [[name, `${name}:cache`, `${name}:wrap`], 1], 'nothing was deleted')
    await a.store.close()
    await deleteStores({ name, IdbStore })
    same(await databases(name), [], 'after')
    same((await refused(() => cache.get('x'), 'the cache after the delete')).code, 'storage-failed', 'cache')
    const b = await opened(name)
    same([b.revision, b.entries.length], [0, 0], 'a new store on the name')
    await b.store.apply(write(0, [put(bytes(1), 'the next device')]))
    await b.store.close()
    const c = await opened(name)
    same([shown(c.entries), (await raw(`${name}:wrap`, 'wrap')).values.length], ['01=the next device', 1], 'the next device, under its new key')
    await c.store.close()
  }],
  ['store: dropIfEmpty: an empty look leaves no database behind; a stored state, or a state whose lock is taken, is kept', async () => {
    const name = fresh('look')
    const a = await opened(name)
    const cache = await openCache(name)
    await a.store.close(); cache.close()
    same(await databases(name), [name, `${name}:cache`], 'what the look made')
    await dropIfEmpty(name)
    same(await databases(name), [], 'after dropIfEmpty')
    await dropIfEmpty(name)
    same(await databases(name), [], 'nothing there: nothing made')
    const b = await opened(name)
    await b.store.apply(write(0, [put(bytes(1), 'kept')]))
    await dropIfEmpty(name)   // (while it is open: the lock is taken)
    same(await databases(name), [name, `${name}:wrap`], 'an owner\'s state')
    await b.store.close()
    await dropIfEmpty(name)
    same(await databases(name), [name, `${name}:wrap`], 'a stored state')
    const c = await opened(name)
    same(shown(c.entries), '01=kept', 'still there')
    await c.store.close()
  }],
  ['store: deleteStores leaves the wrapping key while another tab took the lock in between (it may be creating a device under it)', async () => {
    const name = fresh('raced')
    const a = await opened(name)
    await a.store.apply(write(0, [put(bytes(1), 'old')]))
    await a.store.close()
    // another context takes the state's lock as soon as the binding's destroy gives it back
    const IdbRacing = { destroy: async (n: string) => {
      await IdbStore.destroy(n)
      let taken = () => {}
      const got = new Promise<void>(r => { taken = r })
      void navigator.locks.request(`trommi-core:${n}`, () => { taken(); return sleep(300) })
      await got
    } } as unknown as typeof IdbStore
    await deleteStores({ name, IdbStore: IdbRacing })
    same(await databases(name), [`${name}:wrap`], 'the key stays while the lock is someone else\'s')
    await until(async () => !await lockHeld(name), 'the other context let go')
  }],
  ['cache: get, set, delete, setMany in one transaction, range by prefix with bounds, limit and reverse; no lock, two handles', async () => {
    const name = fresh('cache')
    const c = await openCache(name), second = await openCache(name)
    same(await c.get('nothing'), undefined, 'a missing key')
    await c.set('a/1', { n: 1, m: new Map([['k', new Uint8Array([1, 2])]]) })
    same(((await second.get('a/1')) as { m: Map<string, Uint8Array> }).m.get('k')!.length, 2, 'structured values, read through the other handle')
    await c.setMany([['a/2', 2], ['a/3', 3], ['a/4', 4], ['b/1', 'b'], ['a', 'bare'], ['a/1', undefined]])
    same(await c.range('a/'), [['a/2', 2], ['a/3', 3], ['a/4', 4]], 'prefix')
    same(await c.range('a/', { limit: 2 }), [['a/2', 2], ['a/3', 3]], 'limit')
    same(await c.range('a/', { reverse: true, limit: 2 }), [['a/4', 4], ['a/3', 3]], 'reverse')
    same(await c.range('a/', { after: 'a/2' }), [['a/3', 3], ['a/4', 4]], 'after')
    same(await c.range('a/', { before: 'a/4' }), [['a/2', 2], ['a/3', 3]], 'before')
    same(await c.range('a/', { after: 'a/2', before: 'a/4', reverse: true }), [['a/3', 3]], 'both, reverse')
    same((await c.range('')).map(([k]) => k), ['a', 'a/2', 'a/3', 'a/4', 'b/1'], 'everything')
    await c.delete('a/3')
    same((await c.range('a/')).map(([k]) => k), ['a/2', 'a/4'], 'after delete')
    // a value that cannot be stored: nothing of that setMany is written
    same((await refused(() => c.setMany([['a/9', 9], ['a/8', () => {}]]), 'a function as a value')).code, 'storage-failed', 'uncloneable')
    same([(await c.range('a/')).map(([k]) => k), await second.get('a/9')], [['a/2', 'a/4'], undefined], 'nothing of it')
    await c.set('a/5', 5, { durable: true })
    same(await second.get('a/5'), 5, 'a durable set')
    c.close(); second.close()
    same((await refused(() => c.get('a'), 'a closed cache')).code, 'storage-failed', 'closed')
  }],

  ['core: a real Device over the wrapped store: created, a room founded, closed, opened again from what is stored', async () => {
    const name = fresh('device')
    const device = await core.Device.create(deviceStore(name))
    const id = hex(await device.id())
    same([id.length, await device.room(), await lockHeld(name)], [64, null, true], 'a new device')
    const second = await refused(() => core.Device.open(deviceStore(name)), 'a second Device on an owned store') as { code?: string; cause?: unknown }
    same([second.code, second.cause instanceof core.StoreConflict], ['storage', true], 'the second owner: storage, caused by StoreConflict')
    const room = hex(await device.foundRoom(core.generateRecoveryCode(), Date.now()))
    const outbox = (await device.outbox()).map(e => `${e.id}:${e.kind}:${e.parts.map(p => p.length)}`)
    check(room.length === 64 && outbox.length > 0, `room ${room}, outbox ${outbox}`)
    await device.close()
    await freed(name)
    // nothing of it is stored in the clear, and the binding's own store cannot open it without the unwrapping
    const stored = (await raw(name, 'entries')).values as Uint8Array[]
    check(stored.length > 0 && stored.every(v => v[0] === 1 && v.length >= 29), 'the stored entries are not in the wrapped form')
    same((await refused(() => core.Device.open(new IdbStore(name)), 'opening the wrapped state unwrapped')).code, 'storage', 'unwrapped')
    const again = await core.Device.open(deviceStore(name))
    same([hex(await again.id()), hex((await again.room())!), (await again.outbox()).map(e => `${e.id}:${e.kind}:${e.parts.map(p => p.length)}`)], [id, room, outbox], 'the device after reopening')
    await again.close()
    same((await refused(() => core.Device.create(deviceStore(name)), 'creating over a stored device')).code, 'storage', 'create on a full store')
  }],
  ['core: another owner wrote behind a real Device: its next write fails, it closes itself and frees the lock; the stored state opens again without that write', async () => {
    const name = fresh('device-behind')
    const device = await core.Device.create(deviceStore(name))
    const id = hex(await device.id())
    const before = (await raw(name, 'meta')).values[0] as number
    await plain(name, ['meta'], 'readwrite', tx => { tx.objectStore('meta').put(before + 1, 'revision') })
    const behind = await refused(() => device.foundRoom(core.generateRecoveryCode(), Date.now()), 'the write behind another owner') as { code?: string; cause?: unknown }
    same([behind.code, behind.cause instanceof core.StoreConflict], ['storage', true], 'the failed call: storage, caused by StoreConflict')
    const later = await refused(() => device.id(), 'a call after the failure')
    check(later.code === 'storage' || later.code === 'internal', `a later call: ${later.code}`)
    same(await lockHeld(name), false, 'the device that closed itself gave the lock back')
    // recovery is the app's: a new store, the device from what is stored
    const again = await core.Device.open(deviceStore(name))
    same([hex(await again.id()), await again.room(), (await again.outbox()).length], [id, null, 0], 'the stored state: the device without the room')
    const room = await again.foundRoom(core.generateRecoveryCode(), Date.now())
    same(room.length, 32, 'and it goes on')
    await again.close()
  }],
]

async function single(): Promise<Result[]> {
  const out: Result[] = []
  for (const [name, run] of tests) {
    let timer = 0
    const deadline = new Promise<never>((_, reject) => { timer = setTimeout(() => reject(new Error('not finished after 30 s')), 30_000) as unknown as number })
    try { await Promise.race([run(), deadline]); out.push({ name, ok: true, detail: '' }) } catch (e) { out.push({ name, ok: false, detail: String((e as Error)?.message ?? e) }) } finally { clearTimeout(timer) }
  }
  return out
}

// ---------------------------------------------------------------------------------------------------------------------
// Steps run.mjs drives

let store: DeviceStore | null = null

const steps = {
  single,
  versions: () => core.versions(),
  /** When this document was loaded: another number after a reload. */
  born: () => performance.timeOrigin,

  // ---- a write that fails for a real reason: the origin's quota (run.mjs sets it before the first step)
  async quotaBefore(name: string) {
    const a = await opened(name)
    store = a.store
    await store.apply(write(0, [put(bytes(1), 'the last good state')]))
    return a.revision
  },
  async quotaWrite(megabytes: number) {
    const s = store!
    const big = new Uint8Array(megabytes * 1024 * 1024)
    for (let i = 0; i < big.length; i += 65536) crypto.getRandomValues(big.subarray(i, i + 65536))   // (does not compress)
    const e = await refused(() => s.apply(write(1, [put(bytes(2), big), put(bytes(1), 'overwritten')])), 'the write over quota')
    await s.close()   // (what a Device does when its write failed)
    return { name: e.name, message: e.message }
  },
  async quotaAfter(name: string) {
    const b = await opened(name)   // recovery: a new store, loaded
    const state = { revision: b.revision, entries: shown(b.entries) }
    await b.store.apply(write(b.revision, [put(bytes(4), 'on again')]))
    await b.store.close()
    const c = await opened(name)
    await c.store.close()
    return { ...state, then: [c.revision, shown(c.entries)] }
  },

  // ---- a reload in the middle of a write
  async midWriteSetup(name: string) {
    const a = await opened(name)
    await a.store.apply(write(0, [put(bytes(0, 0, 0, 0), 'base')]))
    await a.store.close()
    return null
  },
  /**
   * Starts a write of many entries and leaves the page before it is awaited to the end: `at` >= 0 leaves `at` ms
   * after the write was handed over; `at` < 0 leaves from INSIDE the write's transaction, when its (-at)-th entry was
   * put (seen by wrapping IDBObjectStore.put), so the transaction is open and half filled when the page goes.
   */
  async midWrite(name: string, at: number, entries: number, size: number) {
    const a = await opened(name)
    const value = crypto.getRandomValues(new Uint8Array(size))   // (size: at most 65536, the most getRandomValues gives)
    const puts: StoreEntry[] = [put(bytes(0, 0, 0, 0), 'replaced')]
    for (let i = 1; i <= entries; i++) puts.push(put(keyOf(i), value))
    if (at < 0) {
      const original = IDBObjectStore.prototype.put
      let n = 0
      IDBObjectStore.prototype.put = function (this: IDBObjectStore, ...args: Parameters<IDBObjectStore['put']>) {
        const request = original.apply(this, args)
        if (this.name === 'entries' && ++n === -at) location.reload()
        return request
      }
    }
    void a.store.apply(write(a.revision, puts)).catch(() => {})
    if (at >= 0) setTimeout(() => location.reload(), at)
    return null
  },
  async midWriteCheck(name: string, entries: number, size: number) {
    const b = await opened(name)
    await b.store.close()
    const base = b.entries.find(e => e.key.length === 4 && new DataView(e.key.buffer, e.key.byteOffset).getUint32(0) === 0)
    return { revision: b.revision, count: b.entries.length - 1, base: base ? text.decode(base.value) : null, sizesOk: b.entries.every(e => e === base || e.value.length === size), want: entries }
  },
  async drop(name: string) { await deleteStores({ name, IdbStore }); return null },
}
;(globalThis as unknown as { T: typeof steps }).T = steps
