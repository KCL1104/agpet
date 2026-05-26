//! Agent Router: run a workflow across running pet instances, threading each
//! step's output into the next, and emitting handoff events so the frontend can
//! animate one pet "delivering" to another (M3 Slice 2).
//!
//! Uses existing running instances matched by agent type (the first running one
//! of each type). Built-ins are merged with user-editable YAML files dropped in
//! `app_config_dir()/workflows/*.yaml` (Slice 2b).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{mpsc, oneshot};

use super::{AcpCommand, Instance};

#[derive(Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub agent_type: String,
    pub prompt: String,
    pub output_var: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub required_types: Vec<String>,
    /// Deserialized from YAML but not sent to the frontend (it only needs
    /// id/name/required_types; this keeps `list_workflows` payloads lean).
    #[serde(default, skip_serializing)]
    pub steps: Vec<WorkflowStep>,
}

/// Built-in workflows. Merged with user YAML by [`load_all`].
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

/// All workflows: built-ins merged with user YAML from
/// `app_config_dir()/workflows/*.yaml`. A workflow whose `id` matches a built-in
/// replaces it (so the seeded example can be edited to override the default).
/// Loaded on demand, so edits/additions apply without an app restart.
pub fn load_all(app: &AppHandle) -> Vec<Workflow> {
    let mut workflows = builtins();

    let dir = match app.path().app_config_dir() {
        Ok(d) => d.join("workflows"),
        Err(e) => {
            tracing::warn!("workflows: no config dir ({e}); using built-ins only");
            return workflows;
        }
    };

    if !dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::warn!("workflows: create {} failed: {e}", dir.display());
            return workflows;
        }
        let example = dir.join("plan-then-execute.yaml");
        if let Err(e) = std::fs::write(&example, EXAMPLE_WORKFLOW_YAML) {
            tracing::warn!("workflows: write example failed: {e}");
        } else {
            tracing::info!("wrote example workflow at {}", example.display());
        }
    }

    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("workflows: read_dir {} failed: {e}", dir.display());
            return workflows;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_yaml = matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("yaml") | Some("yml")
        );
        if !is_yaml {
            continue;
        }
        let txt = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("workflows: read {} failed: {e}", path.display());
                continue;
            }
        };
        match serde_yaml::from_str::<Workflow>(&txt) {
            Ok(mut wf) => {
                if wf.required_types.is_empty() {
                    wf.required_types = distinct_types(&wf.steps);
                }
                match workflows.iter_mut().find(|w| w.id == wf.id) {
                    Some(slot) => *slot = wf,
                    None => workflows.push(wf),
                }
            }
            Err(e) => tracing::warn!("workflows: skip {} (parse error: {e})", path.display()),
        }
    }
    workflows
}

/// Distinct agent types across a workflow's steps, preserving first-seen order.
fn distinct_types(steps: &[WorkflowStep]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in steps {
        if !out.contains(&s.agent_type) {
            out.push(s.agent_type.clone());
        }
    }
    out
}

/// Seeded into `workflows/plan-then-execute.yaml` on first run — a real,
/// working template users can copy. `{user_input}` is the task; `{{var}}`
/// substitutes a previous step's `output_var`.
const EXAMPLE_WORKFLOW_YAML: &str = r#"# agpet workflow — hands a task between running pets, step by step.
# Drop more *.yaml files in this folder to add workflows (one per file).
# A matching `id` overrides the built-in. Launch the required agent types first.
#
# Placeholders: {user_input} = the task you type; {{var}} = a prior step's output_var.
# `required_types` is optional — if omitted it is derived from the steps.
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
"#;

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
