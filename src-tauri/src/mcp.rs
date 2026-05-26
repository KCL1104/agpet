//! In-process MCP server (HTTP) exposing `list_agents` + `delegate` so a
//! "mother" agent can farm subtasks out to sibling agents and (optionally) get
//! their results back. Served on 127.0.0.1; attached to agent sessions via
//! `NewSessionRequest.mcp_servers` when the agent supports HTTP MCP.

use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{tool, tool_handler, tool_router, ServerHandler};
use schemars::JsonSchema;
use serde::Deserialize;
use tauri::{AppHandle, Manager};

use crate::acp::AcpManager;

#[derive(Debug, Deserialize, JsonSchema)]
struct DelegateArgs {
    /// The agent to delegate to — its display name (case-insensitive) or id.
    agent: String,
    /// The task/prompt to give that agent.
    task: String,
    /// If true, block and return the agent's result. Default false: dispatch the
    /// task and return immediately so you can keep working in parallel (the
    /// agent runs concurrently and reports in its own chat).
    #[serde(default)]
    wait: Option<bool>,
}

#[derive(Clone)]
struct DelegateServer {
    tool_router: ToolRouter<Self>,
    app: AppHandle,
}

impl DelegateServer {
    fn new(app: AppHandle) -> Self {
        Self { tool_router: Self::tool_router(), app }
    }
}

#[tool_router]
impl DelegateServer {
    #[tool(description = "List the other agents you can delegate tasks to (name, type, id).")]
    async fn list_agents(&self) -> String {
        let mgr = self.app.state::<AcpManager>();
        let list = mgr.list_instances();
        if list.is_empty() {
            return "(no agents running)".into();
        }
        list.iter()
            .map(|i| format!("- {} (type: {}, id: {})", i.name, i.type_id, i.instance_id))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[tool(
        description = "Delegate a task to another agent by name. Default (wait=false) dispatches \
                       it to run in parallel and returns immediately so you can keep doing your \
                       own work; pass wait=true only when you need that agent's result before \
                       continuing. Busy agents are refused."
    )]
    async fn delegate(&self, Parameters(args): Parameters<DelegateArgs>) -> String {
        let mgr = self.app.state::<AcpManager>();
        match mgr.delegate(&args.agent, args.task, args.wait.unwrap_or(false)).await {
            Ok(out) => out,
            Err(e) => format!("Error: {e}"),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for DelegateServer {}

/// Start the MCP HTTP server on 127.0.0.1 (random port). Returns the URL agents
/// connect to (…/mcp). The `app` handle lets tools reach the `AcpManager`.
pub fn start(app: AppHandle) -> anyhow::Result<String> {
    let listener =
        tauri::async_runtime::block_on(async { tokio::net::TcpListener::bind("127.0.0.1:0").await })?;
    let addr = listener.local_addr()?;
    let url = format!("http://{addr}/mcp");
    let app_for_factory = app.clone();
    let service = StreamableHttpService::new(
        move || Ok(DelegateServer::new(app_for_factory.clone())),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );
    let router = axum::Router::new().nest_service("/mcp", service);
    tauri::async_runtime::spawn(async move {
        if let Err(e) = axum::serve(listener, router).await {
            tracing::error!("MCP server stopped: {e}");
        }
    });
    tracing::info!("MCP delegate server at {url}");
    Ok(url)
}
