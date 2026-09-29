#!/usr/bin/env python3
"""Open the token usage panel for the pane a Herdr action was invoked on."""

import json
import os
import subprocess
import sys

HERDR = os.environ.get("HERDR_BIN_PATH") or "herdr"
PANE_KEYS = ("pane_id", "target_pane_id", "focused_pane_id")


def main():
    raw = os.environ.get("HERDR_PLUGIN_CONTEXT_JSON") or "{}"
    print(raw)
    context = json.loads(raw)
    pane_id = next((context[key] for key in PANE_KEYS if context.get(key)), None)
    if not pane_id:
        print("action context has no pane", file=sys.stderr)
        return 1
    return subprocess.run(
        [
            HERDR, "plugin", "pane", "open",
            "--plugin", os.environ.get("HERDR_PLUGIN_ID") or "agent-tokens",
            "--entrypoint", "details",
            "--placement", "overlay",
            "--env", f"AGENT_TOKENS_PANE={pane_id}",
            "--focus",
        ],
        check=False,
    ).returncode


if __name__ == "__main__":
    sys.exit(main())
