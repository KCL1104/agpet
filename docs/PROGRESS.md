# ACP Desktop Pet — 進度與交接 (Handoff)

> 最後更新：2026-05-26。此檔記錄目前進度。Source of truth 仍是 [`acp-desktop-pet-spec.md`](./acp-desktop-pet-spec.md)。

## 專案位置

- **路徑**：`C:\Users\USER\agpet`（Windows 檔案系統，原生 Windows 建置 + `tauri dev` hot-reload）。
- 獨立 git repo（已 `git init`），目前只有本機 commit，尚無 GitHub remote。

## 已確認的開發決策

| 主題 | 決定 |
|------|------|
| 執行目標 | **原生 Windows 先做**；macOS 之後用 GitHub Actions (macOS runner) CI 建置（不能交叉編譯）。不用 WSLg（寵物要浮在真正的桌面上）。 |
| 開發流程 | **Claude 直接跑在原生 Windows（PowerShell）**，可自行 `cargo check`／`tauri dev`／殺進程驗證。**只有原生 GUI 的「目視」確認（透明度、走動、點擊穿透）需要使用者親眼看**。（先前文件假設 Claude 在 WSL — 已不適用。） |
| 前端 | vanilla TypeScript + Vite + Canvas（無框架） |
| **ACP Rust SDK** | **官方 `agent-client-protocol`，釘 `=0.12.1`**。⚠️ 注意 `sacp`/`sacp-tokio` 是 **Symposium 的另一個專案**（非官方 crate 改名），不要用。 |
| **ACP 認證** | **沿用既有 Claude Code 訂閱登入；不設 `ANTHROPIC_API_KEY`**。adapter 在 `initialize` 回報 `authMethods: [claude-login]`，會用 `claude /login` 存的 OAuth 憑證。（先前文件寫「需要 ANTHROPIC_API_KEY」是錯的。） |
| 版本控制 | `agpet` 內的巢狀 repo + `.gitignore`（node_modules / dist / target 已排除） |

---

## Milestone 1 — 步驟 1（基礎，單一 agent）✅ 完成並驗證

Tauri v2 + 透明 always-on-top overlay + Canvas 占位寵物左右走動。

- [x] scaffold（vanilla-ts）、透明覆蓋視窗、Canvas 走動寵物、前端建置
- [x] **`cargo check` 通過**（裝好 MSVC 後）。修了一個 scaffold 設定不一致：`tauri.conf.json` 設 `macOSPrivateApi: true`，但 `Cargo.toml` 的 `tauri` 依賴要對應開 `features = ["macos-private-api"]`。
- [x] **使用者目視驗證通過**：無邊框透明置頂視窗、橘色方塊寵物沿底部走動+彈跳+踏步+眼睛朝向、**點擊穿透**、**背景透明無黑底**。

---

## Milestone 1 — 步驟 2（ACP client core）✅ 完成並驗證

實作 Rust ACP client：spawn adapter、完成 `initialize` + `session/new` 握手、log 所有 JSON-RPC。

### 做法
- **SDK**：官方 `agent-client-protocol = "=0.12.1"`。用內建的 `AcpAgent`（自己 spawn 子行程、走 `async_process`+`futures`）+ `Client.builder().on_receive_notification(..).on_receive_request(..).connect_with(agent, |conn| async {..})`。
- **執行緒**：整個 client 是「一個 future」，直接 `tauri::async_runtime::spawn` 在 Tauri 的 runtime 上（future 是 `Send`，**不需**先前計畫的專屬 thread + LocalSet）。失敗只更新 status，不會弄垮寵物（crash 隔離）。
- **Logging**：用 `AcpAgent::with_debug(|line, dir| ..)` 攔截每一行 NDJSON（stdin/stdout/stderr）→ 同時寫 console（tracing）與 **JSONL 檔**。

### 驗證結果（`npm run tauri dev` 實測）
- ✅ `initialize` 往返成功；adapter 回報 `agentInfo: Claude Code v0.16.2`、`authMethods: [claude-login]`（確認走訂閱登入、無 API key）。
- ✅ `session/new` 成功，拿到 `sessionId`、可用 models（Opus 4.6 / Sonnet 4.5 / Haiku 4.5）、modes。
- ✅ 額外收到 inbound `session/update`（`available_commands_update`）→ 證明通知 handler 路徑也通。
- ✅ 所有 JSON-RPC 都進了 **`C:\Users\USER\AppData\Local\com.agpet.pet\logs\acp-messages.jsonl`**（每行 `{ts,dir,msg}`，dir = out/in/stderr）。

### Windows / 環境眉角（已處理）
1. **`npx` 是 `.cmd`**，`CreateProcess` 不能直接跑 → 用 **`cmd /c npx -y @zed-industries/claude-code-acp@latest`** 啟動（見 `acp/config.rs`，可設定）。
2. **必須清掉繼承來的 `CLAUDECODE` 環境變數**：adapter 包著 Claude Code，偵測到此變數會以為是「Claude Code 套 Claude Code」而拒絕開 session（`session/new` 報 Internal error）。若 app 是從 Claude Code 終端啟動就會中鏢；`acp/client.rs` 啟動時 `std::env::remove_var("CLAUDECODE")` 處理掉。一般從桌面啟動不會有此變數。

### 已知後續事項（非阻塞）
- adapter 套件**已更名/標 deprecated**：`@zed-industries/claude-code-acp` → `@agentclientprotocol/claude-agent-acp`。目前舊的仍可用（v0.16.2），M1 維持 spec 的舊名；之後可換新名以持續取得更新。

---

## 關鍵檔案

| 檔案 | 作用 |
|------|------|
| `src-tauri/tauri.conf.json` | overlay 視窗設定（transparent / decorations:false / alwaysOnTop / skipTaskbar / focus:false…）。 |
| `src-tauri/src/lib.rs` | `setup`：撐滿視窗成透明 overlay + `set_ignore_cursor_events(true)`；初始化 ACP（logging + `AcpManager::start`）；`acp_status` command。 |
| `src-tauri/src/acp/mod.rs` | `AcpStatus`（Idle/Connecting/Connected/AuthRequired/Error/Exited，pet 狀態機前身）+ `AcpManager`（管理共享 status）。 |
| `src-tauri/src/acp/config.rs` | `AcpConfig`：spawn 指令/args、cwd、log 路徑（皆可設定）；Windows 走 `cmd /c npx`；不帶 API key。 |
| `src-tauri/src/acp/client.rs` | 核心：清 CLAUDECODE、建 `AcpAgent` + `with_debug`、跑 builder + `initialize`/`session/new` 握手、inbound handler（log）、crash 隔離。 |
| `src-tauri/src/acp/logging.rs` | tracing 初始化 + `MessageLog`（JSONL 寫每行 JSON-RPC）。 |
| `src-tauri/Cargo.toml` | 加 `agent-client-protocol = "=0.12.1"`, `anyhow`, `tracing`, `tracing-subscriber`；`tauri` 開 `macos-private-api`。 |
| `src/main.ts` / `src/styles.css` / `index.html` | Canvas 走動寵物（步驟 1）。 |

## 環境快照（驗證過）

- Windows：Node v23.11.1、npm 10.9.2、Rust/cargo 1.81.0（`x86_64-pc-windows-msvc`）、WebView2 已裝、**VS C++ Build Tools 已裝 ✓**。
- 首次 `tauri dev` 全量建置約 6 分鐘（含 ACP 依賴 async-process/futures/rmcp 等）；之後改 Rust 程式碼為增量編譯（數十秒）。

## Milestone 1 — 步驟 3（ACP 事件 → 寵物狀態）✅ 完成並驗證

把 `session/update` 變體對應成寵物狀態動畫。

### 做法
- `acp/client.rs` 的 `on_receive_notification` 用 `map_update()` 解析 `update.sessionUpdate`（讀 JSON，不綁 Rust enum 變體名）→ 推導狀態，`app.emit("pet-state", {state, detail})` 送前端。映射：`agent_thought_chunk`→thinking、`agent_message_chunk`→responding、`tool_call`/`tool_call_update(pending|in_progress)`→tool_running（帶 title）、`plan`→thinking；其餘忽略。permission 請求→permission 狀態。連線狀態→connecting/idle/error/auth_required/exited。
- 前端 `src/main.ts` listen `pet-state`：依狀態換體色、忙碌時停走、冒 emote 泡泡（💭🔧❓💬✓🔑✕💤）、加小狀態標籤。`completed` 為 transient，前端 ~2.2s 後自動回 idle。

### 驗證結果（實測 log）
狀態鏈完整跑通：`connecting → idle →（送測試 prompt）→ tool_running(`date`) → responding → completed → idle`。測試 prompt 讓 agent 真的跑了 Bash `date`（stdout 回 "Tue, May 26, 2026 …"），stop_reason `end_turn`。

### ⚠️ 臨時驗證機制（步驟 4 要移除/取代）
- `AcpConfig.test_prompt`：握手後**自動送一次**測試 prompt（跑 `date`）。每次啟動會耗一點訂閱額度。步驟 4 換成使用者打字輸入。
- permission handler 目前**自動核可**（選第一個 allow option），好讓測試 prompt 的工具跑起來看到 tool_running。步驟 4 換成真正的授權 UI。
- 前端狀態標籤是 debug 用，之後可拿掉。

## Milestone 1 — 步驟 4（點擊開聊天面板，寵物變可點）✅ 完成並驗證

### 做法
- **動態點擊穿透**（`src-tauri/src/overlay.rs`）：背景執行緒每 ~33ms 讀 `window.cursor_position()`（全域 physical px），對比前端回報的寵物 CSS 矩形（× `scale_factor` + `outer_position` 換算），游標在寵物上或面板開啟時 `set_ignore_cursor_events(false)`，否則 `true`。`OverlayState{ pet_rect, panel_open }` 為 managed state。
- **指令通道**：`AcpManager` 持 `mpsc` 指令通道；`connect_with` 握手後改成指令迴圈（取代 `pending()`），`send_prompt` command 把輸入送進活著的 session。
- **真授權**：`on_receive_request` emit `permission-request` + 存 oneshot 等待；`respond_permission` command 回填使用者選的 optionId；移除自動核可。
- **聊天事件**：`on_receive_notification` 加 emit `chat-event`（agent_message 串流 / agent_thought / tool_call / tool_update / plan）。
- **前端**（`index.html` / `styles.css` / `main.ts`）：右下角 DOM 面板；canvas click 落在寵物矩形內 → 開面板；listen `chat-event` 渲染文字泡泡/thinking/工具 chip；listen `permission-request` 跳 Allow/Deny 按鈕。
- 移除步驟 3 的自動測試 prompt（改由使用者打字）與自動核可。

### 驗證結果（實測 log + 使用者操作）
- hover 寵物 → 可點；移開 → 桌面恢復穿透 ✅
- 點寵物開面板、打字送 `session/prompt` → 面板顯示回覆、寵物 responding→completed ✅
- 送「建立 hello.txt」→ `tool_call: Write` → `session/request_permission`（allow_always/allow/reject）→ Allow/Deny UI → 按 Allow → 工具執行「File created successfully」→ 回覆完成 ✅
- 寵物動畫：tool_running(Write 路徑) → permission → responding → completed ✅

## 🎉 Milestone 1 完成（步驟 1–4）

桌面寵物已是可對話的 Claude Code 前端：透明 overlay 走動寵物、ACP 握手、狀態動畫、點擊聊天、真授權。spec 的「Stop. Verify everything works.」達成。

### 已知小取捨 / 後續可優化
- 面板開啟期間整個視窗暫不穿透（點面板外透明區會被擋）；關閉即恢復。可優化成只有面板矩形非穿透。
- 前端寵物狀態小標籤、工具輸出渲染都還陽春，可再美化。
- adapter 套件 `@zed-industries/claude-code-acp` 已 deprecated → 之後可換 `@agentclientprotocol/claude-agent-acp`。
- spec 步驟 5（`sysinfo` 偵測外部 session）、步驟 6（WSL 偵測）尚未做（M1 列為可選的延伸）。

## 下一步：Milestone 2（Session handoff，feature D）

加 SQLite（`sqlx`）存 session artifact、session 結束產生摘要、history 面板 + resume。詳見 [`acp-desktop-pet-spec.md`](./acp-desktop-pet-spec.md) Milestone 2。

## 此階段刻意未做（之後里程碑）

SQLite/session 持久化（M2）、多 agent 編排（M3）、跨機器（M4）、`sysinfo`/WSL 偵測 — 留待後續。
