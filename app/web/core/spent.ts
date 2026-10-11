// spent.ts: where a device's time goes, summed in milliseconds by part, for measuring (tests/web/e2e/catchup.browser.mjs
// reads it from the core worker as `globalThis.__trommiSpent`). Parts nest: `core` (every call into the device)
// holds `store` (the store's writes of those calls), which holds `wrap` (the values encrypted at rest) and `idb` (the
// strict IndexedDB transaction). `model` is the client's own work on what the engine tells it (outside `core`),
// `cache` the writes of the app's cache, `hub` the waits for the hub's pages of a catch-up. Across them: `log` the
// log entries handed to the core one by one (`commits` counts the log entries of the catch-up's pages), `welcome` the joins by Welcome
// (`welcomes`), `keys` the wishes for keys and their answers, `learn` the walk of a joined group's past, `desk` the
// newest state read first at a start (client.ts `deskFirst`). Nothing here is sent anywhere; the sums cost two clock
// reads per counted step.
export interface Spent {
  core: number; store: number; wrap: number; idb: number; model: number; cache: number; hub: number
  log: number; welcome: number; keys: number; learn: number; desk: number
  rescan: number
  items: number; calls: number; writes: number; commits: number; welcomes: number; again: number
  /** When each step of a start began and ended (ms since the worker began), in order: what a measurement prints. */
  marks: [string, number][]
}
export const spent: Spent = { core: 0, store: 0, wrap: 0, idb: 0, model: 0, cache: 0, hub: 0, log: 0, welcome: 0, keys: 0, learn: 0, desk: 0, rescan: 0, items: 0, calls: 0, writes: 0, commits: 0, welcomes: 0, again: 0, marks: [] }
;(globalThis as { __trommiSpent?: Spent }).__trommiSpent = spent
type Part = 'core' | 'store' | 'wrap' | 'idb' | 'model' | 'cache' | 'hub' | 'log' | 'welcome' | 'keys' | 'learn' | 'desk' | 'rescan'
const clock = (): number => (typeof performance !== 'undefined' ? performance.now() : Date.now())
/** Adds the time `job` takes (until its promise settles) to `part`. */
export async function timed<T>(part: Part, job: () => Promise<T>): Promise<T> {
  const t = clock()
  try { return await job() } finally { spent[part] += clock() - t }
}
/** The same for a step that does not wait. */
export function timedSync<T>(part: Part, job: () => T): T {
  const t = clock()
  try { return job() } finally { spent[part] += clock() - t }
}
/** For a measurement only: `itemByItem` makes the engine hand a catch-up to the core one item per call, as before
 *  `feed` (`globalThis.__trommiMeasure`). */
export const measuring = { itemByItem: false }
;(globalThis as { __trommiMeasure?: typeof measuring }).__trommiMeasure = measuring
/** Sets every sum back to zero. */
export function resetSpent(): void { for (const k of Object.keys(spent) as (keyof Spent)[]) if (k !== 'marks') spent[k] = 0; spent.marks = [] }
/** Notes the moment a step of a start began or ended (at most 200 are kept). */
export function mark(what: string): void { if (spent.marks.length < 200) spent.marks.push([what, Math.round(clock())]) }
