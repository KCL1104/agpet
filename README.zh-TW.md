# agpet

[English](README.md) · **繁體中文** · [简体中文](README.zh-CN.md)

**agpet** 把你的 AI 編碼代理變成在桌面上走動的寵物。它是一層透明、可穿透點擊的覆蓋層（以 Tauri + 原生 TypeScript 打造），疊在你的桌面之上：每個執行中的代理都是一隻在螢幕上漫步的小生物。點一下牠就會打開專屬的對話面板，並在你指定的專案目錄裡開始工作。

在底層，每隻寵物都是透過 [Agent Client Protocol（ACP）](https://agentclientprotocol.com) 連線的即時代理。agpet 開箱即支援 Claude Code、Codex、OpenCode、GitHub Copilot 與 Gemini——你也可以在簡單的 `agents.toml` 裡自行新增或調整代理。每個代理都沿用它自己 CLI 的登入狀態，所以 agpet 從不向你索取 API 金鑰。

## 功能特色

- **桌面上的寵物**——每個執行中的代理實例對應一隻走動的寵物；從系統匣（system tray）啟動與管理牠們。可同時跑好幾隻，甚至同一種類型開多隻。
- **每隻寵物獨立對話**——點一隻寵物即開啟牠的對話面板，支援 Markdown 渲染、`@` 提及檔案選擇器、貼上圖片，以及各代理獨立的模式／模型切換。
- **多代理協作（orchestration）**——一隻「母」代理可透過內建的 MCP `delegate` 工具把子任務交給其他手足代理，再用 `collect` 收回結果。委派的工作在獨立的 git worktree 中執行，不會弄亂你的工作樹。
- **工作流程（Workflows）**——把多個代理串成一條循序管線（例如 *用 Claude 規劃 → 用 Codex 執行 → 用 Claude 審查*），交接過程以寵物對寵物的動畫呈現。你可以在 `workflows/*.yaml` 中自訂。
- **你的 CLI、你的登入**——代理自行啟動各自的 ACP 轉接器；不儲存任何供應商金鑰。對話歷史以 SQLite 保存在本機。

---

## 快速上手

### 1. 安裝 agpet

到 [**Releases**](https://github.com/KCL1104/agpet/releases) 頁面下載最新版本（各平台說明見[下載](#下載)），或[從原始碼建置](#從原始碼建置)。

### 2. 安裝並登入至少一個代理 CLI

agpet 本身不附帶代理——它驅動的是你已經裝好的 CLI。請至少安裝一個並登入一次，讓該 CLI 帶著自己的登入狀態：

| 代理 | 安裝 | 登入 |
| --- | --- | --- |
| **Claude Code** | `npm i -g @anthropic-ai/claude-code` | 執行 `claude` 後輸入 `/login` |
| **Codex** | 參見 Codex CLI 文件 | 依其認證流程操作 |
| **OpenCode** | 參見 [opencode.ai](https://opencode.ai) | `opencode auth login` |
| **GitHub Copilot** | `npm i -g @github/copilot` | 執行 `copilot` 後輸入 `/login` |
| **Gemini** | `npm i -g @google/gemini-cli` | 執行 `gemini` 後登入 |

> 你只需要安裝打算使用的那幾個。agpet 沿用各 CLI 自己的登入——絕不儲存 API 金鑰。預設的 `agents.toml` 透過 `npx` 呼叫 ACP 轉接器，所以只要已登入，連 Claude Code 與 Codex 都不必另外做全域安裝即可運作。

### 3. 啟動你的第一隻寵物

1. 啟動 agpet。它以**系統匣圖示**的形式執行（沒有一般視窗——桌面覆蓋層是透明的）。
2. 點系統匣圖示，選擇 **Open Launcher…（開啟啟動器）**。
3. 選一個**代理類型**（例如 *Claude Code*）和一個**工作目錄**——這就是代理要操作的專案資料夾。
4. 啟動。一隻寵物會出現並開始在桌面漫步。牠的顏色取自 `agents.toml` 裡該代理的 `color`。

### 4. 與寵物對話

- **點一下寵物**即可開啟牠的對話面板。
- 輸入任務後按 **Enter** 送出（**Shift+Enter** 換行）。
- 代理會在你選定的目錄內工作，串流回傳 Markdown 回覆、思考區塊、工具呼叫與差異（diff）。當牠需要對敏感操作取得授權時，會出現 **Allow / Deny（允許／拒絕）**列。

### 5. 組一支團隊

啟動更多寵物——甚至同一種代理類型開好幾隻——讓牠們一起漫步。接著你可以：

- 在對話中用 `//` **提及另一隻寵物**來協調牠們（[多代理協作](#多代理協作)）。
- **執行工作流程**，讓任務沿著一條代理管線往下傳遞（[工作流程](#工作流程)）。

---

## 使用 agpet

### 系統匣

agpet 常駐在系統匣。點圖示可使用：

- **Open Launcher…（開啟啟動器）**——選擇代理類型 + 工作目錄並啟動一隻新寵物。
- **Run Workflow…（執行工作流程）**——開啟工作流程執行器。
- **Worktrees…（工作樹）**——檢視 agpet 為委派／平行任務建立的 git worktree（每一列都有一鍵 **Merge（合併）** 與 **Commit（提交）**）。
- **Close `<名稱>`（關閉）**——每隻執行中的寵物各一項；點選即停止該代理。
- **Quit agpet（結束）**——離開應用程式。

選單會隨寵物的啟動與關閉自動重建，所以執行中的清單永遠是最新的。

### 與寵物互動

| 操作 | 結果 |
| --- | --- |
| **點擊** | 開啟該寵物的對話面板 |
| **點擊 + 拖曳** | 移動寵物；設定自訂的漫步高度（每隻寵物各自記住） |
| **雙擊** | 把寵物重設回預設的基準高度 |

除此之外，覆蓋層其餘部分皆可穿透點擊，因此桌面與其他應用程式照常運作。

### 對話面板

面板標題列會顯示寵物的頭像、可編輯的**對話名稱**（點一下即可改名），以及一個**狀態**圓點（閒置 / 思考中 / 錯誤 / 離線），另有 **＋ New Session（新對話）**、**History（歷史）**、**Reload（重新連線）**、**Settings ⚙（設定）** 與 **Close ×（關閉）** 等按鈕。

**輸入訊息——輸入框認得幾個前綴：**

- `@`——**檔案選擇器。** 開始輸入並從工作目錄中挑選檔案來提及它；路徑會自動插入供代理使用。
- `/`——**指令選擇器。** 自動補全代理本身提供的斜線指令（依代理而異）。
- `//`——**提及代理。** 以名稱引用另一隻執行中的寵物來協調牠，或輸入 `// New <代理類型>` 直接內嵌建立一個新對話。加入 `//` 提及後會顯示**送出模式**選擇器（見下文）。
- **貼上圖片**——會以縮圖形式附加並送給代理（限支援圖片的代理）。

**Settings ⚙（設定）彈出視窗：**

- **Model（模型）**——在該代理提供的模型之間切換。
- **Mode（模式）**——切換代理的模式（例如規劃 vs. 執行），在支援的情況下。
- **Font（字型）**——S / M / L。
- **Density（密度）**——Cozy（寬鬆） / Compact（緊湊）。

這些設定，加上每隻寵物的名稱、位置與上次使用的目錄，都會跨次執行保留。

**History（歷史）** 會列出該寵物過往的對話，附帶時間、狀態與摘要；可恢復其中一個以重新載入它的對話內容。

### 多代理協作

當你用 `//` 提及其他寵物時，會出現一個**送出模式**選擇器，提供四種派送訊息的方式：

- **Orchestrate（協調）**——你正在對話的這隻寵物會成為「母」代理。牠把子任務委派給被提及的寵物、平行作業，再收回並整合牠們的結果成為單一答案。
- **Parallel（平行）**——把同一個任務一次送給所有被提及的寵物，讓牠們各自獨立作業。
- **Vertical（垂直）**——把被提及的寵物串起來，讓每隻的輸出成為下一隻的輸入。
- **Broadcast（廣播）**——以單純的一對多方式，把訊息送給每一隻被提及的寵物。

底層上，母代理使用一個內建的 MCP 伺服器，提供三個工具——`list_agents`、`delegate(agent, task, wait)` 與 `collect(handle)`。當工作目錄是 **git 倉庫**時，每個委派任務都在自己獨立的 worktree 分支（`agpet/delegate/<類型>-<編號>`）中執行，因此手足代理之間絕不會互相覆蓋彼此的檔案，也不會動到你的工作樹。你可以在系統匣的 **Worktrees…** 面板裡審查、**Commit** 或 **Merge** 這些 worktree。無需任何設定——這些工具會自動提供給支援 HTTP MCP 的代理。

### 工作流程

工作流程把一個任務沿著一連串代理腳本化地往下傳，交接過程以寵物對寵物的動畫呈現。從系統匣的 **Run Workflow…** 選單開啟其一，輸入你的任務，看它流經各個步驟。

agpet 內附 **Plan with Claude, execute with Codex（用 Claude 規劃、用 Codex 執行）**，並把它的 YAML 一併寫入，方便你複製。要自訂，只要把 `*.yaml` 檔案放進設定目錄下的 `workflows/` 資料夾即可——參見[自訂工作流程](#自訂工作流程)。

---

## 設定

agpet 把設定保存在應用程式設定目錄：

| 平台 | 路徑 |
| --- | --- |
| **Windows** | `%APPDATA%\com.agpet.pet` |
| **macOS** | `~/Library/Application Support/com.agpet.pet` |
| **Linux** | `~/.config/com.agpet.pet` |

首次執行時，agpet 會在該處寫入一份預設的 `agents.toml` 與一份 `workflows/plan-then-execute.yaml`。

### `agents.toml`——定義代理

每個 `[[agent]]` 區塊就是一種可啟動的寵物類型。各欄位：

| 欄位 | 必填 | 說明 |
| --- | --- | --- |
| `id` | ✅ | 該代理類型的唯一識別碼。 |
| `name` | ✅ | 顯示於啟動器與介面中的名稱。 |
| `command` | ✅ | 要執行的程式（例如 `npx`、`claude`、`opencode`）。在 Windows 上透過 `cmd /c` 執行，因此 `.cmd`／`npx` 的 shim 可正常運作。 |
| `args` | — | 命令列參數（陣列）。 |
| `color` | — | 寵物身體的十六進位顏色（預設 `#e8743b`）。 |
| `wsl` | — | 僅限 Windows：在 WSL 內執行該命令（預設 `false`）。見下文。 |
| `wsl_distro` | — | 指定的 WSL 發行版名稱；省略則使用你的預設發行版。 |

編輯該檔後**重新啟動應用程式**即可套用。預設內容如下：

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

**新增你自己的代理**只是再加一個區塊——把 `command`／`args` 指向任何會講 ACP 的轉接器：

```toml
[[agent]]
id = "my-agent"
name = "My Agent"
command = "my-acp-adapter"
args = ["--acp"]
color = "#3b82f6"
```

### 自訂工作流程

把更多 `*.yaml` 檔案放進 `workflows/` 資料夾（一個檔案一個工作流程）。若某工作流程的 `id` 與內建的相同，就會覆蓋內建版本。檔案是按需讀取的——不需要重新啟動。執行前請**先**啟動該工作流程所需的代理類型。

| 欄位 | 說明 |
| --- | --- |
| `id` | 唯一識別碼（與內建 `id` 相同則會覆蓋它）。 |
| `name` | 顯示於工作流程執行器中的名稱。 |
| `required_types` | 選填。所需的代理類型；省略時會從步驟自動推導。 |
| `steps` | 有序清單。每個步驟有 `agent_type`、一個 `prompt` 與一個 `output_var`。 |

在 prompt 中，`{user_input}` 是你輸入的任務，`{{var}}` 會插入前一步的 `output_var`。內附的範例：

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

### Windows：透過 WSL 執行代理

如果你的代理 CLI 是裝在 **WSL**（Windows Subsystem for Linux）裡，而非原生安裝於 Windows，agpet 可以在那裡啟動它們。在 `agents.toml` 的某個代理加入 `wsl = true`：

```toml
[[agent]]
id = "claude-wsl"
name = "Claude (WSL)"
command = "npx"
args = ["-y", "@agentclientprotocol/claude-agent-acp"]
wsl = true
# wsl_distro = "Ubuntu"   # 選填；省略則使用你的預設發行版
```

設定 `wsl = true` 後，agpet 會透過 `wsl.exe … -- bash -lc '<command>'` 執行命令，並自動把工作目錄與 `@` 提及的路徑從 `C:\…` 轉換成 `/mnt/c/…`，讓代理能正確解析它們。注意事項：

- 該 CLI 必須位於 WSL 內**登入 shell** 的 `PATH` 上（例如全域安裝，或你的 `~/.profile`／`~/.bash_profile` 會載入 `nvm`）。登入 shell 不可向 stdout 印出內容，因為該通道承載 ACP 協定。
- 內建的 `delegate` MCP 伺服器跑在 Windows 主機的 `127.0.0.1` 上。從 WSL 連到它，仰賴 WSL2 的 localhost 轉發（較新版 Windows 11 的鏡像網路）；如果委派無法連線，這就是要檢查的地方。
- 在 macOS／Linux 上 `wsl = true` 會被忽略。

---

## 下載

到 [**Releases**](https://github.com/KCL1104/agpet/releases) 頁面取得最新版本：

| 平台 | 檔案 |
| --- | --- |
| **Windows** | `.exe`（NSIS 安裝程式）或 `.msi` |
| **macOS** | `.dmg`——通用版，Apple Silicon 與 Intel 皆可執行 |

> **macOS：** 此 App 尚未經 Apple 公證，因此 macOS 可能會說它「已損毀，無法打開」。安裝後執行一次以下指令清除隔離旗標：
> ```sh
> xattr -cr /Applications/agpet.app
> ```

你仍需安裝並登入你想使用的代理 CLI（例如 `claude`、`codex`、`opencode`、`copilot`、`gemini`）。agpet 沿用各 CLI 自己的登入——絕不儲存 API 金鑰。

## 從原始碼建置

先決條件：[Rust](https://www.rust-lang.org/tools/install)（stable）、[Node.js](https://nodejs.org)（LTS），以及你作業系統對應的 [Tauri v2 系統相依套件](https://v2.tauri.app/start/prerequisites/)。

```sh
npm install          # 安裝前端相依套件
npm run tauri dev    # 以開發模式執行
npm run tauri build  # 在 src-tauri/target/release/bundle 產生發行包
```

## 發行與 CI/CD

發行由 GitHub Actions 自動產生（[`.github/workflows/release.yml`](.github/workflows/release.yml)）。每次推送到 `main`（以及透過 Actions 分頁手動觸發）時，該工作流程會在 macOS 與 Windows runner 上建置 Tauri App，並把安裝程式發佈到一個標記為 `app-v<version>` 的 GitHub Release，其中 `<version>` 取自 `src-tauri/tauri.conf.json`。

- 推送到 `main` 而**未**變更版本時，會更新既有 `app-v<version>` release 上的資產。
- **調升 `src-tauri/tauri.conf.json` 裡的 `version`** 以切出一個全新的 release。

目前的建置皆未簽署。若要發佈已簽署／公證的 macOS 版本與已簽署的 Windows 安裝程式，請加入相關 secrets 並依照 [Tauri 程式碼簽署指南](https://v2.tauri.app/distribute/sign/) 操作。

## 建議的 IDE 設定

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
