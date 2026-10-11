# trommi-connector

The Trommi connector: one static binary through which a Claude Code session talks to the human's board. It is an
**agent device** of a room on protocol v1 ([`spec/v1.md`](../spec/v1.md)): every key, every check and every byte on
the wire comes from [`trommi-core`](../core); nothing of the first protocol is in it.

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
curl -fsSL https://raw.githubusercontent.com/trommi/trommi/main/install.sh | sh
```

Then once per project folder, inside Claude Code started there:

```
/trommi:connect '<invite link>'
```

A session whose folder is in no room yet starts unconnected: its MCP server offers one tool, `connect(link)`, and
the plugin's command `/trommi:connect` tells the agent to call it. The tool runs the same join as
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

## Helpers read their own events

A helper (a subagent in a child session from `open_session`) calls `inbox` with `session: "<name>"`: it gets that
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

## What is where

| File | What |
| --- | --- |
| `src/store.rs` | the state on disk: one directory per slot (0700), a lock, a snapshot and a log of checksummed records; one record per step |
| `src/vault.rs`, `src/keeper.rs` | the core's `Device` over that journal, on a thread of its own; the device does groups, stored content, joining and sign-in itself |
| `src/hub.rs` | the hub's routes (`spec/hub-api.md`): JSON, sign-in by signed challenge, the lease header, the stream |
| `src/client.rs` | the device at work: the hub's order (one cursor for log and envelopes), Welcomes, the command gate's answers, the outbox, lease, removal, the rollback guard, and when to stop asking a hub that is none |
| `src/agent.rs` | what an agent writes: Chat, cards, registers, permission requests, Artifacts and files, helper sessions, the work trail |
| `src/join.rs` | joining by link with the six emoji |
| `src/model.rs` | what this agent knows of the board |
| `src/bridge.rs`, `src/html.rs` | every tool, and a human's command as a channel event |
| `src/server.rs`, `src/mcp.rs`, `src/member.rs`, `src/slots.rs`, `src/slotstore.rs`, `src/door.rs` | the MCP server, this process as a member, key slots per folder, the sockets hooks reach it through |
| `src/line.rs`, `src/mirror.rs`, `src/trail.rs`, `src/hooks.rs` | the monitor's line, the terminal mirror, the work trail, the permission hooks |
| `src/update.rs` | the check a new binary must pass, and `update` |
| `src/usage.rs` | the account and usage of Claude Code or Codex for the board, the status line, `remove claude` |
| `src/launch.rs` | the launcher: passes the MCP session through to the connector and swaps in a new one |
| `prompt.md`, `tools.json` | every text the agent reads and every tool's schema: the one source, compiled in and checked by `build.rs` |

`prompt.md` and `tools.json` are also what the web app's help page is built from. They used to be read at
`connector/prompt.md` and `connector-rs/tools.json`; both are here now (`connector/prompt.md`,
`connector/tools.json`), and the app's build (`app/web/dev/build.mjs`) names the second one at its new place.

## State and restarts

A slot's state is `~/.local/share/trommi/keys/<room>/<host>-<folder>-<n>.state/` (`TROMMI_KEYS_DIR` moves it): the
device's key, its MLS groups, its content keys, its chains, the cursor in the hub's order and the outbox. A restart
of Claude Code on the same machine opens it, signs in and catches up: it is the same member. Two processes never
own one state (an OS lock on the directory, the core's revision behind it, the hub's lease behind that). A state
that does not read, or that is older than what the device already sent, is said so and never replaced by a new key:
the human reconnects the session in the app.

A key belongs to one agent session: its `.owner` file names it (`cc:<Claude Code session id>` from
`CLAUDE_CODE_SESSION_ID`, `key:<TROMMI_SESSION_KEY>`, or, for a client without a session id such as Codex, its
process). A key `join` made in a terminal belongs to nobody until the first session that uses it adopts it. A
connector only takes its own session's key or one nobody owns; another session's key it never takes, also not
while it is free, an idle holder hands its key over only to a connector of its own session, and `say` goes through
a running connector only for its own session. So a second session in the same folder never writes into the
first one's board session: it needs an invite of its own. `claude --resume` keeps the session id and with it the
key; a new `claude` in the folder is a new session. `trommi-connector claim-key`, run inside a session, makes
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

Releases are built and signed in CI: the releases `v<N>` of `trommi/trommi` (every part; earlier `connector-v<N>`)
hold the four binaries, `manifest.json` and `manifest.json.sig` (`release/manifest.sh`, `release/sign.sh`: Ed25519 over
the manifest's exact bytes). The connector holds the public key (`release/public-key.pem`, compiled in by
`build.rs`) and checks a binary against a manifest that lies beside it (`src/update.rs`): the signature, then
product, repository, tag and version (never older than the one running; CI compiles the release number in with
`TROMMI_RELEASE_VERSION`), then the binary's size and SHA-256 under its release name. `--version` says where a
binary stands: verified with its release, not verified (no manifest beside it), or refused. A refused binary is
never announced as an update.

## Build and tests

```
cargo build --release -p trommi-connector
node connector/build-plugin.mjs [dir]      # the four targets' binaries, as named in a release
cargo test -p trommi-tests                 # tests/connector/*: units, and scenarios against the real hub
```

The four targets are static Linux (`x86_64`, `aarch64`, musl, linked with `rust-lld`, ring's C compiled by clang)
and macOS (`aarch64`, `x86_64`, `cargo zigbuild`, no Apple SDK). The scenario tests start this repository's hub as a child process (built beside the tests, or `TROMMI_HUB_BIN`)
and the built connector over MCP.
