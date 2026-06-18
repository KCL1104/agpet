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

## 🎉 Milestone 2 完成（Session handoff，feature D）

寵物有「記憶」了：每個 session 落地 SQLite、結束自動摘要、history 瀏覽、Resume 帶入摘要延續。

### 做法
- **DB 層**（`src-tauri/src/db.rs`）：`sqlx` + SQLite（vendored，靠 MSVC 編譯），`app_data_dir/agpet.db`（Windows：`AppData\Roaming\com.agpet.pet\agpet.db`）。三張表 `sessions` / `session_events` / `session_files`（UUID TEXT + unix ms，runtime query 不用編譯期巨集）。CRUD：create/set_initial_prompt/append_event/append_file/finish/list/get。
- **Session 生命週期**（`acp/client.rs`）：維護可變的 current（acp session id + db uuid）；`AcpCommand` 加 `NewSession` / `Resume(db_id)`。`New` = 送摘要 prompt（收集 agent 回覆文字當 summary）→ `finish_session(completed)` → 同連線再 `session/new` → 新 db row + emit `session-reset`。`Resume` = finish 當前 → 開新 session → 讀過去 summary，**lazy 注入**（前綴到下一則使用者 prompt，避免多花一輪）→ parent_session_id 記錄。
- **事件/檔案記錄**：notification handler 寫 `tool_call` 事件 + `session_files`（Write→create/Edit→edit/Read→read，從 rawInput.file_path + _meta.toolName）；turn 結束寫 `user_message` / `agent_message`。
- **前端**（`index.html`/`styles.css`/`main.ts`）：header 加「＋ New」「History」；History 視圖 `list_sessions` 列出（時間/狀態/摘要 + Resume 鈕）；listen `session-reset` 清空刷新。
- commands：`new_session` / `resume_session` / `list_sessions`（async，用 `db_handle()` 取 Arc<Db> 複本避免跨 await 持有 State）。

### 驗證（使用者實測）
聊天 → **New** → 摘要存檔 ✅；**History** 列出過去 session（摘要+時間，證明 persist+summary+讀取）✅；**Resume** 後 agent 記得先前說的「藍色」✅。DB 在 `AppData\Roaming\com.agpet.pet\agpet.db`（本機無 sqlite3 CLI，但 History 面板即讀 DB，等同驗證）。

### 已知小取捨 / 後續
- 同連線多 `session/new` 可行（已驗證 New/Resume），未走 fallback 重建連線。
- MVP 事件記錄聚焦 user/agent/tool + files；thinking chunk、project-relative 路徑欄位之後再加。
- 摘要/Resume 各耗一次訂閱額度。

## Milestone 3 — Slice 1（多 agent 存在 + 各自聊天）✅ 完成並驗證

- `agents.toml`（`AppData\Roaming\com.agpet.pet\agents.toml`，toml crate，可編輯）宣告 agents；預設 claude/codex/opencode；Windows 用 `cmd /c` 包裝。
- `AcpManager` 持 `Vec<AgentHandle>`，每個 agent 一條連線/狀態/指令通道/pending；`client::start` 帶 `agent_id`，事件（pet-state/chat-event/permission-request）+ DB 全部帶 agent_id；指令依 agent 分派；`list_agents`、`list_sessions(agent_id)`。
- `overlay.rs` 多 pet 矩形 hit-test。
- 前端：`list_agents` → 每 agent 一隻寵物（各自顏色/狀態），點選取開該 agent 面板，per-agent transcript 容器。
- **驗證**：三隻寵物各自獨立 spawn。Claude ✅ 連上可聊；Codex 🔑 auth_required（codex-acp v0.15.0 跑起來但未登入）；OpenCode 💤 exited（`opencode` 不在 PATH）。互不影響 → 隔離正確。

## Agent UX 強化批次 ✅ 完成並驗證

- **型別 vs 實例**：`agents.toml` = 型別範本；runtime 動態 **instance**（一隻寵物=一連線=一 session），**同型可多開**（Claude Code 2…）。事件/指令以 `instance_id` 為鍵；DB 用 `type_id`（歷史按型別匯總）。`AcpManager` 改實例註冊表 + `launch`/`close`/`retry`。
- **系統托盤**（`tray.rs`，tauri `tray-icon` feature）：`New ▸`（各型多開）/ 執行中實例 `Close`（停 adapter）/ `Quit`；選單動態重建。
- **Auth Retry**：client 擷取 `authMethods` 並 emit `agent-config`；面板對 auth_required/error/exited 顯示登入指示 + **Retry** 鈕（`retry_agent`，免重啟）。解決 Codex「沒地方修登入」。
- **品牌主題**：選取實例時面板 `--accent` 換成該 agent 色（Claude `#da7756` / Codex `#10a37f` / OpenCode `#6e7681`）。
- **模型/模式設定**：⚙ popover 下拉（來自握手 models/modes）；`set_model`（需 `unstable_session_model` feature，`session/set_model`）/`set_mode`（`session/set_mode`）。
- **字體/密度 + 拖曳/縮放**：⚙ 字體(S/M/L)、密度(Cozy/Compact)；header 拖曳、左上角縮放；全部存 localStorage。
- **驗證**：3 隻寵物各自起；Claude 連上、Codex 顯示 Retry 列、OpenCode offline；tray 多開「Claude Code 2」、Close、Quit、主題、⚙ 設定、拖曳/縮放皆 OK（使用者實測）。
- 提醒：Codex 需登入（ChatGPT/Codex CLI 或 OPENAI_API_KEY）、OpenCode 需 `opencode` 在 PATH —— 屬使用者環境，非程式問題；面板 Retry 可在登入後免重啟重連。

## 新增 agent 型別 + Launcher + per-instance cwd ✅ 完成並驗證

- **加 Copilot + Gemini 型別**（config-only，M3 重構的紅利）：`agents.toml` 預設現有 5 型 —— claude / codex / opencode / **copilot**（`copilot --acp`）/ **gemini**（`gemini --experimental-acp`）；品牌色修正。Antigravity CLI 的 ACP 指令未明朗，待之後。
- **開機空桌面**：移除 `launch_defaults` 開機呼叫，不再自動生成寵物。
- **Launcher 面板**：tray 改成 **Open Launcher…**（emit `open-launcher`）；前端 launcher 面板列出所有型別，每個可用原生資料夾選擇器（`tauri-plugin-dialog`，`capabilities` 加 `dialog:allow-open`）選 **工作資料夾** 再 Launch；上次選的 cwd 記在 localStorage。
- **每實例自己的 cwd**：`launch_instance(kind, cwd?)`；`Instance` 存 `cwd`，`client::start` + session/new + DB workdir + retry 都用它。**同型可在不同資料夾各開一隻、同時跑**。
- **驗證**（使用者實測）：開機空桌面 → tray Open Launcher → 選資料夾 → 寵物在該目錄工作；可多開不同資料夾。

## Milestone 3 — Slice 2（Agent Router + workflow + 交接動畫）✅ 完成並驗證

- **擷取每步輸出**：`AcpCommand::RunStep{text, reply: oneshot}`（`acp/client.rs`）—— 送 `session/prompt`、`block_task`、把該回合 `turn_text` 全文回傳。
- **編排引擎**（`acp/workflow.rs`）：`Workflow`/`WorkflowStep` 模型 + 內建 **plan-then-execute**（Claude 規劃→Codex 執行→Claude 審查）。`run()` spawn task：依型別找**現有執行中實例**（`Arc<Mutex<Vec<Instance>>>`）、`{user_input}`/`{{var}}` 代入、串接每步輸出、emit `workflow-handoff/step/done/error`。同型別重用同一實例（步驟1、3同一隻 Claude → session 連續）。
- **commands/tray**：`list_workflows`/`run_workflow`；tray「Run Workflow…」→ emit `open-workflows`。
- **前端**：workflow 面板（選 workflow + 輸入 + Run）；listen `workflow-handoff` → canvas 畫 **📦 從一隻寵物飛到另一隻**；step/done/error → toast。
- **驗證**（使用者實測）：開 Claude + Codex（同夾）→ Run plan-then-execute → Claude 規劃 → 📦→ Codex 執行 → 📦→ Claude 審查；**交接動畫 + 輸出串接正常**。
- 缺對應型別寵物時提示「Launch a {type} first」。

## Milestone 3 — Slice 2b（可編輯 YAML workflow）✅ 完成（待目視驗證）

- **YAML crate**：`Cargo.toml` 加 `serde_yaml = "0.9"`（archived 但穩定；`serde_yml` 為備選 fork）。
- **`acp/workflow.rs`**：`Workflow`/`WorkflowStep` 加 `Deserialize`；`steps` 改 `#[serde(default, skip_serializing)]`（**能從 YAML 反序列化、但不送前端**，`list_workflows` payload 維持精簡）；`required_types` 設 `#[serde(default)]`。新增 `load_all(app)`：從 `builtins()` 起步 → 讀 `app_config_dir()/workflows/*.yaml|*.yml`（資料夾不存在則建立並寫範例 `plan-then-execute.yaml`，內容存於 `EXAMPLE_WORKFLOW_YAML` 常數）→ 逐檔 `serde_yaml::from_str`（**parse/read 失敗只 warn 跳過該檔**，不弄垮整張清單）→ `required_types` 為空時由 steps 的相異 `agent_type` 推導 → **以 `id` 去重**（YAML 同 id 覆蓋內建，所以範例檔可被編輯來覆寫預設）。
- **`acp/mod.rs`**：`list_workflows`/`run_workflow` 改呼叫 `workflow::load_all(&self.app)`（`AcpManager` 已持 `app`，**毋需改 `new()` 或 `lib.rs` setup**）。按需載入 → **編輯/新增 YAML 下次開面板即生效，免重啟**。
- commands/tray/前端 workflow 面板早已是動態清單驅動，多個 workflow 自動列出，**前端零改動**。
- **編譯**：`cargo check` 通過。**待使用者目視驗證**：首啟生成範例檔且面板只列一筆（去重）、加第二個 YAML 重開面板即現、壞 YAML 被跳過、plan-then-execute 仍可跑（交接動畫/串接回歸）。

## 打磨批次 2 ✅ 完成（待目視驗證）

- **thinking 存 history**（`acp/client.rs`）：新增每回合 thought 緩衝 `thought_text`（仿 `turn_text`），`record_update` 累積 `agent_thought_chunk`；回合結束由新 helper `record_turn()` **先寫 `thinking` 事件、再寫 `agent_message`**（時間戳自然排序，thinking 在前），**每回合一筆**而非每 chunk 一筆。`RunStep` 仍由 `record_turn` 回傳 agent 全文供 workflow 串接。
- **adapter 套件更名**：`config.rs` 把 claude 預設 `@zed-industries/claude-code-acp`（v0.16.2）→ `@agentclientprotocol/claude-agent-acp`（**npm 實測 v0.37.0，存在且較新**），`default_agents()` 與 `DEFAULT_AGENTS_TOML` 兩處都改。⚠️ **只影響新安裝**：既有 `AppData\Roaming\com.agpet.pet\agents.toml` 仍是舊名，**需手動把該行改成新套件名、或刪檔讓它重生**（不自動覆寫使用者已編輯的 config）。
- **可拖動寵物 + label 防重疊**（前端 + overlay 小改）：
  - `overlay.rs` 加 `dragging: AtomicBool`；clickthrough 迴圈 `desired_ignore = !(panel_open || dragging || over_pet)`。`lib.rs` 加 `set_dragging` command。**避免快速拖曳時游標跑出 33ms 過期的 pet 矩形而誤觸穿透**。
  - `main.ts`：`Pet` 加 `pinned`；`drawPet` 對 pinned 寵物略過 lane/走動、僅夾在畫面內；click handler 改成 mousedown/move/up —— 超過 4px 視為拖曳（pin 到放開處、不走動），未超過視為點擊（開面板）。
  - **label 防重疊**：label 改第二趟 `drawLabels()` 繪製，依 x 排序、與已放置 label 重疊時上移一列，避免名稱互蓋。
  - **編譯**：`cargo check` + `tsc --noEmit` 通過。**待使用者目視驗證**：拖曳順手不掉穿透、放開停住、輕點仍開面板、寵物聚集時 label 可讀。

## 面板點擊穿透修正 ✅ 完成（待目視驗證）

- **問題**：先前只要任一面板（chat/launcher/workflow）開著，`panel_open` 就把**整個畫面**設為可互動 → 開聊天時點不到桌面/其他 app。
- **做法**：改成**回報「開啟中面板的矩形」**給 overlay，跟寵物矩形一視同仁 hit-test，**只有面板（+ 寵物）擋點擊**，其餘透明區照常穿透。
  - `overlay.rs`：拿掉 `panel_open` 一刀切；`desired_ignore = !(dragging || over)`，`over` 來自 `cursor_over_any_rect`（含面板矩形）。移除 `panel_open` 欄位；`lib.rs` 移除 `set_panel_open` command。
  - `main.ts`：`openPanelRects()` 取各未隱藏面板的 `getBoundingClientRect()`（合成 id `__panel:*`）併入 `reportRects`；修掉「無寵物就 early-return」（空桌開 launcher 也要能點）並用 `lastWasEmpty` 在全部關閉時清一次後台矩形。`updatePanelOpen()` 改成 `lastRectSent=0`（下一幀即時刷新，免新 command）。
  - **面板拖曳/縮放護欄**：header 與 resize 的 pointerdown/up 加 `set_dragging(true/false)`（沿用既有旗標），避免快速拖曳時游標跑出過期矩形而中斷（原本靠 `panel_open` 蓋住，現改用 dragging）。
  - `#settings-popover` 是 `#chat-panel` 子元素且在其框內 → 被 chat 面板矩形涵蓋，免另回報。
- **編譯**：`cargo check` + `tsc --noEmit` 通過。**待使用者目視驗證**：開聊天時面板外透明區可點桌面、面板內/寵物照常可點；空桌開 launcher 仍可選資料夾+Launch；打字時游標移出面板不中斷；拖/縮面板不掉穿透；全關後桌面完全穿透。

## 打磨批次 3（並行三件）✅ 完成（待目視驗證）

- **workflow 併發護欄**（`acp/mod.rs` + `acp/workflow.rs`）：原本兩個 workflow 同時跑會把 `RunStep` prompt **交錯送進同一隻 instance 的 session → 兩邊都壞**（session 一次只處理一個 prompt，`block_task().await`）。加 `AcpManager.workflow_running: Arc<AtomicBool>`；`run_workflow` 先解析 workflow（未知 id 不動旗標）再 `compare_exchange(false,true)`，已在跑就回 `Err("A workflow is already running…")`（`run_workflow` command 已回 Result，前端 `.catch` 既有 toast，**前端零改**）。`workflow::run` 收 flag，spawn 內用 **Drop guard `ReleaseOnDrop`** 在任何結束路徑（完成/早退錯誤/panic）自動釋放。**採「拒絕」非「排隊」**（單人 app，排隊屬過度設計）。
- **拖曳位置跨重啟保留**（`main.ts`，純前端）：洞察 —— instance id 其實**跨重啟可決定**（`next_n` 重啟歸零，第一隻 Claude 永遠是 `claude-1`），故直接用 `pet.id` 當 key 即可（best-effort，依啟動順序；同 session 關掉再開會拿到新 id → 無舊紀錄、不會誤套）。放開（真的有拖動）時存 `agpet.pos.<id>={x}`；`addPet` 在 `layoutPets` 前讀回 → `pinned=true` + 夾範圍。無 schema/後端改動。
- **工具輸出渲染**（`acp/client.rs` + `main.ts` + `styles.css`）：`chat_event` 的 `tool_call_update` 原本只送 status，現加 `result`（`update.content` 經既有 `extract_text` 取文字）。前端 chip 改成 `.tool-head`（可點）+ 折疊 `.tool-out`（`<pre>`）：有輸出時加 `has-out`（顯示 ▸）、點 head 展開/收合；輸出截 4000 字避免 DOM 爆。CSS 加 `.tool-head`/`.tool-out`（等寬、捲動、最高 220px）。
- **編譯**：`cargo check` + `tsc --noEmit` 通過。**待使用者目視驗證**：跑 workflow 期間再觸發第二個 → 被拒 + toast、第一個照跑完；拖寵物後重啟 app 再開同一隻 → 回到放開處；跑工具（如 `date`/讀檔）→ chip 可點開看輸出、completed/failed 樣式照舊、大輸出可捲。

## 互動打磨（@ 檔案選取 + 拖曳高度 + 捲軸）✅ 完成（使用者實測中）

- **`@` 檔案選取（prompt 內 mention）**：聊天輸入框打 `@` → 跳出該寵物**工作資料夾**檔案清單；↑/↓/Enter/Tab/Esc/點選操作，選中插入 `@相對路徑`。
  - 後端：`AcpManager.list_dir_files(instance)`（`std::fs` 遞迴走訪 `Instance.cwd`，跳過 `.git`/`node_modules`/`target`… 上限 3000、`/` 分隔）+ `cwd_of`；`lib.rs` 加 `list_dir_files` command。送出時 `AcpCommand::Prompt` 改帶 `files: Vec<String>`，`client.rs` 在 text block 後**每檔加一個 `ContentBlock::ResourceLink`**（`file://` uri，由 cwd 解析），讓 agent 能讀；`@路徑` 文字保留當備援。
  - 前端（`main.ts`/`index.html`/`styles.css`）：`#file-picker` 下拉；`activeMention()` 抓游標前的 `@token`、`list_dir_files` 結果以 instance 快取（session-reset 失效）、子字串過濾（檔名開頭優先）、鍵盤導覽；`sendPrompt` 帶 `files`（只送仍出現在文字中的）。
- **拖曳＝設定「走動高度」**（修正先前「釘住不動」的誤解）：`Pet.customY`（-1=預設基準線）；拖曳時跟游標、放手後**在該高度繼續左右巡邏**（不再凍結）；**雙擊**重設回預設高度。高度存 `agpet.pos.<id>={y}` 跨重啟。`MARGIN_BOTTOM` 預設基準線可調（目前 30）。
- **捲軸樣式**：`#chat-messages`/`#history-view`/`#file-picker`/`.tool-out` 改細的圓角藥丸 thumb、軌道透明、hover 變亮（取代預設灰條）。
- **編譯**：`cargo check` + `tsc --noEmit` 通過；dev server 實測中。

## ACP 內建功能批次（高價值四項）✅ 完成（實測中）

盤點 ACP 還沒用到的內建能力，先做四項高價值的：
- **Stop 鈕（`session/cancel`）**：agent 忙碌時送出鈕變紅 ■，按下送 `CancelNotification` 中止當回合。後端關鍵：主指令迴圈被 `block_task().await` 卡住，故用**獨立 cancel 通道**（`Instance.cancel_tx` + `AcpManager::cancel` + `cancel_prompt` command）+ Prompt arm 內 `tokio::select!`（需 tokio `macros` feature）邊跑邊聽 cancel；送 cancel 前先 drain 掉閒置時的殘留訊號。`conn`（`ConnectionTo<Agent>`）clone 安全。
- **`/` 指令選單**：`chat_event` 轉發 `available_commands_update`→`{kind:"commands"}`；前端把 `@`/`/` 統一成一個 completion picker（`activeTrigger()`、`PickItem{insert,label,hint,file}`），`/` 只在訊息開頭觸發、選中插入 `/name`。
- **工具 chip 變豐富**：`chat_event` 的 tool_call/tool_update 加 `tool_kind`、`locations`、`diff`（從 content 取 `{path,old,new}`）。前端 chip = icon(依 kind)+name+badge、`.tool-loc` 路徑列、點開展開 `.tool-diff`（紅減綠增，各截 40 行）或 `.tool-out` 文字。
- **Plan 清單**：`plan` 事件帶 `entries`；前端 `renderPlan` 就地更新 `.plan`（⬜/⏳/✅），`Pet.currentPlan` 每回合重置。
- **編譯**：`cargo check`（含 tokio macros）+ `tsc` 通過；dev 實測中。註：diff/plan 是否顯示取決於 adapter 是否送對應 update（指令清單確定有）。

## 貼圖 + mode 同步 ✅ 完成（實測中）

ACP 盤點裡「值得做」的兩項：
- **貼圖（`ContentBlock::Image`）**：聊天輸入框 **Ctrl+V 貼上圖片** → 輸入框上方出現縮圖列（可 × 移除）；送出時夾帶。後端 `PromptImage{mime,data(base64)}`、`AcpCommand::Prompt` 加 `images`、`send_prompt(...,images)`、Prompt arm push `ContentBlock::Image(ImageContent::new(data,mime))`。前端 `paste` 事件 → `FileReader` 取 base64 data URL、`pendingImages` + `renderAttachStrip`、`sendPrompt` 夾帶並允許「只有圖片無文字」也能送。⚠️ 需 agent 宣告 `promptCapabilities.image`（Claude 支援）。
- **`current_mode_update` 同步**：`chat_event` 轉發 `{kind:"mode",mode_id}`；前端更新 `cfg.modes.currentModeId` + 即時設定 ⚙ 的 mode 下拉，讓 agent 自己換 mode 時面板跟著動。

## Reload + 輸入框修正 ✅ 完成（已 push）

- **Reload（⟳）**：header 加 ⟳ 鈕，任何狀態（含卡 connecting）都能重連。`client::start` 改回傳 task `JoinHandle`，`AcpManager` 每實例存著，reload(retry)/close 時 **abort** 它——解決「卡在握手、看不到 cmd_tx 被丟棄」的殭屍 adapter（abort→future drop→adapter stdin 關→退出）。前端呼叫既有 `retry_agent`。Reload 會開新 session。
- **輸入框**：placeholder 縮短避免換行觸發捲軸；移除捲軸方向鍵、`#chat-input` 套細捲軸；輸入框隨內容 auto-grow、送出後收回單行。

## `//` 標註 agent + 平行廣播 + 新 session + 命名 ✅ 完成（純前端，實測中）

像 `@` 標檔案一樣，用 `//` 標註「正在跑的 session」並把 prompt **平行**送給它們：
- **`//` 選單**（`main.ts`，沿用 completion picker）：`activeTrigger` 在 `@`/`/` 之前先判 `//`（前面需空白/開頭，避開 `https://`）。`//` branch 列出執行中寵物（比對 name/handle）+ 每個型別的 **➕ New <type>**（`list_types` 快取）。`PickItem` 加 `mentionId`/`newType`；`selectItem` 改 async：選現有→記 `mentionedAgents[handle]=id`、選 ➕New→`launch_instance(kind, last cwd)` 取回 id 再記並插入 `//id`。
- **平行廣播**（`sendPrompt`）：用 `/\/\/([\w-]+)/` 解析 `//handle`→目標 id（先查 `mentionedAgents`，再以執行中寵物的 handle 比對）；有目標就**逐一**在各自 transcript `addMsgTo` + `send_prompt`、toast「Sent to N」；無目標＝送目前寵物（原行為）。
- **命名 session**：點 header 標題就地改名（inline input）；`Pet.handle`（slug，預設=id）供 `//` 用、`Pet.name` 供顯示（header + canvas label）。存 `agpet.name.<id>` 跨重啟還原（同拖曳位置的 id-keying）。
- 純前端、無 Rust 改動；`tsc` 通過、Vite 熱載實測中。caveat：跨不同資料夾的寵物，`@檔案` 路徑各自以自己的 cwd 解析；改名只在前端（tray 仍顯示啟動名）。

## git worktree 任務（平行 / 垂直）✅ 完成（實測中）

prompt 內有 `//標註` → send 時跳 **平行 / 垂直 / 廣播** 選單；平行/垂直用 git worktree 隔離，**另開同型別 worker pet 進 worktree**（不動原 pet）。
- **git 層**（新 `src-tauri/src/git.rs`，shell `git`，無新 crate）：`is_repo`、`worktree_create`（`git worktree add -b <branch> .agpet-worktrees/<slug> HEAD`，分支衝突加尾碼；`.agpet-worktrees/` 寫進 `.git/info/exclude` 不動 tracked .gitignore）、`worktree_list`（`--porcelain` 解析，只留 `.agpet-worktrees/` 下的）、`worktree_remove`（無 `--force`，有未 commit 變更會報錯）。`lib.rs` 加 `worktree_create/list/remove` + `is_git_repo`（皆以 base_instance 取 repo=cwd_of）。
- **垂直接力**（`workflow.rs` `run_handoff` + `mod.rs`/`lib.rs`）：對**明確的 worker id 清單**依序 `RunStep`、把前一步輸出串進下一步（不能用既有 type-based workflow，否則會抓到原本同型別的 pet）；沿用 workflow 併發旗標 + handoff/step/done 事件（📦 動畫）。
- **前端**（`main.ts`/`index.html`/`styles.css`）：`Pet.type`（worker 用）；`#send-modes` 選單（//標註時 send 跳出，Esc/打字取消）；**平行**＝逐目標 `worktree_create`→`launch_instance(type, path)`→`send_prompt`（各自分支）；**垂直**＝開一個 worktree、把 workers 都啟動進去、`run_handoff`。任務文字會去掉 `//token`。非 git repo→toast 後退回廣播。
- **worktree 管理面板**：tray「Worktrees…」→ `open-worktrees` → 列出（branch + path）+ Remove（清乾淨的，有變更則報錯）。
- 分支命名 `agpet/<task-slug>/<type>`（平行）或 `agpet/<task-slug>`（垂直）；worktree 放 `<repo>/.agpet-worktrees/`，**保留**讓你手動 review/merge。
- **驗證**：`cargo check` + `tsc` 通過；`git worktree add/list/remove` 在臨時 repo 實測序列正確。**待目視**：平行各自分支、垂直 📦 接力、面板清理。caveat：合併回主線是手動；worker 不顯示 user 泡泡（直接看 agent 回覆）。

## Markdown 渲染 ✅ 完成（已 push）

- agent / thinking 泡泡改 markdown：加 `marked` + `dompurify`，`renderMarkdown(el,raw)=DOMPurify.sanitize(marked.parse(raw))`；串流時把累積原文存 `el.dataset.raw` 每 chunk 重渲染（泡泡加 `md` class）。user 泡泡與 `.tool-out` 維持純文字。`styles.css` 加 `.msg.md` 內 code/pre/list/link/table/blockquote 樣式。

## 母 agent 編排（MCP delegate 工具）✅ 完成（實測中）

讓「目前選取的母 agent」收到整段 prompt、自己用 `delegate` 工具把子任務分派給其他 agent（可選擇等結果）。
- **可行性**（已查證）：`NewSessionRequest.mcp_servers` 可由 client 帶 MCP server；claude-agent-acp 宣告 `mcpCapabilities.http:true` 並會連 client 提供的 HTTP MCP server；`rmcp` 1.7 已在依賴樹、可跑 server（HTTP/streamable）。
- **agpet 內建 MCP server**（新 `src-tauri/src/mcp.rs`，`rmcp` + `axum`）：`setup` 時在 `127.0.0.1:<port>/mcp` 起一個 `StreamableHttpService`，工具 `list_agents()`、`delegate(agent, task, wait?)`；handler 持 `AppHandle`、用 `app.state::<AcpManager>()` 路由。
- **路由 + 防死結**（`acp/mod.rs`）：`AcpManager::delegate(agent,task,wait)` 以名稱(忽略大小寫)/id 找 instance、送 `RunStep`、`wait` 就 await 回傳輸出否則立即回「dispatched」。每 instance 加 `busy: AtomicBool`（client.rs Prompt/RunStep 進出時設），**delegate 拒絕 busy 目標** → 母 agent 自己 busy 故不會被回頭委派、避免 A→B→A 死結。
- **session 接線**（`client.rs`）：`session/new` 若 agent 有 `mcpCapabilities.http` 就帶 `McpServer::Http("agpet", url)`。
- **後端改名**（`rename_instance` command + `AcpManager::rename`）：前端改名同步到後端 `Instance.name`（delegate 解析名稱 + tray 一致）；`addPet` 還原自訂名也同步。
- **前端**：`#send-modes` 加第 4 個 **🧠 Orchestrate**＝把整段 prompt（+可委派名單註記）送給目前母 agent，由它呼叫 `delegate`。
- **驗證**：`cargo check`（含 rmcp+axum）+ `tsc` 通過；MCP server 實測會啟動（log 印出 url）。**待使用者實測**：母 agent 是否真的呼叫 delegate、子 agent 在各自面板跑、wait 時母 agent 收到結果。caveat：localhost 無 token（之後可加）；delegate 跨 worktree 尚未整合。

## 下一步（新對話接手）

- **Antigravity CLI**（持續延後，**已查證：目前做不了**）：Google 已於 **2026-05-19 用 Antigravity CLI（`agy`，Go 改寫）取代 Gemini CLI**，但 `agy` **尚無 ACP 模式**（無 `--experimental-acp`/`acp` 子命令；程式化整合走另一套 Antigravity SDK，非 ACP stdio）。社群有請願 [zed-industries/zed #57221] 追蹤。⚠️ 另：既有 `gemini --experimental-acp` 型別還能用，但 **Gemini CLI 個人版 2026-06-18 將停用**，屆時該寵物可能連不上、且尚無 ACP 後繼者。→ 待 `agy` 出 ACP 再加（config-only）。
- 主要 backlog 已清。剩餘多為更大方向（見「更後面」）或小修飾（拖曳位置跨不同螢幕寬度的重映射、工具輸出的語法highlight 等），按需再做。

> 接手提示：架構已成熟 —— 加 agent = 改 `agents.toml`（config-only）；加 workflow = 在 `app_config_dir()/workflows/` 丟一個 `*.yaml`（同 id 覆蓋內建，免重啟）；事件/指令皆以 `instance_id` 為鍵、DB 以 `type_id`。關鍵檔案見各里程碑「關鍵檔案」段。本批 + 先前 commit 仍為本機，需 `git push`。

## 更後面（暫不做）
- M4：跨機器團隊（feature B，spec 標延後/可能不做）。
- `sysinfo`/WSL 外部 session 偵測（M1 步驟 5–6，當初列為可選）。

## 🏗️ Production+桌寵化路線圖啟動（2026-06-18）

接手新方向：把 agpet 從「多 agent 編排 demo」推成「**可信賴的 production 工具 + 保留並擴充桌寵遊戲（餵養/升級/XP）**」。先做 8-agent 稽核（結果與可重跑的 workflow 存於 `.claude/agpet-audit.workflow.js`），與使用者敲定四個方向：**地基優先**、**XP 成果導向且純裝飾**、**前端拆模組+薄 store**、**精選穩定+中性品牌**。Milestone：M0 信任地基 / M1 身分基石 / M2 體驗+架構 / M3 純裝飾遊戲層 / M4 重構+法務。

### Slice A（M1 身分基石，keystone）✅ 完成（`cargo check`+`tsc`+`vite build` 通過，待目視）

**問題**：寵物身分原本只是記憶體計數器 `next_n`（`mod.rs`，`claude-1` 每次重開歸零）+ 前端 localStorage（`agpet.pos/name.<instance_id>`），換資料夾/啟動順序就張冠李戴；且毫無遊戲屬性可掛。

- **DB migration 機制**（`db.rs`）：改用 `PRAGMA user_version` 有序冪等 stepper。v1 = 原 sessions 三表（冪等、可「收編」既有無版號 DB）；v2 = `pets` + `xp_events`。
- **`pets` 表**：`pet_id`(UUID,PK) + `type_id`/`workdir`/`kind`('companion'|'worker')/`parent_pet_id` + 身分(`display_name`/`handle`/`color`/`last_x`/`custom_y`) + **遊戲欄位先建好**(`xp`/`level`/`hunger`/`energy`/`happiness`/`last_fed_at`/`last_decay_at`) + 預埋同步(`owner_id`/`device_id`)。`xp_events` 為**不可變 ledger**（與可刪除的 session 內容分離 → 刪對話不丟等級）。
- **Companion vs Worker**（`acp/mod.rs`）：`Instance` 加 `pet_id`/`kind`/`handle`/`custom_y`/`last_x`。**Companion** 以 `(type_id, workdir)` 為鍵、跨重啟穩定，啟動時從記憶體 `companions` map（開機由 `db.list_companions()` 灌入）同步解析、首見即建立並 write-behind 持久化。**Worker**（`delegate`/workflow 臨時生）走新 `launch_worker(type, cwd, parent_pet_id)`，不進 map、`kind='worker'` 記 parent，供日後 XP 歸功母寵物（避免 fan-out 刷 XP）。`launch`/`launch_worker` 共用 `spawn_resolved(Spawn{...})`。
- **持久化**：`rename`（加 `handle` 參數）與新 `set_pet_position` 都同步更新 instance + 記憶體 map + write-behind 寫 DB（僅 companion）。`InstanceInfo`/`instance-added` 帶 `pet_id`/`handle`/`custom_y`/`last_x`。
- **前端**（`main.ts`）：`Pet` 加 `petId`；`addPet` 改用後端身分/位置；新增 `importLegacyState()` **一次性**把舊 localStorage 名稱/位置匯入後端 pet 並刪鍵（之後後端為唯一真相）。拖曳/雙擊重設改呼叫 `set_pet_position`，改名改呼叫 `rename_instance(name, handle)`。`lib.rs` 註冊 `set_pet_position`。
- **待目視**：拖寵物→重開 app→位置/高度/名字保留；換不同資料夾的同型 agent 各自獨立身分不互蓋；舊 localStorage 自動遷移一次。**XP 尚未發放**（屬 M3；表/身分已就緒）。

### Slice B（M0 安全：MCP 認證）✅ 完成（編譯通過，待目視）

堵掉先前自承的「localhost MCP 無 token = 本機 RCE」缺口。
- **`mcp.rs`**：`start()` 改回傳 `(url, token)`（per-run UUID bearer token）；axum `from_fn` middleware `auth_guard` 檢查 **Host 為 loopback**（防 DNS-rebinding）+ **`Authorization: Bearer <token>`**，否則 401。
- **接線**：`lib.rs` 存 `mcp: Option<(String,String)>` 給 `AcpManager`；`client.rs` `session/new` 帶 MCP server 時附 `HttpHeader::new("Authorization", "Bearer …")`（`McpServerHttp::new(..).headers(..)`）。
- **待目視**：母 agent 仍能呼叫 `delegate`（帶 token 通過）；外部無 token 請求被 401 擋下。

> 接手提示更新：身分現在 **pet_id（持久 UUID）vs instance_id（每次啟動的臨時計數）** 兩層；companion 以 (type,workdir) 認；worker 記 parent_pet_id。下一步 backlog：M0 其餘（釘選 npx adapter 版本、CSP、權限 default-deny、CI 測試 gate、簽章、updater）、M1 收尾（session 連 pet_id）、再進 M2/M3。

## 🔒 M0 信任地基（2026-06-18，分支 feat/m0-trust-floor）

把 agpet 推向「可信賴公開發布」的 M0 批次。除了「簽章/notarize」（需使用者憑證）外全部做完，本機 `clippy -D warnings` / `cargo test`(11) / `tsc` / `vite build` 全綠，待目視。

- **供應鏈**：npx adapter 從 `@latest` 釘成 exact 版（`config.rs` 常數 `CLAUDE_ADAPTER`=`@…/claude-agent-acp@0.47.0`、`CODEX_ADAPTER`=`@zed-industries/codex-acp@0.16.0`），開機不再自動執行任意新版上游；使用者可在 agents.toml 覆寫。
- **CSP**：`tauri.conf.json` 的 `csp` 從 `null` → 真實 policy（`script-src 'self'` 擋注入腳本，style/img 放寬保住 markdown）。
- **依賴健康**：封存的 `serde_yaml` → 維護中的 `serde_yaml_ng`（drop-in；`.yaml` 照常）。
- **CI 測試 gate**：新增 `.github/workflows/ci.yml`（clippy `-D warnings` + cargo test + tsc + vite build，跑在 PR/非-main push，fmt 為 non-blocking）；`release.yml` 加 `check` job 把關 + `release: needs: check`——**壞掉的 main 無法發 release**。
- **第一批測試**（11 個，新 `#[cfg(test)]`）：`git.rs` path_slug、`config.rs` WSL 路徑/shell 引號/adapter 釘選斷言、`workflow.rs` 樣板代入/型別推導/builtin、`db.rs` migration 到 v2 + 冪等 + **「重 upsert 身分不洗掉 xp/level」的 keystone 保證** + worker 不入 companion 清單。`Cargo.toml` 加 `[dev-dependencies] tokio rt+macros`。
- **auto-update（updater）**：`tauri-plugin-updater` + `tauri-plugin-process`；`lib.rs` `#[cfg(desktop)]` 註冊 updater + process plugin；`tauri.conf.json` 加 `bundle.createUpdaterArtifacts:true` + `plugins.updater{pubkey,endpoints→GitHub Releases latest.json}`；`capabilities` 加 `updater:default`/`process:allow-restart`；`release.yml` 帶 `TAURI_SIGNING_PRIVATE_KEY[_PASSWORD]` secret；前端 `main.ts` 啟動時 `checkForUpdates()`（download+install+relaunch，失敗/離線/dev 靜默）。⚠️ **私鑰存於 repo 外 `C:\Users\USER\agpet-updater.key`，需手動加成 GitHub secret 後下次 release 才會簽**（沒加 secret 而 createUpdaterArtifacts=true → release 建置會失敗）。
- **權限政策層（default-deny + 稽核）**：`client.rs` 權限 handler 擷取 `tool_kind` + `target`、**所有決策寫 DB 稽核**（`session_events` event_type=`permission`，含 auto 旗標）；**唯讀工具（read/search）可選擇性自動核可**（`auto_allow_reads` per-instance，預設 off、每次啟動歸零；其餘 edit/execute/delete/fetch 永遠 prompt）。`mod.rs` `Instance.auto_allow_reads` + `auto_allow_reads_for`/`set_auto_allow_reads`；`lib.rs` `set_auto_allow_reads` command；前端設定面板加勾選框、權限列顯示 kind+target。

> M0 剩餘：**簽章/notarize**（待使用者辦 Apple Developer + Windows Authenticode 憑證，我再補 `tauri.conf.json`+`release.yml` 接線）。next：M1 收尾（session 連 pet_id）、M2/M3。
