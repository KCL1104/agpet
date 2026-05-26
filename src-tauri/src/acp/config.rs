//! Configuration for spawning the ACP adapter.
//!
//! Nothing about the agent command is hard-coded deeper in the stack — the
//! command, args, working directory and log path all live here so they can be
//! made user-configurable later (spec: "All paths configurable, never hard-coded").

use std::path::PathBuf;

use anyhow::Context;
use tauri::{AppHandle, Manager};

/// How to spawn the ACP adapter, where it runs, and where to log its messages.
#[derive(Debug, Clone)]
pub struct AcpConfig {
    /// Program to execute (e.g. `cmd` on Windows, `npx` elsewhere).
    pub command: String,
    /// Arguments following the program.
    pub args: Vec<String>,
    /// Working directory passed to `session/new` (must be absolute).
    pub cwd: PathBuf,
    /// JSONL file that every JSON-RPC line (both directions) is appended to.
    pub message_log_path: PathBuf,
}

impl AcpConfig {
    /// Default config: spawn `@zed-industries/claude-code-acp` via npx.
    ///
    /// On Windows `npx` is a `.cmd` batch script that `CreateProcess` cannot run
    /// directly, so we go through `cmd /c`. We deliberately do **not** set
    /// `ANTHROPIC_API_KEY`: the adapter reuses the existing Claude Code login
    /// (run `claude /login` once). If a key were set it would override the
    /// subscription, so we leave the environment untouched.
    pub fn default_for(app: &AppHandle) -> anyhow::Result<Self> {
        let (command, args) = if cfg!(windows) {
            (
                "cmd".to_string(),
                vec![
                    "/c".into(),
                    "npx".into(),
                    "-y".into(),
                    "@zed-industries/claude-code-acp@latest".into(),
                ],
            )
        } else {
            (
                "npx".to_string(),
                vec!["-y".into(), "@zed-industries/claude-code-acp@latest".into()],
            )
        };

        let cwd = std::env::current_dir().context("resolve current dir for session cwd")?;

        let log_dir = app.path().app_log_dir().context("resolve app log dir")?;
        std::fs::create_dir_all(&log_dir).context("create app log dir")?;
        let message_log_path = log_dir.join("acp-messages.jsonl");

        Ok(Self {
            command,
            args,
            cwd,
            message_log_path,
        })
    }

    /// Full argv (program + args) for [`agent_client_protocol::AcpAgent::from_args`].
    pub fn argv(&self) -> Vec<String> {
        let mut v = Vec::with_capacity(1 + self.args.len());
        v.push(self.command.clone());
        v.extend(self.args.iter().cloned());
        v
    }
}
