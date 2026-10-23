# Security

How Trommi protects your content, its releases, its servers and this repository. The hub is the server that
stores and relays encrypted data; the web app is delivered separately, by Cloudflare at app.trommi.com.

## App / E2E
- Encryption is MLS (RFC 9420, with OpenMLS `=0.9.1`), in one Rust core shared by web, iOS, connector and hub.
- One fixed set of algorithms, never negotiated: `MLS_128_DHKEMX25519_CHACHA20POLY1305_SHA256_Ed25519`.
- Everything is encrypted on your device. The hub stores only encrypted data and never has the keys to read it.
- The hub does see: your e-mail, which devices are in which group, when something is written, padded sizes, push
  tokens, and of every card, Note, permission request and Artifact its kind, its state (open, answered, closed) and
  one knock bit (urgent or not). The urgency level stays in the encrypted body, and new headers carry no separate
  answer time; the hub still sees when each answer is written, as it sees when anything is written. Headers written
  before 11 October 2026 keep the level and the answer time they had.
- Sizes are padded: stored messages, MLS messages and new files are rounded up to fixed buckets (a file to 4 KiB at
  least, and above that at most about 12 % larger), so a size tells little of the content. Files from before
  11 October 2026 keep their exact size.
- The hub records when a device last signed in or opened or closed its stream (persistent times coalesced to a minute).
  Settings → Devices shows offline contact times. These are hub activity hints, never evidence for membership or removal.
- Settings → Data shows you what the hub keeps of your account: counts and sizes only, to your own devices only.
- Your password never leaves your device; the hub keeps only a slow hash of a key derived from it.
- Each wrong password makes that guesser wait longer (up to 15 minutes); no account is ever locked.
- A recovery code opens everything. The hub keeps it only encrypted under your password or Emergency Kit words.
- Logging in with your password checks the room from a state authenticated by your recovery code before the
  device joins. When it needs earlier member history, it checks that history from the start against its held
  state before using it; new membership changes wait for that check too. It reads a conversation's past only
  after checking that conversation from its start, or once another of your devices adds it, as for any new device.
- Nobody, not even we, can reset an account: without password, Emergency Kit and devices, the content is gone.
- An invite link holds a secret the hub never sees, so the hub cannot slip in an invite of its own.
- Invite links work once and expire: 10 minutes for a device, 15 for an agent.
- A new device gets in only after you compare six emoji on both screens; this stops the hub adding its own.
- A computer can be approved once with an invite and the six emoji. The human app records approval in an
  authenticated room register bound to that exact agent device, never in agent-writable metadata. A new agent
  session stays off the board until explicitly activated with `connect`; startup and listing or calling other
  tools do not activate it. An approved computer can then request up to 8 live sessions without another tap,
  with at most 24 admitted sessions per room and 8 open activation links per human app. An online human app
  opens the invite and confirms only the Request whose hash the computer passed end-to-end; the hub can neither
  make nor swap that Request. Each activation gets a separate agent device and session, with no access to other
  sessions' history. Any program running as that OS user can use the approved computer key to request sessions;
  approval is for that computer user, across program accounts and config folders. Removing the computer removes
  its admitted sessions too, with their helpers' cards and any helper session of before; a session taken over by
  another device does not inherit its approval or removal relationship. Missing registers or the hub's
  online/device list never prove a computer's removal: automatic cleanup requires verified room membership.
  Existing ordinary agents gain no computer authority.
- Signing out erases the device key, private cache and extension keys immediately. A public self-retirement
  notice, signed by that exact device for that room, stays durably queued without any bearer or content key
  until the hub acknowledges it. The next online human app checks the signature and verified human membership
  and commits the normal removal; the hub cannot forge that authority. Offline logout retries after a restart.
  The hub briefly holds bounded requests for an own join still in flight, so cancelling an unfinished sign-in
  does not leave an unremovable unnamed key. Activity times and missing profiles never authorize cleanup.
- A removed device loses access at once.
- A device deletes its data only after checking the signed removal itself, never on the hub's word alone.
- A new device starts from the hub's list of what is open and reads what is new. Older entries are fetched when
  you open them. The hub can leave a card out or show an older version of one; it cannot read, make up or alter
  one.
- A room holds at most 1000 devices.
- Files are encrypted on the device. Push notifications carry no content.
- The web app keeps pictures it showed in the browser still encrypted, as the hub stores them (at most 96 MB);
  signing out deletes them with the rest of the account's private browser data.
- What push reveals: Apple and the browser's push service learn that something happened for a device, when, and how
  often. Every push looks the same to them: one text ("Something new."), the same clear headers (Web Push: urgency
  normal and a topic that is random per subscription and names no room; on the iPhone a sealed part of one length).
  Room, change number and urgency travel encrypted to the device, which picks the text it shows. The Live Activity
  tells Apple two marks, whether an agent works and whether something waits (0 or 1 each), and when the counts
  change; the counts themselves travel sealed for the iPhone, never a title.
- With a push the iPhone's notification extension reads ahead what the app will catch up on: the hub's encrypted
  pages, fetched straight from the hub (not through Apple) with the push's one-day ticket, kept in the app's shared
  container until the app opens. The ticket travels sealed in the push and then in a request header, never in a
  URL; while it lasts it reads the room's encrypted changes up to that push as the device itself may, and nothing
  newer. The app checks those pages like any other.
- What the iPhone's notification extension holds, to show the title of the question that rang: a read-only copy of
  one content key per session (the key of its current period as the app last saw it, never the room's own key),
  which the app writes again whenever a key changes. The copy is one Keychain item in an access group that only the
  app and this extension are entitled to (not the Live Activity widget, not the Share Extension), readable after the
  phone's first unlock and never leaving this device. With it the extension can read everything written in those
  sessions during the current period, not only the card that rang. It opens the one message of the push and shows
  a card's title, cut to 120 characters, one line, without invisible or direction-changing characters. Only someone
  holding that session's current key can have written it, so the hub cannot make up a title; but the extension does
  not check which member wrote it or where it stands in its sender's chain (the sender's key is named in the message
  itself), and the hub picks which message a push points to: it can show the title of another card of the room's
  sessions from the current period, an earlier one again included. The app checks everything when it opens. The
  extension writes and signs nothing and holds no device key. The copy goes when you sign out, when the device is
  removed, when notifications are off on this phone, and when a new copy cannot be written; until the app runs after
  a key change, the copy still holds the key of the period before. Without the copy, before the first unlock or
  after a key change the app has not written yet, the notification shows the plain text ("A new question.").
- Team knowledge: a team's entries are end-to-end encrypted in the room group, which only your devices read. Your
  devices copy each team's entries into the sessions of that team, so every agent of a team reads that team's
  knowledge, and nothing of another team's or of the room. Agents write team knowledge directly, without asking you
  (you agreed to this on 18 October 2026): an agent cannot write the room group, so it puts its write into its own
  session, and your devices apply it on their own, only to the team that agent is on, a change or a removal only
  while the entry is as the agent read it. Every change keeps who made it, when, and the text before; the Knowledge
  page marks what agents changed since you last looked, undoes any change, and brings a forgotten entry back for 30
  days. Only you pin. The hub sees nothing new: these are encrypted registers like the others (who wrote one, when,
  its padded size).
- Wakeups (you agreed to this on 20 October 2026): an agent may set itself a wakeup, which its connector keeps in
  the agent's own session, end-to-end encrypted like any register; the hub sees nothing new. Only that agent's
  session is ever woken, on the computer it runs on, and only within fixed limits (5 open, 2 that repeat, 12 runs a
  day, none from 22:00 to 07:00, a run at most 30 minutes, and in Claude Code at most 30 turns; Codex has no turn
  limit to set). You see every wakeup and can cancel or move
  it. When the session is closed, the connector leaves a note of its wakeups (with their notes) next to the device
  key on that computer, readable only by your user, and a small waker process that starts the agent's program
  again on that conversation. A woken run has the rights the program's own settings give it, no more: Claude Code
  asks you for each permission on a yes/no card (no answer is no); Codex runs as its own settings say. The woken
  run takes the wakeup from the board as it stands then, so a cancel made meanwhile holds.
- While a chat agent (Hermes) writes its answer, your devices show it growing: once a second while it answers, its
  connector sends the answer so far, end-to-end encrypted in the session's group, relayed by the hub to your devices
  and never stored. Every preview has the same padded size and comes at the same pace whether the words changed or
  not, so the hub learns nothing of the answer's length or wording. What it does learn: when the agent's answering
  starts and ends, to the second (its lease report already tells it when the agent works), and that this agent
  streams its answers. Only that session's agent device may send one; your devices drop any other.
- Hermes Agent joins through the Trommi add-on, a Hermes platform like Telegram that the signed connector carries and
  writes into the Hermes home (a connector of a newer release writes it again, whole, when it starts). The add-on runs inside Hermes and starts the
  connector once per Trommi chat, over pipes of this computer. Each chat is a session with its own device key, held
  by its connector. The add-on holds no key, and nothing new reaches the hub. All chats of one Hermes are one agent:
  a Hermes conversation can use the Trommi tools of its other chats, and the tools of its conversations outside
  Trommi (Telegram, cron) act in its main chat. Other agents stay apart. "New chat with …" in the app starts a session
  on that computer without anyone running connect there: only on a computer you approved, only from your own devices
  (the ask is signed and end-to-end encrypted, the hub can neither read nor forge it), each ask once, and within the
  8 sessions an approved computer may hold. What Hermes asks to approve comes to you as a yes/no card for exactly that
  request: a yes allows only the one request waiting with that command, and nothing when it cannot be told which one
  it is. No answer is no.
- An agent's plan, account e-mail and usage reach the board end-to-end encrypted, in its session's profile; they
  are never in the hub's metadata or in a log. The connector reads them through the programs' own commands and
  Claude Code's status line (put around one already there), never from their credential files.
- No forward secrecy for history, on purpose: a new device of yours reads everything, and so does anyone who
  holds one of your devices or the recovery code.

## Dictation (sealed)
- Dictation turns speech into text for the chat field without the hub, Cloudflare or anyone else hearing it; only a
  Tinfoil enclave (a confidential-computing machine) does, after your device checked what code it runs.
- What leaves your device: the recording (at most five minutes, recorded first, then sent), sealed end-to-end with
  HPKE to the enclave's key (EHBP), in one request through the hub. The recording stays in memory and is dropped
  when the dictation ends; it is never stored or logged.
- Before anything is sent your device checks the enclave's attestation itself, with Tinfoil's own verifier: AMD's
  hardware report and its certificate chain, and that the measured code is Tinfoil's published build in Sigstore's
  transparency log. It seals only to the key that check yields. If the check fails, nothing is sent.
- What the hub sees: that your account dictates, when, the sealed size, the enclave's address and the model's name.
  It holds the one Tinfoil API key (no device does), adds it, and passes nothing else of your request on. No audio
  and no words reach it, and it logs no body (only the size rounded to 64 KiB, the time taken and the status).
- What Tinfoil sees outside the enclave: the same headers, sizes and times, the hub's address and the hub's key; not
  your account, e-mail or address (the hub calls it, for every account with the same key). Inside the enclave: the
  audio and the words while it transcribes, as its attested code handles them; what you say can of course name you.
- Cloudflare sees what it sees of every hub request (see Deployment): your address, the sign-in token, the sizes and
  times; the audio and the words only sealed.
- Limits: ten dictations a minute and 200 a day per account; a hub without a Tinfoil key offers no dictation.
- Settings → Security shows the enclave, the model, the attestation and the release's digest with its entry in
  Sigstore's log, and a test that checks the enclave anew and sends one second of silence sealed there and back.
- Settings has a switch per device; off, nothing is recorded or checked. Opening Settings checks the enclave (the
  hub sees that request, as it sees a dictation) and shows "Connected · sealed" only when the check passed.
- The trust this rests on: AMD's hardware and keys, Tinfoil's published builds and Sigstore's log, and the verifier
  your device runs (in the browser loaded from app.trommi.com, like the rest of the web app; see Known limits).

## Signing
- The hub and the connector each have their own signed release series (`hub-v<N>`, `connector-v<N>`): a list of the
  part's files with their hashes, signed with Ed25519, one key for both. A part is released only when its own checks
  and the core's checks passed for that commit.
- Public key: `release/public-key.pem`, also pinned on the hub server. The private key stays in 1Password.
- Our hub image has a separate standard ECDSA P-256 Cosign key, pinned in `release/hub-cosign-public.pem` and
  independently on the server. The binary manifest still uses the existing Ed25519 key and includes that public
  container key as an asset. The image private key stays in 1Password as `SIGN_IMAGE_KEY`; CI imports it with a random
  runtime password into an ephemeral Cosign key. The server holds only its public key. YoloDeploy verifies Cosign and
  the digest before replacing the app, then runs the exact verified local image ID. Normal updates refuse older
  signed release-sequence labels and changed app images with the same sequence; health-failure recovery may
  return to the actual previous image. CI promotes the deployment tag only after checks and signature verification
  and refuses promotion past a newer published release. Legacy binary updaters keep their signed manifest checks.
- The connector's `install.sh` and `trommi-connector update` check the same signature, with the key built in.
- Release files and web app files carry GitHub build attestations: a public record of which build made them.
- The web app is delivered as soon as its own checks and the core's checks passed; after each delivery CI checks from
  outside that app.trommi.com serves exactly that build, and puts the build before back when it does not.
- Each web delivery is also published as a release `web-v<N>` (web.tar.gz, signed and attested like the others) as
  the record of what went out; browsers do not check it.
- The iOS app is signed by Apple and ships through TestFlight.
- The Mac app for direct download (`Trommi-macOS.dmg`, `main.yml`) is signed with the team's Developer ID certificate
  with the hardened runtime, notarized by Apple and stapled; macOS checks that before its first start. It is built when
  the core's checks passed for its commit and the app's inputs differ from those of the newest Mac app, and never
  without the certificate and notarization (the job is skipped instead); it is published as its own release
  `mac-v<N>` with a GitHub build attestation (macOS checks Apple's signature and notarization itself; the .dmg is not
  in a manifest signed with the release key). Only the job that builds it sees the certificate, in an environment of
  its own (`mac`).
- iOS uses only standard algorithms and is declared exempt from export paperwork.

## Deployment
- Secrets live in 1Password and are read only by the step that needs them.
- Pull requests get no secrets and deploy nothing.
- CI never logs into the hub server. It publishes the checked signed image and sends one empty HTTPS POST to
  YoloDeploy through Cloudflare Access with an app-specific token; that token chooses a fixed server-owned target,
  never a command, repository or path from the request. CI then verifies public health and the expected commit.
- YoloDeploy pulls and verifies the image signature before replacing the hub and waits for its Docker healthcheck.
  Failed startup/health returns to the previous image on the current data; no data is restored or discarded by
  deployment. CI checks that the actual released predecessors still start on data the new binary migrated, even
  across declared data-epoch changes. Migrations must remain compatible with the previous image. A killed deploy
  or power failure requires inspecting the recovery journal manually.
- A separate nightly job takes a consistent private copy of hub data/configuration during a short pause, under
  the same operation lock as deployment. It resumes that container, normalizes the copied SQLite DB/WAL through
  SQLite's backup API, checks and syncs a private archive and checksum, and retains its archives for 14 days.
  Its interruption marker and service cleanup resume only the container it paused. No production restore test is
  deliberately broken: isolated fixtures verify WAL/data restoration and image-only rollback preserving writes.
- A failed hub or web delivery opens a GitHub issue; the next good one closes it.
- After each web delivery CI asks app.trommi.com from outside whether it serves exactly that build (the start page and
  every file it loads, by SHA-256, with its security headers); when not, the version before is put back at once.
- The hub runs as a non-root container with a read-only root filesystem, dropped capabilities, no Docker socket
  and no published host ports. Its data is writable; configuration is mounted read-only. Cloudflared reaches its
  port 8080 on the private Docker network. YoloDeploy has the Docker socket and therefore broad host rights; the
  old separate systemd hub/updater services are disabled on our hosting. Other fixed targets are not controlled by
  the Trommi request token. Cloudflare service credentials are shared across apps, so Access is not app isolation.
- Public traffic reaches the hub only through a Cloudflare tunnel. Cloudflare ends TLS for the hub, the web app and
  the website: in transit it sees what the hub sees, including sign-in data (your e-mail and the login key derived
  from your password, which signs in but opens no content), tokens and the metadata above. Content stays
  end-to-end encrypted.
- CI deploys the web app (app.trommi.com) and the website (trommi.com) to Cloudflare with a token from 1Password.
- The web app and the website tell browsers to use only HTTPS, for a year, for their name and every name under it
  (HSTS, no preload); the website's covers hub.trommi.com too. The hub sends the same on every answer once
  `HUB_HSTS=on` is set on its server.
- The web app and the website keep their windows and files to themselves: no other site's window keeps a handle on
  them (COOP), and no other site may embed their files (CORP). Both have a strict Content-Security-Policy.
- Local recovery archives have no off-server backup configured. UFW is inactive. The tunnel token and the Trommi
  deploy token exposed during setup need rotation by the owner.
- Server logs are kept 30 days and contain no personal data (no e-mail or IP addresses, no tokens).

## Repository
- How a change reaches `main`:
  1. Any feature branch: no rules, anything goes.
  2. A frozen `release/<date>-<n>` branch: one finished change, or a bundle per topic.
  3. Its pull request into `main`: CI checks it (`pr-check.yml`), and it merges by auto-merge with "Squash and merge"
     once all required checks are green. Builds, TestFlight and deliveries come only from `main`.
- A pull request runs quick checks only (clippy, every test but the slow ones, the web app's build, the apps
  compiled), without secrets. One `main.yml` workflow checks each affected part once and the shared core once,
  then each part independently builds, signs, publishes and delivers; a part that fails opens an issue and is not
  delivered, while the other passing parts still can be. Signing jobs that hold the release key execute no built
  app or connector. Artifact names alone are not a release gate: the actual producing job must have passed and
  the bytes must match the digest exported by that job, including across retries. The iPhone and Mac apps go to
  TestFlight only after their checks and the core's passed for that commit and their inputs differ from those
  last delivered there. The Mac download does not wait for TestFlight processing. Manual recovery still verifies
  the original main build and its checks; the App Store gate remains manual. The slowest tests, fuzzing and the
  torture run with 100 iPhones run every night (`nightly.yml`); a failing night opens an issue.
- `main` is protected: only signed commits, linear history, no force-push, no deletion, changes only through pull requests.
  The squash merge is signed by GitHub, so every commit on `main` is verified. The history before 11 October 2026 was re-signed once by the owner.
- GitHub secret scanning is on and blocks pushes that contain secrets.
- Only `main` can deploy: the environments of the web app, the website, the hub, the connector, the iOS and the Mac
  app allow the branch `main` and nothing else.
- Workflows start with no permissions, outside actions are pinned to exact commits, and the 1Password CLI they
  install to an exact version.
- Toolchain and dependency versions are pinned (lockfiles).
- Rust dependencies are checked daily against RustSec, the list of known vulnerabilities.

## Known limits and planned
- Cloudflare terminates TLS (see Deployment): whoever controls our Cloudflare account could read sign-in data and
  tokens in transit and sign in as you, but not read content. Signing in hands out the sealed recovery code, which
  then rests only on your password's slow key derivation.
- The connector checks signatures when it installs or updates, not each time it starts: whoever can write to
  your home folder could replace it.
- In the browser, the app's code is loaded from Cloudflare (app.trommi.com) each time; whoever controls that
  delivery (our Cloudflare account or the deploy pipeline) could ship code that reads your keys. The hub cannot.
  The iOS app and the connector do not have this risk: they are installed signed releases.
- Container rollback keeps current data. It cannot make an incompatible migration reversible: the predecessor
  binary must keep working, and the migration gate refuses a release when it does not. Backups are separate.
- Rulesets (signed commits, no force push, protected tags): being turned on.
- iOS: account recovery and replacing the recovery code are tested only against a stand-in hub, not a real one.
- Dictation is as private as Tinfoil's attestation holds: AMD's hardware and keys, Tinfoil's published builds and
  Sigstore's log; in the browser the verifier comes with the app's code (above). On the iPhone the same verifier
  runs inside the signed app, in a hidden web view that loads nothing from the network.
- Passkeys: built, but off, because they need the PRF extension, which some password managers lack.
- Planned: post-quantum encryption once OpenMLS supports it.
- Planned: deleted content really gone, also on devices and in backups.

## Possible future hardening
Ideas, not promises:
- Discarding the recovery code: you could choose to have no recovery code at all. Whoever later gets hold of it (or
  of its encrypted copy and your password) could then no longer load your history. The price: lose all your devices
  and the history is gone for good.

Report a vulnerability: trommi@mail101.de
