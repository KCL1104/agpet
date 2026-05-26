# agpet

**agpet** turns your AI coding agents into walking desktop pets. It's a transparent, click-through overlay (built with Tauri + vanilla TypeScript) that sits on top of your desktop: each running agent is a little creature that roams the screen. Click one to open its own chat panel, and it gets to work in whatever project directory you point it at.

Under the hood, every pet is a live agent connected over the [Agent Client Protocol (ACP)](https://agentclientprotocol.com). Out of the box agpet speaks to Claude Code, Codex, OpenCode, GitHub Copilot, and Gemini — and you can add or tweak agents yourself in a simple `agents.toml`. Each agent reuses its own CLI login, so agpet never asks for API keys.

## Features

- **Pets on your desktop** — one walking pet per running agent instance; launch and manage them from the system tray. Run several at once, even multiple of the same type.
- **Per-pet chat** — click a pet to open its chat panel, with Markdown rendering, `@`-mention file picker, image paste, and per-agent mode/model switching.
- **Multi-agent orchestration** — a "mother" agent can hand subtasks to its siblings through a built-in MCP `delegate` tool, then `collect` their results. Delegated work runs in an isolated git worktree so it can't clobber your tree.
- **Workflows** — chain agents into a sequential pipeline (e.g. *plan with Claude → execute with Codex → review with Claude*), with handoffs animated pet-to-pet. Define your own in `workflows/*.yaml`.
- **Your CLIs, your logins** — agents launch their own ACP adapters; no provider keys stored. Session history is kept locally in SQLite.

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
