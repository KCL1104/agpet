# agpet

**agpet** turns your AI coding agents into walking desktop pets. It's a transparent, click-through overlay (built with Tauri + vanilla TypeScript) that sits on top of your desktop: each running agent is a little creature that roams the screen. Click one to open its own chat panel, and it gets to work in whatever project directory you point it at.

Under the hood, every pet is a live agent connected over the [Agent Client Protocol (ACP)](https://agentclientprotocol.com). Out of the box agpet speaks to Claude Code, Codex, OpenCode, GitHub Copilot, and Gemini — and you can add or tweak agents yourself in a simple `agents.toml`. Each agent reuses its own CLI login, so agpet never asks for API keys.

## Features

- **Pets on your desktop** — one walking pet per running agent instance; launch and manage them from the system tray. Run several at once, even multiple of the same type.
- **Per-pet chat** — click a pet to open its chat panel, with Markdown rendering, `@`-mention file picker, image paste, and per-agent mode/model switching.
- **Multi-agent orchestration** — a "mother" agent can hand subtasks to its siblings through a built-in MCP `delegate` tool, then `collect` their results. Delegated work runs in an isolated git worktree so it can't clobber your tree.
- **Workflows** — chain agents into a sequential pipeline (e.g. *plan with Claude → execute with Codex → review with Claude*), with handoffs animated pet-to-pet. Define your own in `workflows/*.yaml`.
- **Your CLIs, your logins** — agents launch their own ACP adapters; no provider keys stored. Session history is kept locally in SQLite.

## Download

Grab the latest build from the [**Releases**](https://github.com/KCL1104/agpet/releases) page:

| Platform | File |
| --- | --- |
| **Windows** | `.exe` (NSIS installer) or `.msi` |
| **macOS** | `.dmg` — universal, runs on both Apple Silicon and Intel |

> **macOS:** the app isn't Apple-notarized yet, so macOS may say it "is damaged and can't be opened." Clear the quarantine flag once after installing:
> ```sh
> xattr -cr /Applications/agpet.app
> ```

You still need the agent CLIs you want to use installed and logged in (e.g. `claude`, `codex`, `opencode`, `copilot`, `gemini`). agpet reuses each CLI's own login — it never stores API keys.

## Build from source

Prerequisites: [Rust](https://www.rust-lang.org/tools/install) (stable), [Node.js](https://nodejs.org) (LTS), and the [Tauri v2 system dependencies](https://v2.tauri.app/start/prerequisites/) for your OS.

```sh
npm install          # install frontend dependencies
npm run tauri dev    # run in development
npm run tauri build  # produce a release bundle in src-tauri/target/release/bundle
```

## Releases & CI/CD

Releases are produced automatically by GitHub Actions ([`.github/workflows/release.yml`](.github/workflows/release.yml)). On every push to `main` (and on demand via the Actions tab), the workflow builds the Tauri app on macOS and Windows runners and publishes the installers to a GitHub Release tagged `app-v<version>`, where `<version>` comes from `src-tauri/tauri.conf.json`.

- Pushing to `main` **without** changing the version updates the assets on the existing `app-v<version>` release.
- **Bump `version` in `src-tauri/tauri.conf.json`** to cut a brand-new release.

Builds are currently unsigned. To ship signed/notarized macOS builds and signed Windows installers, add the relevant secrets and follow the [Tauri code-signing guides](https://v2.tauri.app/distribute/sign/).

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
