# Trommi for iPhone and Mac

Native SwiftUI drawing the [Rust app core](../../core/client/README.md). One AppCore owns the account, sync,
history, actions, board model, cache, ordered screen views and state-dependent words. Swift folds the generated
ViewModel and performs native I/O: URLSession, encrypted files, Keychain, notifications, APNs, Live Activities and
the share sheet. There is no second Swift account, sync or board implementation.

## Layout

| Path | Responsibility |
| --- | --- |
| `TrommiApp/Sources/TrommiApp/AppShell.swift` | Starts AppCore, folds renders, forwards taps and focus, holds drawing state and file bytes |
| `TrommiApp/Sources/TrommiApp/Screens.swift`, `AccountScreen.swift`, `MoreScreens.swift`, `Details.swift`, `ScribbleScreen.swift` | Native screens drawing the core's generated types; gestures, editors and file pickers |
| `TrommiApp/Sources/TrommiApp/NativeServices.swift` | APNs, local notifications, extension context and encrypted read-ahead pages |
| `TrommiApp/Sources/TrommiApp/NativeLive.swift` | ActivityKit I/O and drawing the core's ordered waiting cards and counts |
| `TrommiApp/Sources/TrommiApp/NativeShare.swift` | The App Group's sealed inbox and recipient snapshot; imports go to the core |
| `TrommiApp/Sources/TrommiApp/NativeDemo.swift` | Bundle I/O and demo navigation; AppCore runs the fixture without a device store |
| `TrommiClient/Sources/TrommiShell/` | Product/module `TrommiClient`: the serialized native executor and platform storage |
| `TrommiClient/Sources/ShareInbox/` | Share Extension's encrypted local inbox, intake and App Group access |
| `TrommiClient/Sources/PushNotify/` | Read-only notification/widget payloads, private key-copy storage and extension I/O |
| `TrommiClient/Sources/NotifyCoreLive/` | The notification/widget extension's narrow calls into the Rust binding |
| `dev/pen.mjs` | Writes the faces of `Resources/pen.json` from the web's pen (`app/web/public/ui.mjs`); `--check` in CI |
| `TrommiApp/AppStore/` | XcodeGen project, entitlements, metadata, screenshots and shipping tools |
| `../../core/bindings/swift/` | UniFFI facade, generated Swift types/fold, static libraries and a proof app |
| `../../tests/ios/` | Native executor/store/transport tests, isolated extension tests and opt-in core observers |

`TrommiClient` is a platform shell despite its retained module name. `CoreSession` is an actor: core calls are
serialized, results retain their I/O ids, and device writes form a barrier before subsequent effects run. Its
URLSession transport sends exactly the core's request, disables cookies/cache/redirects, bounds responses before
buffering and forwards stream bytes in order. Retries, authentication and event parsing belong to Rust.

## Build and checks

The pinned Rust toolchain is in `rust-toolchain.toml`. Swift 6.4 and the matching Xcode 27 SDK build the app;
ordinary native shell tests also run on Linux. From the repository root:

```sh
CARGO_BUILD_JOBS=4 nice -n 19 ionice -c3 sh core/bindings/swift/build.sh host
nice -n 19 ionice -c3 swift test --package-path tests/ios -j 4
node tests/bindings/manifest.mjs
node tests/bindings/views.mjs
node tests/bindings/views-replay.mjs
```

Build one heavy step at a time. Rebuild the binding after any core change. `build.sh` writes ignored output into
`core/bindings/swift/TrommiCoreRust`: UniFFI's Swift file and C header, and `lib/<platform>/libtrommi_core_ffi.a`.
`host` builds for the machine running the tests; `ios` builds the device and simulator libraries; `macos` builds
for Apple silicon. `TROMMI_SIMULATOR=0` leaves the simulator out. The iOS archive's duplicate compiler-runtime
members are removed only after checking their symbols against Apple's SDK runtime; an unchecked archive is not
for shipping.

With an installed darwin Swift SDK on Linux, unsigned module compilation checks the app and extension sources:

```sh
nice -n 19 ionice -c3 swift build --build-system native --package-path app/ios/TrommiApp \
  --swift-sdk darwin --triple arm64-apple-ios -j 4
nice -n 19 ionice -c3 swift build --build-system native --package-path app/ios/TrommiApp \
  --swift-sdk darwin --triple arm64-apple-macosx -j 4
```

These are module checks, not an Xcode archive or a device run. CI's unsigned Xcode app job compiles both platforms;
`main.yml` builds the libraries, archives, signs and delivers from main after its checks passed. The shell's
small real-hub observer is `TrommiShellTests.MegaObserverTests`; the fleet is opt-in and is skipped by ordinary
`swift test`. See the root README for client conformance and correctness runs.

## Device storage and ownership

The native AppCore slot is `Application Support/Trommi/AppCore/default`. The migration uses a fresh slot: users
sign in once after upgrading. Existing encrypted legacy storage is left untouched and is not imported or used as
a fallback.

`DeviceStore` takes the exclusive state-directory lock before reading a key and keeps it until close. The app
alone owns the MLS device and private keys; extensions never open its state. One encrypted journal record is
written per batch and flushed before the core hears success. An uncertain write stops the store. The Keychain
revision anchor refuses a state older than a durable outbox write; the state folder is excluded from backups.
`ShellStore` separately seals disposable cache entries and splits encoded cache paths into bounded filesystem
components. Prefix scans skip unrelated encoded subtrees and reuse enumerated file URLs; names-only listings
do not read or decrypt values. Deleting values prunes only empty encoded directories with atomic `rmdir`;
durable deletion syncs each surviving parent. A sibling value and the cache root stay in place. Writes encode
their destination once. Disposable model cache loss costs a catch-up, never a device identity or pending
protocol outbox write. Public signed logout notices are durable until checked acknowledgement, as described in
[the app core](../../core/client/README.md).

Private state, cache, anchor and core-requested secrets use the app's own Keychain access group and
`AfterFirstUnlockThisDeviceOnly`. The app names the group explicitly; a shared extension item cannot become the
app's device key. Tests use private files on Linux and on an unsigned Mac test binary. The signed Mac app selects
the data-protection Keychain at startup.

A sign-out resets the core's plaintext state and deletes its stored slot/cache and notification copies. A verified
signed removal does the same and erases the sealed share inbox. The hub's removal claim alone never erases a
device. If the disk refuses deletion, extension keys are still cleared and the core shows the failure. See
[Security](../../SECURITY.md) for the complete promises and limits.

## Accounts and native screens

Settings → Data reads authenticated counts and sizes through `AccountAction::ReadUsage`; labels, values and errors come from the core. Signing out clears that view.

The generated `CoreView.ViewModel` supplies the Desk, cards, sessions, Notes, Off your mind, Settings, Artifacts
and Scribble Board. Swift uses the core's order, labels, action modes, status lines, history and attachment ids.
Editors keep only unsent UI input. Pen paths, hit testing, image drawing and theme preferences remain native;
stroke encoding is `Action::DrawStroke` in Rust. Goals are edited through `SetGoal` and folded through
`FoldGoals`; the teams whose goals are folded are a local UI preference. The team in view is the core's
(`sidebar.team`); the shell only remembers the last team chosen on this device (`trommi-team` in UserDefaults) and
sends it as `Focus.team` after a start, and the core falls back to the first team when it is gone. A session's
helpers are a line of dots under its name (`helpers`). The Desk's duck for all and Blitz draw `desk.duck` (sent back as `Batch` "sweep") and
`desk.blitz`; the Chat list draws each row's `last` (teaser and time), grey while not `online`, and says `signal` to VoiceOver only
(no dots); Settings has a page per `Screen.settings(page:)`; the Artifacts filter is `Screen.artifacts(kind:)` with
the words of `view.gallery`; the note saves itself with `KeepNote` a moment after typing and when it is left; the
start screen shows `account.notice` (logged out, removed, account deleted).

Account actions include creating an account, password or Emergency Kit sign-in, recovery, pairing with six emoji,
inviting an agent, answering self-join requests, changing password/e-mail/kit, replacing the recovery code,
removing devices and deleting an account. Core account flows check membership history and keep incomplete joins
durable. Passkeys remain off (`Config.can.passkeys = false`); the native shell holds no parallel passkey flow.

On a Mac the same sources draw a window with a sidebar and a menu-bar list of the core's Desk rows. Closing the
window leaves the process in the menu bar. The Mac has APNs through AppKit but no share, notification-service or
Live Activity extension. File selection and the Scribble canvas are available on both platforms; QR scanning is
iPhone-only.

## APNs, notification titles and Live Activities

`NativeServices` performs OS permission/registration calls and returns tokens to the core. The core owns push
policy and hub requests. `Snapshot` supplies counts, ordered waiting cards, share recipients and Live Activity
policy. ActivityKit token updates go back as `PushToken` events; Rust selects the room/device tag and refuses
unrelated activity tags. The widget's answer intent requires authentication and forwards the tap to AppCore in
the app process.

The extension never holds a device key or MLS state and cannot sign or send. It reads a sealed push using the
context's push key, then optionally reads the hub's encrypted changes with its limited ticket. The app hands
those pages to the core for the same checks as any network page. No ticket is put in a URL.

The notification title uses a read-only copy of the current content key of each session, never the room key.
Only the app and notification extension are entitled to that separate Keychain group; the share extension and
widget are not. `NotifyContext` replaces it when keys change, clears it on sign-out/removal/off, and clears it if
a replacement fails. The extension checks the envelope's signature and sanitizes its title; it does not verify
membership or sender-chain position. Before first unlock, without a key or after a key change not yet copied, it
falls back to neutral text. The full limitations are in [Security](../../SECURITY.md).

The widget receives sealed counts and opens them through `NotifyCoreLive`; its context permits no signing.
Titles and answer labels are supplied locally by the app, never in Apple's push payload. ActivityKit updates and
UI clipping are native; card eligibility and priority are the core's.

## Share Extension

`TrommiShare.appex` links only `ShareInbox`, with no device state or sync engine. It seals the selected text,
links and files into the App Group inbox. Its recipient snapshot contains only ordered teams and their crowned
sessions. The app reads and claims inbox items, then sends `ImportShare` to Rust. The core checks the account,
selects the recipient or fallback Note, uploads attachments and deduplicates an import across restarts. The
inbox is acknowledged only after the core's durable write; unreadable payloads stay for another try. Signing out
clears the recipient snapshot and releases claims; verified removal or account deletion erases the inbox and key.

## Demo and screenshots

“Try the demo” runs `demo/data/fixture.json` in a storeless AppCore. The fixture and files are bundled from
`TrommiApp/Sources/TrommiApp/Resources/Demo`; no second Swift demo model exists and no hub request is allowed.
The menu leaves the demo. `TROMMI_SCREEN=<id>` opens a route named by `demo/data/screens.json`; the core's real
views draw it. `TrommiApp/AppStore/shots.sh` uses this on an iPhone/iPad simulator; App Store screenshot metadata
and marketing framing live beside it. UI-specific states need review when native layouts change. The former
Mac-only automatic window/menu capture hook is not implemented by the new shell.

The compact iPhone frame is the system's tab bar (Liquid Glass, smaller while a list scrolls down): Desk · Chats ·
Search. Desk's bell carries its own dot while questions wait (never a red count). Chats is every team's chats in one
list (`sidebar.stacks`, as the wide sidebar's), each team's name a quiet section title; a tap on it folds the team to a
row of its faces (`Focus.open_teams`, kept on the device; every team open until one is folded). Search filters that
list in the core (`Focus.chats_query`, `sidebar.found`). Cards, conversations and settings push onto the tab's
navigation stack (the bar hides there), so Back and swipe-back return to the list. The note opens from the Desk's top
row. Starred and running cards stand at the head of Off your mind on the Desk, as on the web. The place pill (iPhone
Desk) and the top of the sidebar (iPad, Mac) switch the team of the Desk: a menu of the core's `sidebar.teams`, each
with its open questions.

An iPhone sends `Focus.calm` (the web too), and a chat is then calm, as Grok's: the core folds a run of card events into one row of
chips (`TalkRow.kind` "events", `events`, a tap opens the card), leaves the helper list out (their status lines name
them), and leaves out the turn's trail and the chip of a card a status line names; the app draws only bubbles (no face,
name or time beside them: the title pill has the face) and a solid top row. While the agent works (`SessionView.working`:
his word of the last ten minutes, a trail that still runs, a working line) its face alone draws itself with the pen
where its answer will come (`DrawingFace`); a tap opens the details: "Working · <step>", its lines, the trail. Faces
stand bare, without a disc, in the chat and the Chats list.

The views draw the look the app had before the shared core (sign-in, Desk, card, Chat, note, settings, the glass
bar): `Chrome.swift` holds the shared parts (the top pills, tiles, the start screens' buttons and fields). Which
sign-in form is open (start, create, log in, new password, scan) is the screen's own navigation; what is under way,
problems, emoji and kit words come from `view.account`.

`LookTests` holds the signed-out start screen, light and dark, against `tests/ios/UITests/Reference/*.png` (iPhone 17
Pro, iOS 27; the status bar is left out, at most 0.4 % of the pixels may differ). A meant design change replaces the
reference with the test's `<name>-now` attachment.

`NativeScreensTests.testFreshAccountSignInRendersDesk` tests the real sign-in executor and encrypted store
(start screen, Log in, the form), then a seeded card, Chat, Note, Back and a durable restart. Set `TROMMI_SIGNIN_ACCOUNT_FILE` in the
`TrommiSimulator` scheme (or as the Xcode build setting) to a private JSON file with `email` and `password`.
Use a freshly created Simulator and an isolated hub at `http://127.0.0.1:18874`, populated by `seed-account`;
keep its seed devices online with `--serve`. The test types the credentials into the real form; it never sets
`TROMMI_SCREEN` or injects a demo model. The hub override exists only in Simulator builds and accepts loopback
HTTP only. Without the private account file this opt-in test is skipped.

For ordinary launch timing, after building a Release Simulator app, run
`tests/ios/UITests/startup.sh /absolute/Trommi.app /absolute/mega.json 290 /absolute/evidence` on the Mac.
It creates and removes its own Simulator, records three process launches, requires the full Desk row count and
fails if the launch request through first Desk appearance takes 2 seconds or more, including command setup and time before app initialization.
The helper accepts only the PID returned by `simctl`, reads that process's creation timestamp through Darwin's
`PROC_PIDTBSDINFO`, and records command-to-Desk and command-completion time separately. Command setup can
precede process creation by several seconds after Simulator creation; it remains part of the launch budget as well as visible in the evidence. The command interval uses one Python process, since the Mac's Python 3.9
monotonic clock origins differ between processes. `DESK_BUDGET_MS` changes the launch budget explicitly; process timing alone cannot satisfy it.
This avoids XCTest's injected launch services; its lifecycle delay is included in the XCTest measurements below.

The opt-in `NativeScreensTests.testFullSizeDeskLaunchAndMemory` measures the same Rust model and native Desk
with a made-up large room, separately from real sign-in. Generate one with
`node tests/tools/mega-fixture.mjs /path/to/mega.json 10 5000 1 0`, then set `TROMMI_SPEED_FIXTURE` to that
Mac-local file and `TROMMI_SPEED_ROWS=290` in the `TrommiSimulator` test scheme. It records three responsive
launch measurements and attaches first-Desk timing, row count, resident bytes and boundary timings. Resident
bytes include shared Simulator mappings; they are not a count of private card storage. No account or hub is
used by that measurement. The fixture override requires explicit `TROMMI_SCREEN` demo mode.

For a bounded cache I/O benchmark, run `CoreSessionTests.testCachePrefixScansAndEmptyDeviceStartupWithUnrelatedRecords`
with `TROMMI_STORE_BENCH_ROWS=1000` or `10000` (ordinary tests default to 128). It creates private temporary
stores with encrypted 24-byte values; no account, system Keychain or owner data is used. On the same Mac mini,
five-sample scan medians and one empty-device `CoreSession` startup sample gave:

| Cached records | Operation | Before (ms) | After (ms) |
| --- | --- | ---: | ---: |
| 1,000 | Public-prefix lookup | 219.44 | 0.51 |
| 1,000 | Full value scan | 967.63 | 327.91 |
| 1,000 | Empty-device core startup | 215.87 | 18.30 |
| 10,000 | Public-prefix lookup | 2,289.65 | 0.53 |
| 10,000 | Full value scan | 10,286.19 | 3,521.40 |
| 10,000 | Empty-device core startup | 2,415.09 | 13.62 |

`CoreSessionTests.testDeletingCacheRecordsDoesNotLeaveAScanOfTheirEmptyPaths` checks deletion, a surviving
sibling and durable removal of the last value. With `TROMMI_DELETE_BENCH_ROWS=1000`, five names-only scans
after deletion had a median of 122.47 ms before empty-directory pruning and 0.53 ms after, on the same Mac
mini in Release. The ordinary test uses 32 values.

These measure storage and an empty-device core, separately from a rendered Desk or a physical iPhone launch.
Encryption and the existing filename layout stay the same.

## Xcode, TestFlight and releases

`TrommiApp/AppStore/project.yml` generates the Xcode project: the iOS app, Mac app and three iOS extensions link
the Swift package's products. After building the appropriate Rust libraries, an unsigned compile on a Mac is:

```sh
cd app/ios/TrommiApp/AppStore
xcodegen generate --spec project.yml
xcodebuild build -project Trommi.xcodeproj -scheme Trommi -destination 'generic/platform=iOS' \
  CODE_SIGNING_ALLOWED=NO -jobs 4
xcodebuild build -project Trommi.xcodeproj -scheme TrommiMac -destination 'platform=macOS,arch=arm64' \
  CODE_SIGNING_ALLOWED=NO -jobs 4
```

Signed builds need the existing Apple team configuration; no secret belongs in the repository. The main pipeline
is `.github/workflows/main.yml`: independent iOS/Mac TestFlight and Developer ID direct-download Mac destinations.
The manual App Store gate and replay of checked TestFlight artifacts remain in `ship-apple.yml`.
It delivers only after checks passed and its inputs differ from the last delivery. PRs have no secrets and
publish nothing. Apple certificates/profiles and export/notarization checks remain in the shipping scripts.

- `.github/scripts/ios_archive.sh`, `ios_testflight.sh`, `ios_entitlements.py`: ad hoc archive, cloud-managed
  distribution export, validation, upload and processing. `TrommiApp/AppStore/asc.py` handles App Store Connect.
- `TrommiApp/AppStore/ship-local.sh [REF]`: a separate local TestFlight path from a clean worktree of the ref;
  `--dry-run` validates without uploading. It requires the owner's preconfigured distribution environment.
- `.github/scripts/mac_testflight.sh`: Mac App Store/TestFlight export and validation.
- `.github/scripts/mac_direct.sh`: Developer ID signing, hardened runtime, notarization, stapling and a signed
  DMG (`mac-v<N>`). It fails closed when signing/notarization configuration is unavailable. See the root README.
- `TrommiApp/AppStore/store.sh`: metadata and screenshots, status, explicit App Review submission and release.
  `metadata/` contains the locale texts and privacy answers. App records/platforms and App Privacy still need
  their existing manual App Store Connect setup.

The app declares standard encryption (`ITSAppUsesNonExemptEncryption = NO`) in its Info.plist. Privacy manifests
are copied to the root of the app/extension bundles, outside SwiftPM resource bundles; update them when required
reason APIs change. The app's universal links and web credentials use the association file under `app/web/public/`.
