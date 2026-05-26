//! Logging for the ACP client.
//!
//! Two sinks:
//! - `tracing` to stderr (general app + ACP logs), filtered by `RUST_LOG`.
//! - a JSONL file that records **every** JSON-RPC line in both directions. This
//!   is fed by [`agent_client_protocol::AcpAgent::with_debug`], which hands us
//!   each NDJSON line as it is written to / read from the adapter — satisfying
//!   M1 step 2's "log all JSON-RPC messages" requirement directly.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use agent_client_protocol::LineDirection;

/// Initialise tracing for console output. Safe to call more than once (no-ops
/// if a global subscriber is already installed).
pub fn init() {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,agpet=debug,acp=debug"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}

/// Appends every JSON-RPC line (both directions) to a JSONL file.
/// Cloning shares the same underlying file handle, so it can be moved into the
/// `with_debug` callback.
#[derive(Clone)]
pub struct MessageLog {
    file: Arc<Mutex<File>>,
}

impl MessageLog {
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            file: Arc::new(Mutex::new(file)),
        })
    }

    /// Record one line. For stdin/stdout the line is already a JSON object and is
    /// embedded verbatim; for stderr (plain log text) it is JSON-string encoded.
    pub fn record(&self, direction: LineDirection, line: &str) {
        let dir = match direction {
            LineDirection::Stdin => "out",  // we wrote it to the agent
            LineDirection::Stdout => "in",  // the agent wrote it to us
            LineDirection::Stderr => "stderr",
        };
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);

        let payload = if matches!(direction, LineDirection::Stderr) {
            serde_json::to_string(line).unwrap_or_else(|_| "\"\"".into())
        } else {
            line.to_string()
        };
        let record = format!("{{\"ts\":{ts},\"dir\":\"{dir}\",\"msg\":{payload}}}\n");

        if let Ok(mut f) = self.file.lock() {
            let _ = f.write_all(record.as_bytes());
            let _ = f.flush();
        }

        match direction {
            LineDirection::Stderr => tracing::debug!(target: "acp.stderr", "{line}"),
            _ => tracing::info!(target: "acp.rpc", dir, "{line}"),
        }
    }
}
