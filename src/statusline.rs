use std::io::Read;
use std::path::Path;

use serde_json::Value;

use crate::herdr;
use crate::ui::compact;
use crate::usage;

const ACCENT: &str = "\x1b[38;2;203;166;247m";
const CONTEXT: &str = "\x1b[38;2;116;199;236m";
const OUTPUT: &str = "\x1b[38;2;137;180;250m";
const DIM: &str = "\x1b[38;2;108;112;134m";
const RESET: &str = "\x1b[0m";

fn num(value: &Value, path: &[&str]) -> Option<u64> {
    path.iter().try_fold(value, |v, key| v.get(key))?.as_u64()
}

/// Prints a Claude Code status line; inside Herdr it is a link that opens the dashboard.
pub fn run() -> i32 {
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let input: Value = serde_json::from_str(&input).unwrap_or(Value::Null);

    let transcript = input.get("transcript_path").and_then(Value::as_str).map(Path::new);
    let (mut context, output) = transcript.map(usage::claude_quick).unwrap_or((0, 0));
    let window = num(&input, &["context_window", "context_window_size"]).unwrap_or(0);
    if let (Some(session), true) = (input.get("session_id").and_then(Value::as_str), window > 0) {
        usage::remember_window(session, window);
    }
    if let Some(current) = num(&input, &["context_window", "current_usage", "input_tokens"]) {
        let cached = num(&input, &["context_window", "current_usage", "cache_read_input_tokens"]).unwrap_or(0);
        let written = num(&input, &["context_window", "current_usage", "cache_creation_input_tokens"]).unwrap_or(0);
        context = current + cached + written;
    }

    let mut text = format!("{ACCENT}◆{RESET} {CONTEXT}{}{RESET}", compact(context));
    if window > 0 {
        let window = if window % 1_000_000 == 0 { format!("{}M", window / 1_000_000) } else { compact(window) };
        text.push_str(&format!("{DIM}/{window}{RESET}"));
    }
    text.push_str(&format!(" {DIM}ctx ·{RESET} {OUTPUT}{}{RESET} {DIM}out{RESET}", compact(output)));

    match std::env::var("HERDR_PANE_ID").ok().filter(|_| std::env::var("HERDR_ENV").as_deref() == Ok("1")) {
        Some(pane) => {
            let link = herdr::link_for_pane(&pane);
            println!("\x1b]8;;{link}\x1b\\{text} {DIM}· ctrl-click for details{RESET}\x1b]8;;\x1b\\");
        }
        None => println!("{text}"),
    }
    0
}
