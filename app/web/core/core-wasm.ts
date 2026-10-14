// core-wasm.ts: the Rust core (`trommi-core`) as the web app's workers hold it, made from its WASM binding
// (core/bindings/wasm). THE ONLY MODULE that imports the binding's scripts and fetches its .wasm.
//
// Where the files come from (app/web/dev/build.mjs "the Rust core"): the binding's scripts are bundled into the worker
// with this module. Only the .wasm is a file of its own, named by its content in the deployed bundle; the build tells
// this module its address and the SHA-256 of its bytes through the two constants below (the dev server too).
//
// Integrity: the .wasm is fetched with that SHA-256 (`fetch(url, { integrity })`), so the browser hands over nothing
// but the bytes the build saw, and the binding instantiates exactly that response.
import * as binding from '../../../core/bindings/wasm/js/trommi-core.js'
import type { Core, ErrorCode } from './core-api.ts'

/** The binding's store on IndexedDB (store-idb.ts is handed this class and wraps it). */
export { IdbStore } from '../../../core/bindings/wasm/js/idb-store.js'

/** The address of the .wasm and the base64 SHA-256 of its bytes (app/web/dev/build.mjs coreFiles). */
declare const __TROMMI_CORE_WASM__: string
declare const __TROMMI_CORE_WASM_SHA256__: string

async function load(): Promise<Core> {
  await binding.init(fetch(__TROMMI_CORE_WASM__, { integrity: `sha256-${__TROMMI_CORE_WASM_SHA256__}` }))
  const { init: _init, Device: _Device, TrommiError: _TrommiError, StoreConflict: _StoreConflict, ...stateless } = binding
  return { ...stateless, errorCode: (error: unknown): ErrorCode | null => (error instanceof binding.TrommiError ? error.code : null) }
}

let loading: Promise<Core> | null = null
/** The core, loaded and instantiated once. A failed load (offline, a .wasm that is not the build's) is not kept:
 *  the next call tries again. */
export function loadCore(): Promise<Core> {
  loading ??= load().catch(err => { loading = null; throw err })
  return loading
}
