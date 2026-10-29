//! The terminal multiplexer the agents run in: Herdr or Rex.
//!
//! `AGENT_TOKENS_HOST=herdr|rex` picks one explicitly. Otherwise Rex is used when
//! `REX_SESSION` is set and the process isn't inside Herdr (`HERDR_ENV=1`, or a
//! plugin process with `HERDR_PLUGIN_ROOT`), and Herdr in every other case.

use crate::{herdr, rex};

#[derive(Clone)]
pub struct Agent {
    pub pane_id: String,
    pub workspace: String,
    pub kind: String,
    pub status: String,
    pub session_id: Option<String>,
    /// The pane the user was in when the dashboard opened, when the host reports it.
    pub focused: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Host {
    Herdr,
    Rex,
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

pub fn current() -> Host {
    let in_herdr = env("HERDR_ENV").as_deref() == Some("1") || env("HERDR_PLUGIN_ROOT").is_some();
    detect(env("AGENT_TOKENS_HOST").as_deref(), env("REX_SESSION").is_some(), in_herdr)
}

fn detect(explicit: Option<&str>, in_rex: bool, in_herdr: bool) -> Host {
    match explicit {
        Some("rex") => Host::Rex,
        Some("herdr") => Host::Herdr,
        _ if in_rex && !in_herdr => Host::Rex,
        _ => Host::Herdr,
    }
}

pub fn list_agents() -> Result<Vec<Agent>, String> {
    match current() {
        Host::Herdr => {
            herdr::list_agents().ok_or_else(|| "Couldn't reach Herdr. Is this running inside a Herdr pane?".into())
        }
        Host::Rex => rex::list_agents(),
    }
}

/// The pane to select first. Without one, the dashboard selects the agent marked focused.
pub fn initial_pane() -> Option<String> {
    if let Some(pane) = env("AGENT_TOKENS_PANE") {
        return Some(pane);
    }
    match current() {
        Host::Herdr => herdr::initial_pane(),
        Host::Rex => None,
    }
}

pub fn open_dashboard() -> i32 {
    open_dashboard_for(initial_pane())
}

pub fn open_dashboard_for(pane: Option<String>) -> i32 {
    match current() {
        Host::Herdr => herdr::open_dashboard_for(pane),
        Host::Rex => rex::open_dashboard_for(pane),
    }
}

/// A pane ID short enough for the agent list. Herdr IDs are already short.
pub fn display_pane(pane_id: &str) -> &str {
    match pane_id.strip_prefix("block:") {
        Some(uuid) => uuid.get(uuid.len().saturating_sub(8)..).unwrap_or(uuid),
        None => pane_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_host_wins() {
        assert_eq!(detect(Some("herdr"), true, false), Host::Herdr);
        assert_eq!(detect(Some("rex"), false, true), Host::Rex);
    }

    #[test]
    fn rex_session_selects_rex_unless_inside_herdr() {
        assert_eq!(detect(None, true, false), Host::Rex);
        assert_eq!(detect(None, true, true), Host::Herdr);
        assert_eq!(detect(None, false, false), Host::Herdr);
        assert_eq!(detect(Some("other"), false, false), Host::Herdr);
    }

    #[test]
    fn shortens_rex_block_ids_only() {
        assert_eq!(display_pane("block:01a17a5e-c6a5-723e-a62c-a3053af63989"), "3af63989");
        assert_eq!(display_pane("wBK:p9"), "wBK:p9");
    }
}
