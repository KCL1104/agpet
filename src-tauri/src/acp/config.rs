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
}

fn default_color() -> String {
    "#e8743b".to_string()
}

impl AgentDef {
    /// Full argv for [`agent_client_protocol::AcpAgent::from_args`], wrapping in
    /// `cmd /c` on Windows so `.cmd` shims resolve and run.
    pub fn argv(&self) -> Vec<String> {
        let mut v = Vec::with_capacity(self.args.len() + 3);
        if cfg!(windows) {
            v.push("cmd".to_string());
            v.push("/c".to_string());
        }
        v.push(self.command.clone());
        v.extend(self.args.iter().cloned());
        v
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

fn default_agents() -> Vec<AgentDef> {
    vec![
        AgentDef {
            id: "claude".into(),
            name: "Claude Code".into(),
            command: "npx".into(),
            args: vec!["-y".into(), "@zed-industries/claude-code-acp".into()],
            color: "#da7756".into(),
        },
        AgentDef {
            id: "codex".into(),
            name: "Codex".into(),
            command: "npx".into(),
            args: vec!["-y".into(), "@zed-industries/codex-acp".into()],
            color: "#10a37f".into(),
        },
        AgentDef {
            id: "opencode".into(),
            name: "OpenCode".into(),
            command: "opencode".into(),
            args: vec!["acp".into()],
            color: "#6e7681".into(),
        },
        AgentDef {
            id: "copilot".into(),
            name: "Copilot".into(),
            command: "copilot".into(),
            args: vec!["--acp".into()],
            color: "#8b5cf6".into(),
        },
        AgentDef {
            id: "gemini".into(),
            name: "Gemini".into(),
            command: "gemini".into(),
            args: vec!["--experimental-acp".into()],
            color: "#4e8cf5".into(),
        },
    ]
}

const DEFAULT_AGENTS_TOML: &str = r##"# agpet agents — one pet per agent. Edit freely; restart the app to apply.
# `command` + `args` are run via `cmd /c` on Windows so npx/.cmd shims work.
# Each agent uses its own CLI login (run e.g. `claude /login`, Codex/OpenCode auth).

[[agent]]
id = "claude"
name = "Claude Code"
command = "npx"
args = ["-y", "@zed-industries/claude-code-acp"]
color = "#da7756"

[[agent]]
id = "codex"
name = "Codex"
command = "npx"
args = ["-y", "@zed-industries/codex-acp"]
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
