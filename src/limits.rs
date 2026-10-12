use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local, TimeZone};
use serde_json::{json, Value};

use crate::usage::RateLimit;

/// Claude account usage limits, as last reported to a Claude Code status line.
pub struct AccountLimits {
    pub limits: Vec<RateLimit>,
    pub recorded: Option<DateTime<Local>>,
}

fn path() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(".cache/agent-tokens/claude-limits.json")
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// Claude Code status line command: saves `rate_limits` from stdin and prints nothing.
pub fn record() -> i32 {
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let Some(limits) = serde_json::from_str::<Value>(&input).ok().and_then(|v| v.get("rate_limits").cloned()) else {
        return 0;
    };
    if !limits.is_object() || limits.as_object().is_some_and(|o| o.is_empty()) {
        return 0;
    }
    let path = path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let body = json!({ "recorded_at": now(), "rate_limits": limits }).to_string();
    let temporary = path.with_extension(format!("json.{}", std::process::id()));
    if fs::write(&temporary, body).is_ok() {
        let _ = fs::rename(&temporary, &path);
    }
    0
}

fn window(key: &str) -> Option<(u64, &str)> {
    [("five_hour", 300), ("seven_day", 10_080)]
        .into_iter()
        .find_map(|(prefix, minutes)| key.strip_prefix(prefix).map(|rest| (minutes, rest.trim_start_matches('_'))))
}

pub fn load() -> Option<AccountLimits> {
    let saved: Value = serde_json::from_str(&fs::read_to_string(path()).ok()?).ok()?;
    let now = now();
    let mut limits: Vec<RateLimit> = saved
        .get("rate_limits")?
        .as_object()?
        .iter()
        .filter_map(|(key, limit)| {
            let (window_minutes, name) = window(key)?;
            let used = limit.get("used_percentage").and_then(Value::as_f64)? / 100.0;
            let resets = limit.get("resets_at").and_then(Value::as_i64);
            let reset_passed = resets.is_some_and(|t| t <= now);
            Some(RateLimit {
                name: name.replace('_', " "),
                used: if reset_passed { 0.0 } else { used },
                window_minutes,
                resets_at: resets.filter(|_| !reset_passed).and_then(|t| Local.timestamp_opt(t, 0).single()),
            })
        })
        .collect();
    limits.sort_by_key(|l| (l.window_minutes, l.name.clone()));
    let recorded = saved.get("recorded_at").and_then(Value::as_i64).and_then(|t| Local.timestamp_opt(t, 0).single());
    (!limits.is_empty()).then_some(AccountLimits { limits, recorded })
}
