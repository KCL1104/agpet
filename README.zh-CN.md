# agpet

[English](README.md) · [繁體中文](README.zh-TW.md) · **简体中文**

**agpet** 把你的 AI 编码代理变成在桌面上走动的宠物。它是一层透明、可穿透点击的覆盖层（用 Tauri + 原生 TypeScript 打造），叠在你的桌面之上：每个运行中的代理都是一只在屏幕上漫步的小生物。点一下它就会打开专属的对话面板，并在你指定的项目目录里开始工作。

在底层，每只宠物都是通过 [Agent Client Protocol（ACP）](https://agentclientprotocol.com) 连接的实时代理。agpet 开箱即支持 Claude Code、Codex、OpenCode、GitHub Copilot 与 Gemini——你也可以在简单的 `agents.toml` 里自行新增或调整代理。每个代理都沿用它自己 CLI 的登录状态，所以 agpet 从不向你索取 API 密钥。

## 功能特性

- **桌面上的宠物**——每个运行中的代理实例对应一只走动的宠物；从系统托盘（system tray）启动与管理它们。可同时运行好几只，甚至同一种类型开多只。
- **每只宠物独立对话**——点一只宠物即打开它的对话面板，支持 Markdown 渲染、`@` 提及文件选择器、粘贴图片，以及各代理独立的模式／模型切换。
- **多代理编排（orchestration）**——一只「母」代理可通过内置的 MCP `delegate` 工具把子任务交给其他同级代理，再用 `collect` 收回结果。委派的工作在独立的 git worktree 中运行，不会弄乱你的工作树。
- **工作流（Workflows）**——把多个代理串成一条顺序流水线（例如 *用 Claude 规划 → 用 Codex 执行 → 用 Claude 审查*），交接过程以宠物对宠物的动画呈现。你可以在 `workflows/*.yaml` 中自定义。
- **你的 CLI、你的登录**——代理自行启动各自的 ACP 适配器；不存储任何供应商密钥。对话历史以 SQLite 保存在本机。

---

## 快速上手

### 1. 安装 agpet

到 [**Releases**](https://github.com/KCL1104/agpet/releases) 页面下载最新版本（各平台说明见[下载](#下载)），或[从源码构建](#从源码构建)。

### 2. 安装并登录至少一个代理 CLI

agpet 本身不附带代理——它驱动的是你已经装好的 CLI。请至少安装一个并登录一次，让该 CLI 带着自己的登录状态：

| 代理 | 安装 | 登录 |
| --- | --- | --- |
| **Claude Code** | `npm i -g @anthropic-ai/claude-code` | 运行 `claude` 后输入 `/login` |
| **Codex** | 参见 Codex CLI 文档 | 按其认证流程操作 |
| **OpenCode** | 参见 [opencode.ai](https://opencode.ai) | `opencode auth login` |
| **GitHub Copilot** | `npm i -g @github/copilot` | 运行 `copilot` 后输入 `/login` |
| **Gemini** | `npm i -g @google/gemini-cli` | 运行 `gemini` 后登录 |

> 你只需要安装打算使用的那几个。agpet 沿用各 CLI 自己的登录——绝不存储 API 密钥。默认的 `agents.toml` 通过 `npx` 调用 ACP 适配器，所以只要已登录，连 Claude Code 与 Codex 都不必另外做全局安装即可运行。

### 3. 启动你的第一只宠物

1. 启动 agpet。它以**系统托盘图标**的形式运行（没有普通窗口——桌面覆盖层是透明的）。
2. 点系统托盘图标，选择 **Open Launcher…（打开启动器）**。
3. 选一个**代理类型**（例如 *Claude Code*）和一个**工作目录**——这就是代理要操作的项目文件夹。
4. 启动。一只宠物会出现并开始在桌面漫步。它的颜色取自 `agents.toml` 里该代理的 `color`。

### 4. 与宠物对话

- **点一下宠物**即可打开它的对话面板。
- 输入任务后按 **Enter** 发送（**Shift+Enter** 换行）。
- 代理会在你选定的目录内工作，流式返回 Markdown 回复、思考块、工具调用与差异（diff）。当它需要对敏感操作取得授权时，会出现 **Allow / Deny（允许／拒绝）**栏。

### 5. 组一支团队

启动更多宠物——甚至同一种代理类型开好几只——让它们一起漫步。接着你可以：

- 在对话中用 `//` **提及另一只宠物**来协调它们（[多代理编排](#多代理编排)）。
- **运行工作流**，让任务沿着一条代理流水线往下传递（[工作流](#工作流)）。

---

## 使用 agpet

### 系统托盘

agpet 常驻在系统托盘。点图标可使用：

- **Open Launcher…（打开启动器）**——选择代理类型 + 工作目录并启动一只新宠物。
- **Run Workflow…（运行工作流）**——打开工作流运行器。
- **Worktrees…（工作树）**——查看 agpet 为委派／并行任务创建的 git worktree（每一行都有一键 **Merge（合并）** 与 **Commit（提交）**）。
- **Close `<名称>`（关闭）**——每只运行中的宠物各一项；点选即停止该代理。
- **Quit agpet（退出）**——退出应用程序。

菜单会随宠物的启动与关闭自动重建，所以运行中的列表永远是最新的。

### 与宠物互动

| 操作 | 结果 |
| --- | --- |
| **点击** | 打开该宠物的对话面板 |
| **点击 + 拖拽** | 移动宠物；设置自定义的漫步高度（每只宠物各自记住） |
| **双击** | 把宠物重置回默认的基准高度 |

除此之外，覆盖层其余部分皆可穿透点击，因此桌面与其他应用程序照常运行。

### 对话面板

面板标题栏会显示宠物的头像、可编辑的**对话名称**（点一下即可改名），以及一个**状态**圆点（空闲 / 思考中 / 错误 / 离线），另有 **＋ New Session（新对话）**、**History（历史）**、**Reload（重新连接）**、**Settings ⚙（设置）** 与 **Close ×（关闭）** 等按钮。

**输入消息——输入框认得几个前缀：**

- `@`——**文件选择器。** 开始输入并从工作目录中挑选文件来提及它；路径会自动插入供代理使用。
- `/`——**命令选择器。** 自动补全代理本身提供的斜杠命令（因代理而异）。
- `//`——**提及代理。** 以名称引用另一只运行中的宠物来协调它，或输入 `// New <代理类型>` 直接内嵌创建一个新对话。加入 `//` 提及后会显示**发送模式**选择器（见下文）。
- **粘贴图片**——会以缩略图形式附加并发给代理（限支持图片的代理）。

**Settings ⚙（设置）弹出窗口：**

- **Model（模型）**——在该代理提供的模型之间切换。
- **Mode（模式）**——切换代理的模式（例如规划 vs. 执行），在支持的情况下。
- **Font（字体）**——S / M / L。
- **Density（密度）**——Cozy（宽松） / Compact（紧凑）。

这些设置，加上每只宠物的名称、位置与上次使用的目录，都会跨次运行保留。

**History（历史）** 会列出该宠物过往的对话，附带时间、状态与摘要；可恢复其中一个以重新加载它的对话内容。

### 多代理编排

当你用 `//` 提及其他宠物时，会出现一个**发送模式**选择器，提供四种派发消息的方式：

- **Orchestrate（编排）**——你正在对话的这只宠物会成为「母」代理。它把子任务委派给被提及的宠物、并行作业，再收回并整合它们的结果成为单一答案。
- **Parallel（并行）**——把同一个任务一次发给所有被提及的宠物，让它们各自独立作业。
- **Vertical（垂直）**——把被提及的宠物串起来，让每只的输出成为下一只的输入。
- **Broadcast（广播）**——以单纯的一对多方式，把消息发给每一只被提及的宠物。

底层上，母代理使用一个内置的 MCP 服务器，提供三个工具——`list_agents`、`delegate(agent, task, wait)` 与 `collect(handle)`。当工作目录是 **git 仓库**时，每个委派任务都在自己独立的 worktree 分支（`agpet/delegate/<类型>-<编号>`）中运行，因此同级代理之间绝不会互相覆盖彼此的文件，也不会动到你的工作树。你可以在系统托盘的 **Worktrees…** 面板里审查、**Commit** 或 **Merge** 这些 worktree。无需任何配置——这些工具会自动提供给支持 HTTP MCP 的代理。

### 工作流

工作流把一个任务沿着一连串代理脚本化地往下传，交接过程以宠物对宠物的动画呈现。从系统托盘的 **Run Workflow…** 菜单打开其一，输入你的任务，看它流经各个步骤。

agpet 内置 **Plan with Claude, execute with Codex（用 Claude 规划、用 Codex 执行）**，并把它的 YAML 一并写入，方便你复制。要自定义，只要把 `*.yaml` 文件放进配置目录下的 `workflows/` 文件夹即可——参见[自定义工作流](#自定义工作流)。

---

## 配置

agpet 把配置保存在应用程序配置目录：

| 平台 | 路径 |
| --- | --- |
| **Windows** | `%APPDATA%\com.agpet.pet` |
| **macOS** | `~/Library/Application Support/com.agpet.pet` |
| **Linux** | `~/.config/com.agpet.pet` |

首次运行时，agpet 会在该处写入一份默认的 `agents.toml` 与一份 `workflows/plan-then-execute.yaml`。

### `agents.toml`——定义代理

每个 `[[agent]]` 块就是一种可启动的宠物类型。各字段：

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `id` | ✅ | 该代理类型的唯一标识符。 |
| `name` | ✅ | 显示于启动器与界面中的名称。 |
| `command` | ✅ | 要运行的程序（例如 `npx`、`claude`、`opencode`）。在 Windows 上通过 `cmd /c` 运行，因此 `.cmd`／`npx` 的 shim 可正常工作。 |
| `args` | — | 命令行参数（数组）。 |
| `color` | — | 宠物身体的十六进制颜色（默认 `#e8743b`）。 |
| `wsl` | — | 仅限 Windows：在 WSL 内运行该命令（默认 `false`）。见下文。 |
| `wsl_distro` | — | 指定的 WSL 发行版名称；省略则使用你的默认发行版。 |

编辑该文件后**重启应用程序**即可生效。默认内容如下：

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

**新增你自己的代理**只是再加一个块——把 `command`／`args` 指向任何会讲 ACP 的适配器：

```toml
[[agent]]
id = "my-agent"
name = "My Agent"
command = "my-acp-adapter"
args = ["--acp"]
color = "#3b82f6"
```

### 自定义工作流

把更多 `*.yaml` 文件放进 `workflows/` 文件夹（一个文件一个工作流）。若某工作流的 `id` 与内置的相同，就会覆盖内置版本。文件是按需读取的——不需要重启。运行前请**先**启动该工作流所需的代理类型。

| 字段 | 说明 |
| --- | --- |
| `id` | 唯一标识符（与内置 `id` 相同则会覆盖它）。 |
| `name` | 显示于工作流运行器中的名称。 |
| `required_types` | 选填。所需的代理类型；省略时会从步骤自动推导。 |
| `steps` | 有序列表。每个步骤有 `agent_type`、一个 `prompt` 与一个 `output_var`。 |

在 prompt 中，`{user_input}` 是你输入的任务，`{{var}}` 会插入前一步的 `output_var`。内置的示例：

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

### Windows：通过 WSL 运行代理

如果你的代理 CLI 是装在 **WSL**（Windows Subsystem for Linux）里，而非原生安装于 Windows，agpet 可以在那里启动它们。在 `agents.toml` 的某个代理加入 `wsl = true`：

```toml
[[agent]]
id = "claude-wsl"
name = "Claude (WSL)"
command = "npx"
args = ["-y", "@agentclientprotocol/claude-agent-acp"]
wsl = true
# wsl_distro = "Ubuntu"   # 选填；省略则使用你的默认发行版
```

设置 `wsl = true` 后，agpet 会通过 `wsl.exe … -- bash -lc '<command>'` 运行命令，并自动把工作目录与 `@` 提及的路径从 `C:\…` 转换成 `/mnt/c/…`，让代理能正确解析它们。注意事项：

- 该 CLI 必须位于 WSL 内**登录 shell** 的 `PATH` 上（例如全局安装，或你的 `~/.profile`／`~/.bash_profile` 会加载 `nvm`）。登录 shell 不可向 stdout 打印内容，因为该通道承载 ACP 协议。
- 内置的 `delegate` MCP 服务器跑在 Windows 主机的 `127.0.0.1` 上。从 WSL 连到它，依赖 WSL2 的 localhost 转发（较新版 Windows 11 的镜像网络）；如果委派无法连接，这就是要检查的地方。
- 在 macOS／Linux 上 `wsl = true` 会被忽略。

---

## 下载

到 [**Releases**](https://github.com/KCL1104/agpet/releases) 页面获取最新版本：

| 平台 | 文件 |
| --- | --- |
| **Windows** | `.exe`（NSIS 安装程序）或 `.msi` |
| **macOS** | `.dmg`——通用版，Apple Silicon 与 Intel 皆可运行 |

> **macOS：** 此 App 尚未经 Apple 公证，因此 macOS 可能会说它「已损坏，无法打开」。安装后运行一次以下命令清除隔离标志：
> ```sh
> xattr -cr /Applications/agpet.app
> ```

你仍需安装并登录你想使用的代理 CLI（例如 `claude`、`codex`、`opencode`、`copilot`、`gemini`）。agpet 沿用各 CLI 自己的登录——绝不存储 API 密钥。

## 从源码构建

先决条件：[Rust](https://www.rust-lang.org/tools/install)（stable）、[Node.js](https://nodejs.org)（LTS），以及你操作系统对应的 [Tauri v2 系统依赖](https://v2.tauri.app/start/prerequisites/)。

```sh
npm install          # 安装前端依赖
npm run tauri dev    # 以开发模式运行
npm run tauri build  # 在 src-tauri/target/release/bundle 生成发行包
```

## 发行与 CI/CD

发行由 GitHub Actions 自动产生（[`.github/workflows/release.yml`](.github/workflows/release.yml)）。每次推送到 `main`（以及通过 Actions 标签页手动触发）时，该工作流会在 macOS 与 Windows runner 上构建 Tauri App，并把安装程序发布到一个标记为 `app-v<version>` 的 GitHub Release，其中 `<version>` 取自 `src-tauri/tauri.conf.json`。

- 推送到 `main` 而**未**更改版本时，会更新既有 `app-v<version>` release 上的资产。
- **调升 `src-tauri/tauri.conf.json` 里的 `version`** 以切出一个全新的 release。

目前的构建均未签名。若要发布已签名／公证的 macOS 版本与已签名的 Windows 安装程序，请加入相关 secrets 并按照 [Tauri 代码签名指南](https://v2.tauri.app/distribute/sign/) 操作。

## 推荐的 IDE 配置

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
