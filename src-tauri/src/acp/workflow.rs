//! Agent Router: run a workflow across running pet instances, threading each
//! step's output into the next, and emitting handoff events so the frontend can
//! animate one pet "delivering" to another (M3 Slice 2).
//!
//! Uses existing running instances matched by agent type (the first running one
//! of each type). YAML-editable workflows are deferred (Slice 2b).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, oneshot};

use super::{AcpCommand, Instance};

#[derive(Clone)]
pub struct WorkflowStep {
    pub agent_type: String,
    pub prompt: String,
    pub output_var: String,
}

#[derive(Clone, Serialize)]
pub struct Workflow {
    pub id: String,
    pub name: String,
    pub required_types: Vec<String>,
    #[serde(skip)]
    pub steps: Vec<WorkflowStep>,
}

/// Built-in workflows. (Slice 2b will also load user YAML from the config dir.)
pub fn builtins() -> Vec<Workflow> {
    fn step(agent: &str, prompt: &str, out: &str) -> WorkflowStep {
        WorkflowStep { agent_type: agent.into(), prompt: prompt.into(), output_var: out.into() }
    }
    vec![Workflow {
        id: "plan-then-execute".into(),
        name: "Plan with Claude, execute with Codex".into(),
        required_types: vec!["claude".into(), "codex".into()],
        steps: vec![
            step("claude", "Create a concise, numbered step-by-step plan for this task. Reply with only the plan.\n\nTask: {user_input}", "plan"),
            step("codex", "Execute this plan precisely in the current project. Plan:\n\n{{plan}}", "result"),
            step("claude", "Review this execution result against the original task and flag any issues or missing pieces. Be brief.\n\nTask: {user_input}\n\nResult:\n{{result}}", "review"),
        ],
    }]
}

/// Find the first running instance of `type_id`; returns (instance_id, sender).
fn find_instance(
    instances: &Arc<Mutex<Vec<Instance>>>,
    type_id: &str,
) -> Option<(String, mpsc::UnboundedSender<AcpCommand>)> {
    let guard = instances.lock().ok()?;
    guard
        .iter()
        .find(|i| i.type_id == type_id && i.cmd_tx.is_some())
        .map(|i| (i.instance_id.clone(), i.cmd_tx.clone().unwrap()))
}

fn substitute(template: &str, vars: &HashMap<String, String>) -> String {
    let mut s = template.to_string();
    for (k, v) in vars {
        s = s.replace(&format!("{{{{{k}}}}}"), v); // {{k}}
        s = s.replace(&format!("{{{k}}}"), v); // {k}
    }
    s
}

/// Spawn the router task that drives the workflow.
pub fn run(app: AppHandle, instances: Arc<Mutex<Vec<Instance>>>, wf: Workflow, user_input: String) {
    tauri::async_runtime::spawn(async move {
        let _ = app.emit("workflow-step", json!({ "workflow_id": wf.id, "status": "started" }));
        let mut vars: HashMap<String, String> = HashMap::new();
        vars.insert("user_input".into(), user_input);
        let mut prev_instance: Option<String> = None;

        for step in &wf.steps {
            let Some((instance_id, tx)) = find_instance(&instances, &step.agent_type) else {
                let _ = app.emit("workflow-error", json!({
                    "workflow_id": wf.id,
                    "message": format!("Launch a {} agent first.", step.agent_type),
                }));
                return;
            };

            let prompt = substitute(&step.prompt, &vars);
            let _ = app.emit("workflow-handoff", json!({
                "from_instance": prev_instance, "to_instance": instance_id,
            }));
            let _ = app.emit("workflow-step", json!({
                "workflow_id": wf.id, "step": step.output_var,
                "instance_id": instance_id, "type": step.agent_type,
            }));

            let (rtx, rrx) = oneshot::channel::<Result<String, String>>();
            if tx.send(AcpCommand::RunStep { text: prompt, reply: rtx }).is_err() {
                let _ = app.emit("workflow-error", json!({ "message": "agent stopped mid-workflow" }));
                return;
            }
            match rrx.await {
                Ok(Ok(out)) => {
                    vars.insert(step.output_var.clone(), out);
                }
                Ok(Err(e)) => {
                    let _ = app.emit("workflow-error", json!({ "message": e }));
                    return;
                }
                Err(_) => {
                    let _ = app.emit("workflow-error", json!({ "message": "step cancelled" }));
                    return;
                }
            }
            prev_instance = Some(instance_id);
        }

        let _ = app.emit("workflow-done", json!({ "workflow_id": wf.id, "ok": true }));
    });
}
