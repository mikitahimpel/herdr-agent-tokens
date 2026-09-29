# herdr-agent-tokens

A [Herdr](https://herdr.dev) plugin that adds a **Token usage** panel for Claude Code and Codex agents.

Right-click an agent pane and choose **Token usage**. An overlay shows:

- **Context**: current and peak context size, a fill bar when the context window is known (Codex), and compaction count.
- **Session totals**: API calls, uncached input, cache writes, cache reads with hit rate, output with reasoning tokens, and total processed tokens.
- **By model**: per-model split when a session used more than one model.
- **Subagents**: token usage of each Claude Code subagent, and a combined total.
- **Rate limits**: Codex primary/secondary window usage and reset times.
- **Recent prompts**: the last 15 prompts with API calls, output tokens, and context size for each.

Keys: `r` refresh (it also refreshes every 5 seconds) · `j`/`k` or arrows scroll · `space`/`b` page · `g`/`G` top/bottom · `q` or `esc` close.

## Install

```sh
herdr plugin install mikitahimpel/herdr-agent-tokens
```

Or from a local checkout:

```sh
herdr plugin link /path/to/herdr-agent-tokens
```

Requires Herdr 0.7.1+ and `python3` (3.9+) on `PATH`.

## How it works

The `details` action resolves the pane from the action context and opens the `details` overlay pane for it. The panel resolves the agent session with `herdr agent get` and reads the session transcript:

| Agent  | Transcript                                                               |
| ------ | ------------------------------------------------------------------------ |
| claude | `~/.claude/projects/*/<session-id>.jsonl` and `<session-id>/subagents/`  |
| codex  | `$CODEX_HOME/sessions/YYYY/MM/DD/*<session-id>.jsonl` (default `~/.codex`) |

Other agents show a message that usage isn't available.
