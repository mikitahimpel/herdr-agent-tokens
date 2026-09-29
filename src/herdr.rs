use std::collections::HashMap;
use std::process::Command;

use serde_json::Value;

#[derive(Clone)]
pub struct Agent {
    pub pane_id: String,
    pub workspace: String,
    pub kind: String,
    pub status: String,
    pub session_id: Option<String>,
}

fn herdr_bin() -> String {
    std::env::var("HERDR_BIN_PATH").unwrap_or_else(|_| "herdr".to_string())
}

fn call(args: &[&str]) -> Option<Value> {
    let output = Command::new(herdr_bin()).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    serde_json::from_slice::<Value>(&output.stdout).ok()?.get("result").cloned()
}

fn items(result: &Option<Value>, key: &str) -> Vec<Value> {
    result.as_ref().and_then(|r| r.get(key)).and_then(Value::as_array).cloned().unwrap_or_default()
}

fn field(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

pub fn list_agents() -> Option<Vec<Agent>> {
    let panes = call(&["pane", "list"]);
    panes.as_ref()?;
    let labels: HashMap<String, String> = items(&call(&["workspace", "list"]), "workspaces")
        .iter()
        .map(|w| (field(w, "workspace_id"), field(w, "label")))
        .collect();

    let mut agents: Vec<Agent> = items(&panes, "panes")
        .iter()
        .filter(|p| !field(p, "agent").is_empty())
        .map(|p| {
            let workspace_id = field(p, "workspace_id");
            Agent {
                pane_id: field(p, "pane_id"),
                workspace: labels.get(&workspace_id).filter(|l| !l.is_empty()).cloned().unwrap_or(workspace_id),
                kind: field(p, "agent"),
                status: field(p, "agent_status"),
                session_id: p
                    .get("agent_session")
                    .and_then(|s| s.get("value"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
            }
        })
        .collect();
    agents.sort_by(|a, b| a.workspace.cmp(&b.workspace).then(a.pane_id.cmp(&b.pane_id)));
    Some(agents)
}

pub fn open_dashboard() -> i32 {
    open_dashboard_for(initial_pane())
}

pub fn open_dashboard_for(pane: Option<String>) -> i32 {
    let plugin = std::env::var("HERDR_PLUGIN_ID").unwrap_or_else(|_| "agent-tokens".to_string());
    let mut command = Command::new(herdr_bin());
    command.args(["plugin", "pane", "open", "--plugin", &plugin, "--entrypoint", "dashboard", "--focus"]);
    if let Some(pane) = pane {
        command.args(["--env", &format!("AGENT_TOKENS_PANE={pane}")]);
    }
    command.output().ok().and_then(|o| o.status.code()).unwrap_or(1)
}

pub fn initial_pane() -> Option<String> {
    if let Ok(pane) = std::env::var("AGENT_TOKENS_PANE") {
        if !pane.is_empty() {
            return Some(pane);
        }
    }
    let context: Value = serde_json::from_str(&std::env::var("HERDR_PLUGIN_CONTEXT_JSON").ok()?).ok()?;
    context.get("focused_pane_id").and_then(Value::as_str).map(str::to_string)
}

