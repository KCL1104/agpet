//! Multi-agent configuration (Milestone 3).
//!
//! Agents are declared in a user-editable `agents.toml` under the app config
//! dir. Nothing about an agent's launch command is hard-coded deeper in the
//! stack. We deliberately do **not** inject `ANTHROPIC_API_KEY` / provider keys
//! — each adapter reuses its own CLI's login (e.g. `claude /login`).

use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

/// One declared agent (a pet) and how to launch its ACP adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDef {
    pub id: String,
    pub name: String,
    /// Program to run (logical, e.g. `npx` or `opencode`). On Windows it is
    /// invoked via `cmd /c` so `.cmd` shims (npx, npm-installed bins) work.
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Pet body colour (hex).
    #[serde(default = "default_color")]
    pub color: String,
    /// Run this agent's CLI inside WSL (Windows Subsystem for Linux). Use this
    /// when the agent CLI is installed in your WSL distro rather than native
    /// Windows. The command runs in a `bash` login shell so your profile's PATH
    /// (nvm, etc.) applies, and working-directory / `@`-mention paths are
    /// translated to their `/mnt/<drive>/…` form. Ignored on non-Windows hosts.
    #[serde(default)]
    pub wsl: bool,
    /// Optional WSL distro to use (`wsl -d <distro>`). Defaults to your default
    /// distro. Only relevant when `wsl = true`.
    #[serde(default)]
    pub wsl_distro: Option<String>,
}

fn default_color() -> String {
    "#e8743b".to_string()
}

/// Whether this agent should be launched through WSL on the current host.
/// WSL only exists on Windows, so this is always false elsewhere.
fn use_wsl(def: &AgentDef) -> bool {
    cfg!(windows) && def.wsl
}

/// Single-quote a string for a POSIX shell command line, escaping embedded
/// single quotes (`'` -> `'\''`).
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Translate a Windows path (`C:\Users\me\proj`) into its WSL mount equivalent
/// (`/mnt/c/Users/me/proj`) so an agent running inside WSL can resolve it.
/// Paths that are already POSIX-style, or UNC paths, are returned with forward
/// slashes and otherwise unchanged.
pub fn win_to_wsl_path(path: &std::path::Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let bytes = s.as_bytes();
    // Drive-letter absolute path: "C:/..." (or bare "C:").
    if bytes.len() >= 2 && bytes[1] == b':' && (bytes[0] as char).is_ascii_alphabetic() {
        let drive = (bytes[0] as char).to_ascii_lowercase();
        let rest = s[2..].strip_prefix('/').unwrap_or(&s[2..]);
        return format!("/mnt/{drive}/{rest}");
    }
    s
}

impl AgentDef {
    /// Full argv for [`agent_client_protocol::AcpAgent::from_args`].
    ///
    /// - With `wsl = true` on Windows: `wsl.exe [-d <distro>] -- bash -lc '<cmd>'`
    ///   so the CLI installed inside WSL runs with the user's login-shell PATH.
    /// - On Windows otherwise: wrapped in `cmd /c` so `.cmd` shims (npx,
    ///   npm-installed bins) resolve and run.
    /// - Elsewhere: the command and args verbatim.
    pub fn argv(&self) -> Vec<String> {
        if use_wsl(self) {
            // Build a single POSIX command line for `bash -lc`.
            let mut cmdline = sh_quote(&self.command);
            for a in &self.args {
                cmdline.push(' ');
                cmdline.push_str(&sh_quote(a));
            }
            let mut v = Vec::with_capacity(6);
            v.push("wsl.exe".to_string());
            if let Some(distro) = self.wsl_distro.as_deref().filter(|d| !d.is_empty()) {
                v.push("-d".to_string());
                v.push(distro.to_string());
            }
            v.push("--".to_string());
            v.push("bash".to_string());
            v.push("-lc".to_string());
            v.push(cmdline);
            return v;
        }
        let mut v = Vec::with_capacity(self.args.len() + 3);
        if cfg!(windows) {
            v.push("cmd".to_string());
            v.push("/c".to_string());
        }
        v.push(self.command.clone());
        v.extend(self.args.iter().cloned());
        v
    }

    /// Translate `cwd` to the form the agent should receive: the WSL mount path
    /// when this agent runs inside WSL, otherwise the path unchanged. Used for
    /// the session cwd and `@`-mention file URIs (never for local filesystem or
    /// git operations, which must keep the native Windows path).
    pub fn agent_path(&self, cwd: &std::path::Path) -> std::path::PathBuf {
        if use_wsl(self) {
            std::path::PathBuf::from(win_to_wsl_path(cwd))
        } else {
            cwd.to_path_buf()
        }
    }
}

#[derive(Debug, Deserialize)]
struct AgentsFile {
    #[serde(rename = "agent", default)]
    agents: Vec<AgentDef>,
}

/// Loaded agent set plus shared runtime paths.
pub struct AgentsConfig {
    pub agents: Vec<AgentDef>,
    pub cwd: PathBuf,
    pub log_dir: PathBuf,
}

impl AgentsConfig {
    /// Load `agents.toml` from the app config dir, writing defaults if missing.
    pub fn load(app: &AppHandle) -> anyhow::Result<Self> {
        let cfg_dir = app.path().app_config_dir().context("app config dir")?;
        std::fs::create_dir_all(&cfg_dir).context("create config dir")?;
        let path = cfg_dir.join("agents.toml");

        let agents = if path.exists() {
            let txt = std::fs::read_to_string(&path).context("read agents.toml")?;
            let parsed: AgentsFile = toml::from_str(&txt).context("parse agents.toml")?;
            parsed.agents
        } else {
            std::fs::write(&path, DEFAULT_AGENTS_TOML).context("write default agents.toml")?;
            tracing::info!("wrote default agents.toml at {}", path.display());
            default_agents()
        };
        let agents = if agents.is_empty() { default_agents() } else { agents };

        let cwd = std::env::current_dir().context("current dir")?;
        let log_dir = app.path().app_log_dir().context("app log dir")?;
        std::fs::create_dir_all(&log_dir).context("create log dir")?;

        Ok(Self { agents, cwd, log_dir })
    }
}

/// Pinned ACP adapter packages (exact versions). See the comment in
/// [`default_agents`] for why these are pinned rather than `@latest`.
const CLAUDE_ADAPTER: &str = "@agentclientprotocol/claude-agent-acp@0.47.0";
const CODEX_ADAPTER: &str = "@zed-industries/codex-acp@0.16.0";

fn default_agents() -> Vec<AgentDef> {
    vec![
        AgentDef {
            id: "claude".into(),
            name: "Claude Code".into(),
            command: "npx".into(),
            // Pinned (not @latest): npx -y runs whatever npm resolves at launch, so
            // an unpinned name executes an arbitrary new upstream release on every
            // cold start. Bump deliberately; users can override in agents.toml.
            args: vec!["-y".into(), CLAUDE_ADAPTER.into()],
            color: "#da7756".into(),
            wsl: false,
            wsl_distro: None,
        },
        AgentDef {
            id: "codex".into(),
            name: "Codex".into(),
            command: "npx".into(),
            args: vec!["-y".into(), CODEX_ADAPTER.into()],
            color: "#10a37f".into(),
            wsl: false,
            wsl_distro: None,
        },
        AgentDef {
            id: "opencode".into(),
            name: "OpenCode".into(),
            command: "opencode".into(),
            args: vec!["acp".into()],
            color: "#6e7681".into(),
            wsl: false,
            wsl_distro: None,
        },
        AgentDef {
            id: "copilot".into(),
            name: "Copilot".into(),
            command: "copilot".into(),
            args: vec!["--acp".into()],
            color: "#8b5cf6".into(),
            wsl: false,
            wsl_distro: None,
        },
        AgentDef {
            id: "gemini".into(),
            name: "Gemini".into(),
            command: "gemini".into(),
            args: vec!["--experimental-acp".into()],
            color: "#4e8cf5".into(),
            wsl: false,
            wsl_distro: None,
        },
    ]
}

const DEFAULT_AGENTS_TOML: &str = r##"# agpet agents — one pet per agent. Edit freely; restart the app to apply.
# `command` + `args` are run via `cmd /c` on Windows so npx/.cmd shims work.
# Each agent uses its own CLI login (run e.g. `claude /login`, Codex/OpenCode auth).
#
# WSL (Windows only): if an agent's CLI lives in your WSL distro rather than
# native Windows, add `wsl = true` (and optionally `wsl_distro = "Ubuntu"`). The
# command then runs in a WSL `bash` login shell and working-directory paths are
# translated to /mnt/<drive>/… automatically. Example:
#   [[agent]]
#   id = "claude-wsl"
#   name = "Claude (WSL)"
#   command = "npx"
#   args = ["-y", "@agentclientprotocol/claude-agent-acp"]
#   wsl = true
#   # wsl_distro = "Ubuntu"

[[agent]]
id = "claude"
name = "Claude Code"
command = "npx"
# Pinned to an exact version (not @latest) so launches are deterministic and an
# upstream release can't run automatically. Bump deliberately to update.
args = ["-y", "@agentclientprotocol/claude-agent-acp@0.47.0"]
color = "#da7756"

[[agent]]
id = "codex"
name = "Codex"
command = "npx"
args = ["-y", "@zed-industries/codex-acp@0.16.0"]
color = "#10a37f"

[[agent]]
id = "opencode"
name = "OpenCode"
command = "opencode"
args = ["acp"]
color = "#6e7681"

# GitHub Copilot CLI (ACP public preview): needs `copilot` installed + logged in.
[[agent]]
id = "copilot"
name = "Copilot"
command = "copilot"
args = ["--acp"]
color = "#8b5cf6"

# Google Gemini CLI (reference ACP impl): needs `gemini` installed + logged in.
[[agent]]
id = "gemini"
name = "Gemini"
command = "gemini"
args = ["--experimental-acp"]
color = "#4e8cf5"
"##;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn win_to_wsl_path_translates_drive_letters() {
        // Forward-slash drive paths exercise the drive-letter → /mnt/<d> mapping.
        assert_eq!(win_to_wsl_path(Path::new("C:/Users/me/proj")), "/mnt/c/Users/me/proj");
        assert_eq!(win_to_wsl_path(Path::new("D:/data")), "/mnt/d/data");
        // Already-POSIX paths pass through unchanged.
        assert_eq!(win_to_wsl_path(Path::new("/home/me/x")), "/home/me/x");
    }

    #[test]
    fn sh_quote_wraps_in_single_quotes() {
        assert_eq!(sh_quote("plain"), "'plain'");
        assert_eq!(sh_quote("a b"), "'a b'");
        // An embedded single quote is escaped; the result still parses as one
        // shell word (begins and ends with a quote, longer than the input).
        let q = sh_quote("a'b");
        assert!(q.starts_with("'") && q.ends_with("'"));
        assert!(q.len() > "a'b".len() + 2);
    }

    #[test]
    fn default_agents_pin_adapter_versions() {
        // The default claude/codex adapters must be pinned (contain '@<version>'),
        // never resolving to @latest at launch.
        let agents = default_agents();
        let claude = agents.iter().find(|a| a.id == "claude").unwrap();
        assert!(claude.args.last().unwrap().contains("@agentclientprotocol/claude-agent-acp@"));
        let codex = agents.iter().find(|a| a.id == "codex").unwrap();
        assert!(codex.args.last().unwrap().contains("@zed-industries/codex-acp@"));
    }
}
