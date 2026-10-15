// The types of the IndexedDB store (idb-store.js).
import type { Store, StoredState, StoreWrite } from './trommi-core.js'

/**
 * A device's store on IndexedDB: one strict transaction per write, the revision compared inside it, and a Web Lock
 * on `name` held from `load` until `close`, so that one tab or worker owns a stored state at a time.
 */
export class IdbStore implements Store {
  /** `name` names the stored state: one per device. With `wait`, `load` waits for the lock instead of failing. */
  constructor(name: string, options?: { wait?: boolean; steal?: boolean; onLost?: () => void })
  load(): Promise<StoredState>
  apply(write: StoreWrite): Promise<void>
  /** Several writes, in order, in one transaction: all of them or none. */
  applyAll(writes: StoreWrite[]): Promise<void>
  /** Resolves once the lock is released: the state can be opened again without waiting. A `load` still waiting
   *  for the lock is given up. */
  close(): Promise<void>
  /** Deletes a stored state for good. StoreConflict while a device has it open. */
  static destroy(name: string): Promise<void>
}

/** Probe an existing holder through its page, and acquire its store after a checked takeover. */
export function deviceOwner<T extends { load(): Promise<unknown>; close(): Promise<void> }>(
  name: string, openStore: (steal: boolean) => T, probePage: (nonce: string) => void, lost: () => void,
): { open(take: boolean): Promise<T | null>; respond(nonce: string): void; close(): void }
