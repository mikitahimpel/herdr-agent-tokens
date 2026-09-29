"""Read token usage from Claude Code and Codex session transcripts."""

import glob
import json
import os
from dataclasses import dataclass, field
from datetime import datetime
from typing import Optional


@dataclass
class Totals:
    calls: int = 0
    input: int = 0
    cache_write: int = 0
    cache_read: int = 0
    output: int = 0
    reasoning: int = 0

    def add(self, other):
        self.calls += other.calls
        self.input += other.input
        self.cache_write += other.cache_write
        self.cache_read += other.cache_read
        self.output += other.output
        self.reasoning += other.reasoning

    @property
    def prompt(self):
        return self.input + self.cache_write + self.cache_read

    @property
    def cache_hit(self):
        return self.cache_read / self.prompt if self.prompt else 0.0


@dataclass
class Turn:
    started: Optional[datetime]
    prompt: str
    totals: Totals = field(default_factory=Totals)
    context: int = 0


@dataclass
class Subagent:
    name: str
    model: str
    totals: Totals


@dataclass
class Report:
    agent: str
    session_id: str
    transcript: str
    cwd: str = ""
    branch: str = ""
    version: str = ""
    started: Optional[datetime] = None
    updated: Optional[datetime] = None
    context: int = 0
    peak_context: int = 0
    context_window: int = 0
    compactions: int = 0
    totals: Totals = field(default_factory=Totals)
    models: dict = field(default_factory=dict)
    turns: list = field(default_factory=list)
    subagents: list = field(default_factory=list)
    rate_limits: list = field(default_factory=list)


def read_jsonl(path):
    with open(path, encoding="utf-8", errors="replace") as handle:
        for line in handle:
            try:
                yield json.loads(line)
            except ValueError:
                continue


def parse_time(value):
    if not value:
        return None
    try:
        return datetime.fromisoformat(value.replace("Z", "+00:00")).astimezone()
    except ValueError:
        return None


def prompt_text(content):
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        if any(isinstance(part, dict) and part.get("type") == "tool_result" for part in content):
            return None
        texts = [part.get("text", "") for part in content if isinstance(part, dict) and part.get("type") == "text"]
        return " ".join(texts) if texts else None
    return None


def claude_call_totals(usage):
    details = usage.get("output_tokens_details") or {}
    return Totals(
        calls=1,
        input=usage.get("input_tokens", 0),
        cache_write=usage.get("cache_creation_input_tokens", 0),
        cache_read=usage.get("cache_read_input_tokens", 0),
        output=usage.get("output_tokens", 0),
        reasoning=details.get("thinking_tokens", 0),
    )


def claude_calls(path):
    seen = set()
    for entry in read_jsonl(path):
        message = entry.get("message")
        if not isinstance(message, dict):
            continue
        usage, message_id = message.get("usage"), message.get("id")
        if not usage or message_id in seen:
            continue
        seen.add(message_id)
        yield entry, message.get("model") or "unknown", claude_call_totals(usage)


def claude_subagents(session_dir):
    subagents = []
    for path in sorted(glob.glob(os.path.join(session_dir, "subagents", "*.jsonl"))):
        meta = {}
        try:
            with open(path[: -len(".jsonl")] + ".meta.json", encoding="utf-8") as handle:
                meta = json.load(handle)
        except (OSError, ValueError):
            pass
        totals, model = Totals(), meta.get("model") or ""
        for _, call_model, call in claude_calls(path):
            totals.add(call)
            model = call_model
        if totals.calls:
            name = meta.get("description") or meta.get("agentType") or os.path.basename(path)
            subagents.append(Subagent(name=name, model=model, totals=totals))
    return subagents


def claude_report(session_id):
    paths = glob.glob(os.path.expanduser(f"~/.claude/projects/*/{session_id}.jsonl"))
    if not paths:
        return None
    path = max(paths, key=os.path.getmtime)
    report = Report(agent="claude", session_id=session_id, transcript=path)
    turn = None
    seen = set()

    for entry in read_jsonl(path):
        if entry.get("isSidechain"):
            continue
        stamp = parse_time(entry.get("timestamp"))
        if stamp:
            report.started = report.started or stamp
            report.updated = stamp
        report.cwd = entry.get("cwd") or report.cwd
        report.branch = entry.get("gitBranch") or report.branch
        report.version = entry.get("version") or report.version
        if entry.get("type") == "system" and entry.get("subtype") == "compact_boundary":
            report.compactions += 1

        message = entry.get("message")
        if not isinstance(message, dict):
            continue

        if entry.get("type") == "user" and not entry.get("isMeta"):
            text = prompt_text(message.get("content"))
            if text and not text.lstrip().startswith("<"):
                turn = Turn(started=stamp, prompt=" ".join(text.split()))
                report.turns.append(turn)
            continue

        usage, message_id = message.get("usage"), message.get("id")
        if not usage or message_id in seen:
            continue
        seen.add(message_id)
        call = claude_call_totals(usage)
        model = message.get("model") or "unknown"
        report.totals.add(call)
        report.models.setdefault(model, Totals()).add(call)
        report.context = call.prompt
        report.peak_context = max(report.peak_context, call.prompt)
        if turn:
            turn.totals.add(call)
            turn.context = call.prompt

    report.subagents = claude_subagents(os.path.join(os.path.dirname(path), session_id))
    return report


def codex_report(session_id):
    codex_home = os.environ.get("CODEX_HOME") or os.path.expanduser("~/.codex")
    paths = glob.glob(os.path.join(codex_home, "sessions", "*", "*", "*", f"*{session_id}.jsonl"))
    if not paths:
        return None
    path = max(paths, key=os.path.getmtime)
    report = Report(agent="codex", session_id=session_id, transcript=path)
    turn, info = None, None

    for entry in read_jsonl(path):
        stamp = parse_time(entry.get("timestamp"))
        if stamp:
            report.started = report.started or stamp
            report.updated = stamp
        payload = entry.get("payload")
        if not isinstance(payload, dict):
            continue
        kind = payload.get("type")

        if entry.get("type") == "session_meta":
            report.cwd = payload.get("cwd") or report.cwd
            report.version = payload.get("cli_version") or report.version
            report.branch = (payload.get("git") or {}).get("branch") or report.branch
        elif entry.get("type") == "turn_context" and payload.get("model"):
            report.models.setdefault(payload["model"], Totals())
        elif entry.get("type") == "response_item" and kind == "message" and payload.get("role") == "user":
            parts = payload.get("content") or []
            text = " ".join(part.get("text", "") for part in parts if isinstance(part, dict))
            if text.strip() and not text.lstrip().startswith("<"):
                turn = Turn(started=stamp, prompt=" ".join(text.split()))
                report.turns.append(turn)
        elif kind == "compacted" or entry.get("type") == "compacted":
            report.compactions += 1
        elif kind == "token_count" and payload.get("info"):
            info = payload["info"]
            last = info.get("last_token_usage") or {}
            cached = last.get("cached_input_tokens", 0)
            call = Totals(
                calls=1,
                input=last.get("input_tokens", 0) - cached,
                cache_read=cached,
                output=last.get("output_tokens", 0),
                reasoning=last.get("reasoning_output_tokens", 0),
            )
            report.totals.add(call)
            report.context = last.get("input_tokens", 0)
            report.peak_context = max(report.peak_context, report.context)
            report.context_window = info.get("model_context_window") or report.context_window
            if turn:
                turn.totals.add(call)
                turn.context = report.context
            limits = payload.get("rate_limits") or {}
            report.rate_limits = [
                (name, limits[name]) for name in ("primary", "secondary") if isinstance(limits.get(name), dict)
            ]

    if len(report.models) == 1:
        report.models[next(iter(report.models))] = report.totals
    return report


READERS = {"claude": claude_report, "codex": codex_report}
