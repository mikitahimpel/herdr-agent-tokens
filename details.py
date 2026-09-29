#!/usr/bin/env python3
"""Interactive token usage panel for one Herdr agent pane."""

import json
import os
import select
import shutil
import subprocess
import sys
import termios
import tty
from datetime import datetime

from usage import READERS

HERDR = os.environ.get("HERDR_BIN_PATH") or "herdr"

RESET, BOLD, DIM = "\033[0m", "\033[1m", "\033[2m"
ACCENT, GREEN, YELLOW, RED = "\033[36m", "\033[32m", "\033[33m", "\033[31m"


def compact(count):
    if count >= 1_000_000_000:
        return f"{count / 1_000_000_000:.2f}B"
    if count >= 1_000_000:
        return f"{count / 1_000_000:.2f}M"
    if count >= 10_000:
        return f"{count / 1000:.0f}k"
    if count >= 1000:
        return f"{count / 1000:.1f}k"
    return str(count)


def visible_len(text):
    length, escape = 0, False
    for char in text:
        if char == "\033":
            escape = True
        elif escape and char == "m":
            escape = False
        elif not escape:
            length += 1
    return length


def clip(text, width):
    return text if len(text) <= width else text[: max(0, width - 1)] + "…"


def pad(text, width):
    return text + " " * max(0, width - visible_len(text))


def bar(ratio, width):
    filled = round(max(0.0, min(1.0, ratio)) * width)
    color = GREEN if ratio < 0.6 else YELLOW if ratio < 0.85 else RED
    return f"{color}{'█' * filled}{DIM}{'░' * (width - filled)}{RESET}"


def duration(start, end):
    if not start or not end:
        return "—"
    minutes = int((end - start).total_seconds() // 60)
    hours, minutes = divmod(minutes, 60)
    return f"{hours}h {minutes:02d}m" if hours else f"{minutes}m"


def resolve_agent(pane_id):
    result = subprocess.run([HERDR, "agent", "get", pane_id], capture_output=True, text=True, timeout=10)
    if result.returncode != 0:
        return None
    return json.loads(result.stdout).get("result", {}).get("agent")


def load(pane_id):
    agent = resolve_agent(pane_id)
    if not agent:
        return None, f"No agent is running in pane {pane_id}."
    session = agent.get("agent_session") or {}
    reader = READERS.get(agent.get("agent"))
    if not reader:
        return None, f"Token usage isn't available for '{agent.get('agent')}' agents."
    if not session.get("value"):
        return None, "Herdr hasn't reported a session for this agent yet."
    report = reader(session["value"])
    if not report:
        return None, f"No transcript found for session {session['value']}."
    return report, None


def section(title, width):
    return ["", f"{BOLD}{title}{RESET} {DIM}{'─' * max(0, width - len(title) - 1)}{RESET}"]


def row(label, value, note=""):
    return f"  {DIM}{pad(label, 16)}{RESET}{pad(value, 12)}{DIM}{note}{RESET}"


def totals_rows(totals):
    prompt = totals.prompt or 1
    rows = [
        row("API calls", str(totals.calls)),
        row("Input", compact(totals.input), f"{totals.input / prompt:.0%} of prompt tokens, uncached"),
    ]
    if totals.cache_write:
        rows.append(row("Cache writes", compact(totals.cache_write), f"{totals.cache_write / prompt:.0%}"))
    rows += [
        row("Cache reads", compact(totals.cache_read), f"{totals.cache_hit:.0%} hit rate"),
        row("Output", compact(totals.output), f"{compact(totals.reasoning)} reasoning" if totals.reasoning else ""),
        row("Processed", compact(totals.prompt + totals.output), "all prompt + output tokens"),
    ]
    return rows


def render(pane_id, report, error, width):
    inner = max(40, width - 4)
    if error:
        return ["", f"  {BOLD}Token usage{RESET} {DIM}· {pane_id}{RESET}", "", f"  {error}"]

    lines = [
        "",
        f"  {BOLD}{ACCENT}{report.agent}{RESET}{BOLD} token usage{RESET} {DIM}· pane {pane_id}{RESET}",
        f"  {DIM}{clip(report.cwd, inner)}{f' · {report.branch}' if report.branch else ''}{RESET}",
        f"  {DIM}session {report.session_id} · v{report.version or '?'} · "
        f"{duration(report.started, report.updated)} · {len(report.turns)} prompt{'' if len(report.turns) == 1 else 's'}{RESET}",
    ]

    lines += section("Context", inner)
    if report.context_window:
        ratio = report.context / report.context_window
        lines.append(
            f"  {bar(ratio, min(40, inner - 30))}  {compact(report.context)} / "
            f"{compact(report.context_window)} {DIM}({ratio:.0%}){RESET}"
        )
    else:
        lines.append(row("Current", compact(report.context)))
    lines.append(row("Peak", compact(report.peak_context)))
    if report.compactions:
        lines.append(row("Compactions", str(report.compactions)))

    lines += section("Session totals", inner)
    lines += totals_rows(report.totals)
    if report.subagents:
        combined = sum(sub.totals.prompt + sub.totals.output for sub in report.subagents)
        combined += report.totals.prompt + report.totals.output
        lines.append(row("With subagents", compact(combined), "processed incl. all subagents"))

    if len(report.models) > 1:
        lines += section("By model", inner)
        for model, totals in sorted(report.models.items(), key=lambda item: -item[1].output):
            if model.startswith("<") or not totals.calls:
                continue
            lines.append(f"  {DIM}{pad(clip(model, 23), 24)}{RESET}{pad(compact(totals.output) + ' out', 12)}"
                         f"{DIM}{totals.calls} calls · {compact(totals.prompt)} in{RESET}")

    if report.subagents:
        total = sum(sub.totals.output for sub in report.subagents)
        lines += section(f"Subagents ({len(report.subagents)} · {compact(total)} out)", inner)
        for sub in sorted(report.subagents, key=lambda s: -s.totals.output)[:12]:
            note = f"{sub.totals.calls} calls · {compact(sub.totals.prompt)} in · {clip(sub.model, 18)}"
            lines.append(f"  {pad(clip(sub.name, 30), 31)}{pad(compact(sub.totals.output) + ' out', 11)}{DIM}{note}{RESET}")

    if report.rate_limits:
        lines += section("Rate limits", inner)
        for name, limit in report.rate_limits:
            used = (limit.get("used_percent") or 0) / 100
            resets = limit.get("resets_at")
            when = datetime.fromtimestamp(resets).strftime("%a %H:%M") if resets else "?"
            window = limit.get("window_minutes") or 0
            label = f"{name} ({window // 60}h)" if window < 1440 else f"{name} ({window // 1440}d)"
            lines.append(f"  {DIM}{pad(label, 16)}{RESET}{bar(used, 20)} {used:.0%} {DIM}resets {when}{RESET}")

    if report.turns:
        lines += section("Recent prompts", inner)
        lines.append(f"  {DIM}{pad('time', 7)}{pad('calls', 7)}{pad('out', 8)}{pad('ctx', 8)}prompt{RESET}")
        for turn in report.turns[-15:][::-1]:
            when = turn.started.strftime("%H:%M") if turn.started else "--:--"
            prompt = clip(turn.prompt, max(10, inner - 30))
            lines.append(
                f"  {pad(when, 7)}{pad(str(turn.totals.calls), 7)}{pad(compact(turn.totals.output), 8)}"
                f"{pad(compact(turn.context), 8)}{DIM}{prompt}{RESET}"
            )

    lines += ["", f"  {DIM}{clip(report.transcript, inner)}{RESET}"]
    return lines


def read_key(fd, timeout):
    ready, _, _ = select.select([fd], [], [], timeout)
    if not ready:
        return None
    data = os.read(fd, 16)
    return {b"\x1b[A": "up", b"\x1b[B": "down", b"\x1b[5~": "pgup", b"\x1b[6~": "pgdn"}.get(data, data.decode(errors="ignore"))


def main():
    pane_id = os.environ.get("AGENT_TOKENS_PANE") or (sys.argv[1] if len(sys.argv) > 1 else "")
    if not pane_id:
        print("No target pane was provided.")
        return 1

    fd = sys.stdin.fileno()
    saved = termios.tcgetattr(fd)
    offset, lines = 0, []
    report, error = load(pane_id)
    try:
        tty.setcbreak(fd)
        sys.stdout.write("\033[?1049h\033[?25l")
        while True:
            size = shutil.get_terminal_size()
            lines = render(pane_id, report, error, size.columns)
            body = size.lines - 2
            offset = max(0, min(offset, len(lines) - body))
            view = lines[offset : offset + body]
            footer = f"  {DIM}r refresh · j/k scroll · q close{RESET}"
            sys.stdout.write("\033[H\033[2J" + "\n".join(view) + f"\033[{size.lines};1H" + footer)
            sys.stdout.flush()

            key = read_key(fd, 5.0)
            if key in ("q", "\x1b", "\x03"):
                return 0
            if key is None or key == "r":
                report, error = load(pane_id)
            elif key in ("j", "down"):
                offset += 1
            elif key in ("k", "up"):
                offset -= 1
            elif key in (" ", "pgdn"):
                offset += body
            elif key in ("b", "pgup"):
                offset -= body
            elif key == "g":
                offset = 0
            elif key == "G":
                offset = len(lines)
    finally:
        sys.stdout.write("\033[?25h\033[?1049l")
        sys.stdout.flush()
        termios.tcsetattr(fd, termios.TCSADRAIN, saved)


if __name__ == "__main__":
    sys.exit(main())
