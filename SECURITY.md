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
- Settings → Data shows you what the hub keeps of your account: counts and sizes only, to your own devices only.
- Your password never leaves your device; the hub keeps only a slow hash of a key derived from it.
- Each wrong password makes that guesser wait longer (up to 15 minutes); no account is ever locked.
- A recovery code opens everything. The hub keeps it only encrypted under your password or Emergency Kit words.
- Logging in with your password checks the account's whole member history before the device joins; it reads a
  conversation's past only after checking that conversation from its start, or once another of your devices
  adds it, as for any new device.
- Nobody, not even we, can reset an account: without password, Emergency Kit and devices, the content is gone.
- An invite link holds a secret the hub never sees, so the hub cannot slip in an invite of its own.
- Invite links work once and expire: 10 minutes for a device, 15 for an agent.
- A new device gets in only after you compare six emoji on both screens; this stops the hub adding its own.
- One exception, self-join: for a new Claude Code or Codex session vouched for by a session that is already on the
  board, runs on the same machine as the same OS user and the same client and config folder, one tap ("Allow")
  takes the place of the six emoji. Nothing is opened before that tap. Your app then confirms only the Request whose
  hash the vouching agent passed on end-to-end, and only while that agent's session is still live; the hub can
  neither make nor swap that Request. "Deny" refuses it. The new device is an agent of its own new session: it reads
  that session (and helpers it opens), never another session's history. At most 8 such live sessions per account
  and 24 per room, open links counted in; archiving one removes its device, also when an app finds it archived
  elsewhere. Account, name and host in the prompt are the vouching agent's word: an agent you no longer trust, or a
  program running as that OS user, can ask, and removing an agent leaves the sessions it vouched for; archive those
  too.
- A removed device loses access at once.
- A device deletes its data only after checking the signed removal itself, never on the hub's word alone.
- A new device starts from the hub's list of what is open and reads what is new. Older entries are fetched when
  you open them. The hub can leave a card out or show an older version of one; it cannot read, make up or alter
  one.
- A room holds at most 1000 devices.
- Files are encrypted on the device. Push notifications carry no content.
- The web app keeps pictures it showed in the browser still encrypted, as the hub stores them (at most 96 MB);
  signing out deletes them with the rest of the browser's data.
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
- An agent's plan, account e-mail and usage reach the board end-to-end encrypted, in its session's profile; they
  are never in the hub's metadata or in a log. The connector reads them through the programs' own commands and
  Claude Code's status line (put around one already there), never from their credential files.
- No forward secrecy for history, on purpose: a new device of yours reads everything, and so does anyone who
  holds one of your devices or the recovery code.

## Signing
- The hub and the connector each have their own signed release series (`hub-v<N>`, `connector-v<N>`): a list of the
  part's files with their hashes, signed with Ed25519, one key for both. A part is released only when its own checks
  and the core's checks passed for that commit.
- Public key: `release/public-key.pem`, also pinned on the hub server. The private key stays in 1Password.
- The hub server installs a release only if its signature checks out, and never one older than it accepted before.
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
- CI never logs into the hub server and sends it no code, only "take release `hub-v<N>`".
- CI reaches the hub server over a private network (Tailscale) with a short-lived GitHub login, no stored key.
- The hub server downloads and checks the release itself.
- Before the switch, the new hub is tried on a copy of the live data, without network; if that fails, nothing changes.
- If the new hub is not healthy within 60 seconds, the previous release runs again. Its data (database, keys,
  uploaded files) goes back to the copy made just before only if the new hub had not yet served a single request.
- Every hub build checks that the release before still runs on data the new one migrated; a change that cannot
  keep that is marked in the signed release, and then the program is never put back over data the new hub served.
- A failed hub or web delivery opens a GitHub issue; the next good one closes it.
- After each web delivery CI asks app.trommi.com from outside whether it serves exactly that build (the start page and
  every file it loads, by SHA-256, with its security headers); when not, the version before is put back at once.
- Hub and updater run as their own users (`trommi`, `trommi-updater`), neither with root rights.
- Root runs only a tiny helper (`hub-ctl`) that starts or stops the hub, or its dry run or data restore (two fixed
  units that run as the hub's user), when the updater asks; nothing else.
- The hub runs in a systemd sandbox and listens only on its own machine (`127.0.0.1`).
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
- Going back to the previous hub after the new one served keeps the current data (by design: nothing anyone sent is
  lost); the copy made before the update is only put back when the new hub served nothing.
- Rulesets (signed commits, no force push, protected tags): being turned on.
- iOS: account recovery and replacing the recovery code are tested only against a stand-in hub, not a real one.
- Passkeys: built, but off, because they need the PRF extension, which some password managers lack.
- Planned: post-quantum encryption once OpenMLS supports it.
- Planned: deleted content really gone, also on devices and in backups.

## Possible future hardening
Ideas, not promises:
- Discarding the recovery code: you could choose to have no recovery code at all. Whoever later gets hold of it (or
  of its encrypted copy and your password) could then no longer load your history. The price: lose all your devices
  and the history is gone for good.

Report a vulnerability: trommi@mail101.de
