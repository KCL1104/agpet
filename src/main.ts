// Milestone 3 (slice 1): one walking pet per agent (Claude / Codex / OpenCode).
// Each pet reflects its agent's ACP state; clicking a pet opens that agent's own
// chat panel. Per-agent transcripts are kept in separate DOM containers so
// switching pets preserves each conversation. The backend tags every event with
// `agent_id`; we route to the right pet/panel and report all pet rects so the
// backend can toggle click-through.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const canvas = document.getElementById("pet-canvas") as HTMLCanvasElement;
const ctx = canvas.getContext("2d")!;

const PET_W = 64;
const PET_H = 64;
const SPEED = 120;
const BOB_HZ = 3;
const BOB_AMP = 6;
const MARGIN_BOTTOM = 28;

let baselineY = 0;
let last = performance.now();

interface AgentInfo {
  id: string;
  name: string;
  color: string;
}

interface Pet {
  id: string;
  name: string;
  color: string;
  x: number;
  dir: 1 | -1;
  state: string;
  detail: string;
  revertAt: number; // transient "completed" -> idle
  container: HTMLDivElement; // per-agent transcript
  currentAgent: HTMLDivElement | null;
  currentThinking: HTMLDivElement | null;
  toolChips: Map<string, HTMLDivElement>;
}

const pets: Pet[] = [];
const petById = new Map<string, Pet>();
let selected: string | null = null;

const STATE_META: Record<string, { emote: string; walk: boolean; label: string }> = {
  connecting:    { emote: "…",  walk: false, label: "connecting" },
  idle:          { emote: "",   walk: true,  label: "" },
  thinking:      { emote: "💭", walk: false, label: "thinking" },
  tool_running:  { emote: "🔧", walk: false, label: "tool" },
  permission:    { emote: "❓", walk: false, label: "permission" },
  responding:    { emote: "💬", walk: true,  label: "responding" },
  completed:     { emote: "✓",  walk: true,  label: "done" },
  auth_required: { emote: "🔑", walk: false, label: "login needed" },
  error:         { emote: "✕",  walk: false, label: "error" },
  exited:        { emote: "💤", walk: false, label: "offline" },
};
function meta(state: string) {
  return STATE_META[state] ?? STATE_META.idle;
}

// --- DOM refs -------------------------------------------------------------

const panel = document.getElementById("chat-panel") as HTMLDivElement;
const messagesHost = document.getElementById("chat-messages") as HTMLDivElement;
const inputEl = document.getElementById("chat-input") as HTMLTextAreaElement;
const sendBtn = document.getElementById("chat-send") as HTMLButtonElement;
const closeBtn = document.getElementById("chat-close") as HTMLButtonElement;
const permBar = document.getElementById("permission-bar") as HTMLDivElement;
const newBtn = document.getElementById("chat-new") as HTMLButtonElement;
const historyBtn = document.getElementById("chat-history") as HTMLButtonElement;
const historyView = document.getElementById("history-view") as HTMLDivElement;
const chatEmpty = document.getElementById("chat-empty") as HTMLDivElement;
const statusDot = document.getElementById("status-dot") as HTMLSpanElement;
const statusText = document.getElementById("status-text") as HTMLSpanElement;
const titleEl = document.getElementById("chat-title") as HTMLSpanElement;
const avatarEl = document.getElementById("pet-avatar") as HTMLSpanElement;

function selectedPet(): Pet | undefined {
  return selected ? petById.get(selected) : undefined;
}

function updateEmpty() {
  const p = selectedPet();
  const has = p && p.container.querySelector(".msg, .tool");
  chatEmpty.style.display = has ? "none" : "flex";
}

function addMsgTo(pet: Pet, cls: string, text: string): HTMLDivElement {
  const div = document.createElement("div");
  div.className = `msg ${cls}`;
  div.textContent = text;
  pet.container.appendChild(div);
  if (pet.id === selected) {
    updateEmpty();
    scrollToBottom();
  }
  return div;
}

function scrollToBottom() {
  messagesHost.scrollTop = messagesHost.scrollHeight;
}

function resetTurn(pet: Pet) {
  pet.currentAgent = null;
  pet.currentThinking = null;
}

function refreshHeader() {
  const p = selectedPet();
  if (!p) return;
  titleEl.textContent = p.name;
  avatarEl.style.background = p.color + "33";
  avatarEl.style.boxShadow = `inset 0 0 0 1px ${p.color}`;
  const m = meta(p.state);
  statusDot.style.background = p.color;
  statusText.textContent = m.label || "idle";
}

function selectAgent(id: string) {
  selected = id;
  for (const p of pets) {
    p.container.style.display = p.id === id ? "flex" : "none";
  }
  historyView.classList.add("hidden");
  permBar.classList.add("hidden");
  refreshHeader();
  updateEmpty();
  scrollToBottom();
}

function openPanelFor(id: string) {
  selectAgent(id);
  panel.classList.remove("hidden");
  invoke("set_panel_open", { open: true }).catch(() => {});
  inputEl.focus();
}

function closePanel() {
  panel.classList.add("hidden");
  invoke("set_panel_open", { open: false }).catch(() => {});
}

function sendPrompt() {
  const pet = selectedPet();
  const text = inputEl.value.trim();
  if (!pet || !text) return;
  addMsgTo(pet, "user", text);
  resetTurn(pet);
  invoke("send_prompt", { agent: pet.id, text }).catch((e) => addMsgTo(pet, "system", `send failed: ${e}`));
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

newBtn.addEventListener("click", () => {
  const pet = selectedPet();
  if (!pet) return;
  invoke("new_session", { agent: pet.id }).catch(() => {});
  addMsgTo(pet, "system", "Summarizing & starting a new session…");
});

historyBtn.addEventListener("click", () => {
  if (historyView.classList.contains("hidden")) {
    loadHistory();
    historyView.classList.remove("hidden");
  } else {
    historyView.classList.add("hidden");
  }
});

interface SessionRow {
  id: string;
  started_at: number;
  status: string;
  initial_prompt: string | null;
  summary: string | null;
}

async function loadHistory() {
  const pet = selectedPet();
  if (!pet) return;
  historyView.innerHTML = `<div class="hist-empty">Loading…</div>`;
  try {
    const rows = await invoke<SessionRow[]>("list_sessions", { agent: pet.id });
    historyView.innerHTML = "";
    if (!rows || rows.length === 0) {
      historyView.innerHTML = `<div class="hist-empty">No past sessions for ${pet.name} yet.</div>`;
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
        invoke("resume_session", { agent: pet.id, id: r.id }).catch(() => {});
        historyView.classList.add("hidden");
        clearTranscript(pet);
        addMsgTo(pet, "system", "Resuming previous session…");
      });
      historyView.appendChild(row);
    }
  } catch (e) {
    historyView.innerHTML = `<div class="hist-empty">Failed to load history: ${e}</div>`;
  }
}

function clearTranscript(pet: Pet) {
  pet.container.querySelectorAll(".msg, .tool").forEach((n) => n.remove());
  resetTurn(pet);
  pet.toolChips.clear();
  if (pet.id === selected) {
    permBar.classList.add("hidden");
    permBar.innerHTML = "";
    updateEmpty();
  }
}

// --- Backend events (routed by agent_id) ----------------------------------

interface PetStatePayload { agent_id: string; state: string; detail?: string | null; }

listen<PetStatePayload>("pet-state", (event) => {
  const pet = petById.get(event.payload.agent_id);
  if (!pet) return;
  pet.state = event.payload.state;
  pet.detail = event.payload.detail ?? "";
  if (pet.state === "completed") {
    pet.revertAt = performance.now() + 2200;
    resetTurn(pet);
  }
  if (pet.id === selected) refreshHeader();
});

interface ChatEvent {
  agent_id: string;
  kind: string;
  text?: string;
  tool_call_id?: string | null;
  title?: string | null;
  status?: string | null;
}

listen<ChatEvent>("chat-event", (event) => {
  const ev = event.payload;
  const pet = petById.get(ev.agent_id);
  if (!pet) return;
  switch (ev.kind) {
    case "agent_message": {
      if (!ev.text) break;
      pet.currentThinking = null;
      if (!pet.currentAgent) pet.currentAgent = addMsgTo(pet, "agent", "");
      pet.currentAgent.textContent += ev.text;
      if (pet.id === selected) scrollToBottom();
      break;
    }
    case "agent_thought": {
      if (!ev.text) break;
      pet.currentAgent = null;
      if (!pet.currentThinking) pet.currentThinking = addMsgTo(pet, "thinking", "");
      pet.currentThinking.textContent += ev.text;
      if (pet.id === selected) scrollToBottom();
      break;
    }
    case "tool_call": {
      resetTurn(pet);
      const id = ev.tool_call_id ?? `${Date.now()}`;
      let chip = pet.toolChips.get(id);
      if (!chip) {
        chip = document.createElement("div");
        chip.className = "tool";
        pet.container.appendChild(chip);
        pet.toolChips.set(id, chip);
      }
      chip.innerHTML = `🔧 <span class="name"></span> <span class="badge"></span>`;
      chip.querySelector(".name")!.textContent = ev.title ?? "tool";
      chip.querySelector(".badge")!.textContent = ev.status ?? "running";
      if (pet.id === selected) {
        updateEmpty();
        scrollToBottom();
      }
      break;
    }
    case "tool_update": {
      const id = ev.tool_call_id ?? "";
      const chip = pet.toolChips.get(id);
      if (chip) {
        const status = ev.status ?? "running";
        chip.classList.toggle("completed", status === "completed");
        chip.classList.toggle("failed", status === "failed");
        const badge = chip.querySelector(".badge");
        if (badge) badge.textContent = status;
      }
      break;
    }
  }
});

interface PermissionOption { optionId: string; name: string; kind: string; }
interface PermissionRequest { agent_id: string; request_id: string; title: string; options: PermissionOption[]; }

listen<PermissionRequest>("permission-request", (event) => {
  const { agent_id, request_id, title, options } = event.payload;
  const pet = petById.get(agent_id);
  if (!pet) return;
  openPanelFor(agent_id); // surface the request on its agent

  permBar.innerHTML = "";
  const titleEl2 = document.createElement("div");
  titleEl2.className = "perm-title";
  titleEl2.textContent = `${pet.name} — allow: ${title}?`;
  permBar.appendChild(titleEl2);

  const btnRow = document.createElement("div");
  btnRow.className = "perm-buttons";
  for (const opt of options ?? []) {
    const btn = document.createElement("button");
    btn.textContent = opt.name;
    if (opt.kind?.includes("allow")) btn.classList.add("allow");
    if (opt.kind?.includes("reject")) btn.classList.add("reject");
    btn.addEventListener("click", () => {
      invoke("respond_permission", { agent: agent_id, id: request_id, choice: opt.optionId }).catch(() => {});
      permBar.classList.add("hidden");
      permBar.innerHTML = "";
    });
    btnRow.appendChild(btn);
  }
  permBar.appendChild(btnRow);
  permBar.classList.remove("hidden");
});

listen<{ agent_id: string }>("session-reset", (event) => {
  const pet = petById.get(event.payload.agent_id);
  if (pet) clearTranscript(pet);
});

// --- Rendering ------------------------------------------------------------

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

function petBox(pet: Pet) {
  return { x: pet.x - 8, y: baselineY - 40, w: PET_W + 16, h: PET_H + 90 };
}

let lastRectSent = 0;
function reportRects(now: number) {
  if (now - lastRectSent < 40 || pets.length === 0) return;
  lastRectSent = now;
  const rects = pets.map((p) => {
    const b = petBox(p);
    return { id: p.id, x: b.x, y: b.y, w: b.w, h: b.h };
  });
  invoke("update_pet_rects", { rects }).catch(() => {});
}

function shade(hex: string, amt: number): string {
  const n = parseInt(hex.slice(1, 7), 16);
  const clamp = (v: number) => Math.max(0, Math.min(255, Math.round(v)));
  const r = clamp(((n >> 16) & 0xff) * (1 + amt));
  const g = clamp(((n >> 8) & 0xff) * (1 + amt));
  const b = clamp((n & 0xff) * (1 + amt));
  return `rgb(${r}, ${g}, ${b})`;
}

function drawPet(pet: Pet, now: number, dt: number, w: number) {
  if (pet.state === "completed" && now >= pet.revertAt) {
    pet.state = "idle";
    pet.detail = "";
    if (pet.id === selected) refreshHeader();
  }
  const m = meta(pet.state);
  const dim = pet.state === "error" || pet.state === "exited" || pet.state === "auth_required";

  if (m.walk) {
    pet.x += pet.dir * SPEED * dt;
    if (pet.x <= 0) {
      pet.x = 0;
      pet.dir = 1;
    } else if (pet.x + PET_W >= w) {
      pet.x = w - PET_W;
      pet.dir = -1;
    }
  }

  const phase = (now / 1000) * BOB_HZ * Math.PI * 2 + pet.x * 0.01;
  const bob = Math.sin(phase) * BOB_AMP;
  const step = m.walk ? Math.sin(phase) * 6 : 0;
  const y = baselineY + Math.abs(bob);

  ctx.globalAlpha = dim ? 0.5 : 1;

  // Feet.
  ctx.fillStyle = shade(pet.color, -0.25);
  ctx.fillRect(pet.x + 12, y + PET_H, 14, 9 + step);
  ctx.fillRect(pet.x + PET_W - 26, y + PET_H, 14, 9 - step);

  // Body.
  ctx.fillStyle = pet.color;
  roundRect(ctx, pet.x, y, PET_W, PET_H, 14);
  ctx.fill();

  // Eye.
  const eyeCx = pet.dir === 1 ? pet.x + PET_W - 18 : pet.x + 18;
  const eyeCy = y + 24;
  ctx.fillStyle = "#ffffff";
  ctx.beginPath();
  ctx.arc(eyeCx, eyeCy, 8, 0, Math.PI * 2);
  ctx.fill();
  ctx.fillStyle = "#1a1a1a";
  ctx.beginPath();
  ctx.arc(eyeCx + pet.dir * 2.5, eyeCy, 4, 0, Math.PI * 2);
  ctx.fill();

  ctx.globalAlpha = 1;

  // Emote.
  if (m.emote) {
    ctx.font = "26px system-ui, sans-serif";
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText(m.emote, pet.x + PET_W / 2, y - 18);
  }

  // Name (+ state) label.
  const label = m.label ? `${pet.name} · ${m.label}` : pet.name;
  ctx.font = "12px system-ui, sans-serif";
  ctx.textAlign = "center";
  ctx.textBaseline = "top";
  const labelW = ctx.measureText(label).width + 14;
  ctx.fillStyle = pet.id === selected ? "rgba(244,121,59,0.85)" : "rgba(0,0,0,0.55)";
  roundRect(ctx, pet.x + PET_W / 2 - labelW / 2, y + PET_H + 14, labelW, 18, 9);
  ctx.fill();
  ctx.fillStyle = "#ffffff";
  ctx.fillText(label, pet.x + PET_W / 2, y + PET_H + 17);
}

function draw(now: number) {
  const dt = Math.min((now - last) / 1000, 0.05);
  last = now;
  const w = window.innerWidth;
  ctx.clearRect(0, 0, w, window.innerHeight);
  for (const pet of pets) drawPet(pet, now, dt, w);
  reportRects(now);
  requestAnimationFrame(draw);
}

canvas.addEventListener("click", (e) => {
  for (const pet of pets) {
    const b = petBox(pet);
    if (e.clientX >= b.x && e.clientX <= b.x + b.w && e.clientY >= b.y && e.clientY <= b.y + b.h) {
      openPanelFor(pet.id);
      return;
    }
  }
});

async function init() {
  let agents: AgentInfo[] = [];
  try {
    agents = await invoke<AgentInfo[]>("list_agents");
  } catch (e) {
    console.error("list_agents failed", e);
  }
  const w = window.innerWidth;
  agents.forEach((a, i) => {
    const container = document.createElement("div");
    container.className = "agent-transcript";
    container.style.display = "none";
    messagesHost.appendChild(container);
    const pet: Pet = {
      id: a.id,
      name: a.name,
      color: a.color,
      x: Math.max(0, ((i + 1) * w) / (agents.length + 1) - PET_W / 2),
      dir: i % 2 === 0 ? 1 : -1,
      state: "connecting",
      detail: "",
      revertAt: 0,
      container,
      currentAgent: null,
      currentThinking: null,
      toolChips: new Map(),
    };
    pets.push(pet);
    petById.set(a.id, pet);
  });
  if (pets.length > 0) selected = pets[0].id;
}

window.addEventListener("resize", resize);
resize();
init().then(() => {
  requestAnimationFrame(draw);
});
