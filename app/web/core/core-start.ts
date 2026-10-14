// core-start.ts: start the core worker as early as the page can. index.html loads this tiny module async, before its
// style sheets, so the worker's file and the core's .wasm are fetched beside the sheets and the entry. The page
// (public/core.mjs) takes the worker over and sends it `start`; what the worker says meanwhile is kept for it (a
// Worker drops messages nobody listens to). Not for a share page (/artifact/<id>): it opens no room.
// The worker's address: __TROMMI_CORE_WORKER__ (the deployed bundle, dev/build.mjs), else the dev server's.
declare const __TROMMI_CORE_WORKER__: string | undefined

/** The worker started here and what it said so far. */
export interface EarlyCore { worker: Worker; messages: MessageEvent[]; error: ErrorEvent | null }

const g = globalThis as typeof globalThis & { __trommiCore?: EarlyCore | null }

if (g.__trommiCore === undefined) {
  g.__trommiCore = null
  if (typeof Worker === 'function' && typeof location !== 'undefined' && !/^\/(?:artifact|a)\/(?:[0-9a-f]{32}|[A-Za-z0-9_-]{22})$/.test(location.pathname)) {
    const url = typeof __TROMMI_CORE_WORKER__ === 'string' ? __TROMMI_CORE_WORKER__ : '/gen/vendor/core-worker.mjs'
    const worker = new Worker(url, { type: 'module', name: 'trommi-core' })
    const early: EarlyCore = { worker, messages: [], error: null }
    worker.onmessage = e => { early.messages.push(e) }
    worker.onerror = e => { early.error = e }
    g.__trommiCore = early
  }
}
export {}
