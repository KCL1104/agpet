// Milestone 3 + UX: one walking pet per running agent *instance* (multiple of
// the same type allowed). Each pet reflects its instance's ACP state; clicking
// opens that instance's own chat panel. Per-instance transcripts live in
// separate DOM containers. Backend events are tagged with `instance_id`;
// instances can be added/removed at runtime (tray).

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

const canvas = document.getElementById("pet-canvas") as HTMLCanvasElement;
const ctx = canvas.getContext("2d")!;

const PET_W = 64;
const PET_H = 64;
const SPEED = 120;
const BOB_HZ = 3;
const BOB_AMP = 6;
const MARGIN_BOTTOM = 30; // leave room below the pet for its name label + taskbar

let baselineY = 0;
let last = performance.now();

interface InstanceInfo {
  instance_id: string;
  type_id: string;
  name: string;
  color: string;
}

interface Pet {
  id: string; // instance_id
  name: string;
  color: string;
  x: number;
  customY: number; // roaming height set by dragging; -1 = default baseline
  dir: 1 | -1;
  state: string;
  detail: string;
  revertAt: number;
  container: HTMLDivElement;
  currentAgent: HTMLDivElement | null;
  currentThinking: HTMLDivElement | null;
  currentPlan: HTMLDivElement | null;
  toolChips: Map<string, HTMLDivElement>;
  cfg: any; // agent-config: auth_methods / models / modes
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

const panel = document.getElementById("chat-panel") as HTMLDivElement;
const messagesHost = document.getElementById("chat-messages") as HTMLDivElement;
const inputEl = document.getElementById("chat-input") as HTMLTextAreaElement;
const filePicker = document.getElementById("file-picker") as HTMLDivElement;
const sendBtn = document.getElementById("chat-send") as HTMLButtonElement;
const stopBtn = document.getElementById("chat-stop") as HTMLButtonElement;
const attachStrip = document.getElementById("attach-strip") as HTMLDivElement;
const closeBtn = document.getElementById("chat-close") as HTMLButtonElement;
const permBar = document.getElementById("permission-bar") as HTMLDivElement;
const newBtn = document.getElementById("chat-new") as HTMLButtonElement;
const historyBtn = document.getElementById("chat-history") as HTMLButtonElement;
const reloadBtn = document.getElementById("chat-reload") as HTMLButtonElement;
const historyView = document.getElementById("history-view") as HTMLDivElement;
const chatEmpty = document.getElementById("chat-empty") as HTMLDivElement;
const statusDot = document.getElementById("status-dot") as HTMLSpanElement;
const statusText = document.getElementById("status-text") as HTMLSpanElement;
const titleEl = document.getElementById("chat-title") as HTMLSpanElement;
const avatarEl = document.getElementById("pet-avatar") as HTMLSpanElement;
const settingsBtn = document.getElementById("chat-settings") as HTMLButtonElement;
const settingsPopover = document.getElementById("settings-popover") as HTMLDivElement;
const setModelSel = document.getElementById("set-model") as HTMLSelectElement;
const setModeSel = document.getElementById("set-mode") as HTMLSelectElement;
const setFontSeg = document.getElementById("set-font") as HTMLDivElement;
const setDensitySeg = document.getElementById("set-density") as HTMLDivElement;
const statusBar = document.getElementById("status-bar") as HTMLDivElement;
const resizeHandle = document.getElementById("resize-handle") as HTMLDivElement;
const headerEl = document.querySelector(".chat-header") as HTMLDivElement;
const launcherPanel = document.getElementById("launcher-panel") as HTMLDivElement;
const launcherClose = document.getElementById("launcher-close") as HTMLButtonElement;
const launcherList = document.getElementById("launcher-list") as HTMLDivElement;
const workflowPanel = document.getElementById("workflow-panel") as HTMLDivElement;
const workflowClose = document.getElementById("workflow-close") as HTMLButtonElement;
const workflowList = document.getElementById("workflow-list") as HTMLDivElement;
const toastEl = document.getElementById("toast") as HTMLDivElement;

// Panels now report their bounding rect to the overlay (see reportRects) so only
// the panel area blocks clicks — not the whole screen. Opening/closing a panel
// nudges an immediate rect refresh so interactivity tracks within one frame.
function updatePanelOpen() {
  lastRectSent = 0;
}

let toastTimer = 0;
function showToast(msg: string) {
  toastEl.textContent = msg;
  toastEl.classList.remove("hidden");
  clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => toastEl.classList.add("hidden"), 3500);
}

interface TypeInfo { type_id: string; name: string; color: string; }

async function buildLauncher() {
  let types: TypeInfo[] = [];
  try {
    types = await invoke<TypeInfo[]>("list_types");
  } catch (e) {
    console.error("list_types failed", e);
  }
  launcherList.innerHTML = "";
  for (const t of types) {
    const row = document.createElement("div");
    row.className = "launch-row";
    row.innerHTML =
      `<span class="launch-dot"></span>` +
      `<div class="launch-info"><div class="launch-name"></div><div class="launch-dir"></div></div>` +
      `<button class="launch-folder">Folder…</button><button class="launch-go">Launch</button>`;
    (row.querySelector(".launch-dot") as HTMLElement).style.background = t.color;
    row.querySelector(".launch-name")!.textContent = t.name;
    const dirEl = row.querySelector(".launch-dir") as HTMLElement;
    let chosen = localStorage.getItem("agpet.cwd." + t.type_id) || localStorage.getItem("agpet.cwd.last") || "";
    dirEl.textContent = chosen || "(default folder)";

    row.querySelector(".launch-folder")!.addEventListener("click", async () => {
      const sel = await openDialog({ directory: true, multiple: false, defaultPath: chosen || undefined });
      if (typeof sel === "string") {
        chosen = sel;
        dirEl.textContent = sel;
        localStorage.setItem("agpet.cwd." + t.type_id, sel);
        localStorage.setItem("agpet.cwd.last", sel);
      }
    });
    row.querySelector(".launch-go")!.addEventListener("click", () => {
      invoke("launch_instance", { kind: t.type_id, cwd: chosen || null }).catch((e) => console.error(e));
      hideLauncher();
    });
    launcherList.appendChild(row);
  }
}

function hideLauncher() {
  launcherPanel.classList.add("hidden");
  updatePanelOpen();
}

launcherClose.addEventListener("click", hideLauncher);
listen("open-launcher", () => {
  buildLauncher();
  launcherPanel.classList.remove("hidden");
  updatePanelOpen();
});

// --- Workflows (orchestration) -------------------------------------------

interface WorkflowInfo { id: string; name: string; required_types: string[]; }

async function buildWorkflows() {
  let wfs: WorkflowInfo[] = [];
  try {
    wfs = await invoke<WorkflowInfo[]>("list_workflows");
  } catch (e) {
    console.error("list_workflows failed", e);
  }
  workflowList.innerHTML = "";
  for (const wf of wfs) {
    const row = document.createElement("div");
    row.className = "wf-row";
    row.innerHTML =
      `<div class="wf-name"></div><div class="wf-need"></div>` +
      `<textarea placeholder="What should they work on?"></textarea>` +
      `<button class="wf-run">Run</button>`;
    row.querySelector(".wf-name")!.textContent = wf.name;
    row.querySelector(".wf-need")!.textContent = "Needs running: " + wf.required_types.join(" + ");
    const ta = row.querySelector("textarea") as HTMLTextAreaElement;
    row.querySelector(".wf-run")!.addEventListener("click", () => {
      const input = ta.value.trim();
      if (!input) { ta.focus(); return; }
      invoke("run_workflow", { workflowId: wf.id, input }).catch((e) => showToast(`Workflow failed: ${e}`));
      hideWorkflows();
      showToast(`Running “${wf.name}”…`);
    });
    workflowList.appendChild(row);
  }
}
function hideWorkflows() {
  workflowPanel.classList.add("hidden");
  updatePanelOpen();
}
workflowClose.addEventListener("click", hideWorkflows);
listen("open-workflows", () => {
  buildWorkflows();
  workflowPanel.classList.remove("hidden");
  updatePanelOpen();
});

listen<any>("workflow-step", (e) => {
  if (e.payload?.type) showToast(`→ ${e.payload.type} (${e.payload.step})`);
});
listen<any>("workflow-done", () => showToast("✓ Workflow complete"));
listen<any>("workflow-error", (e) => showToast(`⚠ ${e.payload?.message ?? "workflow error"}`));

// 📦 handoff animation: a package travels from one pet to the next.
interface Handoff { fromX: number; toX: number; y: number; start: number; }
const handoffs: Handoff[] = [];
listen<{ from_instance: string | null; to_instance: string }>("workflow-handoff", (e) => {
  const to = petById.get(e.payload.to_instance);
  const from = e.payload.from_instance ? petById.get(e.payload.from_instance) : undefined;
  if (!to || !from) return; // first step has no "from": the pet just starts working
  handoffs.push({
    fromX: from.x + PET_W / 2,
    toX: to.x + PET_W / 2,
    y: baselineY,
    start: performance.now(),
  });
});

// Theme the panel to the selected agent's brand colour.
function applyTheme(color: string) {
  document.documentElement.style.setProperty("--accent", color);
  document.documentElement.style.setProperty("--accent-soft", color + "28");
}

// Show a login/retry bar for instances that aren't connected.
function updateStatusBar(pet: Pet) {
  if (pet.id !== selected) return;
  const needs = ["auth_required", "error", "exited"].includes(pet.state);
  if (!needs) {
    statusBar.classList.add("hidden");
    statusBar.innerHTML = "";
    return;
  }
  statusBar.innerHTML = "";
  const msg = document.createElement("div");
  msg.className = "sb-msg";
  msg.textContent =
    pet.state === "auth_required" ? `${pet.name} needs login.` :
    pet.state === "exited" ? `${pet.name} is offline.` : `${pet.name} hit an error.`;
  statusBar.appendChild(msg);

  const methods = pet.cfg?.auth_methods;
  if (pet.state === "auth_required" && Array.isArray(methods) && methods.length) {
    const hint = document.createElement("div");
    hint.className = "sb-hint";
    hint.textContent = "Login options:\n" +
      methods.map((m: any) => `• ${m.name}${m.description ? " — " + m.description : ""}`).join("\n");
    statusBar.appendChild(hint);
  }
  const btn = document.createElement("button");
  btn.className = "sb-retry";
  btn.textContent = "Retry connection";
  btn.addEventListener("click", () => {
    invoke("retry_agent", { instance: pet.id }).catch(() => {});
    statusBar.classList.add("hidden");
  });
  statusBar.appendChild(btn);
  statusBar.classList.remove("hidden");
}

// Settings popover: model / mode (per instance) + font / density (global).
function populateSettings(pet: Pet) {
  const models = pet.cfg?.models;
  const avail: any[] = models?.availableModels ?? [];
  setModelSel.innerHTML = "";
  if (avail.length === 0) {
    setModelSel.innerHTML = `<option>(not available)</option>`;
    setModelSel.disabled = true;
  } else {
    setModelSel.disabled = false;
    for (const m of avail) {
      const o = document.createElement("option");
      o.value = m.modelId;
      o.textContent = m.name ?? m.modelId;
      if (m.modelId === models?.currentModelId) o.selected = true;
      setModelSel.appendChild(o);
    }
  }
  const modes = pet.cfg?.modes;
  const am: any[] = modes?.availableModes ?? [];
  setModeSel.innerHTML = "";
  if (am.length === 0) {
    setModeSel.innerHTML = `<option>(not available)</option>`;
    setModeSel.disabled = true;
  } else {
    setModeSel.disabled = false;
    for (const m of am) {
      const o = document.createElement("option");
      o.value = m.id;
      o.textContent = m.name ?? m.id;
      if (m.id === modes?.currentModeId) o.selected = true;
      setModeSel.appendChild(o);
    }
  }
}

settingsBtn.addEventListener("click", () => {
  const pet = selectedPet();
  if (!pet) return;
  if (settingsPopover.classList.contains("hidden")) {
    populateSettings(pet);
    settingsPopover.classList.remove("hidden");
  } else {
    settingsPopover.classList.add("hidden");
  }
});
setModelSel.addEventListener("change", () => {
  const pet = selectedPet();
  if (pet) invoke("set_model", { instance: pet.id, model: setModelSel.value }).catch(() => {});
});
setModeSel.addEventListener("change", () => {
  const pet = selectedPet();
  if (pet) invoke("set_mode", { instance: pet.id, mode: setModeSel.value }).catch(() => {});
});

function markSeg(seg: HTMLDivElement, val: string) {
  seg.querySelectorAll("button").forEach((b) => b.classList.toggle("active", b.getAttribute("data-v") === val));
}
function applyPrefs() {
  const font = localStorage.getItem("agpet.font") || "m";
  const density = localStorage.getItem("agpet.density") || "comfortable";
  panel.classList.remove("font-s", "font-m", "font-l");
  panel.classList.add("font-" + font);
  panel.classList.toggle("density-compact", density === "compact");
  markSeg(setFontSeg, font);
  markSeg(setDensitySeg, density);
}
setFontSeg.addEventListener("click", (e) => {
  const b = (e.target as HTMLElement).closest("button");
  if (!b) return;
  localStorage.setItem("agpet.font", b.getAttribute("data-v")!);
  applyPrefs();
});
setDensitySeg.addEventListener("click", (e) => {
  const b = (e.target as HTMLElement).closest("button");
  if (!b) return;
  localStorage.setItem("agpet.density", b.getAttribute("data-v")!);
  applyPrefs();
});

// Drag (via header) + resize (top-left handle), persisted.
function savePanelBox() {
  const r = panel.getBoundingClientRect();
  localStorage.setItem("agpet.box", JSON.stringify({ left: r.left, top: r.top, w: r.width, h: r.height }));
}
function loadPanelBox() {
  const s = localStorage.getItem("agpet.box");
  if (!s) return;
  try {
    const b = JSON.parse(s);
    panel.style.right = "auto";
    panel.style.bottom = "auto";
    panel.style.left = `${b.left}px`;
    panel.style.top = `${b.top}px`;
    panel.style.width = `${b.w}px`;
    panel.style.height = `${b.h}px`;
  } catch {}
}
let dragging = false;
let dragOff = { x: 0, y: 0 };
headerEl.addEventListener("pointerdown", (e) => {
  if ((e.target as HTMLElement).closest("button")) return;
  dragging = true;
  invoke("set_dragging", { dragging: true }).catch(() => {});
  const r = panel.getBoundingClientRect();
  dragOff = { x: e.clientX - r.left, y: e.clientY - r.top };
  panel.style.right = "auto";
  panel.style.bottom = "auto";
  panel.style.left = `${r.left}px`;
  panel.style.top = `${r.top}px`;
  headerEl.setPointerCapture(e.pointerId);
});
headerEl.addEventListener("pointermove", (e) => {
  if (!dragging) return;
  const x = Math.max(0, Math.min(window.innerWidth - 80, e.clientX - dragOff.x));
  const y = Math.max(0, Math.min(window.innerHeight - 40, e.clientY - dragOff.y));
  panel.style.left = `${x}px`;
  panel.style.top = `${y}px`;
});
headerEl.addEventListener("pointerup", (e) => {
  if (!dragging) return;
  dragging = false;
  invoke("set_dragging", { dragging: false }).catch(() => {});
  headerEl.releasePointerCapture(e.pointerId);
  savePanelBox();
});
let resizing = false;
let rs = { mx: 0, my: 0, right: 0, bottom: 0 };
resizeHandle.addEventListener("pointerdown", (e) => {
  resizing = true;
  invoke("set_dragging", { dragging: true }).catch(() => {});
  const r = panel.getBoundingClientRect();
  rs = { mx: e.clientX, my: e.clientY, right: r.right, bottom: r.bottom };
  panel.style.right = "auto";
  panel.style.bottom = "auto";
  resizeHandle.setPointerCapture(e.pointerId);
  e.stopPropagation();
});
resizeHandle.addEventListener("pointermove", (e) => {
  if (!resizing) return;
  const w = Math.max(300, Math.min(720, rs.right - e.clientX));
  const h = Math.max(320, Math.min(900, rs.bottom - e.clientY));
  panel.style.width = `${w}px`;
  panel.style.height = `${h}px`;
  panel.style.left = `${rs.right - w}px`;
  panel.style.top = `${rs.bottom - h}px`;
});
resizeHandle.addEventListener("pointerup", (e) => {
  if (!resizing) return;
  resizing = false;
  invoke("set_dragging", { dragging: false }).catch(() => {});
  resizeHandle.releasePointerCapture(e.pointerId);
  savePanelBox();
});

function selectedPet(): Pet | undefined {
  return selected ? petById.get(selected) : undefined;
}

function updateEmpty() {
  const p = selectedPet();
  const has = p && p.container.querySelector(".msg, .tool");
  chatEmpty.style.display = has ? "none" : "flex";
}

function scrollToBottom() {
  messagesHost.scrollTop = messagesHost.scrollHeight;
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
  statusDot.style.background = p.color;
  statusText.textContent = meta(p.state).label || "idle";
}

function selectAgent(id: string) {
  selected = id;
  for (const p of pets) {
    p.container.style.display = p.id === id ? "flex" : "none";
  }
  historyView.classList.add("hidden");
  permBar.classList.add("hidden");
  settingsPopover.classList.add("hidden");
  refreshHeader();
  const p = petById.get(id);
  if (p) {
    applyTheme(p.color);
    updateStatusBar(p);
  }
  updateInputControls();
  hidePicker();
  updateEmpty();
  scrollToBottom();
}

function openPanelFor(id: string) {
  selectAgent(id);
  panel.classList.remove("hidden");
  updatePanelOpen();
  inputEl.focus();
}

function closePanel() {
  panel.classList.add("hidden");
  updatePanelOpen();
}

// --- Completion picker (@ files, / commands) ------------------------------
interface PickItem { insert: string; label: string; hint?: string; file?: string }
const fileCache = new Map<string, string[]>(); // instance_id -> cwd file list
const cmdsByInstance = new Map<string, { name: string; description: string }[]>();
let picker: { items: PickItem[]; sel: number; tokenStart: number } | null = null;
const mentionedFiles = new Set<string>();

async function ensureFiles(instanceId: string): Promise<string[]> {
  const cached = fileCache.get(instanceId);
  if (cached) return cached;
  try {
    const files = await invoke<string[]>("list_dir_files", { instance: instanceId });
    fileCache.set(instanceId, files);
    return files;
  } catch {
    fileCache.set(instanceId, []);
    return [];
  }
}

// Active completion trigger: an "@" mention anywhere (no whitespace after), or a
// "/" command when the whole message starts with "/".
function activeTrigger(): { trigger: "@" | "/"; query: string; start: number } | null {
  const pos = inputEl.selectionStart ?? inputEl.value.length;
  const upto = inputEl.value.slice(0, pos);
  const at = upto.lastIndexOf("@");
  if (at >= 0 && (at === 0 || /\s/.test(upto[at - 1])) && !/\s/.test(upto.slice(at + 1))) {
    return { trigger: "@", query: upto.slice(at + 1), start: at };
  }
  if (upto.startsWith("/") && !/\s/.test(upto.slice(1))) {
    return { trigger: "/", query: upto.slice(1), start: 0 };
  }
  return null;
}

function scoreFile(path: string, q: string): number {
  if (!q) return 0;
  const p = path.toLowerCase();
  const base = p.slice(p.lastIndexOf("/") + 1);
  if (base.startsWith(q)) return 3;
  if (base.includes(q)) return 2;
  if (p.includes(q)) return 1;
  return 0;
}

function renderPicker() {
  if (!picker) return;
  filePicker.innerHTML = "";
  picker.items.forEach((it, i) => {
    const row = document.createElement("div");
    row.className = "fp-row" + (i === picker!.sel ? " sel" : "");
    const label = document.createElement("span");
    label.textContent = it.label;
    row.appendChild(label);
    if (it.hint) {
      const h = document.createElement("span");
      h.className = "fp-hint";
      h.textContent = it.hint;
      row.appendChild(h);
    }
    // mousedown (not click) so the textarea keeps focus/selection for insertion
    row.addEventListener("mousedown", (e) => { e.preventDefault(); selectItem(i); });
    filePicker.appendChild(row);
  });
  filePicker.classList.remove("hidden");
  (filePicker.children[picker.sel] as HTMLElement | undefined)?.scrollIntoView({ block: "nearest" });
}

function hidePicker() {
  picker = null;
  filePicker.classList.add("hidden");
  filePicker.innerHTML = "";
}

async function refreshPicker() {
  const pet = selectedPet();
  const t = pet ? activeTrigger() : null;
  if (!pet || !t) { hidePicker(); return; }
  let items: PickItem[] = [];
  if (t.trigger === "@") {
    const files = await ensureFiles(pet.id);
    const t2 = activeTrigger(); // re-validate after await
    if (!t2 || t2.trigger !== "@" || t2.start !== t.start) return;
    const q = t2.query.toLowerCase();
    items = files
      .filter((f) => f.toLowerCase().includes(q))
      .sort((a, b) => scoreFile(b, q) - scoreFile(a, q))
      .slice(0, 12)
      .map((f) => ({ insert: "@" + f + " ", label: f, file: f }));
  } else {
    const q = t.query.toLowerCase();
    items = (cmdsByInstance.get(pet.id) ?? [])
      .filter((c) => c.name.toLowerCase().includes(q) || c.description.toLowerCase().includes(q))
      .slice(0, 12)
      .map((c) => ({ insert: "/" + c.name + " ", label: "/" + c.name, hint: c.description }));
  }
  if (items.length === 0) { hidePicker(); return; }
  picker = { items, sel: 0, tokenStart: t.start };
  renderPicker();
}

function selectItem(i: number) {
  if (!picker) return;
  const it = picker.items[i];
  const pos = inputEl.selectionStart ?? inputEl.value.length;
  const before = inputEl.value.slice(0, picker.tokenStart);
  const after = inputEl.value.slice(pos);
  inputEl.value = before + it.insert + after;
  const caret = before.length + it.insert.length;
  inputEl.setSelectionRange(caret, caret);
  if (it.file) mentionedFiles.add(it.file);
  hidePicker();
  inputEl.focus();
}

// Pasted images pending on the current draft (base64 + mime, data URL for thumb).
const pendingImages: { mime: string; data: string; url: string }[] = [];

function renderAttachStrip() {
  attachStrip.innerHTML = "";
  if (pendingImages.length === 0) { attachStrip.classList.add("hidden"); return; }
  pendingImages.forEach((img, i) => {
    const thumb = document.createElement("div");
    thumb.className = "attach-thumb";
    thumb.innerHTML = `<img src="${img.url}" alt="" /><button class="rm" title="Remove">×</button>`;
    thumb.querySelector(".rm")!.addEventListener("click", () => {
      pendingImages.splice(i, 1);
      renderAttachStrip();
    });
    attachStrip.appendChild(thumb);
  });
  attachStrip.classList.remove("hidden");
}

inputEl.addEventListener("paste", (e) => {
  const items = e.clipboardData?.items;
  if (!items) return;
  for (const it of items) {
    if (!it.type.startsWith("image/")) continue;
    const blob = it.getAsFile();
    if (!blob) continue;
    e.preventDefault();
    const reader = new FileReader();
    reader.onload = () => {
      const url = reader.result as string; // data:<mime>;base64,<DATA>
      pendingImages.push({ mime: blob.type, data: url.slice(url.indexOf(",") + 1), url });
      renderAttachStrip();
    };
    reader.readAsDataURL(blob);
  }
});

function sendPrompt() {
  const pet = selectedPet();
  const text = inputEl.value.trim();
  if (!pet || (!text && pendingImages.length === 0)) return;
  // Only attach files still referenced in the text (the user may have deleted some).
  const files = [...mentionedFiles].filter((f) => text.includes("@" + f));
  const images = pendingImages.map((i) => ({ mime: i.mime, data: i.data }));
  addMsgTo(pet, "user", text || `📎 ${images.length} image${images.length > 1 ? "s" : ""}`);
  resetTurn(pet);
  pet.currentPlan = null; // a new turn gets a fresh plan block
  invoke("send_prompt", { instance: pet.id, text, files, images }).catch((e) => addMsgTo(pet, "system", `send failed: ${e}`));
  inputEl.value = "";
  inputEl.style.height = "auto"; // collapse back to one line
  mentionedFiles.clear();
  pendingImages.length = 0;
  renderAttachStrip();
  hidePicker();
}

// Swap Send ⇄ Stop based on whether the selected pet is mid-turn.
const BUSY_STATES = ["thinking", "responding", "tool_running", "permission"];
function updateInputControls() {
  const pet = selectedPet();
  const busy = !!pet && BUSY_STATES.includes(pet.state);
  sendBtn.style.display = busy ? "none" : "grid";
  stopBtn.style.display = busy ? "grid" : "none";
}

// Grow the textarea with its content (up to the CSS max-height) so it never
// shows a scrollbar until it's actually tall.
function autoGrow() {
  inputEl.style.height = "auto";
  inputEl.style.height = Math.min(inputEl.scrollHeight, 120) + "px";
}

stopBtn.addEventListener("click", () => {
  const pet = selectedPet();
  if (pet) invoke("cancel_prompt", { instance: pet.id }).catch(() => {});
});
sendBtn.addEventListener("click", sendPrompt);
closeBtn.addEventListener("click", () => { hidePicker(); closePanel(); });
inputEl.addEventListener("input", () => { autoGrow(); void refreshPicker(); });
inputEl.addEventListener("blur", () => { setTimeout(hidePicker, 120); });
inputEl.addEventListener("keydown", (e) => {
  if (picker) {
    if (e.key === "ArrowDown") { e.preventDefault(); picker.sel = (picker.sel + 1) % picker.items.length; renderPicker(); return; }
    if (e.key === "ArrowUp") { e.preventDefault(); picker.sel = (picker.sel - 1 + picker.items.length) % picker.items.length; renderPicker(); return; }
    if (e.key === "Enter" || e.key === "Tab") { e.preventDefault(); selectItem(picker.sel); return; }
    if (e.key === "Escape") { e.preventDefault(); hidePicker(); return; }
  }
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    sendPrompt();
  }
});

newBtn.addEventListener("click", () => {
  const pet = selectedPet();
  if (!pet) return;
  invoke("new_session", { instance: pet.id }).catch(() => {});
  addMsgTo(pet, "system", "Summarizing & starting a new session…");
});

// Reload: kill this agent's (possibly stuck) connection and reconnect it.
reloadBtn.addEventListener("click", () => {
  const pet = selectedPet();
  if (!pet) return;
  invoke("retry_agent", { instance: pet.id }).catch((e) => showToast(`Reload failed: ${e}`));
  pet.state = "connecting";
  refreshHeader();
  updateStatusBar(pet);
  updateInputControls();
  addMsgTo(pet, "system", "Reloading…");
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
    const rows = await invoke<SessionRow[]>("list_sessions", { instance: pet.id });
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
        invoke("resume_session", { instance: pet.id, id: r.id }).catch(() => {});
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
  pet.container.querySelectorAll(".msg, .tool, .plan").forEach((n) => n.remove());
  resetTurn(pet);
  pet.currentPlan = null;
  pet.toolChips.clear();
  if (pet.id === selected) {
    permBar.classList.add("hidden");
    permBar.innerHTML = "";
    updateEmpty();
  }
}

// --- Pet lifecycle --------------------------------------------------------

function layoutPets() {
  const w = window.innerWidth;
  pets.forEach((p, i) => {
    if (p.x === -1) p.x = Math.max(0, ((i + 1) * w) / (pets.length + 1) - PET_W / 2);
  });
}

function addPet(info: InstanceInfo) {
  if (petById.has(info.instance_id)) return;
  const container = document.createElement("div");
  container.className = "agent-transcript";
  container.style.display = "none";
  messagesHost.appendChild(container);
  const pet: Pet = {
    id: info.instance_id,
    name: info.name,
    color: info.color,
    x: -1, // assigned by layoutPets
    customY: -1, // roaming height; -1 = default baseline, set by dragging
    dir: pets.length % 2 === 0 ? 1 : -1,
    state: "connecting",
    detail: "",
    revertAt: 0,
    container,
    currentAgent: null,
    currentThinking: null,
    currentPlan: null,
    toolChips: new Map(),
    cfg: null,
  };
  // Restore a dropped position from a previous run, if any. Instance ids are
  // deterministic across restarts (the counter resets), so this is best-effort
  // keyed by id; clamp in case the screen is now narrower.
  const savedPos = localStorage.getItem("agpet.pos." + pet.id);
  if (savedPos) {
    try {
      const { y } = JSON.parse(savedPos);
      if (typeof y === "number") pet.customY = y; // walk at the saved height
    } catch {}
  }
  pets.push(pet);
  petById.set(pet.id, pet);
  layoutPets();
  if (!selected) selected = pet.id;
}

function removePet(id: string) {
  const idx = pets.findIndex((p) => p.id === id);
  if (idx < 0) return;
  const [pet] = pets.splice(idx, 1);
  pet.container.remove();
  petById.delete(id);
  if (selected === id) {
    selected = pets[0]?.id ?? null;
    if (selected) selectAgent(selected);
    else closePanel();
  }
}

// --- Backend events (routed by instance_id) -------------------------------

interface PetStatePayload { instance_id: string; state: string; detail?: string | null; }

listen<PetStatePayload>("pet-state", (event) => {
  const pet = petById.get(event.payload.instance_id);
  if (!pet) return;
  pet.state = event.payload.state;
  pet.detail = event.payload.detail ?? "";
  if (pet.state === "completed") {
    pet.revertAt = performance.now() + 2200;
    resetTurn(pet);
  }
  if (pet.id === selected) {
    refreshHeader();
    updateStatusBar(pet);
    updateInputControls();
  }
});

listen<any>("agent-config", (event) => {
  const pet = petById.get(event.payload.instance_id);
  if (!pet) return;
  pet.cfg = event.payload;
  if (pet.id === selected) {
    updateStatusBar(pet);
    if (!settingsPopover.classList.contains("hidden")) populateSettings(pet);
  }
});

interface ChatEvent {
  instance_id: string;
  kind: string;
  text?: string;
  tool_call_id?: string | null;
  title?: string | null;
  status?: string | null;
  result?: string | null;
  tool_kind?: string | null;
  locations?: string[];
  diff?: { path?: string | null; old?: string | null; new?: string | null } | null;
  entries?: { content: string; status: string; priority?: string }[] | null;
  commands?: { name: string; description: string }[] | null;
  mode_id?: string | null;
}

function toolIcon(kind?: string | null): string {
  switch (kind) {
    case "read": return "📖";
    case "edit": return "✏️";
    case "delete": return "🗑️";
    case "move": return "📦";
    case "search": return "🔍";
    case "execute": return "⚡";
    case "fetch": return "🌐";
    case "think": return "💭";
    case "switch_mode": return "🔀";
    default: return "🔧";
  }
}

// Naive line diff: removed (old) lines then added (new) lines, each capped.
function renderDiff(el: HTMLElement, diff: { old?: string | null; new?: string | null }) {
  el.innerHTML = "";
  if (diff.old) {
    for (const l of diff.old.split("\n").slice(0, 40)) {
      const s = document.createElement("span"); s.className = "del"; s.textContent = "- " + l; el.appendChild(s);
    }
  }
  for (const l of (diff.new ?? "").split("\n").slice(0, 40)) {
    const s = document.createElement("span"); s.className = "add"; s.textContent = "+ " + l; el.appendChild(s);
  }
}

const PLAN_ICON: Record<string, string> = { pending: "⬜", in_progress: "⏳", completed: "✅" };
function renderPlan(pet: Pet, entries: { content: string; status: string }[]) {
  if (!pet.currentPlan) {
    pet.currentPlan = document.createElement("div");
    pet.currentPlan.className = "plan";
    pet.container.appendChild(pet.currentPlan);
  }
  pet.currentPlan.innerHTML = `<div class="plan-title">Plan</div>`;
  for (const e of entries) {
    const row = document.createElement("div");
    row.className = "plan-row" + (e.status === "completed" ? " done" : e.status === "in_progress" ? " active" : "");
    row.textContent = `${PLAN_ICON[e.status] ?? "⬜"} ${e.content}`;
    pet.currentPlan.appendChild(row);
  }
  if (pet.id === selected) scrollToBottom();
}

listen<ChatEvent>("chat-event", (event) => {
  const ev = event.payload;
  const pet = petById.get(ev.instance_id);
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
        // head (clickable) = icon + name + status; optional locations line; a
        // collapsible diff or text-output detail toggled by clicking the head.
        chip.innerHTML =
          `<div class="tool-head"><span class="ticon"></span> <span class="name"></span> <span class="badge"></span></div>` +
          `<div class="tool-loc hidden"></div>` +
          `<div class="tool-diff hidden"></div>` +
          `<pre class="tool-out hidden"></pre>`;
        chip.querySelector(".tool-head")!.addEventListener("click", () => {
          const diff = chip!.querySelector(".tool-diff") as HTMLElement;
          const out = chip!.querySelector(".tool-out") as HTMLElement;
          if (diff.childElementCount) diff.classList.toggle("hidden");
          else if (out.textContent) out.classList.toggle("hidden");
        });
        pet.container.appendChild(chip);
        pet.toolChips.set(id, chip);
      }
      (chip.querySelector(".ticon") as HTMLElement).textContent = toolIcon(ev.tool_kind);
      chip.querySelector(".name")!.textContent = ev.title ?? "tool";
      chip.querySelector(".badge")!.textContent = ev.status ?? "running";
      if (ev.locations && ev.locations.length) {
        const loc = chip.querySelector(".tool-loc") as HTMLElement;
        loc.textContent = ev.locations.join("  ·  ");
        loc.classList.remove("hidden");
      }
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
        if (ev.tool_kind) (chip.querySelector(".ticon") as HTMLElement).textContent = toolIcon(ev.tool_kind);
        if (ev.diff && (ev.diff.new || ev.diff.old)) {
          renderDiff(chip.querySelector(".tool-diff") as HTMLElement, ev.diff);
          chip.classList.add("has-out");
        } else if (ev.result) {
          (chip.querySelector(".tool-out") as HTMLElement).textContent = ev.result.slice(0, 4000);
          chip.classList.add("has-out"); // shows the ▸ expand affordance
        }
        if (ev.locations && ev.locations.length) {
          const loc = chip.querySelector(".tool-loc") as HTMLElement;
          loc.textContent = ev.locations.join("  ·  ");
          loc.classList.remove("hidden");
        }
        if (pet.id === selected) scrollToBottom();
      }
      break;
    }
    case "plan": {
      if (ev.entries && ev.entries.length) {
        resetTurn(pet);
        renderPlan(pet, ev.entries);
        if (pet.id === selected) updateEmpty();
      }
      break;
    }
    case "commands": {
      if (ev.commands) cmdsByInstance.set(ev.instance_id, ev.commands);
      break;
    }
    case "mode": {
      if (ev.mode_id) {
        if (pet.cfg?.modes) pet.cfg.modes.currentModeId = ev.mode_id; // so the popover reopens correct
        if (pet.id === selected) setModeSel.value = ev.mode_id; // live-update if popover open
      }
      break;
    }
  }
});

interface PermissionOption { optionId: string; name: string; kind: string; }
interface PermissionRequest { instance_id: string; request_id: string; title: string; options: PermissionOption[]; }

listen<PermissionRequest>("permission-request", (event) => {
  const { instance_id, request_id, title, options } = event.payload;
  const pet = petById.get(instance_id);
  if (!pet) return;
  openPanelFor(instance_id);

  permBar.innerHTML = "";
  const t = document.createElement("div");
  t.className = "perm-title";
  t.textContent = `${pet.name} — allow: ${title}?`;
  permBar.appendChild(t);

  const btnRow = document.createElement("div");
  btnRow.className = "perm-buttons";
  for (const opt of options ?? []) {
    const btn = document.createElement("button");
    btn.textContent = opt.name;
    if (opt.kind?.includes("allow")) btn.classList.add("allow");
    if (opt.kind?.includes("reject")) btn.classList.add("reject");
    btn.addEventListener("click", () => {
      invoke("respond_permission", { instance: instance_id, id: request_id, choice: opt.optionId }).catch(() => {});
      permBar.classList.add("hidden");
      permBar.innerHTML = "";
    });
    btnRow.appendChild(btn);
  }
  permBar.appendChild(btnRow);
  permBar.classList.remove("hidden");
});

listen<{ instance_id: string }>("session-reset", (event) => {
  const pet = petById.get(event.payload.instance_id);
  if (pet) clearTranscript(pet);
  fileCache.delete(event.payload.instance_id); // re-list files for the new session
});

listen<InstanceInfo>("instance-added", (event) => {
  addPet(event.payload);
});

listen<{ instance_id: string }>("instance-removed", (event) => {
  removePet(event.payload.instance_id);
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

// Vertical anchor (no bob): the pet's dragged height, else the roaming baseline.
function petTop(pet: Pet) {
  return pet.customY >= 0 ? pet.customY : baselineY;
}

function petBox(pet: Pet) {
  return { x: pet.x - 8, y: petTop(pet) - 40, w: PET_W + 16, h: PET_H + 90 };
}

// Bounding rects of any open panels, so the overlay keeps just those areas
// interactive (synthetic ids can't collide with instance ids).
function openPanelRects() {
  const panels: [string, HTMLElement][] = [
    ["__panel:chat", panel],
    ["__panel:launcher", launcherPanel],
    ["__panel:workflow", workflowPanel],
  ];
  return panels
    .filter(([, el]) => !el.classList.contains("hidden"))
    .map(([id, el]) => {
      const r = el.getBoundingClientRect();
      return { id, x: r.left, y: r.top, w: r.width, h: r.height };
    });
}

let lastRectSent = 0;
let lastWasEmpty = false;
function reportRects(now: number) {
  if (now - lastRectSent < 40) return;
  const panelRects = openPanelRects();
  const empty = pets.length === 0 && panelRects.length === 0;
  if (empty && lastWasEmpty) return; // nothing on screen: stay quiet
  lastRectSent = now;
  lastWasEmpty = empty;
  const rects = pets
    .map((p) => {
      const b = petBox(p);
      return { id: p.id, x: b.x, y: b.y, w: b.w, h: b.h };
    })
    .concat(panelRects);
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

interface LabelReq { text: string; cx: number; top: number; selected: boolean }

function drawPet(pet: Pet, now: number, dt: number, w: number, idx: number, count: number): LabelReq {
  if (pet.state === "completed" && now >= pet.revertAt) {
    pet.state = "idle";
    pet.detail = "";
    if (pet.id === selected) refreshHeader();
  }
  const m = meta(pet.state);
  const dim = pet.state === "error" || pet.state === "exited" || pet.state === "auth_required";

  const beingDragged = !!drag && drag.pet === pet && drag.moved;
  if (beingDragged) {
    // Follow the cursor during the drag (x + customY are set by mousemove).
    pet.x = Math.max(0, Math.min(w - PET_W, pet.x));
  } else {
    // Roam a horizontal lane (at the pet's height) so they don't pile up.
    const laneW = w / Math.max(1, count);
    const minX = idx * laneW + 4;
    const maxX = idx * laneW + laneW - PET_W - 4;
    if (maxX <= minX) {
      pet.x = idx * laneW + Math.max(0, (laneW - PET_W) / 2);
    } else {
      if (pet.x < minX) pet.x = minX;
      if (pet.x > maxX) pet.x = maxX;
      if (m.walk) {
        pet.x += pet.dir * SPEED * dt;
        if (pet.x <= minX) { pet.x = minX; pet.dir = 1; }
        else if (pet.x >= maxX) { pet.x = maxX; pet.dir = -1; }
      }
    }
  }
  // Keep a dragged height within the window (body + below-label visible).
  if (pet.customY >= 0) {
    pet.customY = Math.max(4, Math.min(window.innerHeight - PET_H - 36, pet.customY));
  }

  const phase = (now / 1000) * BOB_HZ * Math.PI * 2 + pet.x * 0.01;
  const bob = Math.sin(phase) * BOB_AMP;
  const step = m.walk ? Math.sin(phase) * 6 : 0;
  const y = petTop(pet) + Math.abs(bob);

  ctx.globalAlpha = dim ? 0.5 : 1;
  ctx.fillStyle = shade(pet.color, -0.25);
  ctx.fillRect(pet.x + 12, y + PET_H, 14, 9 + step);
  ctx.fillRect(pet.x + PET_W - 26, y + PET_H, 14, 9 - step);
  ctx.fillStyle = pet.color;
  roundRect(ctx, pet.x, y, PET_W, PET_H, 14);
  ctx.fill();
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

  if (m.emote) {
    ctx.font = "26px system-ui, sans-serif";
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText(m.emote, pet.x + PET_W / 2, y - 18);
  }

  const label = m.label ? `${pet.name} · ${m.label}` : pet.name;
  // Labels are drawn in a second pass (drawLabels) so clustered pets don't
  // overprint each other's names.
  return { text: label, cx: pet.x + PET_W / 2, top: y + PET_H + 14, selected: pet.id === selected };
}

const LABEL_H = 18;
/// Draw pet labels, nudging any that would overlap an already-placed one upward.
function drawLabels(reqs: LabelReq[]) {
  ctx.font = "12px system-ui, sans-serif";
  ctx.textAlign = "center";
  ctx.textBaseline = "top";
  const placed: { x0: number; x1: number; y0: number; y1: number }[] = [];
  // Place left-to-right for stable stacking.
  for (const r of [...reqs].sort((a, b) => a.cx - b.cx)) {
    const lw = ctx.measureText(r.text).width + 14;
    const x0 = r.cx - lw / 2;
    const x1 = r.cx + lw / 2;
    let top = r.top;
    let guard = 0;
    while (
      guard++ < 24 &&
      placed.some((p) => x0 < p.x1 && x1 > p.x0 && top < p.y1 + 2 && top + LABEL_H > p.y0 - 2)
    ) {
      top -= LABEL_H + 3;
    }
    placed.push({ x0, x1, y0: top, y1: top + LABEL_H });
    ctx.fillStyle = r.selected ? "rgba(244,121,59,0.85)" : "rgba(0,0,0,0.55)";
    roundRect(ctx, x0, top, lw, LABEL_H, 9);
    ctx.fill();
    ctx.fillStyle = "#ffffff";
    ctx.fillText(r.text, r.cx, top + 3);
  }
}

function draw(now: number) {
  const dt = Math.min((now - last) / 1000, 0.05);
  last = now;
  const w = window.innerWidth;
  ctx.clearRect(0, 0, w, window.innerHeight);
  const labels = pets.map((pet, i) => drawPet(pet, now, dt, w, i, pets.length));
  drawLabels(labels);
  drawHandoffs(now);
  reportRects(now);
  requestAnimationFrame(draw);
}

const HANDOFF_MS = 1100;
function drawHandoffs(now: number) {
  for (let i = handoffs.length - 1; i >= 0; i--) {
    const h = handoffs[i];
    const p = (now - h.start) / HANDOFF_MS;
    if (p >= 1) {
      handoffs.splice(i, 1);
      continue;
    }
    const x = h.fromX + (h.toX - h.fromX) * p;
    const arc = Math.sin(p * Math.PI) * 46; // hop up and over
    ctx.font = "24px system-ui, sans-serif";
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText("📦", x, h.y - 8 - arc);
  }
}

// Pointer drag: moving past the threshold sets the pet's roaming height (it keeps
// walking there); a press that barely moves is a click that opens the chat panel.
const DRAG_THRESHOLD = 4;
let drag: { pet: Pet; offsetX: number; offsetY: number; startX: number; startY: number; moved: boolean } | null = null;

canvas.addEventListener("mousedown", (e) => {
  for (const pet of pets) {
    const b = petBox(pet);
    if (e.clientX >= b.x && e.clientX <= b.x + b.w && e.clientY >= b.y && e.clientY <= b.y + b.h) {
      drag = { pet, offsetX: e.clientX - pet.x, offsetY: e.clientY - petTop(pet), startX: e.clientX, startY: e.clientY, moved: false };
      // Keep the window interactive for the whole drag, even if the cursor
      // briefly outruns the pet's (33ms-stale) hit rect.
      invoke("set_dragging", { dragging: true }).catch(() => {});
      e.preventDefault();
      return;
    }
  }
});

window.addEventListener("mousemove", (e) => {
  if (!drag) return;
  if (!drag.moved && Math.hypot(e.clientX - drag.startX, e.clientY - drag.startY) > DRAG_THRESHOLD) {
    drag.moved = true;
  }
  if (drag.moved) {
    drag.pet.x = Math.max(0, Math.min(window.innerWidth - PET_W, e.clientX - drag.offsetX));
    drag.pet.customY = e.clientY - drag.offsetY; // sets the walking height; clamped in drawPet
  }
});

window.addEventListener("mouseup", () => {
  if (!drag) return;
  const { pet, moved } = drag;
  drag = null;
  invoke("set_dragging", { dragging: false }).catch(() => {});
  if (moved) {
    localStorage.setItem("agpet.pos." + pet.id, JSON.stringify({ y: pet.customY })); // remember the walking height
  } else {
    openPanelFor(pet.id); // a click, not a drag
  }
});

// Double-click a pet to reset it to the default roaming height.
canvas.addEventListener("dblclick", (e) => {
  for (const pet of pets) {
    const b = petBox(pet);
    if (e.clientX >= b.x && e.clientX <= b.x + b.w && e.clientY >= b.y && e.clientY <= b.y + b.h) {
      if (pet.customY >= 0) {
        pet.customY = -1;
        localStorage.removeItem("agpet.pos." + pet.id);
        showToast(`${pet.name} back to default height`);
      }
      return;
    }
  }
});

async function init() {
  try {
    const instances = await invoke<InstanceInfo[]>("list_instances");
    for (const inst of instances) addPet(inst);
  } catch (e) {
    console.error("list_instances failed", e);
  }
}

window.addEventListener("resize", resize);
resize();
applyPrefs();
loadPanelBox();
init().then(() => requestAnimationFrame(draw));
