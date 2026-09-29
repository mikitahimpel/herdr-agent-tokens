use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local, TimeZone};
use serde_json::Value;

#[derive(Clone, Copy, Default)]
pub struct Totals {
    pub calls: u64,
    pub input: u64,
    pub cache_write: u64,
    pub cache_read: u64,
    pub output: u64,
    pub reasoning: u64,
}

impl Totals {
    pub fn add(&mut self, other: &Totals) {
        self.calls += other.calls;
        self.input += other.input;
        self.cache_write += other.cache_write;
        self.cache_read += other.cache_read;
        self.output += other.output;
        self.reasoning += other.reasoning;
    }

    pub fn prompt(&self) -> u64 {
        self.input + self.cache_write + self.cache_read
    }

    pub fn processed(&self) -> u64 {
        self.prompt() + self.output
    }

    pub fn cache_hit(&self) -> f64 {
        ratio(self.cache_read, self.prompt())
    }
}

pub fn ratio(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64
    }
}

#[derive(Clone)]
pub struct Turn {
    pub started: Option<DateTime<Local>>,
    pub prompt: String,
    pub totals: Totals,
    pub context: u64,
}

#[derive(Clone)]
pub struct Subagent {
    pub name: String,
    pub model: String,
    pub totals: Totals,
}

#[derive(Clone)]
pub struct RateLimit {
    pub name: String,
    pub used: f64,
    pub window_minutes: u64,
    pub resets_at: Option<DateTime<Local>>,
}

#[derive(Clone, Copy)]
pub struct Call {
    pub context: u64,
    pub output: u64,
}

#[derive(Clone, Default)]
pub struct Report {
    pub session_id: String,
    pub transcript: PathBuf,
    pub cwd: String,
    pub branch: String,
    pub version: String,
    pub started: Option<DateTime<Local>>,
    pub updated: Option<DateTime<Local>>,
    pub context: u64,
    pub peak_context: u64,
    pub context_window: u64,
    pub compactions: u64,
    pub totals: Totals,
    pub models: Vec<(String, Totals)>,
    pub turns: Vec<Turn>,
    pub subagents: Vec<Subagent>,
    pub rate_limits: Vec<RateLimit>,
    pub calls: Vec<Call>,
}

impl Report {
    pub fn subagent_totals(&self) -> Totals {
        let mut totals = Totals::default();
        for subagent in &self.subagents {
            totals.add(&subagent.totals);
        }
        totals
    }

    fn observe(&mut self, stamp: Option<DateTime<Local>>) {
        if let Some(stamp) = stamp {
            self.started.get_or_insert(stamp);
            self.updated = Some(stamp);
        }
    }

    fn record(&mut self, call: Totals, context: u64, turn: Option<usize>) {
        self.totals.add(&call);
        self.context = context;
        self.peak_context = self.peak_context.max(context);
        self.calls.push(Call { context, output: call.output });
        if let Some(turn) = turn.and_then(|index| self.turns.get_mut(index)) {
            turn.totals.add(&call);
            turn.context = context;
        }
    }
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

fn num(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn parse_time(value: &Value) -> Option<DateTime<Local>> {
    let raw = value.get("timestamp")?.as_str()?;
    DateTime::parse_from_rfc3339(raw).ok().map(|t| t.with_timezone(&Local))
}

fn one_line(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_user_prompt(raw: &str) -> bool {
    let trimmed = raw.trim_start();
    !trimmed.is_empty() && !trimmed.starts_with('<')
}

fn read_jsonl(path: &Path) -> impl Iterator<Item = Value> {
    File::open(path)
        .ok()
        .map(BufReader::new)
        .into_iter()
        .flat_map(|reader| reader.lines().map_while(Result::ok))
        .filter_map(|line| serde_json::from_str(&line).ok())
}

pub fn modified(path: &Path) -> Option<(std::time::SystemTime, u64)> {
    let meta = fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

pub fn find_transcript(agent: &str, session_id: &str) -> Option<PathBuf> {
    match agent {
        "claude" => {
            let name = format!("{session_id}.jsonl");
            fs::read_dir(home().join(".claude/projects"))
                .ok()?
                .flatten()
                .map(|entry| entry.path().join(&name))
                .filter(|path| path.is_file())
                .max_by_key(|path| modified(path).map(|m| m.0))
        }
        "codex" => {
            let root = std::env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home().join(".codex"))
                .join("sessions");
            let suffix = format!("{session_id}.jsonl");
            let mut dirs = vec![(root, 0)];
            while let Some((dir, depth)) = dirs.pop() {
                for entry in fs::read_dir(&dir).ok()?.flatten() {
                    let path = entry.path();
                    if depth < 3 && path.is_dir() {
                        dirs.push((path, depth + 1));
                    } else if path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(&suffix)) {
                        return Some(path);
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn window_file(session_id: &str) -> PathBuf {
    home().join(".cache/agent-tokens/windows").join(session_id)
}

/// Claude transcripts don't record the context window size; the status line reports it.
pub fn remember_window(session_id: &str, window: u64) {
    let path = window_file(session_id);
    if fs::read_to_string(&path).ok().and_then(|raw| raw.trim().parse::<u64>().ok()) == Some(window) {
        return;
    }
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(path, window.to_string());
}

fn remembered_window(session_id: &str) -> u64 {
    fs::read_to_string(window_file(session_id)).ok().and_then(|raw| raw.trim().parse().ok()).unwrap_or(0)
}

pub fn supported(agent: &str) -> bool {
    matches!(agent, "claude" | "codex")
}

pub fn load(agent: &str, session_id: &str, transcript: &Path) -> Report {
    let mut report = Report {
        session_id: session_id.to_string(),
        transcript: transcript.to_path_buf(),
        ..Report::default()
    };
    match agent {
        "claude" => load_claude(&mut report),
        _ => load_codex(&mut report),
    }
    report
}

fn claude_call(usage: &Value) -> Totals {
    Totals {
        calls: 1,
        input: num(usage, "input_tokens"),
        cache_write: num(usage, "cache_creation_input_tokens"),
        cache_read: num(usage, "cache_read_input_tokens"),
        output: num(usage, "output_tokens"),
        reasoning: usage.get("output_tokens_details").map(|d| num(d, "thinking_tokens")).unwrap_or(0),
    }
}

fn claude_prompt(content: &Value) -> Option<String> {
    match content {
        Value::String(s) => Some(s.clone()),
        Value::Array(parts) => {
            if parts.iter().any(|p| text(p, "type") == Some("tool_result")) {
                return None;
            }
            let texts: Vec<&str> =
                parts.iter().filter(|p| text(p, "type") == Some("text")).filter_map(|p| text(p, "text")).collect();
            (!texts.is_empty()).then(|| texts.join(" "))
        }
        _ => None,
    }
}

fn load_claude(report: &mut Report) {
    let mut seen = HashSet::new();
    let mut models: HashMap<String, Totals> = HashMap::new();
    let mut turn = None;

    for entry in read_jsonl(&report.transcript.clone()) {
        if entry.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        report.observe(parse_time(&entry));
        for (key, field) in [("cwd", &mut report.cwd), ("gitBranch", &mut report.branch), ("version", &mut report.version)] {
            if let Some(value) = text(&entry, key) {
                *field = value.to_string();
            }
        }
        if text(&entry, "type") == Some("system") && text(&entry, "subtype") == Some("compact_boundary") {
            report.compactions += 1;
        }
        let Some(message) = entry.get("message").filter(|m| m.is_object()) else { continue };

        if text(&entry, "type") == Some("user") && entry.get("isMeta").and_then(Value::as_bool) != Some(true) {
            if let Some(prompt) = message.get("content").and_then(claude_prompt).filter(|p| is_user_prompt(p)) {
                report.turns.push(Turn { started: parse_time(&entry), prompt: one_line(&prompt), totals: Totals::default(), context: 0 });
                turn = Some(report.turns.len() - 1);
            }
            continue;
        }

        let Some(usage) = message.get("usage").filter(|u| u.is_object()) else { continue };
        if let Some(id) = text(message, "id") {
            if !seen.insert(id.to_string()) {
                continue;
            }
        }
        let call = claude_call(usage);
        let model = text(message, "model").unwrap_or("unknown");
        if !model.starts_with('<') {
            models.entry(model.to_string()).or_default().add(&call);
        }
        report.record(call, call.prompt(), turn);
    }

    report.models = sorted_models(models);
    report.context_window = remembered_window(&report.session_id);
    report.subagents = claude_subagents(&report.transcript.with_extension("").join("subagents"));
}

fn claude_subagents(dir: &Path) -> Vec<Subagent> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut subagents = Vec::new();
    for path in entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "jsonl")) {
        let meta: Value = fs::read_to_string(path.with_extension("meta.json"))
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or(Value::Null);
        let mut totals = Totals::default();
        let mut model = text(&meta, "model").unwrap_or("").to_string();
        let mut seen = HashSet::new();
        for entry in read_jsonl(&path) {
            let Some(message) = entry.get("message") else { continue };
            let Some(usage) = message.get("usage").filter(|u| u.is_object()) else { continue };
            if let Some(id) = text(message, "id") {
                if !seen.insert(id.to_string()) {
                    continue;
                }
            }
            totals.add(&claude_call(usage));
            if let Some(m) = text(message, "model").filter(|m| !m.starts_with('<')) {
                model = m.to_string();
            }
        }
        if totals.calls > 0 {
            let name = text(&meta, "description")
                .or_else(|| text(&meta, "agentType"))
                .map(str::to_string)
                .unwrap_or_else(|| path.file_stem().unwrap_or_default().to_string_lossy().into_owned());
            subagents.push(Subagent { name: one_line(&name), model, totals });
        }
    }
    subagents.sort_by(|a, b| b.totals.output.cmp(&a.totals.output));
    subagents
}

fn load_codex(report: &mut Report) {
    let mut models: HashMap<String, Totals> = HashMap::new();
    let mut current_model = String::new();
    let mut turn = None;

    for entry in read_jsonl(&report.transcript.clone()) {
        report.observe(parse_time(&entry));
        let Some(payload) = entry.get("payload").filter(|p| p.is_object()) else { continue };
        let entry_type = text(&entry, "type").unwrap_or("");
        let kind = text(payload, "type").unwrap_or("");

        match (entry_type, kind) {
            ("session_meta", _) => {
                if let Some(cwd) = text(payload, "cwd") {
                    report.cwd = cwd.to_string();
                }
                if let Some(version) = text(payload, "cli_version") {
                    report.version = version.to_string();
                }
                if let Some(branch) = payload.get("git").and_then(|g| text(g, "branch")) {
                    report.branch = branch.to_string();
                }
            }
            ("turn_context", _) => {
                if let Some(model) = text(payload, "model") {
                    current_model = model.to_string();
                }
            }
            ("response_item", "message") if text(payload, "role") == Some("user") => {
                let prompt: Vec<&str> = payload
                    .get("content")
                    .and_then(Value::as_array)
                    .map(|parts| parts.iter().filter_map(|p| text(p, "text")).collect())
                    .unwrap_or_default();
                let prompt = prompt.join(" ");
                if is_user_prompt(&prompt) {
                    report.turns.push(Turn { started: parse_time(&entry), prompt: one_line(&prompt), totals: Totals::default(), context: 0 });
                    turn = Some(report.turns.len() - 1);
                }
            }
            ("compacted", _) | (_, "compacted") => report.compactions += 1,
            (_, "token_count") => {
                let Some(info) = payload.get("info").filter(|i| i.is_object()) else { continue };
                let last = info.get("last_token_usage").cloned().unwrap_or(Value::Null);
                let input = num(&last, "input_tokens");
                let cached = num(&last, "cached_input_tokens");
                let call = Totals {
                    calls: 1,
                    input: input.saturating_sub(cached),
                    cache_write: 0,
                    cache_read: cached,
                    output: num(&last, "output_tokens"),
                    reasoning: num(&last, "reasoning_output_tokens"),
                };
                let window = num(info, "model_context_window");
                if window > 0 {
                    report.context_window = window;
                }
                if !current_model.is_empty() {
                    models.entry(current_model.clone()).or_default().add(&call);
                }
                report.record(call, input, turn);
                report.rate_limits = codex_limits(payload.get("rate_limits"));
            }
            _ => {}
        }
    }
    report.models = sorted_models(models);
}

fn codex_limits(limits: Option<&Value>) -> Vec<RateLimit> {
    let Some(limits) = limits else { return Vec::new() };
    ["primary", "secondary"]
        .into_iter()
        .filter_map(|name| {
            let limit = limits.get(name).filter(|l| l.is_object())?;
            Some(RateLimit {
                name: name.to_string(),
                used: limit.get("used_percent").and_then(Value::as_f64).unwrap_or(0.0) / 100.0,
                window_minutes: num(limit, "window_minutes"),
                resets_at: limit.get("resets_at").and_then(Value::as_i64).and_then(|t| Local.timestamp_opt(t, 0).single()),
            })
        })
        .collect()
}

fn sorted_models(models: HashMap<String, Totals>) -> Vec<(String, Totals)> {
    let mut models: Vec<_> = models.into_iter().collect();
    models.sort_by(|a, b| b.1.output.cmp(&a.1.output));
    models
}

/// Current context and total output for a Claude transcript, without building a full report.
pub fn claude_quick(path: &Path) -> (u64, u64) {
    let Ok(file) = File::open(path) else { return (0, 0) };
    let mut seen = HashSet::new();
    let (mut context, mut output) = (0, 0);
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        if !line.contains("\"usage\"") || line.contains("\"isSidechain\":true") {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<Value>(&line) else { continue };
        let Some(message) = entry.get("message") else { continue };
        let Some(usage) = message.get("usage").filter(|u| u.is_object()) else { continue };
        if let Some(id) = text(message, "id") {
            if !seen.insert(id.to_string()) {
                continue;
            }
        }
        let call = claude_call(usage);
        output += call.output;
        context = call.prompt();
    }
    (context, output)
}
