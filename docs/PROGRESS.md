# ACP Desktop Pet — 進度與交接 (Handoff)

> 最後更新：2026-05-26。此檔記錄目前進度，方便切換到 Windows 端繼續開發。
> Source of truth 仍是 [`acp-desktop-pet-spec.md`](./acp-desktop-pet-spec.md)。

## 專案位置

- **路徑**：`C:\Users\USER\agpet`（Windows 檔案系統，方便原生 Windows 建置與 `tauri dev` hot-reload）
- 這是獨立的 git repo（已 `git init`，與 home 目錄的 repo 無關），目前只有本機 commit，尚無 GitHub remote。

## 已確認的開發決策

| 主題 | 決定 |
|------|------|
| 執行目標 | **原生 Windows 先做**；macOS 之後用 GitHub Actions (macOS runner) CI 建置（不能從 Win/Linux 交叉編譯）。**不用 WSLg**（桌面寵物要浮在真正的桌面上）。 |
| 專案位置 | Windows 檔案系統 `C:\Users\USER\agpet` |
| 開發流程 | 在 WSL 的 Claude 負責寫程式 + 用 interop 跑 `cargo check`／build 驗證；**原生 GUI 由使用者在 Windows 跑 `npm run tauri dev` 親自確認**（WSL 看不到原生視窗）。 |
| 前端 | vanilla TypeScript + Vite + Canvas（無框架） |
| 版本控制 | `agpet` 內的巢狀 repo + `.gitignore`（node_modules / dist / target / gen/schemas 已排除） |

## 進度：Milestone 1 — 步驟 1（基礎，單一 agent、無編排）✅ 程式碼完成

對應 spec「First task」：Tauri v2 專案 + 透明 always-on-top 視窗 + 一隻占位方塊左右走動，然後停下驗證。

已完成：
- [x] `create-tauri-app`（vanilla-ts）scaffold，Windows npm install（exit 0）
- [x] 規格文件複製進專案、巢狀 git init、首次 commit
- [x] 透明覆蓋視窗設定（`src-tauri/tauri.conf.json` + `src-tauri/src/lib.rs`）
- [x] Canvas 走動寵物（`src/main.ts` + `src/styles.css` + `index.html`）
- [x] 前端建置驗證：`tsc && vite build` 通過（無型別錯誤）
- [ ] **Windows `cargo check`** — ⛔ 被 MSVC 擋住（見下方前置作業）
- [ ] **使用者在 Windows 跑 `npm run tauri dev` 目視驗證寵物** — 待做

## ⚠️ 繼續前必做的環境前置（Windows）

Windows 的 Rust 是 `x86_64-pc-windows-msvc` toolchain，但 **Visual Studio C++ Build Tools 尚未安裝**（找不到 `link.exe`），因此 Rust 後端無法連結，`tauri dev` 跑不起來。

安裝（系統管理員，約數 GB）：
```powershell
winget install --id Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```
> 若裝完 `cargo` 抱怨 Rust 版本太舊（目前 1.81.0），執行 `rustup update stable`。

## 在 Windows 端繼續的步驟

於 PowerShell 或終端機：
```powershell
cd C:\Users\USER\agpet

# 1) 確認後端可編譯（裝好 MSVC 後）
cargo check --manifest-path src-tauri\Cargo.toml

# 2) 啟動 app（會開出透明 overlay 視窗）
npm run tauri dev
```

### 目視驗證清單（步驟 1 過關條件）
- [ ] 出現一個**無邊框、透明、置頂**的視窗，蓋住主螢幕
- [ ] 一隻橘色圓角方塊寵物沿**螢幕底部左右走動**，碰到邊緣會轉向
- [ ] 走動時有上下彈跳 + 踏步動畫，眼睛朝行進方向
- [ ] **點擊會穿透**到後面的桌面 / 其他視窗（寵物本身此階段不可互動）
- [ ] 背景真的是透明的（若 WebView2 出現黑底，記下來——這是已知的透明度議題，回報即可）

驗證 OK 後即達成 spec 的「Stop. Verify everything works.」，可進入步驟 2。

## 關鍵檔案

| 檔案 | 作用 |
|------|------|
| `src-tauri/tauri.conf.json` | `app.windows[0]`：`transparent / decorations:false / alwaysOnTop / skipTaskbar / resizable:false / shadow:false / focus:false`；`app.macOSPrivateApi:true`（為 mac 鋪路）。視窗實際大小於執行期改成螢幕大小。 |
| `src-tauri/src/lib.rs` | `setup` hook：抓主螢幕尺寸把視窗撐滿成透明 overlay，並 `set_ignore_cursor_events(true)` 讓點擊穿透。 |
| `src-tauri/src/main.rs` | scaffold 預設，呼叫 `agpet_lib::run()`。 |
| `src/main.ts` | Canvas `requestAnimationFrame` 走動迴圈（走動 / 邊緣反向 / 彈跳 / 踏步 / 朝向眼睛 / DPR 縮放）。 |
| `src/styles.css` | `html,body` 透明、無捲動；canvas 固定全螢幕。 |
| `index.html` | 只剩一個 `<canvas id="pet-canvas">`。 |

## 環境快照（驗證過）

- Windows：Node v23.11.1、npm 10.9.2、Rust/cargo 1.81.0（`x86_64-pc-windows-msvc`）、WebView2 runtime 已裝、**VS C++ Build Tools 未裝**。
- WebView2 runtime：已安裝 ✓

## 下一步：Milestone 1 — 步驟 2（ACP client core）

實作 Rust ACP client：spawn `@zed-industries/claude-code-acp`，完成 `initialize` + `session/new` 握手，記錄所有 JSON-RPC 訊息。

- ACP 協定細節（握手、`session/prompt`、`session/update` 變體、`session/request_permission`、`session/cancel`、stop reasons、認證）已整理在 [`acp-protocol-reference.md`](./acp-protocol-reference.md)。
- 認證：`claude-code-acp` 需要環境變數 **`ANTHROPIC_API_KEY`**（Console API key；目前未設定）。

## 此階段刻意未做（之後里程碑）

ACP client、JSON-RPC 握手、SQLite、process polling、WSL 偵測、聊天面板、動態點擊穿透切換、多 agent 編排 — 全部留待後續里程碑。
