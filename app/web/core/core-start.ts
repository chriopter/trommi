// core-start.ts: start the core worker as early as the page can. index.html loads this tiny module async, before its
// style sheets, so the worker's file and the core's .wasm are fetched beside the sheets and the entry. The page
// (public/core.mjs) takes the worker over and sends it `start`; what the worker says meanwhile is kept for it (a
// Worker drops messages nobody listens to). Not for a share page (/artifact/<id>): it opens no room.
// Where the browser has SharedWorker it is that one worker every tab of this profile shares (one core for all
// windows, core/core-worker.ts); the demo, and browsers without it, start a worker of their own.
// The worker's address: __TROMMI_CORE_WORKER__ (the deployed bundle, dev/build.mjs), else the dev server's.
declare const __TROMMI_CORE_WORKER__: string | undefined

/** The worker started here and what it said so far. `port`: what the page talks to (the worker itself, or its
 *  port when it is the SharedWorker every tab of this profile shares). */
export interface EarlyCore { worker: Worker | SharedWorker; port: Worker | MessagePort; shared: boolean; messages: MessageEvent[]; error: ErrorEvent | Event | null }

const g = globalThis as typeof globalThis & { __trommiCore?: EarlyCore | null }

/** The demo room (?mock=1, kept for the tab; ?mock=0 ends it): app.mjs reads the same. It runs a core of its own. */
function demoTab(): boolean {
  const mock = new URLSearchParams(location.search).get('mock')
  if (mock !== null) return mock !== '0'
  try { return Boolean(sessionStorage.getItem('trommi-mock')) } catch { return false }
}

if (g.__trommiCore === undefined) {
  g.__trommiCore = null
  if (typeof Worker === 'function' && typeof location !== 'undefined' && !/^\/(?:artifact|a)\/(?:[0-9a-f]{32}|[A-Za-z0-9_-]{22})$/.test(location.pathname)) {
    const url = typeof __TROMMI_CORE_WORKER__ === 'string' ? __TROMMI_CORE_WORKER__ : '/gen/vendor/core-worker.mjs'
    // (one core for every tab where the browser shares a worker between them; else one per tab)
    const shared = typeof SharedWorker === 'function' && !demoTab()
    const worker = shared ? new SharedWorker(url, { type: 'module', name: 'trommi-core' }) : new Worker(url, { type: 'module', name: 'trommi-core' })
    const port = shared ? (worker as SharedWorker).port : (worker as Worker)
    const early: EarlyCore = { worker, port, shared, messages: [], error: null }
    port.onmessage = e => { early.messages.push(e) }
    worker.onerror = e => { early.error = e }
    g.__trommiCore = early
  }
}
export {}
