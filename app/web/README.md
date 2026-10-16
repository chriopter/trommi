# The Trommi web app

The app at **https://app.trommi.com**: a static, local-first single-page app. Every device makes its own keys, keeps
the device and cache in IndexedDB, and decrypts locally. `AppCore` runs in a Web Worker; the page folds its
ViewModel deltas and draws them ([the shared app core](../../core/client/README.md)). Synchronization, account
flows, actions, list order and state-dependent texts are all Rust. No framework: plain ES
modules and CSS, one file per view; deployed as one minified bundle that loads each view when it is first needed
(Cloudflare Workers static assets: `wrangler.jsonc`, directory `public/`, build step `dev/build.mjs`).

## Running it

With Node 26 (`.node-version`) and `npm ci` once in `app/web/` (its `package.json`), from the repository root:

```bash
node app/web/dev/serve.mjs 8900            # the app from its sources, built in memory on every request (never stale, no service worker)
node app/web/dev/serve.mjs 8900 --bundle   # the same as deployed: the minified bundle, views loaded on demand
```

- `http://127.0.0.1:8900/` on a device without an account: Create account or Log in.
- `http://127.0.0.1:8900/?mock=1` the demo room: the same app on a mock of the core, no hub, nothing sent. The room is
  the repository's [`demo/`](../../demo/README.md), for now an empty one. `?mock=0` leaves the demo.
- `http://127.0.0.1:8900/screens?mock=1` every listed screen and state of the demo side by side, desktop and phone.
- `http://127.0.0.1:8900/?mock=1&onboard=create` one account screen drawn by the demo (`auth.mjs` `demoFlow`).

A change to a view: edit its `.mjs` and `.css`, reload. Nothing generated is committed; the deploy bundles it.

### Developer switches

Not shown anywhere in the app; all are kept for the tab only (sessionStorage).

| Switch | What |
| --- | --- |
| `?hub=<url>` | another hub than the default, for self-hosting and local work. Default: `https://hub.trommi.com`; on localhost `http://127.0.0.1:8890`. The deployed app's CSP (`public/_headers`, `connect-src`) names only the default hub: an app served for another hub names that hub there. The dev server also lets `http://127.0.0.1:*` and `http://localhost:*` through |
| `?found_code=<code>` | a founding token, for a hub that asks for one when an account is created |
| `?mock=1` | the demo room: the app core runs the fixture of `demo/data/` (`Config.demo`): no hub, nothing stored; what is done shows and stays until the tab is closed. `?mock=0` ends it |
| `?state=<name>`, `?onboard=<name>` | demo only: the click a state of `/screens` needs, an account screen |

## Checks

```bash
(cd app/web && npm test)                   # types, the layout rules below, the demo data, crypto and core tests
node app/web/dev/check.mjs                 # the layout rules alone
node app/web/dev/build.mjs                 # the build, in memory: prints the cold start's size, writes nothing
node app/web/dev/look.mjs URL 1440,900 out.png [--dark] [--js '...']   # one screenshot, console errors (needs Chromium)
```

Against a real hub: `TROMMI_HUB_BIN=<trommi-hub> node --test tests/web/shell/real-hub.test.mjs` (the web's executor
of the app core: an account founded, a second device signs in, the account deleted). In the browser, start a hub
with `HUB_ORIGINS=http://127.0.0.1:8900` and the dev server, and open the app.

## Layout

```
worker.js              http -> https, the universal links' file, /whats-new.json (whats-new.mjs: What's new from the releases' notes)
wrangler.jsonc  THIRD-PARTY.md   fonts, Argon2, the QR reader, the word list: whose they are
core/                  the executor of the Rust app core (core/client, as WebAssembly) in a Web Worker: I/O, storage,
                       one tab owning the device (core/README.md). All of the app's logic is the core's
dev/                   serve, build, check, verify; look and the browser tests (cdp.mjs drives Chromium)
public/
  index.html  sw.js  manifest.webmanifest  _headers  robots.txt  frame.html (the sandbox a published page runs in)
  help.html            help and "how it works", its own style and script inline
  app.mjs  app.css     start, router (pages drawn from the core's view model, morphed in place on every render),
                       frame, the core's toasts, new versions; app.css: tokens, base layout, type, links, focus
  ui.mjs               what every view shares: html, the pen, words, toasts, the controllers (controller(name, class)),
                       rich text, agent layouts, keys, a card's row, a session's mark, the room's frame
  auth.mjs/.css        account screens, passkeys, password, Emergency Kit, recovery, pairing, devices, settings, log out
  desk  card  session  sidebar  notes  media  agents  teams  whiteboard   (.mjs + .css each)
  proof.mjs            Self Test (the core's self test), a row of Settings → Security
  news.mjs             What's new: /whats-new.json (whats-new.mjs, the worker's: the releases' notes), from the menu
  core.mjs             the page's side of the app core (its worker: core/core-worker.ts): the view model, actions, focus, files
  platform.mjs         what only the page can do for the core: passkeys (WebAuthn, prf), Web Push
  paths.mjs            the addresses of before (/s/…, /a/…) and their new form; shared by app.mjs and worker.js
  demo/                the demo room's code (demo.mjs, screens.css); its data is the repository's demo/data/, which
                       the build copies here as fixture.json and files/ (not in git)
  changelog/           What's new's day summaries (CHANGELOG.md: one to three sentences a day), the days before the releases had
                       notes (history.json, frozen) and the pictures, one per day at most (<YYYY-MM-DD>.webp): shots
                       of the demo room only, webp, at most 150 KB each; the iOS app loads them from here too
  fonts/  icons/  drawings.json
  apple-app-site-association.json   the iOS app's universal links, served at /.well-known/apple-app-site-association
  gen/                 generated by dev/build.mjs, not in git: app/ (the bundle: app-<hash>.mjs and its chunks),
                       vendor/tools-reference.mjs (for the help page), bundle.<hash>.css, build.txt, manifest.json; the release adds
                       release.json (its number, commit and time, for the line at the top of What's new; .github/scripts/web_release.mjs)
```

**Inviting an agent.** The invite page (`auth.mjs` `clipboard()`) shows three commands to copy: once per machine
`curl -fsSL https://raw.githubusercontent.com/chriopter/trommi/main/install.sh | sh` (the connector's signed release,
`connector/README.md`), once per program `trommi-connector setup claude` (or `setup codex`), once per folder
`trommi-connector connect '<link>'` with the invite's link. The iOS app and the website show the same three. The app
serves no connector binaries. Two of the connector's files feed the help
page's list of tools: `connector/tools.json` and `connector/prompt.md`.

## Rules

1. A view imports only from `app.mjs` and `ui.mjs` (the core through `app.mjs`), never from another view. What two views
   need goes to `ui.mjs` (markup, controllers) or `app.mjs` (data, the room). `ui.mjs` imports nothing.
2. No app decisions or crypto in the page: read `t.vm`, send actions through `t.act`, and report focus. The worker
   runs the Rust core and performs I/O; it builds no authentication, retry policy or domain model of its own.
3. `gen/` is never edited by hand and never committed: the build makes it.
4. A file is split only when it passes about 3000 lines.
5. The views are `.mjs`, the core is strict TypeScript (`app/web/tsconfig.json`). Importing a module
   does nothing; `app.mjs` boots the page.
6. A view other than the Desk's (desk, sidebar, notes) is loaded on demand: `app.mjs` `LAZY` names the addresses each
   answers; the router loads it before the first such address, and all of them once the first page is idle. A view
   that adds a page adds its addresses there.
7. **No `<style>` elements and no inline scripts** in what a view renders: the CSP refuses them (`style-src 'self'`,
   no `'unsafe-inline'`). Styles go into the view's `.css` file (or a file of its own under `public/`, named in
   `dev/check.mjs`); a `style="…"` attribute is fine (`style-src-attr`: the views set `--hue` and the like per element).
   An inline `<style>` or `<script>` in `index.html` or `help.html` needs its hash in `_headers` (`dev/check.mjs` says which).

`dev/check.mjs` fails on a file outside this layout, a view that imports another view, crypto outside the core, an
inline script or style whose hash is not in `_headers`, and broken demo data.

## The build

`dev/build.mjs` makes everything generated, at deploy time:

- `public/gen/app/` (esbuild, minified and split): the entry `app-<hash>.mjs` with app, ui, desk, sidebar and notes,
  one chunk per lazy view, the demo, the core's worker in one file
  (`core-worker-<hash>.mjs`) and `core-start-<hash>.mjs`, which `index.html` runs first to start the worker. Beside them lies the Rust core,
  `trommi-core-<hash>.wasm`, as `core/bindings/wasm/build.sh` made it (`core/bindings/wasm/pkg/`; the build runs that script only when
  its output is missing or older than the core's sources); the binding's scripts are inside the worker. Every name
  carries its content's hash; `_headers` keeps them immutable.
- `public/gen/vendor/tools-reference.mjs`: the connector's tools and events, for the help page.
- `public/gen/bundle.<hash>.css`: the `<link>`s of `index.html` as one stylesheet, in their order.
- `public/gen/build.txt` and `public/gen/manifest.json`: the commit, every served file with its SHA-256, the build hash.
- in `index.html` the scripts and the modulepreload list, in `sw.js` `VERSION` and `SHELL`, in `_headers` the hash of
  the import map. In the repository these three are templates (empty preload block, `VERSION = "dev"`, `SHELL = []`).
- `public/demo/fixture.json` and `public/demo/files/`: the demo room, checked first.

`node dev/build.mjs` checks only and prints the cold start's size; `--write` writes into `public/` (never commit that:
it rewrites `index.html`, `sw.js` and `_headers` too). In Cloudflare's build (`WORKERS_CI=1`) it runs `npm ci` in
`app/web/` first, writes, and refuses another Node, esbuild, Rust or wasm-bindgen than the pinned ones
(`.node-version`, `app/web/package-lock.json`, `rust-toolchain.toml`, `core/bindings/wasm/Cargo.toml`). `dev/serve.mjs` serves the
build from memory: by default the sources as modules of their own, with `--bundle` as deployed.

**Integrity.** The bundle's `index.html` carries `integrity` (sha384) on the entry, `core-start` and every
modulepreload, and an import map with the integrity of every module of the page; its hash is added to the CSP of the
generated `_headers`. The core worker loads from this origin under the same CSP (workers take no integrity attribute);
the Rust core's scripts are inside it, and its `.wasm` is fetched with the SHA-256 the build computed
(`fetch(url, { integrity })`), so the browser hands the worker nothing but those bytes.

**Deploying.** Cloudflare Workers: root directory `app/web`, build command `node dev/build.mjs` (`wrangler.jsonc`),
deploy command `npx wrangler deploy`, Node from `.node-version`. The build reads `app/web/`, `demo/data/`,
`connector/tools.json`, `connector/prompt.md`, `core/` (through `core/bindings/wasm/pkg/`), `Cargo.toml`, `Cargo.lock`,
`rust-toolchain.toml` and `.node-version` (`package.json` and `package-lock.json` are in `app/web/`): a change to any of them is a new build.

### Verifying the build

`gen/manifest.json` lists every file the app serves with its SHA-256; `gen/build.txt` names the commit and the **build
hash**, the SHA-256 of that manifest. The build is reproducible (content-named files, no time stamps, the toolchain
pinned and named in the manifest: Node, esbuild, Rust and wasm-bindgen; its output does not depend on the directory it
runs in), so anyone can check that a server serves exactly this source:

```bash
curl -s https://app.trommi.com/gen/build.txt          # commit: <c>, build: <hash>
git checkout <c> && (cd app/web && npm ci)
node app/web/dev/verify.mjs                           # or --app <url>: builds here, compares the manifests file by
                                                      # file, fetches every file and checks its bytes; exit 0 = verified
```

The service worker: a new deploy takes over at once and the page reloads (a field with unsent words: a quiet "Reload"
instead); files come network first, the cache only offline. On the dev server (`VERSION "dev"`) there is none.

## The Desk

- **Goals:** under the greeting, the team's own checklist (at most 20 lines of 200 characters: `GOALS_LINES`,
  `GOALS_LINE_MAX` are editor limits; Rust `actions::run::clean_goals` validates the text), written in place with a click; empty only a faint "Goals…". One toggle, a chevron beside them, folds them to their first line and opens them again (folded or
  not is kept per team in `localStorage` `trommi-goals-folded`), so a long list need not push the cards off the screen. A line that starts with an emoji or a tick box has its sign in a
  column of its own: the words of such lines start under each other. The field grows with its words to twelve rows,
  then it scrolls. Leaving the field always keeps what was written: on `blur`, and also on a press outside it, when the
  page is hidden or left, and when the field is found to be no longer the active element (a document without focus, a
  background tab, sends no `blur`). Kept in the desk's register (`desk/<id>`, `goals`), so every device of the account sees
  it; the agents on the desk are told them.
- **Rows:** every open card is one row in the hub's order, blocking first, then knocks; infos stand among them (What?? and
  ✓ instead of answers). A row: the session's drawing as a stamp before the title, the urgency sign, the title, square
  answer tiles; under the pointer the teaser and, at the left, a ring that selects it (Shift: a range). Rows of one session
  that follow each other stand in one run, held by a pen curly bracket at the far left (two or more). A row that leaves
  glides out while the rows below move up (`app.mjs` `flipOut`, transform only).
- **Tips:** one tip in the app's look for every element that says something under the pointer (`app.mjs` "tips",
  `app.css` `.tip`): `data-tip`, and any `title` (taken over when the pointer comes, so the browser's own tooltip never
  shows). A row's title says its number, session and whole title.
- **Selection bar:** while rows are selected, Later (the pull-tag: the rows go down into Off your mind), Egal, Read (with
  infos), Shred; one POST `/cards/batch`, one toast with Undo.
- **With the agents:** answered cards whose session is still at it, and an info its agent is still working on
  (`working`, from `start_task`: never on the Desk until its "Done: …" version), one line each, below the open ones. A card answered
  with a final option (a small pen tick on its tile) never lies here: the answer settles it, it goes to "Off the desk"
  at once ("settled by your answer"), and Take back opens it again.
- **Foot:** two stacks docked at the bottom of the Desk (`desk.mjs deskDock`, in the Desk's layer, so not on a card's
  page): the starred cards and the cards still running, each a small tab at the window's lower edge (a phone: a pill
  above the tab pill) with its sign, its word and its count, no card at rest; a tap fans it upward as small cards,
  the running one ends in the way to the complete list. The section
  Off your mind (`offHint`) is a short hinted list of what is put off and closed, beside Artifacts on a wide window;
  its heading opens `/stacks/off`, every card with a search. Which card lies where is the core's `DeskView` and `OffListView` (`core/client/src/off_mind.rs`): a card has one home, a starred card lies in the starred stack
  only. The note stays in its corner (a phone: in the tab pill). Artifacts (the newest four;
  its heading "Artifacts N" is the link that opens `/artifacts`: Media and Pages together, the newest first, the filter
  All · Media · Pages, compact tiles in a soft frame with Open and, on a page, Share as small icons; 25 tiles made at once,
  the rest as they come near).
- **Note:** one drawn yellow note at the window's bottom-right; it unfolds there to write, takes attachments, sends to the crown of the team in view; a square sheet on a wide window (`sidebar.mjs` `cornerNote`).
- **Sidebar:** the Desk box (the team in view; its chevron opens the Trommi menu, the teams at its top with what waits on each), the sessions of that team (each with its helpers as dots under its name, tallies, the red hand), "New agent" and
  the Trommi menu at its foot. One sidebar, two presentations (`sidebar.css` "A phone"): beside the page on a wide
  screen (foldable to a rail), a drawer up to 860px that a finger from the left edge slides in over the dimmed page (a
  tap beside it, a push back, Esc or a choice closes it). A phone's top line is the pill (the desk's drawing and name;
  it opens the menu, `sidebar.mjs` `phoneBar`), in a chat the way back to the chats; at its foot a glass capsule Chat ·
  Desk · Note (`tabBar`).

## The Scribble Board

Drawing is the back of the Desk: its top-right corner is a small dog-ear that lifts as the pointer comes near and
follows it freely when held; a tap, a pull past a third of the way, a fling or `P` turns the page to the
**Scribble Board** (`/scribble-board`; the same corner, Esc or `P` turns back; `ui.mjs` controller `curl`). Under the
lifted sheet lies the other page itself: the board is the frame's part `pad`, mounted once when the corner is first
approached and kept under the Desk (`app.mjs` `keepPad`); the Desk under the board is rendered from the model
(`peek`). The page is
the pad (`whiteboard.mjs` mountPad) as large as the main area; select or frame something and **Send to…** a session.
One drawing on it is a scribble.

What is drawn is the board of the team in view (the core's `scribble::team_board` and `BoardView.board_id` give its
identity): one board per team, and a room without teams draws on the `main` team's. `/scribble-board?team=<id>`
switches the team and stays on the board. Settings · Teams shows every team's board small (controller `thumbs`).

**Parking the corner note (wide windows).** On the Scribble Board the yellow corner note can be dragged onto the board
(`sidebar.mjs` controller `corner-note` `park`, event `trommi:park-note`, taken by `whiteboard.mjs`): closed, the
corner's sticky is dragged; open, the sheet is taken by its paper (its edge, its foot, the grip "onto the board"),
never by its words. Where it is dropped it stays as a `sticky` with its words; its pictures are laid under it as
pictures of the board (decrypted here and uploaded as the board's own attachments), and both leave the corner's note.
Files that are no pictures stay with the note.

**The board's controls.** One row at the foot: undo and redo at its head, a hairline, the tools, the
picture clip, and beside it the cut-out drawing. The zoom (out, 100 %, in, fit) is a small capsule at the top right,
beside the turned corner. Light or dark is not the board's: on a wide window the switch stands at the sidebar's foot
beside the fold button (`sidebar.mjs` `SIDE_FOOT`, `.side-theme`; above it on the rail); a phone, which has no sidebar foot, keeps it in the menu. The board has no "?" button: `?` opens its shortcuts.

## The account

The app says **account**, never "room" (inside, the core founds and joins a room; one account is one room).

- **Create account:** e-mail and password; password derivation, founding and the Emergency Kit belong to the core.
- **Emergency Kit:** shown once by `AccountView`; recovery checks the room's signed history, replaces the code and
  removes the other human devices. Nothing on the server can reset an account for its owner.
- **Sign in:** password, Emergency Kit, or an invite whose six emoji must match. Passkeys remain off.
- **Sign out:** the core ends the token and wipes the device and cache. It clears content and notification state;
  the device remains a member until another device removes it.
- The hub learns the e-mail and membership, never the password or content keys. See [Security](../../SECURITY.md).

## How it works

### Data flow

1. `index.html` runs `core-start` first: the core's worker (`core/core-worker.ts`) is fetched and the `.wasm` compiled
   while the page loads. `app.mjs` boot sends it `start` (the config: hub, platform, the demo's fixture) and Focus
   (the address as the core's `Screen`, `app.mjs screenOf`).
2. The core opens this browser's device from IndexedDB (no network) and sends its first render, the whole view model.
   The page for the address is drawn from it (`router.visit` → `board.request` → a view's route → `bodyParts`); the
   first paint does not wait for the hub.
3. The core signs in, catches up and keeps its live stream. Every render is a delta of parts (`ViewPart`), folded on
   the page (`views-fold.js`); once per animation frame the page in view is drawn again and MORPHED into the body
   (`app.mjs morphNode`/`morphKeyed`): unchanged nodes stay with their controllers, rows of a list are matched by id.
4. A form or a control is answered by the views' handlers in the page: they send an `Action` (`t.act`). The core shows
   it at once (an echo in the view model), seals, signs and sends it; the hub's copy replaces the echo, a refusal rolls
   it back with a toast.

### Addresses

`/` the Desk · `/team/:id` and `/?team=<id>` switch team · `/blitz` Blitz · `/artifacts` Artifacts (`?kind=media`, `?kind=pages`) · `/scribble-board` the Scribble Board · `/card/:nr` a card (`?v=n` an older version, `/picture/:n` a picture) · `/chat/:session` a session (`/files`, `/files/:n`, `/artifact/:id` one of its Artifacts) · `/chat/:session/card/:nr` a card from its session · `/chats` the list of chats · `/artifact/:share` a Share link's page · `/settings` Settings: one list (Invite a Device and Invite an Agent side by side, the device's code in place, `?pair=<invite>`; a row per page) · `/settings/agents` the agents (a program on a computer) with their sessions as a tree (`/settings/sessions` leads there) · `/settings/teams` every team as a card with its board small and its counts; the bin deletes a team with everything on it (held to confirm; client core `team_delete.rs`, finished on the next start when broken off) · `/settings/devices` your devices (phones, tablets, browsers) and Push (`/:device_id` one device: its fingerprint, Rename or Remove; `/removed` the removed ones) · `/settings/account` the account (`/password`, `/passkeys`, `/kit`, `/email`: each way in on its page, a change asks "Confirm it is you") · `/settings/theme` · `/settings/keys` · `/pair/:invite_id` an invite · `/join#v1.<hub>.<room>.<secret>` joining (the secret never reaches a server and leaves the address bar once read). The addresses of before, `/s/…` and `/a/…`, move to `/chat/…` and `/artifact/…` (a 301 of `worker.js`, and the router in the page: `public/paths.mjs`).

### IndexedDB

The core owns what it stores: the device's state in the database `trommi` (the binding's `IdbStore`: packed journal deltas and checkpoint pages, revision checked in each strict transaction, values wrapped at rest, `trommi:wrap` holds the key; one owner by Web Lock) and its cache in `trommi:app:cache` (string keys, bytes; safe to lose). The app keeps only per-browser conveniences in localStorage: theme (`trommi-theme`), rail (`trommi-rail`), the team in view (`trommi-team`, sent to the core as `Focus.team`); the developer switches below are sessionStorage only. The mock room keeps nothing.

### Render strategy (performance)

- **Local first**: first paint from the view model the core made from IndexedDB; the network only adds what is new.
- **Morph only**: after a render, the page is drawn again and only nodes whose markup changed are touched; the body is kept per part (a navigation keeps the topbar and sidebar if their markup is the same).
- **Windowed**: conversations are timelines loaded newest page first (50), older pages on "Earlier"; a session page loads its cards' threads lazily; Desk rows and log messages out of sight are skipped by layout and paint (`content-visibility: auto`).
- **Lazy decrypt**: attachments are rendered as `/att/<id>` with `loading="lazy"`; the service worker asks the page, which fetches and decrypts only that file, only when it is shown or opened.
- **No framework, one stylesheet, a small first load**: one minified bundle; a cold start fetches the entry, the page's
  side of the core worker and their shared chunks (preloaded at once) and the core worker, every other view comes when it is first needed or once the page is idle;
  one stylesheet bundle.
- **CSP**: `script-src 'self' 'wasm-unsafe-eval'` (WebAssembly for Argon2id only) plus the hashes of the two inline scripts (the theme before first paint, the help page) and of the bundle's import map; `style-src 'self'` plus the hash of the help page's style, no `'unsafe-inline'` for style elements (style attributes only, `style-src-attr`: the views set `--hue` and the like per element); fonts self-hosted (`public/fonts`, OFL), all assets from the app origin, `connect-src` only the hub. `dev/check.mjs` checks the hashes; the e2e fails on any CSP violation.

### The demo room

`?mock=1` passes `demo/data/fixture.json` as `Config.demo`. The same Rust App makes the model, views, actions and
optimistic echoes entirely in memory, with no account store or hub. `public/demo/demo.mjs` handles only `/screens`
and the UI clicks named by `?state=`. Bundled demo files supply its pictures. The fixture and the list of states
live in [demo/](../../demo/README.md); no second demo model lives in JavaScript.

Decrypted file URLs live only while the painted page or the Scribble canvas references them; leaving those views revokes them. Published-page text previews keep at most eight documents of at most 1 Mi characters each. The encrypted picture cache remains separate. Giving the device to another tab closes its WASM core and worker, clears decrypted file URLs and drops late I/O results. `tests/web/views/file-memory.browser.mjs` checks file release in an isolated browser without an account or hub. The ownership browser check uses weak references and a real collection cycle to verify that leaving the actual Desk releases its observed offscreen rows. The bounded demo sample had zero detached survivors from 39 observations even before cleanup. The unused generic deferred-list helper and its mutation observer were removed. These checks do not establish the cause of a long-open renderer reaching 12.8 GB; long-run RSS validation remains separate.

Keyed DOM updates keep the cursor on the replacement when a row at that cursor changes element type. The
ownership browser gate exercises replacing the first row while adding a second; real-account updates must
not throw `insertBefore` errors or stop repainting.

The browser ownership gate (`tests/web/views/ownership.browser.mjs`) runs the deployed bundle and service worker:
reload twice, close/reopen, a genuinely live second tab, explicit takeover, and a stranded lock without a live
page. A holder answers a probe through its page; after one second without an answer the new worker can take
the lock. Explicit “Use it here” also requests takeover. The binding fences the revoked store and aborts its
unfinished writes before notifying its worker to stop. Page exit terminates its worker; a restored back/forward
page reloads before using the device. No stored device, keys or cache are deleted to recover ownership.
**The start.** Until the core has something of the room to draw, `index.html` shows a quiet outline of the Desk
(`#trommi-opening`: the sidebar's paper with three piles, the greeting "Opening *Trommi.*" in the display face with
the pen underline, faint rows). Its one line is the core's own (`ViewModel.status` while it starts and opens:
"Unlocking this device", "Loading your Desk", "Catching up: 120 of 480", "Almost there", with `args` step of steps
and done of total; `core/client/src/app/start.rs`); before the core runs it says "Loading the app". One thin bar
under it shows how far along the start is (a slow stroke to and fro until the core names a step). The outline goes
once the core is past its start and the board this device kept is read (`app.mjs` `startDone`); while the room still
opens a 2 px line at the window's top goes on until it is open. The two faces it paints in (Bricolage Grotesque,
IBM Plex Sans; latin) are preloaded and `fonts/fonts.css` is part of the stylesheet bundle, so it is in Trommi's
type from the first paint, not in the fallbacks. A start that takes over 30 s shows a quiet note with Reload and goes
on. The busy/taken screens ("Trommi is open in another tab") use the same frame; browser checks assert their
computed fonts and geometry, and every demo screen must retain the Trommi stylesheet.

The browser driver starts Chrome once and waits up to 30 seconds for its debugging port; it does not retry a
failed browser. A cold host can take more than the former ten-second allowance. Startup failures include the
exit status and a bounded tail of stderr. `tests/web/build/browser-start.mjs`, run by the WASM browser gate,
checks an actual browser delayed by eleven seconds and an early exit with a diagnostic. The build gate also
checks the actual app worker's WASM URL, digest and bundled binding unconditionally; no pre-switch skip remains.

The browser test command refreshes stale generated binding output before opening the store tests, so a branch
change cannot exercise an older IndexedDB adapter against the current test page. Rust/Wasm builds still run
one at a time under the repository build limits.
