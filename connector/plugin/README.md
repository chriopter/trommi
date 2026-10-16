# The Trommi plugin for Claude Code

This folder is the plugin, as Claude Code reads it from the repository's marketplace
(`.claude-plugin/marketplace.json` at the repository's root). It holds no program. Its MCP server, its hooks and
its monitor all name the installed connector, `~/.local/share/trommi/bin/trommi-connector`, which `install.sh`
puts there from a signed release; `install.sh` also runs `trommi-connector setup claude`, which adds this plugin.
In a project folder, approve the computer once with `/trommi:connect --computer '<invite link>'`
([commands/connect.md](commands/connect.md)), or paste the prompt **Approve a computer** shows in Trommi.
Compare the six emoji and confirm in the app. Each later session activates with `/trommi:connect` without a link;
starting the connector or calling other tools does not join it. An online human app is needed to answer.

A hook of a session whose connector is not there ends without a word. See [`connector/README.md`](../README.md).
