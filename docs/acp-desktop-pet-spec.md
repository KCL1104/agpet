# ACP Desktop Pet — Project Spec & Claude Code Prompt

> 給 Claude Code 開發用的初始 prompt。請在專案根目錄保留此檔作為 source of truth，並在每個 milestone 開始前重新讓 Claude 讀過一次。

---

## Goal

A cross-platform desktop pet app where each pet represents an AI coding agent (Claude Code, Codex, OpenCode). Pets visually reflect their agent's state, can hand off tasks to each other (orchestration), and remember past sessions for context continuity. Clicking a pet opens a chat panel to interact with that agent.

---

## Architecture

### Stack

- **Framework**: Tauri v2 (Rust backend + WebView frontend)
- **Frontend**: TypeScript + Canvas for sprite animation
- **Backend**: Rust with `tokio` async runtime
- **Protocol**: Agent Client Protocol (ACP) over JSON-RPC 2.0 via stdio
- **Process detection**: `sysinfo` crate as fallback for external sessions
- **Session storage**: SQLite via `sqlx` (local file, schema designed to be sync-friendly)

### Core components (Rust backend)

```
┌─────────────────────────────────────────────────────┐
│                  Tauri Commands                      │
│        (frontend ↔ backend IPC surface)              │
└─────────────────────────────────────────────────────┘
                          ↕
┌──────────────────┐  ┌──────────────────┐  ┌──────────┐
│   Agent Router   │  │  Session Store   │  │  Pet     │
│  (message bus,   │  │  (SQLite,        │  │  State   │
│   workflow exec) │  │   artifacts)     │  │  Machine │
└──────────────────┘  └──────────────────┘  └──────────┘
        ↕                                          ↕
┌─────────────────────────────────────────────────────┐
│        ACP Adapter Subprocesses (1 per agent)        │
│  claude-code-acp │  codex-acp  │  opencode  │  ...   │
└─────────────────────────────────────────────────────┘
        ↕ (also runs alongside)
┌─────────────────────────────────────────────────────┐
│   External Process Poller (sysinfo, every 1-2s)      │
│       Detects agents user runs in their terminal     │
└─────────────────────────────────────────────────────┘
```

### Hybrid state-detection model

1. **Primary**: ACP adapters spawned by our app. Structured events drive pet animations.
   - `npx @zed-industries/claude-code-acp`
   - Codex ACP adapter
   - OpenCode native ACP
2. **Secondary**: `sysinfo` process polling for external sessions. Reflects as a passive "radar" — pet notices but cannot interact via ACP.

### Cross-platform notes

- **Linux**: Test transparent windows on both X11 and Wayland. Wayland click-through is flaky — document limitations.
- **macOS**: Easiest target. NSPanel-style behavior via Tauri config.
- **Windows**:
  - Detect both native Windows and WSL processes
  - WSL: spawn `wsl.exe -d <distro> --exec npx @zed-industries/claude-code-acp`
  - Enumerate distros: `wsl.exe --list --running`
- **Window config**: `transparent: true, decorations: false, alwaysOnTop: true, skipTaskbar: true, resizable: false`. Dynamic click-through via `set_ignore_cursor_events`.

---

## Multi-agent orchestration (feature A)

### Concept

Each connected agent has its own pet. Pets can pass tasks to each other via predefined workflows.

### Agent registration

Agents are declared in a config file `~/.config/acp-pet/agents.toml`:

```toml
[[agent]]
id = "claude"
name = "Claude Code"
command = "npx"
args = ["@zed-industries/claude-code-acp"]
sprite = "claude-cat.png"

[[agent]]
id = "codex"
name = "Codex"
command = "npx"
args = ["codex-acp"]
sprite = "codex-fox.png"
```

### Workflow definition

Workflows are user-editable YAML in `~/.config/acp-pet/workflows/`:

```yaml
id: plan-then-execute
name: "Plan with Claude, execute with Codex"
trigger: manual  # or: command, hotkey
steps:
  - id: plan
    agent: claude
    prompt: "Create a step-by-step plan for: {user_input}"
    output_var: plan
  - id: execute
    agent: codex
    prompt: "Execute this plan precisely:\n{{ plan }}"
    output_var: result
  - id: review
    agent: claude
    prompt: "Review this execution result and flag any issues:\n{{ result }}"
```

### Agent Router responsibilities

- Spawn and supervise ACP adapter subprocesses (auto-restart on crash with backoff)
- Maintain `HashMap<AgentId, AgentHandle>` with stdio channels
- Execute workflow steps sequentially, threading outputs as inputs
- Emit events to frontend so pets animate to reflect handoffs (pet A walks toward pet B, "delivers" task)
- All inter-agent messages go through the router — no agent-to-agent direct comms

### Conflict handling

- When two workflows want to modify the same file, second one blocks until first completes
- Surface as a "waiting" pet animation
- Workflow failures: bubble up, halt downstream steps, mark with `error` state

---

## Session handoff (feature D)

### Session artifact schema

After every ACP session ends (clean or crash), write to SQLite:

```sql
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,           -- UUID
    agent_id TEXT NOT NULL,
    started_at INTEGER NOT NULL,   -- unix ms
    ended_at INTEGER,
    status TEXT NOT NULL,          -- 'completed', 'cancelled', 'crashed'
    workdir TEXT,                  -- cwd when started
    initial_prompt TEXT,
    summary TEXT,                  -- LLM-generated, see below
    workflow_id TEXT,              -- if part of a workflow
    parent_session_id TEXT,        -- for handoffs
    metadata_json TEXT             -- arbitrary, extensible
);

CREATE TABLE session_events (
    id INTEGER PRIMARY KEY,
    session_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    event_type TEXT NOT NULL,      -- 'tool_call', 'thinking', 'message', etc
    payload_json TEXT,
    FOREIGN KEY(session_id) REFERENCES sessions(id)
);

CREATE TABLE session_files (
    session_id TEXT NOT NULL,
    file_path TEXT NOT NULL,
    operation TEXT NOT NULL,       -- 'read', 'edit', 'create', 'delete'
    PRIMARY KEY (session_id, file_path, operation)
);
```

### Session summary generation

When a session ends, the agent itself generates a 2-3 sentence summary via a final `session/prompt` call:

> "Summarize what this session accomplished in 2-3 sentences."

Store in `sessions.summary`.

### Resume / continue

- Pet UI exposes a "history" panel listing recent sessions per agent
- "Continue" action starts a new ACP session, injects previous summary + relevant context as initial system message
- "Pass to..." action starts new session with another agent, threading the summary

### Network-friendly design (B prep, do NOT implement B)

- All timestamps in UTC unix ms
- All IDs are UUIDs, not autoincrement
- No machine-local paths in artifacts that would need to travel (or store both abs + project-relative)
- Schema uses no SQLite-specific types that would block migration to Postgres
- **But**: do not build a sync layer. Do not add network code. Just keep doors open.

---

## State machine (per pet)

Priority order (top wins):

1. `permission_pending` — agent needs user approval
2. `tool_running` — actively executing
3. `thinking` — model reasoning
4. `handoff_outgoing` — passing task to another agent (walk toward target pet)
5. `handoff_incoming` — receiving task (animated receipt)
6. `idle_active` — session open, no current task
7. `idle_external` — external session detected via process poll
8. `completed` — task just finished (transient, ~2s)
9. `sleeping` — no session for >60s
10. `error` — agent crashed or disconnected

---

## Milestones (strict order)

### Milestone 1: Foundation (single agent, no orchestration)

1. Tauri v2 project. Transparent always-on-top window. Placeholder sprite walks across screen.
2. Implement ACP client core in Rust. Spawn `@zed-industries/claude-code-acp`. Complete `initialize` and `session/new` handshake. Log all JSON-RPC messages.
3. Map subset of ACP events to pet state changes. Verify animations.
4. Click-to-open chat panel. `session/prompt` from user input. Render text / tool calls / thinking in panel.
5. `sysinfo` process polling for external sessions.
6. WSL detection on Windows.

**Stop. Verify everything works. Demo to yourself. Do not proceed until rock solid.**

### Milestone 2: Session handoff (feature D)

1. Add SQLite via `sqlx`. Create schema.
2. On session end, persist artifact. Generate summary via final prompt.
3. UI: history panel per pet. Resume action.
4. "Pass to (same agent)" — start new session injecting previous summary.

### Milestone 3: Multi-agent orchestration (feature A)

1. Agent config file. Spawn multiple ACP adapters.
2. Multiple pets render simultaneously. Each tied to one agent.
3. Agent Router: message bus with workflow execution.
4. One predefined workflow: plan-then-execute (Claude → Codex).
5. Handoff animations.
6. User-editable workflow YAML.

### Milestone 4 (deferred, maybe never): Cross-machine team (feature B)

Do not start until M1–M3 are stable and used in production. Likely approach: publish status to Discord rich presence or Slack status; do NOT build a custom backend.

---

## Reference projects (read before coding)

- [`rullerzhou-afk/clawd-on-desk`](https://github.com/rullerzhou-afk/clawd-on-desk) — similar app, uses hooks not ACP. Steal UX ideas.
- [`codexpets/codex-pets`](https://github.com/codexpets/codex-pets) — pixel pet with sprite-hatching.
- [`@zed-industries/claude-code-acp`](https://www.npmjs.com/package/@zed-industries/claude-code-acp) — the ACP adapter we spawn.
- [`agentclientprotocol/claude-agent-acp`](https://deepwiki.com/agentclientprotocol/claude-agent-acp/1.1-getting-started) on DeepWiki — full ACP reference.
- Outworked — multi-agent pet app, study its orchestration UX.
- [ACP spec](https://agentclientprotocol.com/)

---

## Constraints

- Always-running. Memory budget: <100 MB resident with 2 agents active.
- Pet must not steal focus.
- Crash of one ACP adapter must not crash the pet OR other adapters.
- All paths configurable, never hard-coded.
- Workflows are user-editable plaintext, no DSL compilation step.

---

## Out of scope (v1–v3)

- Full terminal emulator
- Mobile builds
- Custom backend / cloud sync
- Multi-pet on different machines (B is deferred)
- Multi-user same-session (never)

---

## First task

Start Milestone 1, step 1. Set up Tauri v2 project, configure transparent window per OS, render a single placeholder rectangle walking back and forth. Stop and report back so we can verify before continuing.
