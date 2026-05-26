// Milestone 1: a placeholder "pet" that walks along the bottom of the screen,
// reflects the connected agent's ACP state (step 3), and — when clicked — opens
// a chat panel to talk to the agent (step 4). The backend emits `pet-state`,
// `chat-event`, and `permission-request` events; we render them here, and report
// the pet's bounding box back so the backend can toggle click-through.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const canvas = document.getElementById("pet-canvas") as HTMLCanvasElement;
const ctx = canvas.getContext("2d")!;

const PET_W = 64; // body width (px)
const PET_H = 64; // body height (px)
const SPEED = 140; // walk speed (px/sec)
const BOB_HZ = 3; // bob/step cycles per second
const BOB_AMP = 6; // vertical bob amplitude (px)
const MARGIN_BOTTOM = 28; // gap from the bottom edge (px)

let x = 0; // body left, in CSS px
let dir: 1 | -1 = 1; // 1 = walking right, -1 = walking left
let baselineY = 0; // resting top of the body
let last = performance.now();

// --- ACP-driven pet state -------------------------------------------------

interface PetStatePayload {
  state: string;
  detail?: string | null;
}

const STATE_STYLE: Record<
  string,
  { color: string; emote: string; walk: boolean; label: string }
> = {
  connecting:    { color: "#9aa0a6", emote: "…",  walk: false, label: "connecting" },
  idle:          { color: "#e8743b", emote: "",   walk: true,  label: "idle" },
  thinking:      { color: "#6c8cff", emote: "💭", walk: false, label: "thinking" },
  tool_running:  { color: "#3fa45b", emote: "🔧", walk: false, label: "tool" },
  permission:    { color: "#e7b53b", emote: "❓", walk: false, label: "permission" },
  responding:    { color: "#e8743b", emote: "💬", walk: true,  label: "responding" },
  completed:     { color: "#3fa45b", emote: "✓",  walk: true,  label: "done" },
  auth_required: { color: "#e7b53b", emote: "🔑", walk: false, label: "login needed" },
  error:         { color: "#c0392b", emote: "✕",  walk: false, label: "error" },
  exited:        { color: "#7f8c8d", emote: "💤", walk: false, label: "exited" },
};

let petState = "connecting";
let stateDetail = "";
let revertToIdleAt = 0; // `completed` is transient → fall back to idle

function style() {
  return STATE_STYLE[petState] ?? STATE_STYLE.idle;
}

// --- Chat panel -----------------------------------------------------------

const panel = document.getElementById("chat-panel") as HTMLDivElement;
const messagesEl = document.getElementById("chat-messages") as HTMLDivElement;
const inputEl = document.getElementById("chat-input") as HTMLTextAreaElement;
const sendBtn = document.getElementById("chat-send") as HTMLButtonElement;
const closeBtn = document.getElementById("chat-close") as HTMLButtonElement;
const permBar = document.getElementById("permission-bar") as HTMLDivElement;
const newBtn = document.getElementById("chat-new") as HTMLButtonElement;
const historyBtn = document.getElementById("chat-history") as HTMLButtonElement;
const historyView = document.getElementById("history-view") as HTMLDivElement;

// Streaming targets for the current agent turn.
let currentAgent: HTMLDivElement | null = null;
let currentThinking: HTMLDivElement | null = null;
const toolChips = new Map<string, HTMLDivElement>();

function scrollToBottom() {
  messagesEl.scrollTop = messagesEl.scrollHeight;
}

function addMessage(cls: string, text: string): HTMLDivElement {
  const div = document.createElement("div");
  div.className = `msg ${cls}`;
  div.textContent = text;
  messagesEl.appendChild(div);
  scrollToBottom();
  return div;
}

function resetTurn() {
  currentAgent = null;
  currentThinking = null;
}

function openPanel() {
  panel.classList.remove("hidden");
  invoke("set_panel_open", { open: true }).catch(() => {});
  inputEl.focus();
}

function closePanel() {
  panel.classList.add("hidden");
  invoke("set_panel_open", { open: false }).catch(() => {});
}

function sendPrompt() {
  const text = inputEl.value.trim();
  if (!text) return;
  addMessage("user", text);
  resetTurn();
  invoke("send_prompt", { text }).catch((e) => addMessage("system", `send failed: ${e}`));
  inputEl.value = "";
}

sendBtn.addEventListener("click", sendPrompt);
closeBtn.addEventListener("click", closePanel);
inputEl.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    sendPrompt();
  }
});

// --- Sessions: New / History / Resume (M2) --------------------------------

interface SessionRow {
  id: string;
  started_at: number;
  ended_at: number | null;
  status: string;
  initial_prompt: string | null;
  summary: string | null;
}

function clearTranscript() {
  messagesEl.innerHTML = "";
  permBar.classList.add("hidden");
  permBar.innerHTML = "";
  resetTurn();
  toolChips.clear();
}

async function loadHistory() {
  historyView.innerHTML = `<div class="hist-empty">Loading…</div>`;
  try {
    const rows = await invoke<SessionRow[]>("list_sessions");
    historyView.innerHTML = "";
    if (!rows || rows.length === 0) {
      historyView.innerHTML = `<div class="hist-empty">No past sessions yet.</div>`;
      return;
    }
    for (const r of rows) {
      const row = document.createElement("div");
      row.className = "hist-row";
      const when = new Date(r.started_at).toLocaleString();
      const text = r.summary || r.initial_prompt || "(no summary)";
      row.innerHTML =
        `<div class="hist-meta"><span class="hist-when"></span><span class="hist-status"></span></div>` +
        `<div class="hist-summary"></div>` +
        `<button class="hist-resume">Resume</button>`;
      row.querySelector(".hist-when")!.textContent = when;
      row.querySelector(".hist-status")!.textContent = r.status;
      row.querySelector(".hist-summary")!.textContent = text;
      row.querySelector(".hist-resume")!.addEventListener("click", () => {
        invoke("resume_session", { id: r.id }).catch(() => {});
        historyView.classList.add("hidden");
        clearTranscript();
        addMessage("system", "Resuming previous session…");
      });
      historyView.appendChild(row);
    }
  } catch (e) {
    historyView.innerHTML = `<div class="hist-empty">Failed to load history: ${e}</div>`;
  }
}

newBtn.addEventListener("click", () => {
  invoke("new_session").catch(() => {});
  addMessage("system", "Summarizing & starting a new session…");
});

historyBtn.addEventListener("click", () => {
  if (historyView.classList.contains("hidden")) {
    loadHistory();
    historyView.classList.remove("hidden");
  } else {
    historyView.classList.add("hidden");
  }
});

listen("session-reset", () => {
  clearTranscript();
  historyView.classList.add("hidden");
});

// The pet's clickable bounding box (CSS px), covering emote above + label below.
function petBox() {
  return { x: x - 8, y: baselineY - 40, w: PET_W + 16, h: PET_H + 90 };
}

canvas.addEventListener("click", (e) => {
  const b = petBox();
  if (e.clientX >= b.x && e.clientX <= b.x + b.w && e.clientY >= b.y && e.clientY <= b.y + b.h) {
    openPanel();
  }
});

// --- Backend events -------------------------------------------------------

listen<PetStatePayload>("pet-state", (event) => {
  petState = event.payload.state;
  stateDetail = event.payload.detail ?? "";
  if (petState === "completed") {
    revertToIdleAt = performance.now() + 2200;
    resetTurn();
  }
});

interface ChatEvent {
  kind: string;
  text?: string;
  tool_call_id?: string | null;
  title?: string | null;
  status?: string | null;
  output?: string | null;
  tool_kind?: string | null;
}

listen<ChatEvent>("chat-event", (event) => {
  const ev = event.payload;
  switch (ev.kind) {
    case "agent_message": {
      if (!ev.text) break;
      currentThinking = null;
      if (!currentAgent) currentAgent = addMessage("agent", "");
      currentAgent.textContent += ev.text;
      scrollToBottom();
      break;
    }
    case "agent_thought": {
      if (!ev.text) break;
      currentAgent = null;
      if (!currentThinking) currentThinking = addMessage("thinking", "");
      currentThinking.textContent += ev.text;
      scrollToBottom();
      break;
    }
    case "tool_call": {
      resetTurn();
      const id = ev.tool_call_id ?? `${Date.now()}`;
      const title = ev.title ?? "tool";
      let chip = toolChips.get(id);
      if (!chip) {
        chip = document.createElement("div");
        chip.className = "tool";
        messagesEl.appendChild(chip);
        toolChips.set(id, chip);
      }
      chip.innerHTML = `🔧 <span class="name"></span> <span class="badge"></span>`;
      chip.querySelector(".name")!.textContent = title;
      chip.querySelector(".badge")!.textContent = ev.status ?? "running";
      scrollToBottom();
      break;
    }
    case "tool_update": {
      const id = ev.tool_call_id ?? "";
      const chip = toolChips.get(id);
      if (chip) {
        const status = ev.status ?? "running";
        chip.classList.toggle("completed", status === "completed");
        chip.classList.toggle("failed", status === "failed");
        const badge = chip.querySelector(".badge");
        if (badge) badge.textContent = status;
      }
      break;
    }
    case "plan": {
      currentThinking = null;
      currentAgent = null;
      addMessage("thinking", "📋 planning…");
      break;
    }
  }
});

interface PermissionOption {
  optionId: string;
  name: string;
  kind: string;
}
interface PermissionRequest {
  request_id: string;
  title: string;
  options: PermissionOption[];
}

listen<PermissionRequest>("permission-request", (event) => {
  const { request_id, title, options } = event.payload;
  if (panel.classList.contains("hidden")) openPanel();

  permBar.innerHTML = "";
  const titleEl = document.createElement("div");
  titleEl.className = "perm-title";
  titleEl.textContent = `Allow: ${title}?`;
  permBar.appendChild(titleEl);

  const btnRow = document.createElement("div");
  btnRow.className = "perm-buttons";
  for (const opt of options ?? []) {
    const btn = document.createElement("button");
    btn.textContent = opt.name;
    if (opt.kind?.includes("allow")) btn.classList.add("allow");
    if (opt.kind?.includes("reject")) btn.classList.add("reject");
    btn.addEventListener("click", () => {
      invoke("respond_permission", { id: request_id, choice: opt.optionId }).catch(() => {});
      permBar.classList.add("hidden");
      permBar.innerHTML = "";
    });
    btnRow.appendChild(btn);
  }
  permBar.appendChild(btnRow);
  permBar.classList.remove("hidden");
});

// --- Rendering loop -------------------------------------------------------

function resize() {
  const dpr = window.devicePixelRatio || 1;
  canvas.width = Math.floor(window.innerWidth * dpr);
  canvas.height = Math.floor(window.innerHeight * dpr);
  canvas.style.width = `${window.innerWidth}px`;
  canvas.style.height = `${window.innerHeight}px`;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  baselineY = window.innerHeight - PET_H - MARGIN_BOTTOM;
}

function roundRect(c: CanvasRenderingContext2D, rx: number, ry: number, rw: number, rh: number, r: number) {
  c.beginPath();
  c.moveTo(rx + r, ry);
  c.arcTo(rx + rw, ry, rx + rw, ry + rh, r);
  c.arcTo(rx + rw, ry + rh, rx, ry + rh, r);
  c.arcTo(rx, ry + rh, rx, ry, r);
  c.arcTo(rx, ry, rx + rw, ry, r);
  c.closePath();
}

let lastRectSent = 0;
function reportRect(now: number) {
  if (now - lastRectSent < 40) return;
  lastRectSent = now;
  const b = petBox();
  invoke("update_pet_rect", { x: b.x, y: b.y, w: b.w, h: b.h }).catch(() => {});
}

function draw(now: number) {
  const dt = Math.min((now - last) / 1000, 0.05);
  last = now;

  if (petState === "completed" && now >= revertToIdleAt) {
    petState = "idle";
    stateDetail = "";
  }

  const s = style();
  const w = window.innerWidth;

  if (s.walk) {
    x += dir * SPEED * dt;
    if (x <= 0) {
      x = 0;
      dir = 1;
    } else if (x + PET_W >= w) {
      x = w - PET_W;
      dir = -1;
    }
  }

  reportRect(now);

  const phase = (now / 1000) * BOB_HZ * Math.PI * 2;
  const bob = Math.sin(phase) * BOB_AMP;
  const step = Math.sin(phase) * 6;
  const y = baselineY + Math.abs(bob);

  ctx.clearRect(0, 0, w, window.innerHeight);

  const footWiggle = s.walk ? step : 0;
  ctx.fillStyle = shade(s.color, -0.25);
  ctx.fillRect(x + 12, y + PET_H, 14, 9 + footWiggle);
  ctx.fillRect(x + PET_W - 26, y + PET_H, 14, 9 - footWiggle);

  ctx.fillStyle = s.color;
  roundRect(ctx, x, y, PET_W, PET_H, 14);
  ctx.fill();

  const eyeCx = dir === 1 ? x + PET_W - 18 : x + 18;
  const eyeCy = y + 24;
  ctx.fillStyle = "#ffffff";
  ctx.beginPath();
  ctx.arc(eyeCx, eyeCy, 8, 0, Math.PI * 2);
  ctx.fill();
  ctx.fillStyle = "#1a1a1a";
  ctx.beginPath();
  ctx.arc(eyeCx + dir * 2.5, eyeCy, 4, 0, Math.PI * 2);
  ctx.fill();

  if (s.emote) {
    ctx.font = "26px system-ui, sans-serif";
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText(s.emote, x + PET_W / 2, y - 18);
  }

  const labelText = stateDetail ? `${s.label}: ${stateDetail}` : s.label;
  ctx.font = "12px system-ui, sans-serif";
  ctx.textAlign = "center";
  ctx.textBaseline = "top";
  ctx.fillStyle = "rgba(0,0,0,0.55)";
  const labelW = ctx.measureText(labelText).width + 12;
  roundRect(ctx, x + PET_W / 2 - labelW / 2, y + PET_H + 14, labelW, 18, 9);
  ctx.fill();
  ctx.fillStyle = "#ffffff";
  ctx.fillText(labelText, x + PET_W / 2, y + PET_H + 17);

  requestAnimationFrame(draw);
}

function shade(hex: string, amt: number): string {
  const n = parseInt(hex.slice(1), 16);
  const clamp = (v: number) => Math.max(0, Math.min(255, Math.round(v)));
  const r = clamp(((n >> 16) & 0xff) * (1 + amt));
  const g = clamp(((n >> 8) & 0xff) * (1 + amt));
  const b = clamp((n & 0xff) * (1 + amt));
  return `rgb(${r}, ${g}, ${b})`;
}

window.addEventListener("resize", resize);
resize();
requestAnimationFrame(draw);
