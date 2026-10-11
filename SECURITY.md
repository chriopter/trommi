# Security

How Trommi protects your content, its releases, its servers and this repository. The hub is the server that
stores and relays encrypted data; the web app is delivered separately, by Cloudflare at app.trommi.com.

## App / E2E
- Encryption is MLS (RFC 9420, with OpenMLS `=0.9.1`), in one Rust core shared by web, iOS, connector and hub.
- One fixed set of algorithms, never negotiated: `MLS_128_DHKEMX25519_CHACHA20POLY1305_SHA256_Ed25519`.
- Everything is encrypted on your device. The hub stores only encrypted data and never has the keys to read it.
- The hub does see: your e-mail, which devices are in which group, when something is written, padded sizes, push
  tokens, and of every card, Note, permission request and Artifact its kind, its state (open, answered, closed) and
  one knock bit (urgent or not). The urgency level and when you answered stay in the encrypted body.
- Sizes are padded: stored messages, MLS messages and new files are rounded up to fixed buckets (files at most
  about 12 % larger), so a size tells little of the content. Files from before 11 October 2026 keep their exact size.
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
- A removed device loses access at once.
- A device deletes its data only after checking the signed removal itself, never on the hub's word alone.
- A room holds at most 1000 devices.
- Files are encrypted on the device. Push notifications carry no content.
- What push reveals: Apple and the browser's push service learn that something happened for a device, when, and how
  often. Every push looks the same to them: one text ("Something new."), the same clear headers (Web Push: urgency
  normal and a topic that is random per subscription and names no room). Room, change number and urgency travel
  encrypted to the device, which picks the text it shows. The Live Activity tells Apple two marks, whether an
  agent works and whether something waits (0 or 1 each), never how many and never a title.
- An agent's plan, account e-mail and usage reach the board end-to-end encrypted, in its session's profile; they
  are never in the hub's metadata or in a log. The connector reads them through the programs' own commands and
  Claude Code's status line (put around one already there), never from their credential files.
- No forward secrecy for history, on purpose: a new device of yours reads everything, and so does anyone who
  holds one of your devices or the recovery code.

## Signing
- Every change becomes one release `v<N>`: a list of all files with their hashes, signed with Ed25519.
- Public key: `release/public-key.pem`, also pinned on the hub server. The private key stays in 1Password.
- The hub server installs a release only if its signature checks out, and never one older than it accepted before.
- The connector's `install.sh` and `trommi-connector update` check the same signature, with the key built in.
- Release files and web app files carry GitHub build attestations: a public record of which build made them.
- The iOS app is signed by Apple and ships through TestFlight.
- iOS uses only standard algorithms and is declared exempt from export paperwork.

## Deployment
- Secrets live in 1Password and are read only by the step that needs them.
- Pull requests get no secrets and deploy nothing.
- CI never logs into the hub server and sends it no code, only "take release `v<N>`".
- CI reaches the hub server over a private network (Tailscale) with a short-lived GitHub login, no stored key.
- The hub server downloads and checks the release itself.
- Before the switch, the new hub is tried on a copy of the live data, without network; if that fails, nothing changes.
- If the new hub is not healthy within 60 seconds, the previous release runs again. Its data (database, keys,
  uploaded files) goes back to the copy made just before only if the new hub had not yet served a single request.
- Every hub build checks that the release before still runs on data the new one migrated; a change that cannot
  keep that is marked in the signed release, and then the program is never put back over data the new hub served.
- A failed hub or web delivery opens a GitHub issue; the next good one closes it.
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
  3. Its pull request into `main`: CI tests it, and it merges by auto-merge with "Squash and merge" once all required
     checks are green. Builds, TestFlight and deliveries come only from `main`.
- A pull request runs quick checks only (tests, clippy, the web app's build), without secrets. `main` runs the full
  builds; when one fails, an issue is opened and nothing is released or delivered.
- `main` is protected: only signed commits, linear history, no force-push, no deletion, changes only through pull requests.
  The squash merge is signed by GitHub, so every commit on `main` is verified. The history before 11 October 2026 was re-signed once by the owner.
- GitHub secret scanning is on and blocks pushes that contain secrets.
- Only `main` and `v*` tags can deploy the web app, the website and the hub.
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
