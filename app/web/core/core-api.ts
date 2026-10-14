// core-api.ts: the Rust core's WASM binding as the web app's workers see it (core/bindings/wasm/js/trommi-core.d.ts).
// Only core-wasm.ts imports the binding's module; the rest names its types from here.
import type * as Binding from '../../../core/bindings/wasm/js/trommi-core.js'

export type { ErrorCode, Store, StoredState, StoreEntry, StoreWrite, SelfTestReport, Versions } from '../../../core/bindings/wasm/js/trommi-core.js'

/** Everything of the binding's module that has no state (versions, the self test, the app core's class, …). */
export type Stateless = Omit<typeof Binding, 'init' | 'Device' | 'TrommiError' | 'StoreConflict'>

/** The loaded core. */
export interface Core extends Stateless {
  /** Whether a thrown value is a refusal of the core, and which. */
  errorCode(error: unknown): Binding.ErrorCode | null
}
