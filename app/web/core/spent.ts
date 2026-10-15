// Local timings for encrypted writes and cache transactions, exposed only to developer tools.
interface Spent { store: number; wrap: number; idb: number; cache: number; writes: number }
export const spent: Spent = { store: 0, wrap: 0, idb: 0, cache: 0, writes: 0 }
;(globalThis as { __trommiSpent?: Spent }).__trommiSpent = spent
/** Add a platform I/O step's time even when its promise rejects. */
export async function timed<T>(part: Exclude<keyof Spent, 'writes'>, job: () => Promise<T>): Promise<T> {
  const start = performance.now()
  try { return await job() } finally { spent[part] += performance.now() - start }
}
