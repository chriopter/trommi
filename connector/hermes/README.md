# Trommi for Hermes Agent

This add-on makes Trommi a platform of [Hermes Agent](https://hermes-agent.nousresearch.com), the way Telegram is
one (the same build as [hermes-basecamp](https://github.com/chriopter/hermes-basecamp)). Every Trommi chat is a
Hermes chat on the platform `trommi`:

- What the person writes on the board reaches Hermes as a message of that chat.
- Hermes' answer goes back to the same chat, growing on the board while it is written (Hermes' draft streaming, as
  answer previews, spec/v1.md 7.5). While it works, the chat shows it working.
- What Hermes asks to approve comes to the board as a yes/no card. The card's answer settles exactly that request.
  No answer is no.
- The board tools (cards, status lines, knowledge, wakeups) are Hermes tools named `trommi_<name>`. A call from a Trommi
  chat acts in that chat. A call from any other conversation of this Hermes (for example on Telegram or from cron)
  acts in its main chat.
- "New chat with …" in the app starts another chat. The add-on starts its connector, which activates itself on
  this approved computer (at most 8 sessions, as for any approved computer).

## How it works

The add-on (`trommi_platform/`) runs inside the Hermes gateway. For each Trommi chat it starts the Trommi connector
(`trommi-connector`) as a child process and speaks MCP's JSON-RPC to it over stdin and stdout (`child.py`). Each
connector holds the device key of its chat; the add-on holds none. The chats of this Hermes are listed in
`<hermes home>/state/trommi-<name>.json` (this user only). Besides MCP, the connector takes three notifications of the
add-on: `trommi/working` {on}, `trommi/preview` {draft, text}, `trommi/answer` {text}.

## Install and update

The connector carries this add-on and installs it:

```sh
trommi-connector setup hermes                                  # one Hermes
trommi-connector setup hermes --name Einkauf --profile einkauf # another Hermes, a Hermes profile of its own
hermes gateway restart
```

`setup hermes` writes the add-on into `<hermes home>/plugins/trommi-platform/` together with `trommi.json`, the
connector's board tools and instructions. It then turns the plugin on (`hermes plugins enable trommi-platform`) and
configures `platforms.trommi` (`enabled`, `extra.connector`, `extra.instance`, `extra.name`, `extra.folder`). There
is nothing to update by hand. The add-on is part of the signed connector release, and a newer connector writes its
add-on again when it starts. Hermes loads it at its next start. Live text needs Hermes' streaming on
(`hermes config set streaming.enabled true`). `trommi-connector remove hermes` turns it off and removes it.

## Tests

The tests run against a pinned Hermes Agent checkout
(`cb757fac59c92b49e9b8f2910e5cd7ab75efffdd`, NousResearch/hermes-agent):

```sh
HERMES_AGENT_SRC=/path/to/hermes-agent python -m pytest -q    # in connector/hermes, with pytest and pytest-asyncio
```

When Hermes changes its platform interface, the pin moves only after these tests pass against the new version.
