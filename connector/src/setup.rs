//! `trommi-connector setup claude` and `setup codex`: the installed connector is made known to the program that
//! will start it. Both go through that program's own commands (`claude plugin …`, `codex mcp …`), never through
//! its files, and both can be run again: what is there already is left, and every line says what was done.
//!
//! **Claude Code** gets the Trommi plugin from the repository's marketplace (`.claude-plugin/marketplace.json`,
//! plugin folder `connector/plugin`). The plugin holds no program: its MCP server, hooks and monitor name the
//! installed connector, `~/.local/share/trommi/bin/trommi-connector`, so an update of the connector needs no
//! new plugin. Claude Code copies the plugin by its version and `plugin update` copies nothing while that stays the
//! same, so after installing or updating, the copy is compared with the marketplace's folder and installed afresh when
//! it differs; a plugin turned off is turned on. Every `claude` call runs in this process's environment, so a
//! `CLAUDE_CONFIG_DIR` (one Claude Code account of several) is kept to. The one thing a plugin cannot bring is the status line, and Claude Code has no command for it: that
//! one is written into its user settings (`settings.json`, only the `statusLine` member, every other byte as it was),
//! around a status line that is there already (`usage.rs`). `remove claude` puts the one before back.
//!
//! **Codex** gets the connector as a plain MCP server `trommi`. Codex shows no channel events, so the server is
//! registered with `TROMMI_CHANNEL_EVENTS=off`: board events wait for the `inbox` tool, and the connector wakes the
//! session with a queued message (`host.rs`). A Codex that is already running loads it only after a restart.
//! Codex runs hooks as Claude Code does, but a plugin cannot bring them: `setup codex` merges the connector's hooks
//! (terminal mirror, permission prompts on the board, work trail) into `$CODEX_HOME/hooks.json` (default
//! `~/.codex/hooks.json`), leaving every other hook there as it is. Codex runs a new or changed hook only once the
//! human trusted it (its prompt at the start, or /hooks). `remove codex` takes the hooks and the MCP server out again.
//!
//! **Hermes** (Hermes Agent) gets the Trommi add-on (`connector/hermes/`, carried inside this program): a Hermes
//! platform like Telegram, written into the Hermes home's plugins (`<home>/plugins/trommi-platform`), with what this
//! connector says its board tools and instructions are (`trommi.json`). Then the add-on is turned on and the platform
//! "trommi" configured through Hermes' own commands (`hermes plugins enable`, `hermes config set`). The add-on starts
//! this program once per Trommi chat. A connector of a newer release writes its add-on again when it starts, as one
//! package under a lock (`refresh_hermes_addon`), so the add-on is always the one of the newest signed release there. A second Hermes is another Hermes profile
//! (`--profile`), with a name of its own (`--name`). `remove hermes` turns both off and takes the add-on away.
use crate::usage;
use serde_json::Value;
use std::path::Path;
use std::process::Command;

/// The marketplace's source: the repository on GitHub. `TROMMI_MARKETPLACE` names another one (a checkout).
const MARKETPLACE: &str = "chriopter/trommi";
/// What of the repository a marketplace needs.
const SPARSE: [&str; 2] = [".claude-plugin", "connector/plugin"];

/// Runs a command of another program to its end; `Ok` is what it printed, when it ended well.
fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| format!("`{program}` does not start ({error})"))?;
    let said = |bytes: &[u8]| String::from_utf8_lossy(bytes).trim().to_string();
    if output.status.success() {
        Ok(said(&output.stdout))
    } else {
        let mut why = said(&output.stderr);
        if why.is_empty() {
            why = said(&output.stdout);
        }
        Err(format!("`{program} {}` failed: {why}", args.join(" ")))
    }
}

/// The installed connector, which must be there before anything is pointed at it.
fn program() -> Result<String, String> {
    let program = crate::update::installed_program().ok_or("HOME is not set to a folder")?;
    if !program.is_file() {
        return Err(format!(
            "no connector is installed at {}: run install.sh of github.com/{} first",
            program.display(),
            crate::update::REPOSITORY
        ));
    }
    program
        .to_str()
        .map(String::from)
        .ok_or_else(|| "the home folder's name is not plain text".to_string())
}

fn here(name: &str) -> Result<(), String> {
    let found = std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path)
            .any(|dir| !dir.as_os_str().is_empty() && dir.join(name).is_file())
    });
    if found {
        Ok(())
    } else {
        Err(format!(
            "`{name}` is not on the PATH: install it, then run this again"
        ))
    }
}

/// The program's name as people know it.
fn label(which: &str) -> &'static str {
    match which {
        "codex" => "Codex",
        "hermes" => "Hermes",
        _ => "Claude Code",
    }
}

/// What to do next, one line.
fn next(which: &str) -> &'static str {
    if which == "hermes" {
        "restart the Hermes gateway (`hermes gateway restart`); then tell Hermes (in its CLI or any chat) to connect to Trommi with the command from \"Approve a computer\" in the Trommi app. After that, write to it in its chat on the board."
    } else if which == "codex" {
        "quit every running Codex and start it again (a running Codex does not load new MCP servers), in a project folder; then ask it to connect to Trommi with the command from \"Approve a computer\" in the Trommi app."
    } else {
        "start claude in a project folder and paste what \"Approve a computer\" in the Trommi app shows (the prompt with the link, or /trommi:connect --computer '<link>')."
    }
}

/// Bold, green, red and dim, only when that stream is a terminal and `NO_COLOR` is not set.
fn paint(terminal: bool, code: &str, text: &str) -> String {
    let plain = !terminal
        || std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty())
        || std::env::var("TERM").is_ok_and(|term| term == "dumb");
    if plain {
        text.to_string()
    } else {
        format!("\x1b[{code}m{text}\x1b[0m")
    }
}

/// Whether the per-step details are wanted: `--verbose` or `TROMMI_VERBOSE=1`.
fn verbose(flags: &[String]) -> bool {
    flags.iter().any(|flag| flag == "--verbose" || flag == "-v")
        || std::env::var("TROMMI_VERBOSE").is_ok_and(|value| !value.is_empty() && value != "0")
}

/// `setup claude|codex [--verbose]`: one ✓ line, the details under it when asked for, then what to do next.
/// `--details` prints only the details, one per line (install.sh frames them itself). Returns the exit code.
pub fn run_cli(which: Option<&str>, flags: &[String]) -> i32 {
    let done = match which {
        Some("claude") => claude(),
        Some("codex") => codex(),
        Some("hermes") => hermes(flags),
        _ => Err("use `setup claude`, `setup codex` or `setup hermes` (add --verbose for the details)".to_string()),
    };
    report(which.unwrap_or("claude"), flags, done, false)
}

/// `remove claude` or `remove codex`: what `setup` put in, out again. Returns the exit code.
pub fn remove_cli(which: Option<&str>, flags: &[String]) -> i32 {
    let done = match which {
        Some("claude") => return remove_claude(),
        Some("codex") => remove_codex(),
        Some("hermes") => return report("hermes", flags, remove_hermes(flags), true),
        _ => Err("use `remove claude`, `remove codex` or `remove hermes`".to_string()),
    };
    report("codex", flags, done, true)
}

fn report(which: &str, flags: &[String], done: Result<Vec<String>, String>, removed: bool) -> i32 {
    use std::io::IsTerminal;
    let out = std::io::stdout().is_terminal();
    match done {
        Ok(lines) if flags.iter().any(|flag| flag == "--details") => {
            for line in lines {
                println!("{line}");
            }
            0
        }
        Ok(lines) => {
            println!(
                "{} {}",
                paint(out, "32", "✓"),
                paint(
                    out,
                    "1",
                    &format!(
                        "{} {}",
                        label(which),
                        if removed {
                            "no longer uses Trommi"
                        } else {
                            "set up"
                        }
                    )
                )
            );
            if verbose(flags) || removed {
                for line in lines {
                    println!("    {}", paint(out, "2", &line));
                }
            }
            if !removed {
                println!("{} {}", paint(out, "1", "Next:"), next(which));
            }
            0
        }
        Err(why) => {
            let err = std::io::stderr().is_terminal();
            eprintln!("{}", paint(err, "31", &format!("✗ [trommi] {why}")));
            1
        }
    }
}

/// The installed plugins, as Claude Code lists them.
fn list_plugins() -> Result<Value, String> {
    serde_json::from_str(&run("claude", &["plugin", "list", "--json"])?)
        .map_err(|_| "`claude plugin list --json` did not answer in JSON".to_string())
}

/// The Trommi plugin of a `plugin list --json`: the user's install, else any.
fn trommi_plugin(plugins: &Value) -> Option<Value> {
    let ours = || {
        plugins
            .as_array()
            .into_iter()
            .flatten()
            .filter(|plugin| plugin["id"] == "trommi@trommi")
    };
    ours()
        .find(|plugin| plugin["scope"] == "user")
        .or_else(|| ours().next())
        .cloned()
}

/// The plugin's folder in the marketplace trommi as Claude Code keeps it: its `installLocation` and the source
/// its `.claude-plugin/marketplace.json` names. `None` when that cannot be read.
fn market_folder() -> Result<Option<std::path::PathBuf>, String> {
    let listed: Value = serde_json::from_str(&run(
        "claude",
        &["plugin", "marketplace", "list", "--json"],
    )?)
    .map_err(|_| "`claude plugin marketplace list --json` did not answer in JSON".to_string())?;
    let Some(location) = listed
        .as_array()
        .into_iter()
        .flatten()
        .find(|market| market["name"] == "trommi")
        .and_then(|market| market["installLocation"].as_str())
        .map(std::path::PathBuf::from)
    else {
        return Ok(None);
    };
    let declared: Option<Value> =
        std::fs::read_to_string(location.join(".claude-plugin/marketplace.json"))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok());
    Ok(declared.and_then(|declared| {
        declared["plugins"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|plugin| plugin["name"] == "trommi")
            .and_then(|plugin| plugin["source"].as_str())
            .filter(|source| !source.contains(".."))
            .map(|source| location.join(source))
            .filter(|folder| folder.is_dir())
    }))
}

/// Every file below `dir` (not `.git`, not Claude Code's own marks such as `.in_use`), as paths relative to it.
fn files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let mut open = vec![std::path::PathBuf::new()];
    while let Some(rel) = open.pop() {
        let Ok(entries) = std::fs::read_dir(dir.join(&rel)) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name == ".git" || name == ".in_use" || name == ".orphaned_at" {
                continue;
            }
            let path = rel.join(&name);
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => open.push(path),
                Ok(kind) if kind.is_file() => found.push(path),
                _ => {}
            }
        }
    }
    found.sort();
    found
}

/// Why the installed copy of the plugin is not the marketplace's, or `None` when it is. Without the marketplace's
/// folder at hand the copy must at least hold the plugin's command and manifest.
fn stale(plugin: &Value, market: Option<&Path>) -> Option<String> {
    let Some(copy) = plugin["installPath"].as_str().map(Path::new) else {
        return Some("Claude Code names no folder for it".to_string());
    };
    match market {
        Some(market) => files(market).into_iter().find_map(|file| {
            let want = std::fs::read(market.join(&file)).ok()?;
            match std::fs::read(copy.join(&file)) {
                Ok(have) if have == want => None,
                Ok(_) => Some(format!("{} differs", file.display())),
                Err(_) => Some(format!("{} missing", file.display())),
            }
        }),
        None => [".claude-plugin/plugin.json", "commands/connect.md"]
            .into_iter()
            .find(|file| !copy.join(file).is_file())
            .map(|file| format!("{file} missing")),
    }
}

/// `setup claude`. The lines say what was done.
pub fn claude() -> Result<Vec<String>, String> {
    let program = program()?;
    here("claude")?;
    let mut done = Vec::new();
    let source = std::env::var("TROMMI_MARKETPLACE")
        .ok()
        .filter(|source| !source.is_empty());
    let listed: Value = serde_json::from_str(&run("claude", &["plugin", "marketplace", "list", "--json"])?)
        .map_err(|_| "`claude plugin marketplace list --json` did not answer in JSON (is this Claude Code new enough? `claude update`)".to_string())?;
    let known = listed
        .as_array()
        .into_iter()
        .flatten()
        .find(|market| market["name"] == "trommi");
    match known {
        Some(market) => {
            let ours =
                source.is_some() || (market["source"] == "github" && market["repo"] == MARKETPLACE);
            if !ours {
                return Err(format!(
                    "a marketplace called trommi from another source is declared in this Claude Code ({}). Nothing was changed. If that is an earlier Trommi plugin, remove it yourself (`claude plugin uninstall trommi@trommi`, `claude plugin marketplace remove trommi`) and run this again.",
                    market["url"].as_str().or(market["path"].as_str()).or(market["repo"].as_str()).unwrap_or("unknown")
                ));
            }
            run("claude", &["plugin", "marketplace", "update", "trommi"])?;
            done.push("marketplace trommi: there already, read again".to_string());
        }
        None => {
            let mut args = vec!["plugin", "marketplace", "add"];
            match &source {
                Some(source) => args.push(source),
                None => {
                    args.push(MARKETPLACE);
                    args.push("--sparse");
                    args.extend(SPARSE);
                }
            }
            run("claude", &args)?;
            done.push(format!(
                "marketplace trommi: added ({})",
                source.as_deref().unwrap_or("github.com/chriopter/trommi")
            ));
        }
    }
    let installed = trommi_plugin(&list_plugins()?).is_some();
    if installed {
        run("claude", &["plugin", "update", "trommi@trommi"])?;
        done.push("plugin trommi@trommi: there already, brought up to date".to_string());
    } else {
        run(
            "claude",
            &["plugin", "install", "trommi@trommi", "-s", "user", "-y"],
        )?;
        done.push("plugin trommi@trommi: installed for your user".to_string());
    }
    // `plugin update` copies nothing new while the plugin's version stays the same, so a copy made before the
    // plugin got a file (commands/connect.md, say) keeps lacking it: compared with the marketplace, a copy that
    // differs is installed afresh.
    let market = market_folder()?;
    let plugin = trommi_plugin(&list_plugins()?)
        .ok_or("`claude plugin list --json` does not list trommi@trommi after installing it")?;
    if let Some(missing) = stale(&plugin, market.as_deref()) {
        run(
            "claude",
            &["plugin", "uninstall", "trommi@trommi", "-s", "user"],
        )?;
        run(
            "claude",
            &["plugin", "install", "trommi@trommi", "-s", "user", "-y"],
        )?;
        let again = trommi_plugin(&list_plugins()?)
            .ok_or("`claude plugin list --json` does not list trommi@trommi after installing it")?;
        if let Some(still) = stale(&again, market.as_deref()) {
            return Err(format!("the installed Trommi plugin is not the marketplace's even after installing it afresh ({still}); remove it (`claude plugin uninstall trommi@trommi`) and run this again"));
        }
        done.push(format!(
            "plugin trommi@trommi: the installed copy was old ({missing}), installed afresh"
        ));
    }
    let plugin = trommi_plugin(&list_plugins()?)
        .ok_or("`claude plugin list --json` does not list trommi@trommi after installing it")?;
    if plugin["enabled"] == false {
        run(
            "claude",
            &["plugin", "enable", "trommi@trommi", "-s", "user"],
        )?;
        done.push("plugin trommi@trommi: was turned off, turned on".to_string());
    }
    if let Some(dir) = usage::claude_dir() {
        done.push(format!("Claude Code folder: {}", dir.display()));
    }
    done.push(format!("the plugin starts {program}"));
    usage::mark_declined(false);
    match usage::settings_file().map(|f| usage::ensure_statusline(&f)) {
        Some(Ok(said)) => done.push(said),
        Some(Err(why)) => done.push(format!(
            "status line: not set ({why}); the board gets the plan without its usage"
        )),
        None => done.push("status line: not set (HOME is not set to a folder)".to_string()),
    }
    Ok(done)
}

/// `remove claude`: takes out what `setup claude` put in, through Claude Code's own commands, and the status line
/// (the one before stands there again). Returns the exit code.
fn remove_claude() -> i32 {
    let mut failed = false;
    usage::mark_declined(true);
    match usage::settings_file().map(|f| usage::remove_statusline(&f)) {
        Some(Ok(said)) => println!("{said}"),
        Some(Err(why)) => {
            eprintln!("[trommi] status line: {why}");
            failed = true;
        }
        None => {}
    }
    let steps: [&[&str]; 2] = [
        &["plugin", "uninstall", "trommi@trommi"],
        &["plugin", "marketplace", "remove", "trommi"],
    ];
    for args in steps {
        match run("claude", args) {
            Ok(_) => println!("claude {}: done", args.join(" ")),
            Err(why) => println!("claude {}: {why}", args.join(" ")),
        }
    }
    i32::from(failed)
}

/// `setup codex`. The lines say what was done.
pub fn codex() -> Result<Vec<String>, String> {
    let program = program()?;
    here("codex")?;
    // (a CODEX_HOME of its own goes along: Codex passes few variables to its MCP servers, and the connector's
    // `codex queue` must reach this home)
    let codex_home_given = std::env::var("CODEX_HOME").ok().filter(|h| !h.is_empty());
    let now = run("codex", &["mcp", "get", "trommi", "--json"])
        .ok()
        .and_then(|said| {
            // (Codex may print a warning before the JSON)
            let from = said.find('{')?;
            serde_json::from_str::<Value>(&said[from..]).ok()
        });
    let same = now.as_ref().is_some_and(|now| {
        now["transport"]["command"] == program.as_str()
            && now["transport"]["args"]
                .as_array()
                .is_none_or(|args| args.is_empty())
            && now["transport"]["env"]["TROMMI_CHANNEL_EVENTS"] == "off"
            && now["transport"]["env"]["CODEX_HOME"].as_str() == codex_home_given.as_deref()
            && now["enabled"] != false
    });
    let mut done = Vec::new();
    if same {
        done.push("MCP server trommi: there already, unchanged".to_string());
    } else {
        let home_env = codex_home_given.as_ref().map(|h| format!("CODEX_HOME={h}"));
        let mut args = vec!["mcp", "add", "trommi", "--env", "TROMMI_CHANNEL_EVENTS=off"];
        if let Some(e) = &home_env {
            args.extend(["--env", e.as_str()]);
        }
        args.extend(["--", program.as_str()]);
        run("codex", &args)?;
        done.push(
            if now.is_some() {
                "MCP server trommi: was set differently, now set again"
            } else {
                "MCP server trommi: added"
            }
            .to_string(),
        );
    }
    done.push(format!("Codex starts {program}"));
    done.push(merge_hooks(&program)?);
    done.push(
        "a new board event wakes the Codex session with a queued message (`codex queue`); it reads the board with the `inbox` tool"
            .to_string(),
    );
    done.push(
        "a Codex that was running before this loads Trommi only once it is started again; at its start it asks you to trust the new hooks (or /hooks): without that it does not run them"
            .to_string(),
    );
    Ok(done)
}

/// The Trommi add-on for Hermes (`connector/hermes/`), as this program carries it: its files by their path in the
/// add-on's folder.
pub const HERMES_ADDON: [(&str, &str); 8] = [
    ("plugin.yaml", include_str!("../hermes/plugin.yaml")),
    ("__init__.py", include_str!("../hermes/__init__.py")),
    ("README.md", include_str!("../hermes/README.md")),
    ("trommi_platform/__init__.py", include_str!("../hermes/trommi_platform/__init__.py")),
    ("trommi_platform/plugin.py", include_str!("../hermes/trommi_platform/plugin.py")),
    ("trommi_platform/adapter.py", include_str!("../hermes/trommi_platform/adapter.py")),
    ("trommi_platform/chats.py", include_str!("../hermes/trommi_platform/chats.py")),
    ("trommi_platform/child.py", include_str!("../hermes/trommi_platform/child.py")),
];
/// The add-on's folder name in Hermes' plugins.
pub const HERMES_ADDON_DIR: &str = "trommi-platform";

/// Writes the add-on into `dir` as one package: written whole into a folder beside it, then moved in its place, under a
/// lock (several chats' connectors may start at once). `force` (setup): always; else only for a newer release than the
/// one there (`.release`), never a build that is no release. Returns whether it wrote.
pub fn write_hermes_addon(dir: &Path, force: bool) -> Result<bool, String> {
    let spec = crate::cli::hermes_spec();
    let ours = crate::update::RELEASE_VERSION;
    let files: Vec<(&str, String)> = HERMES_ADDON
        .iter()
        .map(|(p, t)| (*p, t.to_string()))
        .chain([("trommi.json", spec), (".release", ours.to_string())])
        .collect();
    // (whether to write: looked at again under the lock, where nobody writes meanwhile)
    let wanted = || {
        let same = files.iter().all(|(rel, text)| std::fs::read_to_string(dir.join(rel)).is_ok_and(|now| now == *text));
        let there = std::fs::read_to_string(dir.join(".release")).ok().and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(0);
        !same && (force || (ours != 0 && ours > there))
    };
    if !wanted() {
        return Ok(false);
    }
    let parent = dir.parent().ok_or("the add-on's folder has no parent")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    let name = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let lock = parent.join(format!(".{name}.lock"));
    if std::fs::OpenOptions::new().write(true).create_new(true).open(&lock).is_err() {
        // (another connector writes it now; one left by a crash a minute ago is taken away)
        let old = std::fs::metadata(&lock).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age.as_secs() > 60);
        if !old {
            return Ok(false);
        }
        let _ = std::fs::remove_file(&lock);
        std::fs::OpenOptions::new().write(true).create_new(true).open(&lock).map_err(|e| format!("{}: {e}", lock.display()))?;
    }
    let done = (|| -> Result<bool, String> {
        if !wanted() {
            return Ok(false);
        }
        let stage = parent.join(format!(".{name}.new-{}", crate::util::random_hex(6)));
        for (rel, text) in &files {
            let file = stage.join(rel);
            std::fs::create_dir_all(file.parent().unwrap_or(&stage)).map_err(|e| e.to_string())?;
            std::fs::write(&file, text).map_err(|e| format!("{}: {e}", file.display()))?;
        }
        let old = parent.join(format!(".{name}.old-{}", crate::util::random_hex(6)));
        let had = dir.exists();
        if had {
            std::fs::rename(dir, &old).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        if let Err(e) = std::fs::rename(&stage, dir) {
            if had {
                let _ = std::fs::rename(&old, dir);
            }
            let _ = std::fs::remove_dir_all(&stage);
            return Err(format!("{}: {e}", dir.display()));
        }
        if had {
            let _ = std::fs::remove_dir_all(&old);
        }
        Ok(true)
    })();
    let _ = std::fs::remove_file(&lock);
    done
}

/// When this connector runs for the Hermes add-on: the add-on in the Hermes home (`TROMMI_FOLDER`) is written again
/// when this connector is a newer release than the one that wrote it. Hermes loads it at its next start.
pub fn refresh_hermes_addon() {
    let Some(home) = std::env::var_os("TROMMI_FOLDER").filter(|h| !h.is_empty()) else { return };
    let plugins = Path::new(&home).join("plugins");
    let dir = plugins.join(HERMES_ADDON_DIR);
    if !dir.is_dir() {
        // (a write that stopped between its two moves left the add-on beside its place: it goes back)
        let left = std::fs::read_dir(&plugins).ok().into_iter().flatten().flatten().map(|e| e.path()).find(|p| {
            p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&format!(".{HERMES_ADDON_DIR}.old-")))
        });
        match left {
            Some(old) if std::fs::rename(&old, &dir).is_ok() => {}
            _ => return,
        }
    }
    match write_hermes_addon(&dir, false) {
        Ok(true) => eprintln!("[trommi] the Hermes add-on in {} is this connector's now; Hermes loads it at its next start", dir.display()),
        Ok(false) => {}
        Err(why) => eprintln!("[trommi] the Hermes add-on was not written again: {why}"),
    }
}

/// One Hermes of this computer (`setup hermes [--name <name>] [--profile <profile>]`): its name on the board ("Hermes",
/// "Hermes Einkauf") and the Hermes profile it runs as (`hermes -p <profile>`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HermesInstance {
    pub name: Option<String>,
    pub profile: Option<String>,
}

impl HermesInstance {
    pub fn of(flags: &[String]) -> Result<Self, String> {
        let get = |k: &str| flags.iter().position(|f| f == k).and_then(|i| flags.get(i + 1)).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        let name = get("--name");
        if name.as_ref().is_some_and(|n| n.chars().count() > 40 || n.chars().any(char::is_control)) {
            return Err("--name: at most 40 characters, one line".into());
        }
        let profile = get("--profile");
        if profile.as_ref().is_some_and(|p| !p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')) {
            return Err("--profile: letters, digits, - and _ only".into());
        }
        Ok(Self { name, profile })
    }
    /// Its slug: "" without a name, else the name's letters and digits, lower case.
    pub fn slug(&self) -> String {
        self.name.as_deref().unwrap_or("").chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).take(32).collect()
    }
    /// Its name on the board.
    pub fn shown(&self) -> String {
        match &self.name {
            Some(n) => format!("Hermes {n}"),
            None => "Hermes".into(),
        }
    }
    /// The arguments of a `hermes` command for this Hermes: `-p <profile>` first when it has one.
    fn args<'a>(&'a self, rest: &[&'a str]) -> Vec<&'a str> {
        let mut out = vec![];
        if let Some(p) = &self.profile {
            out.extend(["-p", p.as_str()]);
        }
        out.extend(rest);
        out
    }
    fn run(&self, rest: &[&str]) -> Result<String, String> {
        run("hermes", &self.args(rest))
    }
    /// This Hermes' home: its profile's folder, or Hermes' own home.
    fn home(&self) -> Option<std::path::PathBuf> {
        let root = std::env::var_os("HERMES_HOME").filter(|h| !h.is_empty()).map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".hermes")))?;
        Some(match &self.profile {
            Some(p) => root.join("profiles").join(p),
            None => root,
        })
    }
    /// One of this Hermes' settings (`hermes config get <key> --json`), none when not set.
    fn setting(&self, key: &str) -> Option<String> {
        let said = self.run(&["config", "get", key, "--json"]).ok()?;
        let line = said.lines().map(str::trim).find(|l| !l.is_empty())?;
        let v = serde_json::from_str::<Value>(line).unwrap_or_else(|_| Value::String(line.to_string()));
        let v = match v {
            Value::String(s) => s,
            Value::Null => return None,
            v => v.to_string(),
        };
        Some(v.trim().to_string()).filter(|v| !v.is_empty())
    }
}

/// `setup hermes [--name <name>] [--profile <profile>] [--link '<invite link>']`: the Trommi add-on written into that
/// Hermes, turned on, and its platform "trommi" configured to start this connector. With `--link` (the app's "Approve a
/// computer", its Hermes tab) this computer is approved with it too, the emoji compared in this terminal as with
/// `connect --computer`, and Hermes' gateway started again: its first Trommi chat is on the board then.
pub fn hermes(flags: &[String]) -> Result<Vec<String>, String> {
    let program = program()?;
    here("hermes")?;
    let me = HermesInstance::of(flags)?;
    let link = flags.iter().position(|f| f == "--link").map(|i| flags.get(i + 1).map(|l| l.trim().to_string()).filter(|l| !l.is_empty()));
    let link = match link {
        Some(None) => return Err("--link: the invite link of \"Approve a computer\" in the app, in single quotes".into()),
        Some(Some(l)) => Some(l),
        None => None,
    };
    let home = me.home().ok_or("HOME is not set to a folder")?;
    let mut done = Vec::new();
    let dir = home.join("plugins").join(HERMES_ADDON_DIR);
    let changed = write_hermes_addon(&dir, true)?;
    done.push(format!("Trommi add-on {} in {}", if changed { "written" } else { "there already, unchanged" }, dir.display()));
    me.run(&["plugins", "enable", "trommi-platform"])?;
    done.push("Hermes plugin trommi-platform: on".to_string());
    let home_text = home.display().to_string();
    let slug = me.slug();
    let shown = me.shown();
    for (key, value) in [
        ("platforms.trommi.enabled", "true"),
        ("platforms.trommi.extra.connector", program.as_str()),
        ("platforms.trommi.extra.instance", slug.as_str()),
        ("platforms.trommi.extra.name", shown.as_str()),
        ("platforms.trommi.extra.folder", home_text.as_str()),
    ] {
        me.run(&["config", "set", "--force", key, value])?;
    }
    done.push(format!("Hermes platform trommi: on, as \"{shown}\", starting {program}"));
    // (Trommi through Hermes' MCP servers, as an earlier setup made it: the add-on replaces it)
    if me.setting("mcp_servers.trommi").is_some() {
        let _ = run_answering("hermes", &me.args(&["mcp", "remove", "trommi"]), "y\n");
        done.push("the MCP server trommi of an earlier setup: removed (the add-on brings the board tools)".to_string());
    }
    if !matches!(me.setting("streaming.enabled").as_deref(), Some("true" | "True")) {
        done.push("live text: Hermes streams answers only with `hermes config set streaming.enabled true` (this streams on its other platforms too); without it the answer comes whole".to_string());
    }
    done.push("each Trommi chat is a Hermes chat: his message goes to Hermes, its answer back into the same chat, growing while it writes; its questions for approval come as yes/no cards; \"New chat with …\" in the app makes another chat".to_string());
    if let Some(link) = link {
        // (the computer approved and the first chat's key made by the connector as the add-on starts it for that
        // chat: the same folder, name and key; the person compares the emoji here, as with `connect --computer`)
        eprintln!("[trommi] approving this computer for \"{shown}\": compare the emoji below with the ones in the app");
        let status = Command::new(&program)
            .args(["connect", "--computer", link.as_str()])
            .env_clear()
            .envs(std::env::vars_os().filter(|(k, _)| inherited(k)))
            .envs(hermes_env(&me, &home_text))
            .status()
            .map_err(|error| format!("`{program} connect` does not start ({error})"))?;
        if !status.success() {
            return Err("the add-on is set up, but this computer was not approved: make a new link in the app (Approve a computer, Hermes) and run this again".to_string());
        }
        done.push(format!("this computer: approved; \"{shown}\" is on the board"));
        match me.run(&["gateway", "restart"]) {
            Ok(_) => done.push("Hermes gateway: started again, with Trommi".to_string()),
            Err(why) => done.push(format!("Hermes gateway: not started again ({why}); start it (`hermes gateway`) to bring the Trommi chat up")),
        }
    } else {
        done.push("next: in the app, Approve a computer, then Hermes; or `trommi-connector setup hermes --link '<link>'` with its link".to_string());
    }
    Ok(done)
}

/// Whether a variable of this terminal goes on to the approving connector: not what names the agent session this runs
/// in (a Claude Code's or Codex' with Trommi), which would make it that session's connector.
fn inherited(key: &std::ffi::OsStr) -> bool {
    const SESSION: [&str; 7] = ["TROMMI_SESSION_KEY", "TROMMI_HOST", "TROMMI_INSTANCE", "TROMMI_AGENT_NAME", "TROMMI_CHANNEL_EVENTS", "TROMMI_JOIN_OTHER", "TROMMI_SPARE"];
    let key = key.to_string_lossy();
    !(SESSION.contains(&key.as_ref()) || key.starts_with("CLAUDE") || key.starts_with("CODEX_"))
}

/// The environment the add-on gives the connector of this Hermes' first chat (trommi_platform/chats.py `env`).
pub fn hermes_env(me: &HermesInstance, folder: &str) -> Vec<(&'static str, String)> {
    let slug = me.slug();
    let key = if slug.is_empty() { "hermes".to_string() } else { format!("hermes-{slug}") };
    vec![
        ("TROMMI_HOST", "hermes".into()),
        ("TROMMI_INSTANCE", slug),
        ("TROMMI_AGENT_NAME", me.shown()),
        ("TROMMI_SESSION_KEY", key),
        ("TROMMI_FOLDER", folder.to_string()),
    ]
}

/// `remove hermes [--name] [--profile]`: the platform and the add-on off, the add-on's folder taken away.
pub fn remove_hermes(flags: &[String]) -> Result<Vec<String>, String> {
    let me = HermesInstance::of(flags)?;
    let mut done = Vec::new();
    match me.run(&["config", "set", "--force", "platforms.trommi.enabled", "false"]) {
        Ok(_) => done.push("Hermes platform trommi: off".to_string()),
        Err(why) => done.push(format!("Hermes platform trommi: not turned off ({why})")),
    }
    match me.run(&["plugins", "disable", "trommi-platform"]) {
        Ok(_) => done.push("Hermes plugin trommi-platform: off".to_string()),
        Err(why) => done.push(format!("Hermes plugin trommi-platform: not turned off ({why})")),
    }
    if let Some(dir) = me.home().map(|h| h.join("plugins").join(HERMES_ADDON_DIR)) {
        if std::fs::remove_dir_all(&dir).is_ok() {
            done.push(format!("the add-on's folder removed ({})", dir.display()));
        }
    }
    done.push("the Trommi chats of this Hermes stay on the board until you archive them".to_string());
    Ok(done)
}

/// `run` with these lines on its standard input (the program's own questions: yes).
fn run_answering(program: &str, args: &[&str], answers: &str) -> Result<String, String> {
    use std::io::Write;
    let mut child = Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("`{program}` does not start ({error})"))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(answers.as_bytes());
    }
    let output = child.wait_with_output().map_err(|error| format!("`{program}` did not end ({error})"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(format!("`{program} {}` failed: {}", args.first().copied().unwrap_or(""), String::from_utf8_lossy(&output.stderr).trim()))
    }
}

/// Codex's own folder: `CODEX_HOME`, else `~/.codex`.
pub fn codex_home() -> Option<std::path::PathBuf> {
    match std::env::var_os("CODEX_HOME").filter(|h| !h.is_empty()) {
        Some(h) => Some(h.into()),
        None => std::env::var_os("HOME")
            .filter(|h| !h.is_empty())
            .map(|h| std::path::PathBuf::from(h).join(".codex")),
    }
}

/// The hooks the connector puts into Codex: (event, connector command, timeout in seconds). Codex has no
/// Notification, PermissionDenied, PostToolUseFailure or MessageDisplay events, and no async hooks.
pub const CODEX_HOOKS: [(&str, &str, u64); 9] = [
    ("UserPromptSubmit", "prompt", 5),
    ("Stop", "stop", 5),
    ("PermissionRequest", "permission", 3700),
    ("PreToolUse", "trail", 5),
    ("PostToolUse", "resolved", 30),
    ("PostToolUse", "trail", 5),
    ("SubagentStart", "trail", 5),
    ("SubagentStop", "trail", 5),
    ("SessionEnd", "trail", 5),
];

/// Whether a hook's command is one of the connector's (any connector, at any path).
fn ours(hook: &Value) -> bool {
    hook["command"]
        .as_str()
        .is_some_and(|c| c.contains("trommi-connector"))
}

fn quoted(path: &str) -> String {
    format!("'{}'", path.replace('\'', "'\\''"))
}

/// The hooks file with every hook of the connector taken out (groups left empty go too).
fn without_ours(mut doc: Value) -> Result<Value, String> {
    if doc.is_null() {
        doc = serde_json::json!({});
    }
    let Some(top) = doc.as_object_mut() else {
        return Err("hooks.json is not a JSON object; nothing was changed".into());
    };
    let hooks = top.entry("hooks").or_insert_with(|| serde_json::json!({}));
    let Some(events) = hooks.as_object_mut() else {
        return Err("hooks.json's \"hooks\" is not an object; nothing was changed".into());
    };
    for groups in events.values_mut() {
        if let Some(list) = groups.as_array_mut() {
            for group in list.iter_mut() {
                if let Some(inner) = group.get_mut("hooks").and_then(|h| h.as_array_mut()) {
                    inner.retain(|h| !ours(h));
                }
            }
            list.retain(|g| g["hooks"].as_array().is_none_or(|h| !h.is_empty()));
        }
    }
    events.retain(|_, groups| groups.as_array().is_none_or(|l| !l.is_empty()));
    Ok(doc)
}

/// `doc` with the connector's hooks for `program`, one group per event at the end, other hooks unchanged.
pub fn with_codex_hooks(doc: Value, program: &str) -> Result<Value, String> {
    let mut doc = without_ours(doc)?;
    let events = doc["hooks"].as_object_mut().expect("an object");
    for (event, verb, timeout) in CODEX_HOOKS {
        let hook = serde_json::json!({
            "type": "command",
            "command": format!("p={}; [ -x \"$p\" ] || exit 0; exec \"$p\" {verb}", quoted(program)),
            "timeout": timeout,
        });
        let list = events.entry(event).or_insert_with(|| serde_json::json!([]));
        let Some(list) = list.as_array_mut() else {
            return Err(format!(
                "hooks.json's {event} is not a list; nothing was changed"
            ));
        };
        // (one group of the connector's per event, holding its hooks in order)
        match list.iter_mut().find(|g| {
            g.get("matcher").is_none()
                && g["hooks"].as_array().is_some_and(|h| h.iter().all(ours))
                && g["hooks"].as_array().is_some_and(|h| !h.is_empty())
        }) {
            Some(group) => group["hooks"].as_array_mut().unwrap().push(hook),
            None => list.push(serde_json::json!({ "hooks": [hook] })),
        }
    }
    Ok(doc)
}

fn read_hooks(file: &Path) -> Result<Value, String> {
    match std::fs::read_to_string(file) {
        Ok(text) if text.trim().is_empty() => Ok(Value::Null),
        Ok(text) => serde_json::from_str(&text).map_err(|e| {
            format!(
                "{} is not valid JSON ({e}); nothing was changed",
                file.display()
            )
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Value::Null),
        Err(e) => Err(format!("{} does not read ({e})", file.display())),
    }
}

fn write_hooks(file: &Path, doc: &Value) -> Result<(), String> {
    let dir = file.parent().ok_or("no folder for hooks.json")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{} not made ({e})", dir.display()))?;
    let tmp = dir.join(format!(".hooks.json.trommi-{}", std::process::id()));
    let text = serde_json::to_string_pretty(doc).unwrap() + "\n";
    std::fs::write(&tmp, text)
        .and_then(|_| {
            if let Ok(m) = std::fs::metadata(file) {
                std::fs::set_permissions(&tmp, m.permissions())?;
            }
            std::fs::rename(&tmp, file)
        })
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("{} not written ({e})", file.display())
        })
}

/// Puts the connector's hooks into Codex's hooks.json; the line says what was done.
fn merge_hooks(program: &str) -> Result<String, String> {
    let file = codex_home()
        .ok_or("HOME is not set to a folder")?
        .join("hooks.json");
    let was = read_hooks(&file)?;
    let now = with_codex_hooks(was.clone(), program)?;
    if now == was {
        return Ok(format!(
            "hooks in {}: there already, unchanged",
            file.display()
        ));
    }
    write_hooks(&file, &now)?;
    Ok(format!(
        "hooks in {}: terminal mirror, permission prompts on the board and work trail {} (other hooks kept)",
        file.display(),
        if was.is_null() { "added" } else { "set" }
    ))
}

/// `remove codex`. The lines say what was done.
pub fn remove_codex() -> Result<Vec<String>, String> {
    let mut done = Vec::new();
    let file = codex_home()
        .ok_or("HOME is not set to a folder")?
        .join("hooks.json");
    let was = read_hooks(&file)?;
    if was.is_null() {
        done.push(format!(
            "hooks: {} is not there, nothing to take out",
            file.display()
        ));
    } else {
        let now = without_ours(was.clone())?;
        if now == was {
            done.push(format!(
                "hooks in {}: none of Trommi's there",
                file.display()
            ));
        } else {
            write_hooks(&file, &now)?;
            done.push(format!(
                "hooks in {}: Trommi's taken out, others kept",
                file.display()
            ));
        }
    }
    here("codex")?;
    if run("codex", &["mcp", "get", "trommi", "--json"]).is_ok() {
        run("codex", &["mcp", "remove", "trommi"])?;
        done.push("MCP server trommi: removed".to_string());
    } else {
        done.push("MCP server trommi: not there".to_string());
    }
    Ok(done)
}

/// Whether this Codex runs the connector's mirror hooks: they are in its hooks.json and the human trusted them
/// (a trusted hash for the Stop and UserPromptSubmit hooks of that file in its config.toml). Read at start, for the
/// instructions; a hook trusted later counts from Codex's next start.
pub fn codex_mirror_hooks() -> bool {
    let Some(home) = codex_home() else {
        return false;
    };
    let file = home.join("hooks.json");
    let Ok(doc) = read_hooks(&file) else {
        return false;
    };
    let has = |event: &str, verb: &str| {
        doc["hooks"][event].as_array().is_some_and(|groups| {
            groups.iter().any(|g| {
                g["hooks"].as_array().is_some_and(|h| {
                    h.iter().any(|h| {
                        ours(h)
                            && h["command"]
                                .as_str()
                                .is_some_and(|c| c.ends_with(&format!(" {verb}")))
                    })
                })
            })
        })
    };
    if !has("Stop", "stop") || !has("UserPromptSubmit", "prompt") {
        return false;
    }
    let config = std::fs::read_to_string(home.join("config.toml")).unwrap_or_default();
    let key = file.display().to_string();
    ["stop", "user_prompt_submit"].iter().all(|ev| {
        config
            .lines()
            .any(|l| l.starts_with("[hooks.state.") && l.contains(&format!("{key}:{ev}:")))
    })
}

/// Whether `path` is the installed connector.
pub fn is_installed(path: &Path) -> bool {
    crate::update::installed_program().is_some_and(|program| {
        matches!((std::fs::canonicalize(path), std::fs::canonicalize(program)), (Ok(a), Ok(b)) if a == b)
    })
}

#[cfg(test)]
mod hermes_tests {
    use super::*;

    #[test]
    fn a_hermes_has_a_name_profile_and_slug_of_its_own() {
        let f = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let one = HermesInstance::of(&[]).unwrap();
        assert_eq!((one.shown(), one.slug(), one.args(&["plugins"])), ("Hermes".to_string(), String::new(), vec!["plugins"]));
        let two = HermesInstance::of(&f(&["--name", "Einkauf", "--profile", "einkauf"])).unwrap();
        assert_eq!((two.shown(), two.slug(), two.args(&["plugins"])), ("Hermes Einkauf".to_string(), "einkauf".to_string(), vec!["-p", "einkauf", "plugins"]));
        assert!(HermesInstance::of(&f(&["--profile", "../x"])).is_err());
    }

    #[test]
    fn the_addon_is_written_whole_and_only_when_it_changed() {
        let base = std::env::temp_dir().join(format!("trommi-addon-{}", crate::util::random_hex(6)));
        let dir = base.join(HERMES_ADDON_DIR);
        assert!(write_hermes_addon(&dir, true).unwrap());
        for (rel, text) in HERMES_ADDON {
            assert_eq!(std::fs::read_to_string(dir.join(rel)).unwrap(), text);
        }
        let spec: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("trommi.json")).unwrap()).unwrap();
        assert!(spec["tools"].as_array().unwrap().iter().any(|t| t["name"] == "create_decision"));
        assert!(!write_hermes_addon(&dir, true).unwrap(), "nothing new: nothing written");
        std::fs::write(dir.join("trommi_platform/adapter.py"), "old").unwrap();
        // (a build that is no release never replaces it by itself; setup does)
        assert!(!write_hermes_addon(&dir, false).unwrap());
        assert!(write_hermes_addon(&dir, true).unwrap());
        assert_eq!(std::fs::read_dir(&base).unwrap().count(), 1, "no stage, old folder or lock left");
        let _ = std::fs::remove_dir_all(&base);
    }
}
