# ACP 協定參考（給 Milestone 1 步驟 2 用）

> Agent Client Protocol (ACP)：JSON-RPC 2.0 over stdio，**NDJSON**（每行一個 JSON 物件）。
> 所有訊息可帶選用的 `_meta` 物件（邏輯上可忽略）。來源：deepwiki `zed-industries/claude-code-acp`、`agentclientprotocol/agent-client-protocol`。

## 1. 握手 (handshake)

**`initialize`**（client → agent，request/response）。協商協定版本與能力。

Request：
```json
{ "jsonrpc": "2.0", "id": 1, "method": "initialize",
  "params": {
    "protocolVersion": 1,
    "clientCapabilities": {
      "fs": { "readTextFile": true, "writeTextFile": true },
      "terminal": true
    }
  }
}
```
> ⚠️ `protocolVersion` 是**整數 `1`**，不是字串 `"1.0"`（官方範例文件寫 `"1.0"` 只是示意，真實 adapter 用整數 1）。

Response：
```json
{ "jsonrpc": "2.0", "id": 1,
  "result": {
    "protocolVersion": 1,
    "agentCapabilities": {
      "promptCapabilities": { "image": true, "embeddedContext": true },
      "loadSession": true
    },
    "agentInfo": { "name": "...", "version": "..." },
    "authMethods": [ ... ]
  }
}
```

**`session/new`**（client → agent）：
```json
{ "jsonrpc": "2.0", "id": 2, "method": "session/new",
  "params": { "cwd": "/abs/path/project", "mcpServers": [] } }
```
Response：`{ "result": { "sessionId": "sess_abc123", "models": [...], "modes": [...] } }`

## 2. 送出使用者 prompt

**`session/prompt`**（client → agent，request/response）：
```json
{ "jsonrpc": "2.0", "id": 3, "method": "session/prompt",
  "params": {
    "sessionId": "sess_abc123",
    "prompt": [ { "type": "text", "text": "Analyze this code" } ]
  }
}
```
`prompt[]` 的 ContentBlock 型別：`text`、`image`（base64 `data`+`mimeType` 或 `uri`）、`resource`（`{uri, mimeType, text}`）、`resource_link`（`uri`）。

Response 只回傳 turn 結束原因（這個 request 會開著整個 turn，串流靠下方的 `session/update`，turn 結束時才 resolve）：
```json
{ "jsonrpc": "2.0", "id": 3, "result": { "stopReason": "end_turn" } }
```

## 3. `session/update` 串流通知（agent → client，無 `id`）

外層：`{ "method": "session/update", "params": { "sessionId", "update": { "sessionUpdate": <variant>, ... } } }`

| `sessionUpdate` | 內容 | 意義 |
|---|---|---|
| `agent_message_chunk` | `{ content }` | 串流的助理回覆文字/圖片 |
| `agent_thought_chunk` | `{ content }` | extended-thinking / 推理串流 |
| `tool_call` | `{ toolCallId, title, kind, status:"pending", rawInput }` | 宣告新的工具呼叫 |
| `tool_call_update` | `{ toolCallId, status, rawOutput?, content? }` | 既有工具呼叫的進度/結果 |
| `plan` | `{ entries: [{ content, priority, status }] }` | agent 的任務計畫 (todo) |
| `user_message_chunk` | `{ content }` | 重播使用者訊息（history/loadSession） |
| `available_commands_update` | `{ availableCommands: [...] }` | slash 指令 |
| `current_mode_update` | `{ currentModeId }` | 權限模式變更 |

- **ToolKind**：`read, edit, delete, move, search, execute, think, fetch, other`
- **ToolCallStatus**：`pending, in_progress, completed, failed, cancelled`

## 4. 權限處理

**`session/request_permission`**：**agent → client 的 REQUEST**（反方向！client 必須實作此 method handler，不只是送）。工具需使用者核可時觸發。
```json
{ "jsonrpc": "2.0", "id": 5, "method": "session/request_permission",
  "params": {
    "sessionId": "sess_abc123",
    "toolCall": { "toolCallId": "call_001", "rawInput": {...}, "title": "..." },
    "options": [
      { "optionId": "allow-once",  "name": "Allow once", "kind": "allow_once" },
      { "optionId": "reject-once", "name": "Reject",     "kind": "reject_once" }
    ]
  }
}
```
Option `kind`：`allow_once, allow_always, reject_once, reject_always`。

Client response（`outcome` 是雙層巢狀的 tagged union，注意 `outcome.outcome`）：
```json
{ "result": { "outcome": { "outcome": "selected", "optionId": "allow-once" } } }
```
或 `{ "result": { "outcome": { "outcome": "cancelled" } } }`

## 5. 取消 / turn 結束

**`session/cancel`**：**client → agent 的 NOTIFICATION**（無 `id`，無 response）：
```json
{ "jsonrpc": "2.0", "method": "session/cancel", "params": { "sessionId": "sess_abc123" } }
```
進行中的 `session/prompt` 會以 `stopReason: "cancelled"` resolve。

**StopReason**：`end_turn, cancelled, max_tokens, max_turn_requests, refusal`。
（claude-code-acp 實際會回 `end_turn`、`cancelled`、`max_turn_requests`。）

## 6. 啟動 `@zed-industries/claude-code-acp` adapter

- **指令**：`npx -y @zed-industries/claude-code-acp`（或全域安裝後的 `claude-code-acp`）。
- **參數**：無（不解析 CLI args）。全部 I/O 走 **stdin/stdout 的 NDJSON**；stderr 是 log。
- **認證 / env**：需要環境變數 **`ANTHROPIC_API_KEY`**（Console API key）。它**不會**重用 `claude` CLI 的訂閱 OAuth login；缺 key 會回 `authRequired`。
  - 備註：Anthropic 政策上第三方 app 不得透過 Free/Pro/Max 方案憑證代送請求，所以用 Console API key 才合規。
  - 進階：`initialize` 會在 `authMethods` 描述終端 `/login` 流程，若 client 設 `terminal-auth` 能力可走 app 內登入；但 M1 階段建議直接給 `ANTHROPIC_API_KEY`。

## Rust client 實作重點

- 需要**雙向** JSON-RPC peer：
  - 對外 request：`initialize`、`session/new`、`session/prompt`
  - 對外 notification：`session/cancel`
  - 對內 request handler：`session/request_permission`（以及若宣告了能力：`fs/read_text_file`、`fs/write_text_file`、terminal）
  - 對內 notification handler：`session/update`
- 訊息以 **NDJSON**（一行一個 JSON）框定，走子行程的 stdin/stdout。
- 用 `id` 對應 request/response；用 `sessionId` 把 `session/update` 路由到正確的寵物。
- `protocolVersion` 固定用整數 `1`。
