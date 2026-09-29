use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use crate::herdr::{self, Agent};
use crate::usage::{self, Report};

pub const REFRESH: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub enum Usage {
    Ready(Arc<Report>),
    Loading,
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

type Stamp = Vec<Option<(SystemTime, u64)>>;

struct Cached {
    path: PathBuf,
    stamp: Stamp,
    report: Arc<Report>,
}

struct Job {
    pane_id: String,
    key: String,
    kind: String,
    session: String,
    path: PathBuf,
    stamp: Stamp,
}

fn stamp(path: &PathBuf) -> Stamp {
    vec![usage::modified(path), usage::modified(&path.with_extension("").join("subagents"))]
}

fn send(snapshots: &Sender<Snapshot>, agents: &[Agent], usage: &HashMap<String, Usage>) -> bool {
    let snapshot = Snapshot { agents: agents.to_vec(), usage: usage.clone(), error: None, at: Instant::now() };
    snapshots.send(snapshot).is_ok()
}

/// Publishes the agent list right away, then parses changed transcripts in parallel,
/// publishing again as each one finishes. Unchanged transcripts come from the cache.
pub fn run(requests: Receiver<()>, snapshots: Sender<Snapshot>) {
    let mut cache: HashMap<String, Cached> = HashMap::new();
    loop {
        let Some(agents) = herdr::list_agents() else {
            let error = Some("Couldn't reach Herdr. Is this running inside a Herdr pane?".into());
            if snapshots.send(Snapshot { agents: Vec::new(), usage: HashMap::new(), error, at: Instant::now() }).is_err() {
                return;
            }
            if matches!(requests.recv_timeout(REFRESH), Err(RecvTimeoutError::Disconnected)) {
                return;
            }
            continue;
        };

        let mut usage = HashMap::new();
        let mut jobs = Vec::new();
        for agent in &agents {
            let (state, job) = plan(agent, &cache);
            usage.insert(agent.pane_id.clone(), state);
            jobs.extend(job);
        }
        if !send(&snapshots, &agents, &usage) {
            return;
        }

        let (done_tx, done_rx) = mpsc::channel();
        for job in jobs {
            let done_tx = done_tx.clone();
            thread::spawn(move || {
                let report = Arc::new(usage::load(&job.kind, &job.session, &job.path));
                let _ = done_tx.send((job, report));
            });
        }
        drop(done_tx);
        for (job, report) in done_rx {
            usage.insert(job.pane_id.clone(), Usage::Ready(report.clone()));
            cache.insert(job.key, Cached { path: job.path, stamp: job.stamp, report });
            if !send(&snapshots, &agents, &usage) {
                return;
            }
        }

        match requests.recv_timeout(REFRESH) {
            Ok(()) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// The usage to show now, and a parse job when the transcript is new or changed.
fn plan(agent: &Agent, cache: &HashMap<String, Cached>) -> (Usage, Option<Job>) {
    if !usage::supported(&agent.kind) {
        return (Usage::Unsupported, None);
    }
    let Some(session) = agent.session_id.as_deref() else { return (Usage::NoSession, None) };
    let key = format!("{}:{session}", agent.kind);
    let cached = cache.get(&key);

    let path = match cached {
        Some(cached) if cached.path.is_file() => cached.path.clone(),
        _ => match usage::find_transcript(&agent.kind, session) {
            Some(path) => path,
            None => return (Usage::NoTranscript, None),
        },
    };
    let current = stamp(&path);
    let shown = cached.map_or(Usage::Loading, |c| Usage::Ready(c.report.clone()));
    if cached.is_some_and(|c| c.path == path && c.stamp == current) {
        return (shown, None);
    }
    let job = Job {
        pane_id: agent.pane_id.clone(),
        key,
        kind: agent.kind.clone(),
        session: session.to_string(),
        path,
        stamp: current,
    };
    (shown, Some(job))
}
