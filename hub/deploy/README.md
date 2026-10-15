# Hub deployment and data

Our hosting now uses the [container and YoloDeploy flow](../container/README.md). The systemd installer and binary
updater described below remain for older self-hosted installations; they no longer deploy our production hub.
The `ship-hub.yml` workflow descriptions below are historical: that workflow has been removed. Current CI uses
`main.yml` and the signed container flow linked above.

How content and keys are organised, how a change becomes a signed release, and how that release reaches the
server. The short form is in the [README](../../README.md); the security list is [SECURITY.md](../../SECURITY.md).


The protocol is version 2: MLS (RFC 9420) distributes the keys, content is Trommi's own signed envelope. The whole of
it is [`spec/v1.md`](../../spec/v1.md); why it leaves the standard in five places is
[`spec/v1-deviations.md`](../../spec/v1-deviations.md). The web app in `app/web` speaks it through the shared Rust core
(`core/`, as WebAssembly).

**Content: what a person or an agent has.** Sorting is not encryption: which Desk an Agent is on, the order, the
crown and which Helper hangs under which Agent are register values; moving changes no key and moves no data.

```
Room                                          everything of one account (an account has a list of rooms; one for now)
├── Desk                                      a register (name, order, crown, Goals)               room group
│   ├── Goals                                 in the Desk's register; copied into each Agent's session for the agent
│   └── Scribble Board (one per Desk,         items: stroke, text, sticky, voice note, picture,    room group
│       one for "All desks")                  move, erase, send away; snapshot = a file
│       └── piece of a stroke in progress     relayed live, never stored (an MLS message)
├── Note                                      versions; sending it makes a Chat message             room group
├── per-card markers                          Later (snooze), duck, drafts, read marks: registers   room group
├── Agent (a session)                         everything below is that one session                  its session group
│   ├── Chat                                  messages, attachments; the terminal's input and final answer
│   ├── work trail                            "Working · 3 steps": MLS messages, kept 30 days
│   ├── Decision card, Info card              versions; answer, take back; the card's own Chat
│   ├── permission request + verdict
│   ├── Artifact                              a published Page or Media: versions, files; Share link
│   ├── Status line, agent profile, Goals     registers
│   └── Helper (a child session)              the same again, in a session group of its own
└── Push, Live Activity                       no content: how the hub reaches a device
```

**Keys: what only protects.**

```
Account (e-mail; password or passkey)         the hub checks the login; no key here
└── sealed copies of the recovery code        under the password, the Emergency Kit words, each passkey        
    └── recovery code → recovery key          public halves in the room group's context; not a member

Device                                        one Ed25519 signature key = its id; KeyPackages at the hub

Room group (MLS)                              all of the person's devices. Not a Desk.
└── content key, per epoch                    MLS exporter; protects Desks, Goals, Scribble Boards, Notes, markers

Session group (MLS), one per Agent/Helper     the person's devices + that session's agent device(s)
└── content key, per epoch                    MLS exporter; protects Chat, cards, permission requests, Artifacts,
                                              Status lines

Old content keys                              kept by every device that had them; handed to a new device or to an
                                              agent taking a session over in an MLS message; one sealed copy per
                                              group and epoch at the hub for the recovery key                  
File key                                      random, one per file; inside the encrypted body that names the file,
                                              and after the # of a Share link                                  
```

- Two boundaries only: the room (the person's devices) and each single session (those devices and its agent devices).
- The hub stores public group state, ordered group messages and ciphertext; it never holds a key or reads content.
- Not forward secret for history, by decision: whoever holds a human device's state or the recovery code reads the
  whole history. After a device's next update or its removal, a copied group state opens nothing new.

**What a hub reads**: who is in which group and every change to it; of each stored item the signed header (sender,
group and epoch, the sender's running number and chain link, recipient, time, kind, the Chat or board it belongs to,
a card's id, type, state and urgency, a register's opaque id, file ids, the push flag) and its padded size. **What a
hub never reads**: a body: texts, card content, choices, verdicts, register names and values, strokes, file keys,
file names and types. **What goes away**: thirty days after a card is answered or closed its bodies and files; work
trails after thirty days. The table per stored thing is in `spec/v1.md`, section 14.

## Delivery

Each part has its own pipeline on `main` (`.github/workflows/ship-<part>.yml`), started by a push that changed the
part or the core: it runs the part's full tests (and the core's, when it changed) and delivers the part, on its own,
when what that part has out there was built from other inputs: `.github/scripts/inputs.sh` hashes, per part (web, connector, ios, hub, updater),
the files its build reads (as git lists them for the commit: mode, object, path; `core/`, `core/client/` and the Rust pins
count for every part, each part's own build and delivery files for it, tests for none), and
`.github/scripts/deployed.sh` compares that with what is out there. A delivery that failed is therefore made again
by the next run. There is no release of every part; N is the pipeline's run number (plus its `RELEASE_BASE`): it only grows.

| Part | Build makes | Released as | Delivered when what is out there differs |
| --- | --- | --- | --- |
| Web app | `web-release`: the files the worker serves, the worker, its settings, `manifest.json`, `checksums.txt` | `web-v<N>`, signed (a record) | `ship-web.yml`: the Cloudflare worker `trommi-app` at https://app.trommi.com, then checked from outside and put back when it does not serve exactly that build |
| Connector | `connector-release`: four binaries | `connector-v<N>`, signed | the release is the delivery: `install.sh` and `trommi-connector update` take the newest; when the same commit delivers the hub, it is published only once the server runs that hub |
| iOS and Mac apps | `ship-apple.yml` (when the apps' inputs differ from those last delivered) makes `ios-release`: the Rust core for iOS and its Swift bindings | `ios-v<N>` (notes only), `mac-v<N>` (the .dmg) | `ship-apple.yml`: a TestFlight build in the groups "Intern" and "Trommi" |
| Hub | `hub-release`: the hub and its updater as static programs | `hub-v<N>`, signed | `ship-hub.yml`: the server takes `hub-v<N>` (below) and restarts the hub only when the hub's inputs changed |

A release of a series is published by one run at a time; a run whose number is not above the newest of its series,
or whose commit does not come after that release's, publishes nothing.

## How the hub is deployed

From a commit to the running server, nobody logs in and no code is handed to the server by CI.

```
commit on main
└── ship-hub.yml       the hub or its updater differs from what the server has: its tests, two static programs
    ├── sign-release.yml  the hub's release hub-v<N>, its manifest signed (Ed25519, key from the 1Password Environment)
    └── deliver        joins the tailnet as tag:trommi-ci and says to the server: "take hub-v<N>"
                       │
server (updater, as its own user without root, port 9443, tailnet address only)
        ├── fetches hub-v<N> from GitHub itself
        ├── checks the signature against the public key pinned on the server, then each file against the manifest
        ├── refuses anything older than what it once accepted
        ├── the same hub inputs as the running release: notes hub-v<N> as accepted, the hub keeps running
        ├── dry run: the new hub on a copy of the live data, without network, while the old one serves;
        │   fails → nothing is touched, the old hub runs on, the call answers dry-run-failed
        ├── has the hub stopped, swaps, has it started (its unit copies the database, the hub's keys and a
        │   hard-link snapshot of the uploaded files first), asks the hub whether it is well
        ├── not well: puts the release before back and starts it; the data goes back with it only if the new
        │   hub never served a request
        └── answers with what happened; the run shows it and fails unless the release runs
ship-hub.yml
├── asks https://hub.trommi.com/healthz until it answers as the new commit (three minutes)
└── report     a failed delivery opens the issue "Hub update hub-v<N> failed" (label deploy-failed) or comments on the
               open one; the next delivery that goes through closes it (the same for the web app)
```

**The release** has the form of every signed release (above); a release never brings a key. The hub is released as static programs, not as an image: one file each, hashable, no registry
and nothing to unpack; the server says the SHA-256 of what it runs, to compare with the manifest of the release. The
version is the run number of `ship-hub.yml` (plus its `RELEASE_BASE`): a whole number that only grows. A published
release is never replaced.

The manifest may carry fields an updater does not know (they are signed all the same and ignored), so a later
release can say more without an older updater refusing it. Its `"inputs"` name, per part, a SHA-256 over what the
build reads; whether a program is started anew is decided in one place (`changes` in `hub/updater/src/lib.rs`), by
the inputs when both releases name them, otherwise by the SHA-256 of the program in the two manifests. That decides
whether the updater is replaced and whether the hub is swapped. A hub that was not restarted keeps running the
binary of the release it started from (the server's status names that release).

**Why the deploy call needs no secret.** The call carries no code and no address, only the name of a release. What
the server then runs must be signed with the release key, be the hub of this repository, and not be older than what
it already accepted; a caller can therefore at most ask for the newest release a little earlier than CI would
have. Who may ask is decided by the tailnet: the policy lets only `tag:trommi-ci` reach port 9443, and the updater
asks the machine's own tailscaled who the caller is and serves only devices with that tag or the owner's own
devices. If tailscaled cannot be asked, nobody is served.

**When something goes wrong.**

| | what the server does |
|---|---|
| GitHub does not answer, or has no such release | nothing is touched; the hub runs on; the call answers `fetch-failed` |
| signature or a file does not match | nothing is touched; `not-verified`; what was fetched is deleted |
| older release | refused before anything is fetched |
| the new hub fails on a copy of the data (dry run: its migration, its start, `/healthz`, a `/v1/` route, SQLite's checks of the migrated copy) | nothing is touched, the old hub serves on; `dry-run-failed` (HTTP 424), the reason in the answer and the issue; the release is not noted, a fixed later one goes through |
| the new hub does not become well in 60 s, or says another commit, and has served no request | the hub is stopped, its data is put back to the copy made right before it started (database, keys, uploaded files), the release before is put back and started; `rolled-back`, `data: restored` |
| the same, after it served a request | only the program goes back: the release before runs on the data as the new hub left it; `rolled-back`, `data: kept`. A release with a higher data epoch is not put back at all (below): it stays, is started again, and the call fails saying so |
| the data cannot be put back (halfway) | nothing is started on it; the hub stands and the updater tries again at its next start (`trommi-hub-updater status` shows the note) |
| the disk is full | a fetch that cannot be written changes nothing; if the copy of the database cannot be made, the new release is not started and the release before is put back |
| the machine or the updater goes down in the middle | a note written before the swap is found at the next start (the hub is started by the updater, never by itself, so a release whose health was never known does not serve): if the new release is in place, proves, runs and is well, it stays; otherwise (also after a reboot, when it does not run) the release before is put back: a recovery never starts a release whose health was never known; if not even that can be written (a full disk), the hub stays stopped until the next start of the updater or the next deploy |
| two calls at once | one after the other; the second finds its release running (`unchanged`) or older (`refused`) |
| the server restarts (it reboots by itself for updates) | the updater starts at boot and starts the hub; a call that finds no connection is tried again for three minutes, then the run fails without having changed anything |
| a new updater is on trial, or another deploy runs | `busy`; the run tries again for three minutes |
| a new updater does not come up | systemd starts it once more, the pre-start script puts the previous updater back, and that one says so in its answers |

**Nothing here runs as root.** The hub runs as the user `trommi`, the updater as the user `trommi-updater`; neither
can log in. The updater owns one folder, `/srv/trommi/deploy`, and can write nowhere else. What it decides is which
program the hub's unit runs; the unit itself is fixed on the server (`trommi-hub.service`, installed by
`hub/deploy/install.sh`, no part of a release) and runs that program as `trommi` inside its limits. So a release
that passed the signature check, or an updater that was taken over, gets what the hub has (its data, its push
credentials), can stop the hub and can refuse further updates, and does not get root or anything else on the
machine. Three things remain root's and are named here so that the claim can be checked:

- `hub/deploy/hub-ctl.sh` behind `trommi-hub-ctl.socket`: the updater cannot start or stop a unit, so it asks this
  helper through a socket only its user can open. The helper takes one word: `start` or `stop` (the hub's unit),
  `dryrun` or `restore` (two fixed oneshot units that run as `trommi`, below); nothing else, and nothing the caller
  sends reaches a command.
- `hub/deploy/install.sh`, run by the owner over SSH: users, units, scripts, settings, the pinned key.
- systemd, which starts the units as their users.

The hub's data is not the updater's to read either: the copy of the database before another release starts is
made by the hub's unit (`hub/deploy/hub-prestart.sh`, as `trommi`, while the hub stands still, so `hub.db` and its
`-wal` and `-shm` files are one consistent state). The rest rests on the machine: on users being kept apart by the
kernel, and on tailscaled telling any local user who a tailnet address is (`tailscale whois`).

**When a new hub fails.** Three steps keep a bad release away from the data people use.

1. *Dry run before the swap.* The updater points the link `candidate` at the release and asks for `dryrun`:
   `trommi-hub-dryrun.service` (as `trommi`; no network at all, `PrivateNetwork`; the live data only to read; no push
   credentials) runs `hub/deploy/hub-dryrun.sh`, which runs `trommi-hub dry-run` of the new release
   (`hub/src/dry_run.rs`): a consistent copy of the database while the old hub goes on writing (`VACUUM INTO` over a
   read-only connection) and of the keys, the new hub started on it (`HUB_DRY_RUN=1`: no push, no jobs, no admin page)
   on 127.0.0.1, `/healthz` well and as its commit, a `/v1/` route without a server error, then SQLite's
   `quick_check` and `foreign_key_check`, the schema version and every table read on the migrated copy. Only then the
   swap. A server whose helper is older than this skips the step and says `dry_run: skipped` in its answer.
2. *Data back only before the first request.* The hub writes the mark `hub.served` beside its database before it
   works on its first request (anything but `/healthz` and a preflight; the admin page too), and `/healthz` says
   `"served"`. `hub-prestart.sh` removes the mark when a newer release begins and notes the copy it made in
   `backups/last-copy`. When the new hub does not become well, the updater stops it and asks for `restore`
   (unless the hub already said it served): `trommi-hub-restore.service` runs `hub/deploy/hub-restore.sh`, which puts
   back database, keys and the uploaded files from that copy, but only when the copy is the one made right before the
   release in place started and there is no mark. What it replaces goes to `backups/after-<release>-<time>/` (three
   kept). After the first request only the program goes back, never the data.
3. *Migrations: expand, then contract.* A hub release must start on a database its successor has migrated, because
   that is what a program rollback starts. So a migration only adds (tables, columns with defaults, indexes) and the
   code that stops using something ships first; dropping or reshaping what the hub before reads comes one release
   later, once no hub that needs it can come back. CI checks it on every hub build (`.github/scripts/hub_compat.sh`
   in the main hub build): every hub that may be put back (the one that runs, the newest released) makes a database,
   this build's hub migrates it, and the hub before must start on it and answer `/healthz`. Increasing `hub/data-epoch`
   does not skip this check: container rollback preserves current data. `SCHEMA_VERSION` in `hub/src/db.rs` rejects
   unknown layouts; changing its number alone cannot make rollback compatible. Historical binary updaters also
   use the signed `data_epoch` to prevent going back across an incompatible epoch after serving, but our container
   deployment requires predecessor compatibility before publishing regardless of that number.

**The updater updates itself.** Every release brings the updater too. When it differs from the one that runs, the
deploy puts it in place beside the old one, answers its caller, and ends; systemd starts the new one, which has to
say that it is up. If it ends instead, or does not say so in time, the previous one is put back
(`hub/deploy/updater-prestart.sh`) and reports which release's updater did not come up. The pinned key, the
settings in `/etc/trommi` and the version floor are no part of a release and stay. The units, the helper and the
pre-start scripts are installed once and are the fixed point; a change to one of them is a run of `install.sh`.

**On the server.**

```
/srv/trommi/                     root's; nothing is written here after the installation
/srv/trommi/deploy/              trommi-updater's:
    releases/v<N>/                   the files of a release as they were proved
    current, previous                links: the hub that runs, the one before
    updater, updater-previous        the same for the updater
    state.json                       the highest version ever accepted
    candidate                        a link to the release being tried, only during its dry run
/srv/trommi/data/                trommi's: the database (hub.db), the hub's own keys (vapid.key, push-ticket.key), files/
/srv/trommi/backups/             trommi's: the three newest copies of database, keys and uploaded files (hard links), one
                                 before each newer release starts (before-<release>-<time>/); last-release, last-copy;
                                 after-<release>-<time>/: what a restore replaced (three kept)
/var/cache/trommi-hub-dryrun/    trommi's: the copy a dry run works on, removed after it
/etc/trommi/                     root's: release-public-key.pem (pinned), hub.env, hub-secrets.env, apns-key.p8, updater.env
/etc/systemd/system/             trommi-hub.service, trommi-hub-updater.service, trommi-hub-ctl.socket, trommi-hub-ctl@.service,
                                 trommi-hub-dryrun.service, trommi-hub-restore.service
/usr/local/lib/trommi/           hub-ctl.sh, hub-prestart.sh, updater-prestart.sh, hub-dryrun.sh, hub-restore.sh
/etc/systemd/journald.conf.d/    trommi.conf: the journal keeps 30 days (MaxRetentionSec=30day), at most 500 MB
```

**Logs.** Hub and updater write one JSON line per event to the journal (`journalctl -u trommi-hub`,
`-u trommi-hub-updater`). No line names a person: no e-mail address, no IP address of a caller or a client, no
token, no user agent; ids are cut short, the rest are counts. The updater logs the tags allowed to deploy and only
how many owner logins are (`caller_users` is a number). `install.sh` sets the journal to keep 30 days and at most
500 MB (the drop-in above, then `systemctl restart systemd-journald`), so nothing older is on the server.

Which hub and which updater run: `ssh root@<server> trommi-hub-updater status`, and the summary of every
`ship-hub` run. A backup of the server must hold `/srv/trommi/data` whole (the two key files with the database:
without `vapid.key` every web push subscription is lost) and `/etc/trommi`.

Rules that follow from this: a hub release must be able to run on the database of the release before it and leave
it usable for that one, because that is what a rollback starts (expand, then contract, above; or a raised
`hub/data-epoch`); renaming `ship-hub.yml` restarts the run number, so
`RELEASE_BASE` is raised then; HSTS is off until `HUB_HSTS=on` is set in `/etc/trommi/hub.env`.

**The installation** is `hub/deploy/install.sh` on the owner's laptop: `inventory` reads the server and changes
nothing; `install` puts the users, the fixed units and scripts, the pinned key, the journal's 30 days and a first
release in place (run
again, it renews the fixed parts; on a server whose updater still runs as root it converts the installation in
place and moves everything back if a step fails); `secrets` sends the push credentials from the 1Password
Environment. Until the server is installed, `ship-hub` publishes each release and then fails saying that nothing
answers on the server's deploy port.
