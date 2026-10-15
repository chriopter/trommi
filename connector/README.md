# trommi-connector

The Trommi connector: one static binary through which a Claude Code session talks to the human's board. It is an
**agent device** of a room on protocol v1 ([`spec/v1.md`](../spec/v1.md)): every key, every check and every byte on
the wire comes from [`trommi-core`](../core/protocol); nothing of the first protocol is in it.

```
trommi-connector                     MCP stdio server "trommi" (what Claude Code starts): the launcher, which runs `serve`
trommi-connector serve               the MCP server itself, as the launcher's child (TROMMI_LAUNCH=0 runs it without one)
trommi-connector connect <link>      join a room with an agent invite link, for scripts (also: join; or TROMMI_INVITE)
trommi-connector say "<text>" [--session <name>] [--urgent]
trommi-connector permission|notice|denied|resolved    the plugin's hooks (JSON on stdin)
trommi-connector prompt|stop         the terminal mirror's hooks (UserPromptSubmit, Stop)
trommi-connector trail               the work trail's hooks (PreToolUse, PostToolUse, …)
trommi-connector monitor             the plugin's monitor: one line per board event
trommi-connector setup claude|codex  make the installed connector known to Claude Code (plugin) or Codex (MCP server)
trommi-connector remove claude       take out what setup claude put in; the old status line stands there again
trommi-connector remove codex        take the connector's hooks and MCP server out of Codex again
trommi-connector update [--check]    install the newest signed release in place of the installed one
trommi-connector whoami | claim-key | allow-tools | --version
```

## Install

Once per machine, in a terminal:

```
curl -fsSL https://raw.githubusercontent.com/chriopter/trommi/main/install.sh | sh
```

Then once per project folder, inside Claude Code (or Codex) started there, the line "Invite an agent" shows:

```
Connect to Trommi with this invite link: '<invite link>'
```

or, in Claude Code, the plugin's command `/trommi:connect '<invite link>'`.

`setup claude` runs every `claude plugin …` command in its own environment, so with `CLAUDE_CONFIG_DIR` set (one of
several Claude Code accounts) the plugin goes into that account. Claude Code keeps a copy of the plugin per version
and `claude plugin update` copies nothing while the version stays the same; `setup claude` therefore compares the
copy with the marketplace's plugin folder and installs it afresh when a file is missing or differs (an older copy
without `commands/connect.md` has no `/trommi:connect`), and turns the plugin on when it is off.

A session whose folder is in no room yet starts unconnected: its MCP server offers one tool, `connect(link)`, and
the pasted prompt, or the plugin's command `/trommi:connect`, makes the agent call it. The tool runs the same join as
`trommi-connector connect` for this folder (an expired link is refused before a hub is asked, the Offer's MAC is
checked, …) and answers with the six emoji and their words, which the agent shows in the chat. Once the human taps
"They match" in the app, the same process goes online and announces its board tools (`notifications/tools/list_changed`);
no restart. In Codex the human asks the agent to connect with the link; Codex never asks for the tool list
again, so it is given every board tool beside `connect` from the start (a call before the join says how to
connect). `trommi-connector connect '<link>'` in a terminal does the same for scripts.

A folder's keys of an earlier format (a room folder whose name is no room id of this protocol, or whose key file is
not one of it) are ignored with one log line. With keys of several rooms, the room the folder joined last decides
(`<folder>/.trommi/room`, written by every join); without it the tool error names the rooms.

`install.sh` (repository root, POSIX sh, curl and OpenSSL 3) finds the newest release that holds `manifest.json` and
the connector for this machine, checks the manifest's Ed25519 signature against its own copy of
`release/public-key.pem`, then product, repository, tag, version (never older than the installed one), size and
SHA-256, and only then puts the program into `~/.local/share/trommi/bin/` with a link in `~/.local/bin/`. It ends by
running `trommi-connector --version`, which must say "signature verified", and prints the version and the
manifest's SHA-256. It refuses root and only talks https to github.com and GitHub's release store. Last it runs
`trommi-connector setup claude` and `setup codex` for those of the two that are on the PATH (a setup that fails
leaves the connector installed and says how to run it later); with neither, one line says how.

`setup claude` adds the marketplace of this repository (`.claude-plugin/marketplace.json`, sparse checkout) and
installs the plugin `trommi@trommi` for the user. The plugin (`plugin/`) holds no program: its MCP server, channel,
hooks and monitor name `${HOME}/.local/share/trommi/bin/trommi-connector`, so `trommi-connector update` needs no new
plugin. `setup codex` registers the same program as Codex's MCP server `trommi` (with `TROMMI_CHANNEL_EVENTS=off`:
Codex shows no channel events, board events wait for the `inbox` tool). Both can be run again and say what they
changed. A Codex that was running loads the server only when it is started again; setup, install.sh and the
invite page say so.

`setup claude` also puts `trommi-connector statusline` in as Claude Code's status line (the user settings'
`statusLine`, every other byte as it was), around one already there, which keeps showing as before. It hands the
plan's 5-hour and weekly usage to the board ([`src/usage.rs`](src/usage.rs); the main README, "Account and usage per
agent"). The installed connector does the same once at its start. `trommi-connector remove claude` puts the old
status line back and removes the plugin and the marketplace.

`setup codex` also merges the connector's hooks into Codex's `hooks.json` (`$CODEX_HOME`, default `~/.codex`): a
Codex plugin cannot bring hooks. They are the plugin's, as far as Codex has the events: `UserPromptSubmit` and
`Stop` (the terminal mirror), `PermissionRequest` (permission prompts on the board), `PreToolUse`, `PostToolUse`,
`SubagentStart`, `SubagentStop`, `SessionEnd` (the work trail). Every other hook in that file stays as it is, the
connector's are put in once, and `remove codex` takes them out again (and `codex mcp remove trommi`). **Codex runs
a new or changed hook only once the human trusted it**: at its next start it shows "New hook - review required";
the human trusts them there (or later in `/hooks`, "trust all"). Codex notes that as `hooks.state."<file>:<event>:…"
.trusted_hash` in its `config.toml`. Until then the hooks do nothing, and the connector tells the agent that the
human does not see the terminal.

## Codex

Codex (known by its `initialize`, `clientInfo.name` "codex-mcp-client", or by the program that started the
connector; `src/host.rs`) differs from Claude Code in four ways, and the connector meets each:

- It shows no channel events. Every tool call carries the thread's id in `_meta` (`threadId`); the connector keeps
  it in the slot (`codex_thread`) and, when a board event comes, runs `codex queue --thread <id> --message
  "[Trommi] … call the trommi inbox tool …"` (once until the agent reads the board; `TROMMI_CODEX_WAKE=off` turns
  it off, `TROMMI_CODEX_BIN` names the program; the queue goes to the home of the Codex that runs the session:
  `CODEX_HOME` as `setup codex` registered it, else the Codex process's own, since Codex passes few variables to
  its MCP servers). That starts a turn in an idle session, or the next one after a
  running turn; the agent reads the event with `inbox`. So a Codex session hears at once (its lease reports
  `hears: true`), also after a restart (`codex resume`) or a swapped-in connector, before its first call. A
  woken turn that never called `inbox` does not leave the session deaf: the next event wakes it again once
  `TROMMI_CODEX_REWAKE_MS` (3 minutes) passed since the last wake.
- It never asks for the tool list again: an unconnected session lists every tool with `connect`.
- It asks for approval of an MCP call unless the tool says it is harmless: every tool carries `annotations`
  (`readOnlyHint`, `destructiveHint`, `openWorldHint`); only `revoke_asset` and `share_asset` still ask.
- Its hooks come from `setup codex`, not from a plugin (above). The connector reads at its start whether Codex's
  `hooks.json` holds its Stop and UserPromptSubmit hooks and `config.toml` trusts them; only then do the
  instructions say that the final text reaches the board by itself. Codex's hook input names the turn (`turn_id`,
  used where Claude Code has `prompt_id`) and the thread (`session_id`): a connector takes only its own thread's
  hooks, and the queued wake ("[Trommi] N new board events …") counts as the board's, not as the human's typing,
  so a turn a card's answer started puts its final text under that card.
- Its words differ: the instructions take "# In Codex" from `prompt.md` (inbox at every turn, never run the
  connector in a shell, restart Codex when the tools are missing), and every restart or update line says to quit
  Codex and start it again instead of /mcp.

`tests/connector/codex.rs` drives the connector as Codex 0.160 does (its `initialize`, `_meta`, no new tool
list, a `codex` script that notes the queued wakes). `tests/connector/codex_real.rs` runs a real Codex (its app server,
one session over many turns) against a local hub, opt-in: `cargo test -p trommi-tests --test connector_codex_real
-- --ignored --nocapture` (`CODEX_MODEL`, `TROMMI_CODEX_RUNS`).

## Helpers and their events

The coordinator (the agent that opened the child sessions) reads every board event itself and forwards each to the
helper it belongs to; helpers never call `inbox`. Before a helper starts, the coordinator files its "Working on"
card in the helper's session (`start_task` with `session`) and hands it the card id; the helper revises that card to
"Done: …" at its end.

Optional, for clients that let helpers read their own events: a helper calls `inbox` with `session: "<name>"`: it gets that
session's events alone, and the call claims the session. While the claim holds, the session's events wait for that
helper only: plain `inbox`, the monitor line and channel events skip them, so the coordinator forwards nothing. A
"Done: …" (or Fertig, Erledigt, Resolved, Gelöst) card the helper files there gives the coordinator one short line
(`kind="helper"`). The claim ends with `close_session`, or after 10 minutes without an `inbox(session)` call
(`TROMMI_CLAIM_MS`); events still waiting then go to the coordinator with a line "Design: helper gone, N events
waiting". Claims live in the connector's memory only: after a restart every waiting event is the coordinator's.

A subagent keeps the tool list it was started with: a helper started before a connector update that brought new
tools or parameters (`notifications/tools/list_changed` reaches the coordinator, not a running subagent) still has
the old ones, e.g. an `inbox` without `session`. Its plain `inbox` reads the coordinator's events, never a claimed
session's, so nothing is lost twice; but restart such helpers (end them and start new ones) to give them the new
tools. `inbox` takes parameters it does not know without an error, so a newer helper never fails on an older
connector.

## Status lines follow their questions

A status line in state `decision` names its card (`object_id`). The connector keeps it true without the agent's
help: filing a question (`create_decision`, `merge_cards`) links that session's line to it, the line that says it
waits on the human ("wartet auf deine Wahl", "waiting for your answer") or else the session's one working line, or
makes a line of its own (`q-<card>`, the card's title) when there is none. A `set_status` in state `working` whose
words say it waits on the human, while the session has an open question, becomes `decision` with that card. When
the human answers, every line on the card, and every line that still says it waits while no question of the
session is open, turns `working` with "Antwort da: <labels>" (a card he put away unanswered: "Ohne Antwort
weggelegt"). The connector's own line goes with `close_card` or `withdraw_card`, or when the agent sets a line of
its own for the card; a final option makes it `done`.

A task's line names its "Working on" info card. `start_task` files that card (low urgency, `working: true` in its
body, no push: the apps keep it off the Desk, under Off your mind as being worked on, spec 9.1.3) and sets the line
`working`, linked to it, in one step; a `set_status` in state `working` with a `card_id` links a line the same way.
While the card is open, the line keeps the link: a later `set_status` without `card_id` does not drop it, and a
question filed in the session gets a line of its own instead of taking this one. When `revise_card` gives the card a
title "Done: …" (or Fertig, Erledigt, Resolved, Gelöst), the version drops `working` (the card comes to the Desk
as a quiet info, with a push) and every line linked to it turns `done` with that title.

## What is where

| File | What |
| --- | --- |
| `src/store.rs` | the state on disk: one directory per slot (0700), a lock, a snapshot and a log of checksummed records; one record per step |
| `src/vault.rs`, `src/keeper.rs` | the core's `Device` over that journal, on a thread of its own; the device does groups, stored content, joining and sign-in itself |
| `src/driver.rs` | the shared `core/client` engine in agent mode, its shared board model and cache; runs effects through `core/io` over the slot's journal, hands commands and lifecycle reports to the MCP server |
| `src/agent.rs` | what an agent writes: Chat, cards, registers, permission requests, Artifacts and files, helper sessions, the work trail |
| `src/join.rs` | joining by link with the six emoji |
| `src/bridge.rs`, `src/html.rs` | every tool, and a human's command as a channel event |
| `src/server.rs`, `src/mcp.rs`, `src/member.rs`, `src/slots.rs`, `src/slotstore.rs`, `src/door.rs` | the MCP server, this process as a member, key slots per folder, the sockets hooks reach it through |
| `src/line.rs`, `src/mirror.rs`, `src/trail.rs`, `src/hooks.rs` | the monitor's line, the terminal mirror, the work trail, the permission hooks |
| `src/update.rs` | the check a new binary must pass, and `update` |
| `src/usage.rs` | the account and usage of Claude Code or Codex for the board, the status line, `remove claude` |
| `src/launch.rs` | the launcher: passes the MCP session through to the connector and swaps in a new one |
| `prompt.md`, `tools.json` | every text the agent reads and every tool's schema: the one source, compiled in and checked by `build.rs` |

`prompt.md` and `tools.json` also feed the web app's help page. The driver owns no duplicate hub client, sync
engine or model: authentication, lease headers, catch-up, Welcomes, history, retries, command gating, outbox
re-sealing and verified removal are the shared core's. The connector keeps its machine/session integration,
vouching, same-account checks and journal I/O. See [The app core](../core/client/README.md).

## State and restarts

A slot's state is `~/.local/share/trommi/keys/<room>/<host>-<folder>-<n>.state/` (`TROMMI_KEYS_DIR` moves it): the
device's key, its MLS groups, its content keys, its chains, the cursor in the hub's order and the outbox. A restart
of Claude Code on the same machine opens it, signs in and catches up: it is the same member. Two processes never
own one state (an OS lock on the directory, the core's revision behind it, the hub's lease behind that). A state
that does not read, or that is older than what the device already sent, is said so and never replaced by a new key:
the human reconnects the session in the app.

A key belongs to one agent session: its `.owner` file names it (`cc:<Claude Code session id>` from
`CLAUDE_CODE_SESSION_ID`, `codex:<thread id>` from the first Codex tool call's `_meta`, `key:<TROMMI_SESSION_KEY>`,
or, for a client without any id, its process). A session with an id finds its key by that exact owner, in every
room and every folder's slots (`member::own_slots`): the room of that key is its room, whatever the folder's other
keys or its note `.trommi/room` say; keys of one session in two rooms are refused by name; when the session joins
a room by a new link, its keys in other rooms are left there marked `left:<owner>`. It never takes a key without an
owner record, nor one of a process without an id: which session that was cannot be known. Codex names its thread
only with its first tool call, so a Codex connector takes no key and asks for nothing before that call, and keeps
the first thread for good. A key `join` made in a terminal belongs to nobody until the first session that uses it
adopts it. A connector only takes its own session's key or one nobody owns; another session's key it never takes, also not
while it is free, an idle holder hands its key over only to a connector of its own session, and `say` goes through
a running connector only for its own session. So a second session in the same folder never writes into the
first one's board session: it is a member of its own, which joins by itself (below) or by an invite. `claude --resume` keeps the session id and with it the
key, also in another folder or under another account of Claude Code; a new `claude` in the folder is a new session. A session whose own key lies
on this machine never joins by itself: while an earlier connector of it still holds that key, it asks that one to hand it over, says so, and
takes the key once it is free (each tick, and when a terminal `connect` of it rings); a key a terminal `connect` made for it in any folder is
taken the same way. A new session of the same machine and account joins by itself (spec/v1.md 12.4, `src/vouch.rs`),
in any folder: as soon as its connector starts or the client lists its tools (and again on a Trommi tool call or a
hook), a session without a key of its own asks a running connector of the same account
to vouch for it; that one writes `join_ask/<nonce>` into its main session; the human's app asks "Allow new Claude
session …?", and on Allow answers with an invite link (`join_link/<nonce>`; on Deny with `denied`, and this process
does not ask again); the new session joins with it, the voucher passes the Request's hash on, and the app confirms
that Request (one tap instead of the emoji). A voucher keeps one ask per session: a new ask of the same session
takes the earlier one back, and an ask whose asking connector ended is taken back within a tick. Meanwhile the session lists every board tool and connect, whose
description says that it joins by itself and needs no link; the tools answer "joining by itself … waiting for the
human to allow it", and when it is in, the client is told to list the tools again. If it cannot join by itself (no
running session of the account to vouch, denied), it offers connect alone with the reason. The new session's first
message says who vouched. It is on by default (the web and iPhone apps answer asks since v225); `TROMMI_SELF_JOIN=0`
turns it off. Another account needs one invite by link.
`trommi-connector claim-key`, run inside a session, makes
the key its running connector holds that session's when nobody owns it yet (a key from a terminal `join`).

When the human reconnects the session on another machine, the
connector here learns it from the Commit, wipes the slot and says that it is retired.

## Updates and releases

Updates apply without a restart or reconnect. What Claude Code starts is a small launcher (`src/launch.rs`); it
owns the session's stdin and stdout and runs the connector as its child (`serve`). The installed connector looks for
a newer signed release every hour (`TROMMI_UPDATE_CHECK_MS`), and at once when the hub refuses it as too old, and
puts it in place as `update` does. When a new binary stands at the connector's path and is not refused, the child
asks the launcher to swap it in: the launcher holds the client's new messages back, waits until the old child
answered every open request (each request gets exactly one answer), tells it to hand its key back and exit (no
last report, no witness), awaits its end, so that the lease and the slot are never held twice, then starts the new
child, gives it the session's `initialize` and `notifications/initialized` again, sends what it held, and tells the
client `notifications/tools/list_changed`; the new child signs in and opens the stream. Open requests that take
longer than a minute call the swap off; it is tried again 30 s later. `reload_connector` asks for the same swap.

The launcher almost never changes. When a new binary needs another launcher (its `launch-abi` differs), or the
session runs without one, the update is announced once as before (`kind="update"`, restart required: /mcp →
Reconnect), and `reload_connector` answers with that restart line.

Releases are built and signed in CI: the releases `connector-v<N>` of `chriopter/trommi` (the connector's own series;
before 13 October 2026 `v<N>`, a release of every part) hold the four binaries, `manifest.json` and `manifest.json.sig` (`release/manifest.sh`, `release/sign.sh`: Ed25519 over
the manifest's exact bytes). The connector holds the public key (`release/public-key.pem`, compiled in by
`build.rs`) and checks a binary against a manifest that lies beside it (`src/update.rs`): the signature, then
product, repository, tag and version (never older than the one running; CI compiles the release number in with
`TROMMI_RELEASE_VERSION`), then the binary's size and SHA-256 under its release name. `--version` says where a
binary stands: verified with its release, not verified (no manifest beside it), or refused. A refused binary is
never announced as an update.

## Build and tests

```
cargo build --release --locked -j4 -p trommi-connector
node connector/build-plugin.mjs [dir]      # the four targets' binaries, as named in a release
TROMMI_TESTS_FULL=1 cargo test --locked -j4 -p trommi-tests --test connector_e2e --test connector_sessions
```

The four targets are static Linux (`x86_64`, `aarch64`, musl, linked with `rust-lld`, ring's C compiled by clang)
and macOS (`aarch64`, `x86_64`, `cargo zigbuild`, no Apple SDK). The scenario tests start this repository's hub as a child process (built beside the tests, or `TROMMI_HUB_BIN`)
and the built connector over MCP.
