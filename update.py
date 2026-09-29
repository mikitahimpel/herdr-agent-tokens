#!/usr/bin/env python3
"""Report an agent's token usage to its Herdr pane as a custom sidebar status."""

import glob
import json
import os
import subprocess
import sys
import time

SOURCE = "plugin:agent-tokens"
READY_STATES = {"idle", "done"}
HERDR = os.environ.get("HERDR_BIN_PATH") or "herdr"


def herdr(*args):
    result = subprocess.run([HERDR, *args], capture_output=True, text=True, timeout=10)
    if result.returncode != 0:
        return None
    return json.loads(result.stdout) if result.stdout.strip() else {}


def read_jsonl(path):
    with open(path, encoding="utf-8", errors="replace") as handle:
        for line in handle:
            try:
                yield json.loads(line)
            except ValueError:
                continue


def claude_usage(session_id):
    paths = glob.glob(os.path.expanduser(f"~/.claude/projects/*/{session_id}.jsonl"))
    if not paths:
        return None
    seen, output, context = set(), 0, 0
    for entry in read_jsonl(max(paths, key=os.path.getmtime)):
        message = entry.get("message")
        if not isinstance(message, dict) or entry.get("isSidechain"):
            continue
        usage, message_id = message.get("usage"), message.get("id")
        if not usage or message_id in seen:
            continue
        seen.add(message_id)
        output += usage.get("output_tokens", 0)
        context = (
            usage.get("input_tokens", 0)
            + usage.get("cache_read_input_tokens", 0)
            + usage.get("cache_creation_input_tokens", 0)
        )
    return context, output


def codex_usage(session_id):
    codex_home = os.environ.get("CODEX_HOME") or os.path.expanduser("~/.codex")
    paths = glob.glob(os.path.join(codex_home, "sessions", "*", "*", "*", f"*{session_id}.jsonl"))
    if not paths:
        return None
    info = None
    for entry in read_jsonl(max(paths, key=os.path.getmtime)):
        payload = entry.get("payload")
        if isinstance(payload, dict) and payload.get("type") == "token_count" and payload.get("info"):
            info = payload["info"]
    if not info:
        return None
    return info["last_token_usage"].get("input_tokens", 0), info["total_token_usage"].get("output_tokens", 0)


READERS = {"claude": claude_usage, "codex": codex_usage}


def compact(count):
    if count >= 1_000_000:
        return f"{count / 1_000_000:.1f}M"
    if count >= 1000:
        return f"{count / 1000:.1f}k"
    return str(count)


def main():
    event = json.loads(os.environ.get("HERDR_PLUGIN_EVENT_JSON") or "{}").get("data", {})
    pane_id = event.get("pane_id")
    if not pane_id or event.get("agent_status") not in READY_STATES:
        return 0

    info = herdr("agent", "get", pane_id)
    agent = (info or {}).get("result", {}).get("agent", {})
    session = agent.get("agent_session") or {}
    reader = READERS.get(agent.get("agent"))
    if not reader or session.get("kind") != "id" or not session.get("value"):
        return 0

    usage = reader(session["value"])
    if usage is None:
        return 0
    context, output = usage

    herdr(
        "pane", "report-metadata", pane_id,
        "--source", SOURCE,
        "--agent", agent["agent"],
        "--custom-status", f"{compact(context)} ctx · {compact(output)} out",
        "--seq", str(time.time_ns()),
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
