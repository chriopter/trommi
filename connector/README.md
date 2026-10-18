# trommi-connector

The Trommi connector: one static binary through which a Claude Code session talks to the human's board. It is an
**agent device** of a room on protocol v1 ([`spec/v1.md`](../spec/v1.md)): every key, every check and every byte on
the wire comes from [`trommi-core`](../core/protocol); nothing of the first protocol is in it.

```
trommi-connector                     MCP stdio server "trommi" (what Claude Code starts): the launcher, which runs `serve`
trommi-connector serve               the MCP server itself, as the launcher's child (TROMMI_LAUNCH=0 runs it without one)
trommi-connector connect             explicitly activate this session on the approved computer
trommi-connector connect --computer <link> approve this computer once, then activate this session
trommi-connector connect <link>      join just one session by a human invite (also: join)
trommi-connector say "<text>" [--helper <name>] [--urgent]
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

Approve this computer once: inside Claude Code (or Codex) started in a project folder, paste the command
**Approve a computer** shows in Trommi:

```
Approve this computer in Trommi with this invite link: '<invite link>'. Call the Trommi connect tool with this link and computer=true.
```

or, in Claude Code, the plugin's command `/trommi:connect --computer '<invite link>'`. Compare the six emoji in the terminal and the app,
then confirm in the app. Each later session activates with `/trommi:connect` without a link; it stays off the board
until that command. An online human app is needed, but no running agent session.

`setup claude` runs every `claude plugin …` command in its own environment, so with `CLAUDE_CONFIG_DIR` set (one of
several Claude Code accounts) the plugin goes into that account. Claude Code keeps a copy of the plugin per version
and `claude plugin update` copies nothing while the version stays the same; `setup claude` therefore compares the
copy with the marketplace's plugin folder and installs it afresh when a file is missing or differs (an older copy
without `commands/connect.md` has no `/trommi:connect`), and turns the plugin on when it is off.

A session whose folder is in no room yet starts unconnected: its MCP server offers one tool, `connect`, and
the human's command `/trommi:connect` activates it using the approved computer. A computer-approval command
with a link and `--computer` first performs the emoji approval. A bare link remains a one-session invite. The tool runs the same join as
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
  `hears: true`), also after a restart (`codex resume`, or Codex starting its app server anew) or a swapped-in
  connector, before its first call: a Codex connector in the room that leaves notes its thread
  (`<keys>/<base>.codex.<hash>`, with its launcher when it hands over in a swap). The connector swapped in under
  the same launcher takes that thread and key at once; a connector of the folder that starts within
  `TROMMI_CODEX_RETURN_MS` (10 minutes) of another's end wakes that thread once ("[Trommi] Back on the board: …"),
  and the thread's next call names its own connector, which takes its key. A connector that does not know its
  thread looks for such a note on every tick: Codex may start a thread's new connector before it ends the old
  one (`codex resume` of a loaded thread). A Codex connector that ends with its input, its parent or a signal
  says no last word: the next connector of the thread takes the key within the lease, so the board does not show
  the agent as gone meanwhile (a Codex that really quit shows as gone when the lease runs out, after 60 s). A
  woken turn that never called `inbox` does not leave the session deaf: the next event wakes it again once
  `TROMMI_CODEX_REWAKE_MS` (3 minutes) passed since the last wake.
- It never asks for the tool list again: an unconnected session lists every tool with `connect`.
- It asks for approval of an MCP call unless the tool says it is harmless: every tool carries `annotations`
  (`readOnlyHint`, `destructiveHint`, `openWorldHint`); only `revoke_asset` and `share_asset` still ask.
- Its hooks come from `setup codex`, not from a plugin (above). The connector reads at its start whether Codex's
  `hooks.json` holds its Stop and UserPromptSubmit hooks and `config.toml` trusts them; only then do the
  instructions say that the final text reaches the board by itself. Codex's hook input names the turn (`turn_id`,
  used where Claude Code has `prompt_id`) and the thread (`session_id`): a connector takes only its own thread's
  hooks; a connector whose thread is named opens a bell of that thread (`bell-t-<hash>.sock`), which a hook rings
  first, since one Codex app server runs the connectors of all its threads. A hook of a thread that is in a
  Trommi room while no connector serves it by name wakes that thread (at most once per `TROMMI_CODEX_REWAKE_MS`;
  not for a Trommi tool call, which names it anyway). Codex starts its MCP servers without `XDG_RUNTIME_DIR` but
  its hooks with it: the sockets' directory falls back to `/run/user/<uid>` so that both find the same bells. The
  queued wake ("[Trommi] N new board events …") counts as the board's, not as the human's typing,
  so a turn a card's answer started puts its final text under that card.
- Its words differ: the instructions take "# In Codex" from `prompt.md` (inbox at every turn, never run the
  connector in a shell, restart Codex when the tools are missing), and every restart or update line says to quit
  Codex and start it again instead of /mcp.

`tests/connector/codex.rs` drives the connector as Codex 0.160 does (its `initialize`, `_meta`, no new tool
list, a `codex` script that notes the queued wakes). `tests/connector/codex_real.rs` runs a real Codex (its app server,
one session over many turns) against a local hub, opt-in: `cargo test -p trommi-tests --test connector_codex_real
-- --ignored --nocapture` (`CODEX_MODEL`, `TROMMI_CODEX_RUNS`). `a_real_codex_tui_soak` in the same file runs a
real Codex TUI in tmux for `TROMMI_SOAK_MINUTES` (default 120), its MCP servers in Codex's app-server daemon (as
`codex resume` runs them): idle stretches, a turn after each event, a hot swap, a daemon restart, a terminal detach
and resize, Codex quit and resumed; it samples the agent's lease on the hub every 2 s, writes the presence timeline
to `TROMMI_SOAK_LOG` and fails on an offline stretch longer than `TROMMI_SOAK_MAX_GAP_S` (10).

## Helpers and their events

A helper (a subagent of the agent) has no session of its own: it writes through its agent's connector into its
agent's session, and what it writes carries its name. The tools that write take `helper: "<name>"` (one line, at
most 40 characters): `reply`, `create_decision`, `create_info`, `start_task`, `merge_cards`, `set_status`,
`clear_status`, `list_cards` (its cards alone) and `publish_asset`. The name goes into the card's, the status line's
or the message's body as `helper` (spec/v1.md 9.1); the board shows the helpers inside their agent's session, a
helper's card with a "Helper <name>" chip. A helper's status line ids are its own (stored as `helper.<8 hex of the
name's SHA-256>.<id>`): two helpers may both keep a line "work", apart from the agent's own.

The coordinator (the agent the helpers run in) reads every board event itself and forwards each to the helper it
belongs to; helpers never call `inbox`. An event about a helper's card (an answer, a hand-back, What??, a comment)
carries `helper="<name>"` in its meta, so the coordinator routes it by name or by card id. Before a helper starts,
the coordinator files its "Working on" card with `start_task` and `helper`, and hands it the card id; the helper
revises that card to "Done: …" at its end. A tool call that names a helper gets no board events with its result,
and `inbox` with a helper says there are none for it: every event stays the coordinator's.

Before 16 October 2026 a helper had a session group of its own (spec/v1.md 5.2.3), opened with `open_session`.
For agents that still hold the tools they were started with: `session: "<name>"` means `helper: "<name>"`,
`open_session` and `close_session` answer what changed and do nothing, `say --session` is `say --helper`. The
helper sessions of before stay readable; their events carry `helper` with the session's name, an answer to one of
their cards goes into that session as before, and the connector archives each (5.2.10) once nothing of it waits
(no open question, no answer it has not closed): at its start and whenever one of their cards is closed or
withdrawn. Events that waited when a connector was swapped are the coordinator's, as every event is.

## Team knowledge

The team's knowledge (spec/v1.md 9.3.7, README "Team knowledge") reaches the agent as the copies the person's devices
keep in its session (`knowledge/<id>`); the connector never reads or writes the room group. `search_knowledge` (words
that begin words of a title or text, from a full-text index of the session's copy, built again only when the copy
changed: `bridge.rs knowledge_index`) and `read_knowledge` read. `add_knowledge`, `update_knowledge` and
`forget_knowledge` write without asking: each puts a write `knowledge_ask/<id>` into the agent's own session, which
the person's devices apply on their own and answer (`knowledge_done/<id>`); the connector then takes the write away
(`knowledge_answers`, with each tool's result) and tells the agent once what was refused. A digest
(`trommi_client_core::knowledge::digest`: how many entries, the pinned ones with their text, the newest titles, each
with its kind) rides on a tool's result when it is new to the agent or changed, after the team's goals. A read of the
knowledge is a step of the trail (`knowledge: true`, with what was searched for), which the app shows as "reads team
knowledge (12)".

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
| `src/agent.rs` | what an agent writes: Chat, cards, registers, permission requests, Artifacts and files, the work trail; archiving a helper session of before |
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
computer-key locking and journal I/O. See [The app core](../core/client/README.md).

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
only with its first tool call, so a Codex connector takes no key and asks for nothing before that call (unless
the connector before it under the same launcher named the thread, see above), and keeps the first thread for good. A key `join` made in a terminal belongs to nobody until the first session that uses it
adopts it. A connector only takes its own session's key or one nobody owns; another session's key it never takes, also not
while it is free, an idle holder hands its key over only to a connector of its own session, and `say` goes through
a running connector only for its own session. So a second session in the same folder never writes into the
first one's board session: it is a member of its own, which joins by itself (below) or by an invite. `claude --resume` keeps the session id and with it the
key, also in another folder or under another account of Claude Code; a new `claude` in the folder is a new session. A session whose own key lies
on this machine never joins by itself: while an earlier connector of it still holds that key, it asks that one to hand it over, says so, and
takes the key once it is free (each tick, and when a terminal `connect` of it rings); a key a terminal `connect` made for it in any folder is
taken the same way.

A fresh session stays off the board until explicitly activated. In Trommi choose **Approve a computer**, copy its
command to the agent, compare the six emoji and confirm them in the app. The command uses
`/trommi:connect --computer '<link>'` (MCP `connect` with `computer: true`). This creates a separate computer key at
`keys/<room>/machine/` and activates the current session. Later sessions use `/trommi:connect` with no link. No
running session is needed to vouch; an online human app answers the approved computer's hash-bound requests.
Starting the connector, listing tools and calling a board tool do not activate a fresh session. A bare human-supplied
invite link continues to join just one session, without granting computer authority.

Approval covers this OS user across Claude Code/Codex accounts. Any process running as that user can request up to
8 live agent sessions without another tap, at most 24 in a room. Remove the computer in Devices to remove its
admitted sessions and helpers. Ordinary agent metadata never grants approval; the authenticated human room register
binds it to the exact computer device. The connector briefly locks and opens the computer state for each operation,
releasing it between polls. Activation uses the explicitly chosen room, then `.trommi/joined`, otherwise the only
approved board; several boards require a choice. New identified session keys use
`keys/<room>/s/<sha256(owner)[..16]>/`; existing owned legacy slots are resumed where they are, without migration.
Activation waits up to 15 minutes for the human app to be online. See [the protocol](../spec/v1.md#124-computer-approval-and-explicit-session-activation).

`trommi-connector claim-key`, run inside a session, makes
the key its running connector holds that session's when nobody owns it yet (a key from a terminal `join`).

When the human reconnects the session on another machine, the
connector here learns it from the Commit, wipes the slot and says that it is retired.

## Updates and releases

Updates with the same MCP contract apply without a restart or reconnect. The connector fingerprints its compiled
tools, schemas and instruction variants, including the descriptions and drawing choices. When they change, it
asks the agent to tell the human to restart the client so that both tools and instructions are loaded again.
Formatting-only changes do not require a restart. Clients with the older numeric launcher ABI need one restart
to start checking this fingerprint too. `codex resume` keeps the conversation.
What Claude Code starts is a small launcher (`src/launch.rs`); it
owns the session's stdin and stdout and runs the connector as its child (`serve`). The installed connector looks for
a newer signed release every hour (`TROMMI_UPDATE_CHECK_MS`), and at once when the hub refuses it as too old, and
puts it in place as `update` does. When a new binary stands at the connector's path and is not refused, the child
asks the launcher to swap it in: the launcher holds the client's new messages back, waits until the old child
answered every open request (each request gets exactly one answer), tells it to hand its key back and exit (no
last report, no witness), awaits its end, so that the lease and the slot are never held twice, then starts the new
child, gives it the session's `initialize` and `notifications/initialized` again, sends what it held, and tells the
client `notifications/tools/list_changed`; the new child signs in and opens the stream. Open requests that take
longer than a minute call the swap off; it is tried again 30 s later. `reload_connector` asks for the same swap.

The launcher almost never changes. Its `launch-abi` probe reports the launcher protocol plus the MCP contract
fingerprint. When either differs, or the session runs without a launcher, the update is announced once
(`kind="update"`, `restart_required="1"`), attached to tool results too, and `reload_connector` gives the same
host-specific restart instruction: /mcp → Reconnect in Claude Code, quit and resume in Codex. A
`tools/list_changed` notification alone cannot refresh a client's cached initialization instructions.

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
