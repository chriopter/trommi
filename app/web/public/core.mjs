// core.mjs: the page's side of the app core. The core runs in its worker (core/core-worker.ts); here its renders are
// folded into the view model the screens draw (`vm()`), and what the person does goes in as events: `act(action)`,
// `focus(focus)`, the window in front or not, the network. Files the core hands over become object URLs
// (`fileUrl(id)`), a platform op (passkeys, push) is answered by platform.mjs. Nothing of the room is decided here.
import { platform } from './platform.mjs'

/* global __TROMMI_CORE_WORKER__ */
const CORE_WORKER = typeof __TROMMI_CORE_WORKER__ === 'string' ? __TROMMI_CORE_WORKER__ : '/gen/vendor/core-worker.mjs'
/** The slot of this browser's device (store-idb.ts names its databases after it). */
export const SLOT = 'trommi'

let worker = null
// (the fold is the binding's views-fold.js, loaded with the core's start: Node's tests import this module without gen/)
let fold = null
let view = { rev: 0 }
const listeners = new Map()
const urls = new Map()        // file id -> object URL
const asked = new Set()       // file ids asked for (OpenFile) and not here yet
// Object URLs let go of (a newer copy came, or the page no longer showed it) -> their file id: markup drawn before
// may still name one, and a picture that then fails asks for its file again (fetchAgain). The newest few hundred.
const gone = new Map()
const letGo = (id, url) => { URL.revokeObjectURL(url); gone.set(url, id); if (gone.size > 500) gone.delete(gone.keys().next().value) }
let lastFocus = ''

const emit = (name, data) => { for (const fn of listeners.get(name) ?? []) { try { fn(data) } catch (err) { console.error(name, err) } } }
/** `render` (the new view model), `file` (an id whose bytes came), `busy` (another tab has the device), `taken`
 *  (another tab took it), `failed` (the core did not start), `notify`. Returns the way to stop listening. */
export function on(name, fn) {
  if (!listeners.has(name)) listeners.set(name, new Set())
  listeners.get(name).add(fn)
  return () => listeners.get(name)?.delete(fn)
}
/** The view model as folded so far (core/bindings/wasm/js/views.d.ts ViewModel). */
export const vm = () => view

function receive(m) {
  switch (m.t) {
    case 'owner_probe': worker?.postMessage({ t: 'owner_alive', nonce: m.nonce }); return
    case 'render': {
      try { view = fold.foldView(view, m.delta) } catch (err) { console.error('render', err); return }
      emit('render', view)
      return
    }
    case 'file': {
      const old = urls.get(m.id)
      if (old) letGo(m.id, old)
      urls.set(m.id, URL.createObjectURL(new Blob([m.bytes], { type: m.media_type || 'application/octet-stream' })))
      asked.delete(m.id)
      emit('file', m.id)
      return
    }
    case 'file_gone': { const old = urls.get(m.id); if (old) URL.revokeObjectURL(old); urls.delete(m.id); emit('file', m.id); return }
    case 'save': {
      const a = Object.assign(document.createElement('a'), { href: URL.createObjectURL(new Blob([m.bytes], { type: m.media_type })), download: m.name })
      document.body.append(a); a.click(); a.remove()
      setTimeout(() => URL.revokeObjectURL(a.href), 60_000)
      return
    }
    case 'open_url': window.open(m.url, '_blank', 'noopener'); return
    case 'notify': emit('notify', m.notice); return
    case 'platform': {
      Promise.resolve(platform(m.op, m.blobs)).then(
        ({ answer, blobs }) => worker?.postMessage({ t: 'platform', id: m.id, answer, blobs }),
        err => worker?.postMessage({ t: 'platform', id: m.id, error: String(err?.message ?? err) }))
      return
    }
    case 'log': (m.level === 'error' ? console.error : m.level === 'warn' ? console.warn : console.debug)('core:', m.text); return
    case 'busy': case 'taken':
      for (const url of urls.values()) URL.revokeObjectURL(url)
      urls.clear(); asked.clear()
      worker?.terminate(); worker = null
      view = fold.emptyView(); lastFocus = ''
      emit(m.t, null); return
    case 'failed': emit('failed', new Error(m.error)); return
  }
}

/** Release decrypted files absent from the painted page. Canvas pictures name their files separately.
 *  The encrypted browser cache is unaffected; revisiting a file asks the core for its bytes again. */
export function releaseUnusedFiles(root, canvasFiles = []) {
  const used = new Set()
  for (const el of root.querySelectorAll('[src], [href], [data-assetthumb-src-value]')) {
    for (const attr of ['src', 'href', 'data-assetthumb-src-value']) {
      const value = el.getAttribute(attr)
      if (value?.startsWith('blob:')) used.add(value.split('#')[0])
    }
  }
  const canvas = new Set(canvasFiles)
  for (const [id, url] of urls) if (!used.has(url) && !canvas.has(id)) {
    letGo(id, url)
    urls.delete(id)
  }
}

/**
 * Starts the core for this page: the worker core-start.mjs began (or a new one), `start` with the config. `take`:
 * the device is asked from the tab that has it (it gives it up). Resolves once the worker took the start.
 */
export async function startCore(config, { take = false, slot = SLOT } = {}) {
  fold ??= await import('./gen/vendor/views-fold.mjs')
  if (!worker) {
    const early = globalThis.__trommiCore ?? null
    globalThis.__trommiCore = null
    worker = early?.worker ?? new Worker(CORE_WORKER, { type: 'module', name: 'trommi-core' })
    worker.onmessage = e => receive(e.data)
    worker.onerror = e => emit('failed', new Error(e.message || 'the core worker failed'))
    for (const e of early?.messages ?? []) receive(e.data)
    if (early?.error) emit('failed', new Error(early.error.message || 'the core worker failed'))
  }
  view = fold.emptyView()
  worker.postMessage({ t: 'start', slot, config, take })
}
/** Stops the core (this tab gives the device up). */
export function stopCore() { worker?.postMessage({ t: 'close' }) }

/** One event of the contract (core/client/src/app/event.rs), with the bytes its JSON names by index. */
export function send(event, blobs = []) { worker?.postMessage({ t: 'event', event, blobs }) }
/** What the person did (core/client/src/actions/mod.rs Action). */
export const act = (action, blobs = []) => send({ type: 'action', action }, blobs)
/** An account step (core/client/src/account/mod.rs AccountAction). */
export const account = action => act({ do: 'account', action })
/** What the person looks at; sent only when it changed. */
export function focus(f) {
  const text = JSON.stringify(f)
  if (text === lastFocus) return
  lastFocus = text
  send({ type: 'focus', focus: f })
}

/** A file's object URL once its bytes are here; else null, and the core is asked for it once. */
export function fileUrl(id) {
  if (!id) return null
  const url = urls.get(id)
  if (url) return url
  if (!asked.has(id) && view.files?.[id]?.state !== 'uploading') { asked.add(id); act({ do: 'open_file', attachment_id: id, save: false }) }
  return null
}
/** A picture whose object URL failed (let go while markup drawn before still named it): its file is asked for again,
 *  and the page is drawn again once it is here. false: no file of this page. */
export function fetchAgain(url) {
  const id = gone.get(url) ?? [...urls].find(([, u]) => u === url)?.[0]
  if (!id) return false
  if (asked.has(id)) return true
  const now = urls.get(id)
  if (now && now !== url) { emit('file', id); return true }   // (a newer copy is here: drawn again with it)
  if (now) { URL.revokeObjectURL(now); urls.delete(id) }
  fileUrl(id)
  return asked.has(id)
}
/** Saves a file (the core fetches it and hands it over as a download). */
export const saveFile = id => act({ do: 'open_file', attachment_id: id, save: true })

/** A file the person picked, to attach: its local id (`vm().files[id]` shows it uploading, then ready). */
export async function attach(file) {
  const id = `local-${crypto.randomUUID()}`
  const bytes = new Uint8Array(await file.arrayBuffer())
  act({ do: 'attach', local_id: id, name: file.name || 'file', media_type: file.type || 'application/octet-stream', bytes: 0 }, [bytes])
  return id
}

// The window in front or not, the network, the time zone: the core hears each change.
if (typeof document !== 'undefined') {
  document.addEventListener('turbo:render', () => releaseUnusedFiles(document, (view.board?.shapes ?? []).map(s => s.attachment).filter(Boolean)))
  document.addEventListener('visibilitychange', () => send({ type: document.visibilityState === 'visible' ? 'foreground' : 'background' }))
  addEventListener('online', () => send({ type: 'net', online: true }))
  addEventListener('offline', () => send({ type: 'net', online: false }))
  addEventListener('pagehide', () => { stopCore(); worker?.terminate(); worker = null; globalThis.__trommiCore = null })
  addEventListener('pageshow', e => { if (e.persisted) location.reload() })
  let zone = -new Date().getTimezoneOffset()
  setInterval(() => { const z = -new Date().getTimezoneOffset(); if (z !== zone) { zone = z; send({ type: 'zone', utc_offset_min: z }) } }, 60_000)
}
