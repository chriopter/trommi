//! The agent programs this connector serves, as presets of one mechanism: how the program is named to the human, how
//! it is restarted or updated, how a board event wakes a running session, and how a closed session is started again
//! for a wakeup (wakeups.rs). Claude Code and Codex are two rows of [`PRESETS`]; nothing else in the connector names
//! their commands.
//!
//! A command is an argument vector, never a shell line. A placeholder fills a whole argument (`{prompt}`, `{resume}`,
//! `{max_turns}`, `{ask_tool}`), so a value can never become a second argument of its own. The first group of a command
//! is always there (an empty value stays an empty argument); a later group is optional, left out when one of its
//! placeholders is empty (the permission tool of a woken Claude Code run).

/// How a board event reaches a running session at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Live {
    /// The program shows the server's events itself (Claude Code's channel events, the plugin's monitor).
    Shown,
    /// A command queues a message for the running conversation (`codex queue`).
    Command(&'static [&'static [&'static str]]),
    /// The connector itself hands the board to the agent through the agent's own chat server on this computer, as
    /// the turns of one conversation, and puts the answer into the board's chat (`chat_api.rs`, the chat bridge).
    ChatApi,
}

/// One agent program.
#[derive(Debug)]
pub struct Preset {
    /// The name kept in a wakeup's note (`program`): "claude", "codex".
    pub id: &'static str,
    /// The program's name, for texts the human reads.
    pub name: &'static str,
    /// The program run when nothing else is named, and the variables that name another.
    pub program: &'static str,
    pub program_env: &'static [&'static str],
    /// How the human restarts this session's connector.
    pub restart: &'static str,
    /// How a newer connector is installed and loaded.
    pub update: &'static str,
    pub live: Live,
    /// The run a waker starts for a closed session's wakeup, on its conversation (none: not woken).
    pub woken_run: Option<&'static [&'static [&'static str]]>,
}

pub const CLAUDE: Preset = Preset {
    id: "claude",
    name: "Claude Code",
    program: "claude",
    program_env: &["TROMMI_CLAUDE"],
    restart: "In the terminal of this Claude Code session: /mcp, then trommi, then Reconnect.",
    update: "run `trommi-connector update` in a terminal; this session's connector swaps the new one in by itself (if it does not: /mcp, then trommi, then Reconnect)",
    live: Live::Shown,
    woken_run: Some(&[
        &["-p", "{prompt}", "--resume", "{resume}", "--max-turns", "{max_turns}"],
        &["--permission-prompt-tool", "{ask_tool}"],
    ]),
};

pub const CODEX: Preset = Preset {
    id: "codex",
    name: "Codex",
    program: "codex",
    program_env: &["TROMMI_CODEX"],
    restart: "Quit Codex and start it again in this folder (`codex resume` keeps the conversation).",
    update: "run `trommi-connector update` in a terminal, then quit Codex and start it again (`codex resume` keeps the conversation)",
    live: Live::Command(&[&["queue", "--thread", "{resume}", "--message", "{prompt}"]]),
    woken_run: Some(&[&["exec", "resume", "{resume}", "{prompt}"]]),
};

/// Hermes Agent: its gateway starts the connector as its MCP server `trommi` (`setup hermes`), and the connector talks
/// to its API server (an OpenAI-compatible chat server on this computer). A wakeup runs while the gateway runs; a
/// closed gateway is not started again.
pub const HERMES: Preset = Preset {
    id: "hermes",
    name: "Hermes",
    program: "hermes",
    program_env: &[],
    restart: "Restart the Hermes gateway (`hermes gateway restart`): it starts the Trommi connector anew.",
    update: "run `trommi-connector update` in a terminal, then restart the Hermes gateway (`hermes gateway restart`)",
    live: Live::ChatApi,
    woken_run: None,
};

pub const PRESETS: [&Preset; 3] = [&CLAUDE, &CODEX, &HERMES];

/// The preset of a note's `program`.
pub fn by_id(id: &str) -> Option<&'static Preset> {
    PRESETS.into_iter().find(|p| p.id == id)
}

/// The values a command's placeholders take.
#[derive(Clone, Debug, Default)]
pub struct Fill<'a> {
    pub prompt: &'a str,
    pub resume: &'a str,
    pub max_turns: &'a str,
    pub ask_tool: &'a str,
}

impl Fill<'_> {
    fn value(&self, placeholder: &str) -> Option<&str> {
        Some(match placeholder {
            "{prompt}" => self.prompt,
            "{resume}" => self.resume,
            "{max_turns}" => self.max_turns,
            "{ask_tool}" => self.ask_tool,
            _ => return None,
        })
    }
}

/// The arguments of a command: each group in order, an optional group (all but the first) left out when one of its
/// placeholders is empty.
pub fn render(groups: &[&[&str]], fill: &Fill) -> Vec<String> {
    let mut out = vec![];
    'group: for (i, group) in groups.iter().enumerate() {
        let mut args = Vec::with_capacity(group.len());
        for arg in *group {
            match fill.value(arg) {
                Some("") if i > 0 => continue 'group,
                Some(v) => args.push(v.to_string()),
                None => args.push(arg.to_string()),
            }
        }
        out.extend(args);
    }
    out
}

impl Preset {
    /// The program a woken run starts: the first of its variables that is set (as it is, even empty), else its plain
    /// name on the PATH.
    pub fn program(&self) -> String {
        self.program_env
            .iter()
            .find_map(|k| std::env::var(k).ok())
            .unwrap_or_else(|| self.program.to_string())
    }
    /// The woken run of a closed session: program and arguments (none without a conversation to resume).
    pub fn woken_command(&self, fill: &Fill) -> Option<(String, Vec<String>)> {
        if fill.resume.is_empty() {
            return None;
        }
        Some((self.program(), render(self.woken_run?, fill)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_as_before() {
        let fill = Fill { prompt: "P", resume: "abc", max_turns: "30", ask_tool: "mcp__trommi__ask_permission" };
        assert_eq!(
            render(CLAUDE.woken_run.unwrap(), &fill).join(" "),
            "-p P --resume abc --max-turns 30 --permission-prompt-tool mcp__trommi__ask_permission"
        );
        assert_eq!(render(CLAUDE.woken_run.unwrap(), &Fill { ask_tool: "", ..fill.clone() }).join(" "), "-p P --resume abc --max-turns 30");
        assert_eq!(render(CODEX.woken_run.unwrap(), &fill), ["exec", "resume", "abc", "P"]);
        let Live::Command(queue) = CODEX.live else { panic!() };
        assert_eq!(render(queue, &fill), ["queue", "--thread", "abc", "--message", "P"]);
        // (a required argument stays, even empty)
        assert_eq!(render(queue, &Fill { prompt: "", ..fill.clone() }), ["queue", "--thread", "abc", "--message", ""]);
        assert!(CLAUDE.woken_command(&Fill { resume: "", ..fill }).is_none(), "nothing to resume, no run");
        assert_eq!(by_id("codex").map(|p| p.name), Some("Codex"));
        assert!(by_id("sh").is_none());
    }

    #[test]
    fn a_value_stays_one_argument() {
        let fill = Fill { prompt: "--dangerous; rm -rf ~", resume: "id", ..Default::default() };
        assert_eq!(render(CODEX.woken_run.unwrap(), &fill), ["exec", "resume", "id", "--dangerous; rm -rf ~"]);
    }
}
