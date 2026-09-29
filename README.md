# herdr-agent-tokens

A [Herdr](https://herdr.dev) plugin that shows token usage for each Claude Code and Codex agent in the Herdr agent sidebar.

```
✓ memoxia · 5
  idle · 87.6k ctx · 11.7k out
```

- **ctx** — tokens in the context window on the latest turn (input + cache reads + cache writes).
- **out** — output tokens generated over the whole session.

The status updates each time an agent finishes a turn (`idle` / `done`).

## Install

```sh
herdr plugin install mikitahimpel/herdr-agent-tokens
```

Or from a local checkout:

```sh
herdr plugin link /path/to/herdr-agent-tokens
```

Requires Herdr 0.7.1+ and `python3` on `PATH`.

## How it works

The plugin subscribes to `pane.agent_status_changed`. When an agent becomes ready, it resolves the pane's agent session with `herdr agent get`, reads the session transcript, and sets the pane's sidebar status with `herdr pane report-metadata --custom-status`.

| Agent  | Transcript                                         |
| ------ | -------------------------------------------------- |
| claude | `~/.claude/projects/*/<session-id>.jsonl`          |
| codex  | `$CODEX_HOME/sessions/YYYY/MM/DD/*<session-id>.jsonl` (default `~/.codex`) |

Other agents are ignored.
