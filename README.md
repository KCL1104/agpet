# agpet

**English** · [繁體中文](README.zh-TW.md) · [简体中文](README.zh-CN.md)

**agpet** turns your AI coding agents into walking desktop pets. It's a transparent, click-through overlay (built with Tauri + vanilla TypeScript) that sits on top of your desktop: each running agent is a little creature that roams the screen. Click one to open its own chat panel, and it gets to work in whatever project directory you point it at.

Under the hood, every pet is a live agent connected over the [Agent Client Protocol (ACP)](https://agentclientprotocol.com). Out of the box agpet speaks to Claude Code, Codex, OpenCode, GitHub Copilot, and Gemini — and you can add or tweak agents yourself in a simple `agents.toml`. Each agent reuses its own CLI login, so agpet never asks for API keys.

## Features

- **Pets on your desktop** — one walking pet per running agent instance; launch and manage them from the system tray. Run several at once, even multiple of the same type.
- **Per-pet chat** — click a pet to open its chat panel, with Markdown rendering, `@`-mention file picker, image paste, and per-agent mode/model switching.
- **Multi-agent orchestration** — a "mother" agent can hand subtasks to its siblings through a built-in MCP `delegate` tool, then `collect` their results. Delegated work runs in an isolated git worktree so it can't clobber your tree.
- **Workflows** — chain agents into a sequential pipeline (e.g. *plan with Claude → execute with Codex → review with Claude*), with handoffs animated pet-to-pet. Define your own in `workflows/*.yaml`.
- **Your CLIs, your logins** — agents launch their own ACP adapters; no provider keys stored. Session history is kept locally in SQLite.

---

## Quick start

### 1. Install agpet

Download the latest build from the [**Releases**](https://github.com/KCL1104/agpet/releases) page (see [Download](#download) for per-platform notes), or [build from source](#build-from-source).

### 2. Install and log in to at least one agent CLI

agpet doesn't ship the agents — it drives the CLIs you already have. Install at least one and sign in once, so the CLI carries its own login:

| Agent | Install | Log in |
| --- | --- | --- |
| **Claude Code** | `npm i -g @anthropic-ai/claude-code` | `claude` then `/login` |
| **Codex** | see Codex CLI docs | follow its auth flow |
| **OpenCode** | see [opencode.ai](https://opencode.ai) | `opencode auth login` |
| **GitHub Copilot** | `npm i -g @github/copilot` | `copilot` then `/login` |
| **Gemini** | `npm i -g @google/gemini-cli` | `gemini` then sign in |

> You only need the ones you plan to use. agpet reuses each CLI's own login — it never stores API keys. The default `agents.toml` invokes the ACP adapters via `npx`, so even Claude Code and Codex work without a separate global install, as long as you're logged in.

### 3. Launch your first pet

1. Start agpet. It runs as a **system-tray icon** (there's no normal window — the desktop overlay is transparent).
2. Click the tray icon and choose **Open Launcher…**.
3. Pick an **agent type** (e.g. *Claude Code*) and a **working directory** — this is the project folder the agent will operate in.
4. Launch. A pet appears and starts roaming your desktop. Its color comes from the agent's `color` in `agents.toml`.

### 4. Chat with your pet

- **Click the pet** to open its chat panel.
- Type a task and press **Enter** to send (**Shift+Enter** for a newline).
- The agent works inside the directory you chose, streaming Markdown replies, thinking blocks, tool calls, and diffs. When it needs permission for a sensitive action, an **Allow / Deny** bar appears.

### 5. Run a team

Launch more pets — even several of the same agent type — and they roam together. From here you can:

- **Mention another pet** in chat with `//` to coordinate them ([multi-agent](#multi-agent-orchestration)).
- **Run a workflow** that hands a task down a pipeline of agents ([workflows](#workflows)).

---

## Using agpet

### The system tray

agpet lives in the tray. Click the icon for:

- **Open Launcher…** — choose an agent type + working directory and start a new pet.
- **Run Workflow…** — open the workflow runner.
- **Worktrees…** — view the git worktrees agpet created for delegated/parallel tasks (with one-click **Merge** and **Commit** on each row).
- **Close `<name>`** — one entry per running pet; stops that agent.
- **Quit agpet** — exit the app.

The menu rebuilds itself as pets come and go, so the running list is always current.

### Interacting with pets

| Action | Result |
| --- | --- |
| **Click** | Open that pet's chat panel |
| **Click + drag** | Move the pet; sets a custom roaming height (remembered per pet) |
| **Double-click** | Reset the pet to its default baseline height |

Everywhere else the overlay is click-through, so the desktop and other apps behave normally.

### The chat panel

The panel header shows the pet's avatar, an editable **session name** (click to rename), and a **status** dot (idle / thinking / error / offline), plus buttons for **＋ New Session**, **History**, **Reload** (reconnect), **Settings ⚙**, and **Close ×**.

**Writing messages — the input box understands a few prefixes:**

- `@` — **file picker.** Start typing and pick a file from the working directory to mention it; the path is inserted for the agent.
- `/` — **command picker.** Autocompletes the slash-commands the agent itself exposes (varies by agent).
- `//` — **agent mention.** Reference another running pet by name to coordinate it, or type `// New <AgentType>` to spawn a fresh session inline. Adding a `//` mention reveals the **send-mode** selector (see below).
- **Paste an image** — it attaches as a thumbnail and is sent to the agent (for agents that accept images).

**Settings ⚙ popover:**

- **Model** — switch between the models the agent offers.
- **Mode** — switch the agent's mode (e.g. plan vs. act), where supported.
- **Font** — S / M / L.
- **Density** — Cozy / Compact.

These, plus each pet's name, position, and last-used directory, are remembered between runs.

**History** lists past sessions for that pet with their time, status, and a summary; resume one to reload its transcript.

### Multi-agent orchestration

When you `//`-mention other pets, a **send-mode** selector appears with four ways to dispatch the message:

- **Orchestrate** — the pet you're chatting with becomes the "mother." It delegates subtasks to the mentioned pets, works in parallel, then collects and combines their results into one answer.
- **Parallel** — send the same task to all mentioned pets at once and let them work independently.
- **Vertical** — chain the mentioned pets so each one's output feeds the next.
- **Broadcast** — send the message to every mentioned pet as a simple fan-out.

Under the hood the mother agent uses a built-in MCP server exposing three tools — `list_agents`, `delegate(agent, task, wait)`, and `collect(handle)`. When the working directory is a **git repo**, each delegated task runs in its own isolated worktree branch (`agpet/delegate/<type>-<n>`), so siblings never step on each other's files or your tree. You can review, **Commit**, or **Merge** those worktrees from the tray's **Worktrees…** panel. No configuration is required — the tools are offered automatically to agents that support HTTP MCP.

### Workflows

A workflow scripts a task down a sequence of agents, with the hand-off animated from pet to pet. Open one from the tray's **Run Workflow…** menu, type your task, and watch it flow through the steps.

agpet ships with **Plan with Claude, execute with Codex** and seeds its YAML so you can copy it. Define your own by dropping `*.yaml` files in the `workflows/` folder of your config directory — see [Customizing workflows](#customizing-workflows).

---

## Configuration

agpet keeps its config in the app config directory:

| Platform | Path |
| --- | --- |
| **Windows** | `%APPDATA%\com.agpet.pet` |
| **macOS** | `~/Library/Application Support/com.agpet.pet` |
| **Linux** | `~/.config/com.agpet.pet` |

On first run agpet writes a default `agents.toml` and a `workflows/plan-then-execute.yaml` there.

### `agents.toml` — defining agents

Each `[[agent]]` block is one launchable pet type. Fields:

| Field | Required | Description |
| --- | --- | --- |
| `id` | ✅ | Unique identifier for the agent type. |
| `name` | ✅ | Display name shown in the launcher and UI. |
| `command` | ✅ | Program to run (e.g. `npx`, `claude`, `opencode`). On Windows it runs via `cmd /c` so `.cmd`/`npx` shims work. |
| `args` | — | Command-line arguments (array). |
| `color` | — | Hex color for the pet's body (default `#e8743b`). |
| `wsl` | — | Windows only: run the command inside WSL (default `false`). See below. |
| `wsl_distro` | — | Specific WSL distro name; omit to use your default distro. |

Edit the file and **restart the app** to apply. The default looks like this:

```toml
# agpet agents — one pet per agent. Edit freely; restart the app to apply.
# `command` + `args` are run via `cmd /c` on Windows so npx/.cmd shims work.
# Each agent uses its own CLI login (run e.g. `claude /login`, Codex/OpenCode auth).

[[agent]]
id = "claude"
name = "Claude Code"
command = "npx"
args = ["-y", "@agentclientprotocol/claude-agent-acp"]
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
```

**Adding your own agent** is just another block — point `command`/`args` at any ACP-speaking adapter:

```toml
[[agent]]
id = "my-agent"
name = "My Agent"
command = "my-acp-adapter"
args = ["--acp"]
color = "#3b82f6"
```

### Customizing workflows

Drop more `*.yaml` files into the `workflows/` folder (one workflow per file). A workflow whose `id` matches a built-in overrides it. Files are read on demand — no restart needed. Launch the agent types a workflow needs *before* running it.

| Field | Description |
| --- | --- |
| `id` | Unique identifier (matching a built-in `id` overrides it). |
| `name` | Display name in the workflow runner. |
| `required_types` | Optional. Agent types needed; auto-derived from the steps if omitted. |
| `steps` | Ordered list. Each step has `agent_type`, a `prompt`, and an `output_var`. |

In prompts, `{user_input}` is the task you type and `{{var}}` injects a prior step's `output_var`. The seeded example:

```yaml
id: plan-then-execute
name: Plan with Claude, execute with Codex
required_types: [claude, codex]
steps:
  - agent_type: claude
    prompt: |
      Create a concise, numbered step-by-step plan for this task. Reply with only the plan.

      Task: {user_input}
    output_var: plan
  - agent_type: codex
    prompt: |
      Execute this plan precisely in the current project. Plan:

      {{plan}}
    output_var: result
  - agent_type: claude
    prompt: |
      Review this execution result against the original task and flag any issues or missing pieces. Be brief.

      Task: {user_input}

      Result:
      {{result}}
    output_var: review
```

### Windows: running agents through WSL

If you keep your agent CLIs inside **WSL** (Windows Subsystem for Linux) rather than installing them natively on Windows, agpet can launch them there. Add `wsl = true` to an agent in `agents.toml`:

```toml
[[agent]]
id = "claude-wsl"
name = "Claude (WSL)"
command = "npx"
args = ["-y", "@agentclientprotocol/claude-agent-acp"]
wsl = true
# wsl_distro = "Ubuntu"   # optional; omit to use your default distro
```

With `wsl = true`, agpet runs the command via `wsl.exe … -- bash -lc '<command>'` and automatically translates the working directory and `@`-mention paths from `C:\…` to `/mnt/c/…` so the agent resolves them correctly. Notes:

- The CLI must be on your **login-shell** `PATH` inside WSL (e.g. installed globally, or your `~/.profile`/`~/.bash_profile` loads `nvm`). The login shell must not print to stdout, since that channel carries the ACP protocol.
- The built-in `delegate` MCP server runs on the Windows host's `127.0.0.1`. Reaching it from WSL relies on WSL2 localhost forwarding (mirrored networking on recent Windows 11); if delegation can't connect, that's the thing to check.
- `wsl = true` is ignored on macOS/Linux.

---

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
