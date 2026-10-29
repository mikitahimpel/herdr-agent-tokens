//! Rex backend. Agents come from `rex-agent list --json`, which reports every
//! Claude Code agent running in a Rex terminal block.

use std::process::Command;

use serde::Deserialize;

use crate::host::Agent;

/// The Rex action that opens the dashboard; it takes `pane=<block_id>`.
const DASHBOARD_ACTION: &str = "tokens_dashboard";

fn rex_agent_bin() -> String {
    std::env::var("REX_AGENT_BIN").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| "rex-agent".to_string())
}

#[derive(Deserialize)]
struct Listing {
    #[serde(default)]
    agents: Vec<RexAgent>,
}

#[derive(Deserialize)]
struct RexAgent {
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    session_label: Option<String>,
    block_id: String,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    claude_session_id: Option<String>,
    #[serde(default)]
    focused: bool,
}

pub fn list_agents() -> Result<Vec<Agent>, String> {
    let bin = rex_agent_bin();
    let output = Command::new(&bin)
        .args(["list", "--json"])
        .output()
        .map_err(|e| format!("Couldn't run {bin}: {e}. Put rex-agent on PATH or set REX_AGENT_BIN."))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("{bin} list failed: {}", stderr.trim()));
    }
    let rex_session = std::env::var("REX_SESSION").ok();
    parse(&output.stdout, rex_session.as_deref()).map_err(|e| format!("Couldn't read {bin} output: {e}"))
}

/// Every session has its own focused pane; the one in the caller's session wins.
fn parse(json: &[u8], rex_session: Option<&str>) -> Result<Vec<Agent>, serde_json::Error> {
    let listing: Listing = serde_json::from_slice(json)?;
    let in_own_session = |a: &RexAgent| rex_session.is_some_and(|s| s == a.session_id);
    let own_focus = listing.agents.iter().any(|a| a.focused && in_own_session(a));

    let mut agents: Vec<Agent> = listing
        .agents
        .into_iter()
        .map(|a| {
            let focused = a.focused && (!own_focus || in_own_session(&a));
            Agent {
                pane_id: a.block_id,
                workspace: a.session_label.filter(|l| !l.is_empty()).unwrap_or(a.session_id),
                kind: "claude".to_string(),
                status: a.state.unwrap_or_default(),
                session_id: a.claude_session_id.filter(|s| !s.is_empty()),
                focused,
            }
        })
        .collect();
    agents.sort_by(|a, b| a.workspace.cmp(&b.workspace).then(a.pane_id.cmp(&b.pane_id)));
    Ok(agents)
}

pub fn open_dashboard_for(pane: Option<String>) -> i32 {
    let mut command = Command::new("rex");
    command.args(["do", DASHBOARD_ACTION]);
    if let Some(pane) = pane {
        command.arg(format!("pane={pane}"));
    }
    command.output().ok().and_then(|o| o.status.code()).unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = r#"{"agents": [
        {"session_id": "session:b", "session_label": "", "window_id": "window:1",
         "block_id": "block:2", "label": "shell", "state": "idle", "kind": null, "msg": "",
         "updated_at": null, "pid": 2, "claude_session_id": null, "name": null, "cwd": null,
         "placed": true, "focused": true},
        {"session_id": "session:a", "session_label": "memoxia", "window_id": "window:1",
         "block_id": "block:1", "label": "claude", "state": "working", "kind": "tool", "msg": "Reading",
         "updated_at": "2026-10-29T20:25:43+01:00", "pid": 1, "claude_session_id": "abc-123",
         "name": "x", "cwd": "/tmp", "placed": true, "focused": true}
    ]}"#;

    #[test]
    fn maps_rex_agents() {
        let agents = parse(LISTING.as_bytes(), None).unwrap();
        assert_eq!(agents.len(), 2);
        let memoxia = &agents[0];
        assert_eq!(memoxia.workspace, "memoxia");
        assert_eq!(memoxia.pane_id, "block:1");
        assert_eq!(memoxia.kind, "claude");
        assert_eq!(memoxia.status, "working");
        assert_eq!(memoxia.session_id.as_deref(), Some("abc-123"));
        let unlabeled = &agents[1];
        assert_eq!(unlabeled.workspace, "session:b");
        assert_eq!(unlabeled.session_id, None);
    }

    #[test]
    fn prefers_focus_in_own_session() {
        let agents = parse(LISTING.as_bytes(), Some("session:b")).unwrap();
        let focused: Vec<&str> = agents.iter().filter(|a| a.focused).map(|a| a.pane_id.as_str()).collect();
        assert_eq!(focused, ["block:2"]);
    }

    #[test]
    fn empty_listing() {
        assert!(parse(br#"{"agents": []}"#, None).unwrap().is_empty());
        assert!(parse(b"not json", None).is_err());
    }
}
