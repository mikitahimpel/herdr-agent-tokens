use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use crate::herdr::{self, Agent};
use crate::usage::{self, Report};

pub const REFRESH: Duration = Duration::from_secs(5);

pub enum Usage {
    Ready(Arc<Report>),
    Unsupported,
    NoSession,
    NoTranscript,
}

pub struct Snapshot {
    pub agents: Vec<Agent>,
    pub usage: HashMap<String, Usage>,
    pub error: Option<String>,
    pub at: Instant,
}

struct Cached {
    path: PathBuf,
    stamp: Vec<Option<(SystemTime, u64)>>,
    report: Arc<Report>,
}

fn stamp(path: &PathBuf) -> Vec<Option<(SystemTime, u64)>> {
    vec![usage::modified(path), usage::modified(&path.with_extension("").join("subagents"))]
}

pub fn run(requests: Receiver<()>, snapshots: Sender<Snapshot>) {
    let mut cache: HashMap<String, Cached> = HashMap::new();
    loop {
        let snapshot = match herdr::list_agents() {
            Some(agents) => {
                let usage = agents.iter().map(|agent| (agent.pane_id.clone(), resolve(agent, &mut cache))).collect();
                Snapshot { agents, usage, error: None, at: Instant::now() }
            }
            None => Snapshot {
                agents: Vec::new(),
                usage: HashMap::new(),
                error: Some("Couldn't reach Herdr. Is this running inside a Herdr pane?".into()),
                at: Instant::now(),
            },
        };
        if snapshots.send(snapshot).is_err() {
            return;
        }
        match requests.recv_timeout(REFRESH) {
            Ok(()) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn resolve(agent: &Agent, cache: &mut HashMap<String, Cached>) -> Usage {
    if !usage::supported(&agent.kind) {
        return Usage::Unsupported;
    }
    let Some(session) = agent.session_id.as_deref() else { return Usage::NoSession };
    let key = format!("{}:{session}", agent.kind);

    let path = match cache.get(&key) {
        Some(cached) if cached.path.is_file() => cached.path.clone(),
        _ => match usage::find_transcript(&agent.kind, session) {
            Some(path) => path,
            None => return Usage::NoTranscript,
        },
    };
    let current = stamp(&path);
    if let Some(cached) = cache.get(&key).filter(|c| c.path == path && c.stamp == current) {
        return Usage::Ready(cached.report.clone());
    }
    let report = Arc::new(usage::load(&agent.kind, session, &path));
    cache.insert(key, Cached { path, stamp: current, report: report.clone() });
    Usage::Ready(report)
}
