// shell.ts: the web's executor of the app core (core/client/README.md, "The app core"). One `AppCore`, its events
// handed in one at a time in the order they happened, and every effect it asks for run here: HTTP by fetch (no auth
// of its own, no retries, no redirects, at most `max_answer` bytes of an answer), the live stream by a streamed fetch,
// the device's state and the cache on the stores given (IndexedDB in the browser), one timer. Renders, files and
// links go to the host (the page). Nothing here decides anything about the room: that is the core's.
//
// The barrier: a `Store` with `DeviceApply` holds back every effect after it, of the same answer and of later ones,
// until it is durable; events are handled meanwhile and their effects queue up behind it. When it failed, what queued
// up is dropped and the core hears only the failure. Store operations run one after the other in the order asked,
// so a `Get` after a `Put` reads what was put.

/** The binding's `AppCore` as the shell uses it: JSON text plus the bytes it names (`core/client/src/app/edge.rs`). */
export interface Packet { json: string; blobs: Uint8Array[] }
export interface CoreLike { handle(nowMs: number, event: Packet): Packet }

/** The device's state of one slot: all or nothing, one revision, one owner (idb-store.js). */
export interface DeviceSpace {
  load(): Promise<{ revision: number; entries: { key: Uint8Array; value: Uint8Array }[] }>
  /** Rejects with `{ kind: 'conflict' | 'full' | 'denied' | 'failed' }` when nothing was written. */
  apply(expectedRevision: number, put: { key: Uint8Array; value: Uint8Array }[], remove: Uint8Array[]): Promise<void>
}
/** The cache space: string keys, last write wins. */
export interface CacheSpace {
  get(keys: string[]): Promise<(Uint8Array | null)[]>
  scan(prefix: string): Promise<{ key: string; value: Uint8Array }[]>
  put(put: { key: string; value: Uint8Array }[], remove: string[]): Promise<void>
}
/** What only the page can do. */
export interface ShellHost {
  render(delta: unknown): void
  fileReady?(fileId: string, mediaType: string, bytes: Uint8Array): void
  fileGone?(fileId: string): void
  saveFile?(name: string, mediaType: string, bytes: Uint8Array): void
  openUrl?(url: string): void
  notify?(notice: unknown): void
  log?(level: string, text: string): void
}

type Json = Record<string, unknown>
type IoError = { kind: 'offline' | 'timeout' | 'too_large' | 'cancelled' | 'conflict' | 'full' | 'denied' | 'failed'; detail?: string }
const failure = (kind: IoError['kind'], detail = ''): { err: IoError } => ({ err: { kind, detail } })

export interface ShellOptions {
  core: CoreLike
  device: DeviceSpace
  cache: CacheSpace
  host: ShellHost
  fetch?: typeof fetch
  now?: () => number
}

export class CoreShell {
  private readonly core: CoreLike
  private readonly device: DeviceSpace
  private readonly cache: CacheSpace
  private readonly host: ShellHost
  private readonly fetcher: typeof fetch
  private readonly now: () => number
  /** Events waiting for the core, in order. */
  private readonly queue: { json: Json; blobs: Uint8Array[] }[] = []
  private busy = false
  /** The `DeviceApply` everything waits for, and what waits behind it. */
  private barrier: number | null = null
  private held: { effect: Json; blobs: Uint8Array[] }[] = []
  /** Requests and streams in flight, by id. */
  private readonly flying = new Map<number, AbortController>()
  private stores: Promise<unknown> = Promise.resolve()
  private timer: ReturnType<typeof setTimeout> | null = null
  private closed = false

  constructor(o: ShellOptions) {
    this.core = o.core
    this.device = o.device
    this.cache = o.cache
    this.host = o.host
    this.fetcher = o.fetch ?? fetch.bind(globalThis)
    this.now = o.now ?? Date.now
  }

  /** An event of the page (`start`, `action`, `focus`, `foreground`, `net`, `zone`, …), as the contract spells it.
   *  `blobs`: the bytes its JSON names by index (a picked file). */
  send(event: Json, blobs: Uint8Array[] = []): void {
    if (this.closed) return
    this.queue.push({ json: event, blobs })
    this.pump()
  }

  /** Ends the shell: requests in flight are aborted, the timer cleared; late results are dropped. */
  close(): void {
    this.closed = true
    for (const c of this.flying.values()) c.abort()
    this.flying.clear()
    if (this.timer) clearTimeout(this.timer)
  }

  private pump(): void {
    if (this.busy) return
    this.busy = true
    try {
      while (this.queue.length && !this.closed) {
        const { json, blobs } = this.queue.shift()!
        const out = this.core.handle(this.now(), { json: JSON.stringify(json), blobs })
        const effects = JSON.parse(out.json) as Json[]
        for (const effect of effects) this.start(effect, out.blobs)
      }
    } finally { this.busy = false }
  }

  private result(json: Json): void {
    this.queue.push({ json, blobs: [] })
    this.pump()
  }
  private resultWith(json: Json, blobs: Uint8Array[]): void {
    this.queue.push({ json, blobs })
    this.pump()
  }

  /** Starts one effect, or holds it behind the barrier. */
  private start(effect: Json, blobs: Uint8Array[]): void {
    if (this.closed) return
    if (this.barrier !== null) { this.held.push({ effect, blobs }); return }
    const id = effect['id'] as number
    switch (effect['type']) {
      case 'render': return this.host.render(effect['delta'])
      case 'http': return void this.http(id, effect['request'] as Json, blobs)
      case 'open_stream': return void this.stream(id, effect['request'] as Json, blobs)
      case 'cancel': {
        const c = this.flying.get(id)
        this.flying.delete(id)
        c?.abort()
        return
      }
      case 'store': return this.store(id, effect['op'] as Json, blobs)
      case 'secret': return this.result({ type: 'secret', id, result: failure('denied') })
      case 'platform': return this.result({ type: 'platform', id, result: { ok: { answer: 'refused' } } })
      case 'timer': {
        if (this.timer) clearTimeout(this.timer)
        this.timer = null
        const at = effect['at_ms'] as number | null
        if (at != null) this.timer = setTimeout(() => { this.timer = null; this.result({ type: 'wake' }) }, Math.max(0, at - this.now()))
        return
      }
      case 'file_ready': return this.host.fileReady?.(effect['file_id'] as string, effect['media_type'] as string, blobs[effect['bytes'] as number] ?? new Uint8Array())
      case 'file_gone': return this.host.fileGone?.(effect['file_id'] as string)
      case 'save_file': return this.host.saveFile?.(effect['name'] as string, effect['media_type'] as string, blobs[effect['bytes'] as number] ?? new Uint8Array())
      case 'open_url': return this.host.openUrl?.(effect['url'] as string)
      case 'notify': return this.host.notify?.(effect['notice'])
      case 'snapshot': return
      case 'log': return this.host.log?.(effect['level'] as string, effect['text'] as string)
      default: return this.host.log?.('warn', `an effect this shell does not know: ${String(effect['type'])}`)
    }
  }

  /** The request as fetch takes it: no redirects, the shell's own abort and timeout. */
  private requestOf(id: number, r: Json, blobs: Uint8Array[]): { url: string; init: RequestInit; controller: AbortController; max: number; timedOut: () => boolean } {
    const controller = new AbortController()
    this.flying.set(id, controller)
    const headers: [string, string][] = ((r['headers'] as { name: string; value: string }[] | undefined) ?? []).map(h => [h.name, h.value])
    const body = typeof r['body'] === 'number' ? blobs[r['body'] as number] : undefined
    let timedOut = false
    const ms = r['timeout_ms'] as number | undefined
    if (ms) setTimeout(() => { timedOut = true; controller.abort() }, ms)
    const init: RequestInit = { method: r['method'] as string, headers, signal: controller.signal, redirect: 'manual', ...(body ? { body: body as BodyInit } : {}) }
    return { url: r['url'] as string, init, controller, max: (r['max_answer'] as number | undefined) ?? 64 << 10, timedOut: () => timedOut }
  }

  private errorOf(e: unknown, timedOut: boolean, id: number): IoError {
    if (timedOut) return { kind: 'timeout', detail: '' }
    if (!this.flying.has(id)) return { kind: 'cancelled', detail: '' }
    if ((e as { kind?: string })?.kind === 'too_large') return { kind: 'too_large', detail: '' }
    return { kind: 'offline', detail: '' }
  }

  private async http(id: number, r: Json, blobs: Uint8Array[]): Promise<void> {
    const { url, init, max, timedOut } = this.requestOf(id, r, blobs)
    try {
      const response = await this.fetcher(url, init)
      if (response.type === 'opaqueredirect' || (response.status >= 300 && response.status < 400)) throw new Error('a redirect')
      const length = Number(response.headers.get('content-length') ?? NaN)
      if (length > max) throw Object.assign(new Error('too large'), { kind: 'too_large' })
      const body = await readCapped(response, max)
      if (!this.flying.delete(id)) return
      const headers = [...response.headers].map(([name, value]) => ({ name, value }))
      this.resultWith({ type: 'http', id, result: { ok: { status: response.status, headers, body: 0 } } }, [body])
    } catch (e) {
      const error = this.errorOf(e, timedOut(), id)
      if (!this.flying.delete(id) && error.kind === 'cancelled') return
      this.result({ type: 'http', id, result: { err: error } })
    }
  }

  private async stream(id: number, r: Json, blobs: Uint8Array[]): Promise<void> {
    const { url, init, max } = this.requestOf(id, r, blobs)
    const item = (it: Json, bytes?: Uint8Array): void => {
      if (!this.flying.has(id)) return
      if (bytes) this.resultWith({ type: 'stream', id, item: { ...it, bytes: 0 } }, [bytes])
      else this.result({ type: 'stream', id, item: it })
    }
    try {
      const response = await this.fetcher(url, init)
      item({ item: 'opened', status: response.status, headers: [...response.headers].map(([name, value]) => ({ name, value })) })
      if (!response.body) { item({ item: 'ended' }); this.flying.delete(id); return }
      const reader = response.body.getReader()
      let total = 0
      for (;;) {
        const { done, value } = await reader.read()
        if (done) break
        total += value.length
        // (the events of an open stream are bounded by the core; an answer that is no 200 by its cap)
        if (response.status !== 200 && total > max) { reader.cancel().catch(() => {}); throw Object.assign(new Error('too large'), { kind: 'too_large' }) }
        item({ item: 'chunk' }, value)
      }
      item({ item: 'ended' })
      this.flying.delete(id)
    } catch (e) {
      if (!this.flying.has(id)) return
      const kind = (e as { kind?: string })?.kind === 'too_large' ? 'too_large' : 'offline'
      item({ item: 'failed', error: { kind, detail: '' } })
      this.flying.delete(id)
    }
  }

  private store(id: number, op: Json, blobs: Uint8Array[]): void {
    const kind = op['op'] as string
    if (kind === 'device_apply') this.barrier = id
    const run = async (): Promise<{ result: Json; blobs: Uint8Array[] }> => {
      switch (kind) {
        case 'device_load': {
          const { revision, entries } = await this.device.load()
          const out: Uint8Array[] = []
          const list = entries.map(e => ({ key: out.push(e.key) - 1, value: out.push(e.value) - 1 }))
          return { result: { ok: { answer: 'device', revision, entries: list } }, blobs: out }
        }
        case 'device_apply': {
          const put = (op['put'] as { key: number; value: number }[]).map(e => ({ key: blobs[e.key]!, value: blobs[e.value]! }))
          const remove = (op['delete'] as number[]).map(i => blobs[i]!)
          await this.device.apply(op['expected_revision'] as number, put, remove)
          return { result: { ok: { answer: 'done' } }, blobs: [] }
        }
        case 'get': {
          const values = await this.cache.get(op['keys'] as string[])
          const out: Uint8Array[] = []
          return { result: { ok: { answer: 'values', values: values.map(v => (v ? out.push(v) - 1 : null)) } }, blobs: out }
        }
        case 'scan': {
          const entries = await this.cache.scan(op['prefix'] as string)
          const out: Uint8Array[] = []
          return { result: { ok: { answer: 'entries', entries: entries.map(e => ({ key: e.key, value: out.push(e.value) - 1 })) } }, blobs: out }
        }
        case 'put': {
          const put = ((op['put'] as { key: string; value: number }[] | undefined) ?? []).map(e => ({ key: e.key, value: blobs[e.value]! }))
          await this.cache.put(put, (op['delete'] as string[] | undefined) ?? [])
          return { result: { ok: { answer: 'done' } }, blobs: [] }
        }
        default: throw Object.assign(new Error(`a store op this shell does not know: ${kind}`), { kind: 'failed' })
      }
    }
    this.stores = this.stores.then(run).then(
      ({ result, blobs: out }) => this.stored(id, kind, result, out, true),
      (e: unknown) => {
        const k = (e as { kind?: IoError['kind'] })?.kind
        const known: IoError['kind'][] = ['conflict', 'full', 'denied', 'failed']
        this.stored(id, kind, failure(k && known.includes(k) ? k : 'failed'), [], false)
      },
    )
  }

  private stored(id: number, kind: string, result: Json, blobs: Uint8Array[], ok: boolean): void {
    if (this.closed) return
    if (kind === 'device_apply' && this.barrier === id) {
      this.barrier = null
      const held = this.held
      this.held = []
      this.resultWith({ type: 'store', id, result }, blobs)
      // (a failed write: what waited behind it is dropped, the core hears only the failure)
      if (ok) for (const h of held) this.start(h.effect, h.blobs)
      return
    }
    this.resultWith({ type: 'store', id, result }, blobs)
  }
}

/** The body, at most `max` bytes; one byte more fails as `too_large` and the rest is not read. */
async function readCapped(response: Response, max: number): Promise<Uint8Array> {
  if (!response.body) return new Uint8Array()
  const reader = response.body.getReader()
  const parts: Uint8Array[] = []
  let total = 0
  for (;;) {
    const { done, value } = await reader.read()
    if (done) break
    total += value.length
    if (total > max) { reader.cancel().catch(() => {}); throw Object.assign(new Error('too large'), { kind: 'too_large' }) }
    parts.push(value)
  }
  const out = new Uint8Array(total)
  let at = 0
  for (const p of parts) { out.set(p, at); at += p.length }
  return out
}

/** A device space and a cache space in memory: for tests, and a page that keeps nothing. */
export function memorySpaces(): { device: DeviceSpace; cache: CacheSpace } {
  let revision = 0
  const entries = new Map<string, { key: Uint8Array; value: Uint8Array }>()
  const hexOf = (b: Uint8Array): string => Array.from(b, x => x.toString(16).padStart(2, '0')).join('')
  const kv = new Map<string, Uint8Array>()
  return {
    device: {
      load: async () => ({ revision, entries: [...entries.values()] }),
      apply: async (expected, put, remove) => {
        if (expected !== revision) throw Object.assign(new Error('another owner wrote'), { kind: 'conflict' })
        for (const k of remove) entries.delete(hexOf(k))
        for (const e of put) entries.set(hexOf(e.key), e)
        revision += 1
      },
    },
    cache: {
      get: async keys => keys.map(k => kv.get(k) ?? null),
      scan: async prefix => [...kv].filter(([k]) => k.startsWith(prefix)).sort(([a], [b]) => (a < b ? -1 : 1)).map(([key, value]) => ({ key, value })),
      put: async (put, remove) => { for (const k of remove) kv.delete(k); for (const e of put) kv.set(e.key, e.value) },
    },
  }
}
