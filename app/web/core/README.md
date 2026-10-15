# The web's executor of the app core

The web app's logic is the Rust app core's (`core/client`, see [its README](../../../core/client/README.md), "The app
core"), loaded as WebAssembly. This folder only RUNS it in the browser: one `AppCore` in a dedicated Web Worker, the
I/O it asks for done here, its view model handed to the page (`public/core.mjs`), which draws it. Nothing here decides
anything about the room. The shrinking-only freeze (`core/client/tests/shells.json`) now lists only these platform executor files; the retired app logic is deleted.

**TypeScript.** Strict (`app/web/tsconfig.json`, `npx tsc -p .` in `app/web/`). Node 26 runs `.ts` itself (type
stripping); the build erases the types for the browser.

| File | What |
| --- | --- |
| `core-worker.ts` | The worker: loads the core, makes one `AppCore` and its shell, owns the slot `trommi` (the device's state in IndexedDB under a Web Lock, the cache in its own database), and talks to the page: renders, files, links and logs out; events, platform answers in. One tab owns the slot: another says `busy` and takes it over with `take` (the owner gives it up over a BroadcastChannel and says `taken`). The demo (`Config.demo`) and the share page keep and own nothing |
| `shell.ts` | `CoreShell`: the executor. Events go to the core one at a time; every effect runs here: HTTP and the live stream by fetch (no redirects, the core's size caps), the store (a `DeviceApply` is a barrier: what follows waits until it is durable), the timer, and platform ops (passkeys, push) handed to the page |
| `core-start.ts` | Starts the worker as early as `index.html` can, before the entry; the page takes it over |
| `store-idb.ts` | The device's state over the binding's `IdbStore` (one strict transaction per write, the revision checked inside it, one owner by Web Lock), its values wrapped at rest under a non-extractable AES-GCM key; the cache space |
| `core-wasm.ts`, `core-api.ts` | The only module that imports the binding and fetches its `.wasm` (with the build's SHA-256); the binding's types |
| `proof-worker.ts` | The core's self test in a worker of its own (Settings → Security → Self Test), with no room |
| `spent.ts` | Where the time of a write goes (wrapping, IndexedDB), for the developer tools |
| `qr-decode.mjs` | A QR reader for browsers without their own (the account screens' "Scan a code") |

The contract between the page and the core is the core's: `Event` and `Action` in, `Effect` out
(`core/client/src/app/`), the view model's types in `core/bindings/wasm/js/views.d.ts`, folded on the page by
`views-fold.js`. Tests: `tests/web/shell/` (the shell, and against the real hub with `TROMMI_HUB_BIN`),
`tests/web/store/` (the store in Chromium).

Apple-only `Snapshot`, `NotifyContext`, `ShareInbox` and `ForgetShared` effects need no browser service and are
ignored. Account authority, push policy, file references and all shared decisions stay in Rust.
