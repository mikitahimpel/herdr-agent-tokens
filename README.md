# herdr-agent-tokens

A [Herdr](https://herdr.dev) plugin: a terminal dashboard (built with [ratatui](https://ratatui.rs)) showing token usage for every Claude Code and Codex agent in your Herdr session.

```
 ◆ Agent Tokens                                              8 agents · updated 0s ago · auto 5s
╭ Agents ─────────────────────╮╭ memoxia · claude · wBK:p9 ─────────────────────────────────────╮
│▌● memoxia              170k ││  1 Overview    2 Prompts    3 Subagents    4 Models             │
│   working · claude · wBK:p9 ││ ╭ Context ──────╮╭ Output ───────╮╭ Cache hit ────╮╭ Processed ─╮│
│                             ││ │     170k      ││      70k      ││      98%      ││   7.50M    ││
│ ● rough-edges           54k ││ │   peak 170k   ││ 17k thinking  ││  7.28M read   ││ 72 calls   ││
│   idle · claude · wCJ:p2    ││ ╰───────────────╯╰───────────────╯╰───────────────╯╰────────────╯│
```

- **Agents**: every agent pane with its status and current context size. Opens on the pane you were focused on.
- **Overview**: context (with a fill gauge when the window is known), output and thinking tokens, cache hit rate, total processed tokens, a token-mix bar, context growth per API call, output per prompt, and Codex rate limits.
- **Prompts**: every prompt with API calls, output, context and cache hit rate.
- **Subagents**: Claude Code subagent tasks ranked by output, with model, calls and processed tokens.
- **Models**: per-model breakdown for the main session and for subagents.

Keys: `↑↓`/`j k` agent · `←→`/`h l`/`1-4` view · `PgUp PgDn` scroll · `r` refresh · `q`/`esc` close. Data refreshes every 5 seconds; unchanged transcripts are cached.

## Install

Requires Herdr 0.9.0+ and a Rust toolchain (`cargo`); the install builds the binary.

```sh
herdr plugin install mikitahimpel/herdr-agent-tokens
```

## Token bar

A compact, always-visible pane that lists every agent as a chip with its context and output. Click a chip and the dashboard opens as a popup for that agent. No keys needed.

```
 ◆ Agent Tokens  click an agent for details                         5 agents · 3.15M out total
  ● herdr-agent-history pD 670k ctx  364k out   ● mascot-design-lab p2 211k ctx  1.97M out
  ● memoxia p6 526k ctx  254k out   ● memoxia p9 248k ctx  114k out
```

Open it below a pane (it's a normal pane, so drag its border to size it):

```sh
herdr plugin pane open --plugin agent-tokens --entrypoint bar --placement split --target-pane <pane_id> --direction down --no-focus
```

## Keybinding

You can also bind the action to a key in `~/.config/herdr/config.toml`:

```toml
[[keys.command]]
key = "prefix+u"
type = "plugin_action"
command = "agent-tokens.dashboard"
description = "token usage"
```

Then run `herdr server reload-config`. You can also open it with `herdr plugin action invoke agent-tokens.dashboard`.

## Claude usage limits

Claude transcripts don't record account limits; Claude Code only passes them to status line commands. Set this silent status line in `~/.claude/settings.json` to save them; it prints nothing:

```json
"statusLine": {
  "type": "command",
  "command": "$HOME/.config/herdr/plugins/github/agent-tokens-*/target/release/agent-tokens claude-limits"
}
```

The dashboard then shows the 5-hour and 7-day windows (and any model-specific weekly window) in its header and in the Overview of Claude agents, with reset times and when they were last updated. They refresh whenever any Claude session updates its status line.

## Rex

The dashboard also runs inside the Rex terminal multiplexer, for the Claude Code agents running in Rex panes. Agents come from `rex-agent list --json` (set `REX_AGENT_BIN` to its path if it isn't on `PATH`); each Rex session label shows as the workspace and the block ID as the pane.

```sh
cargo build --release
AGENT_TOKENS_PANE=<block_id> target/release/agent-tokens
```

Without `AGENT_TOKENS_PANE`, it opens on the focused agent. The host is picked at startup: Rex when `REX_SESSION` is set and the process isn't inside Herdr (`HERDR_ENV=1` or `HERDR_PLUGIN_ROOT` set), Herdr otherwise. Set `AGENT_TOKENS_HOST=rex` or `AGENT_TOKENS_HOST=herdr` to choose explicitly.

Under Rex, `agent-tokens open` and clicking a chip in the token bar (`agent-tokens mini`) run `rex do tokens_dashboard pane=<block_id>`, so define a `tokens_dashboard` action in your Rex `init.lua` that opens the dashboard with that `AGENT_TOKENS_PANE`.

## Data sources

| Agent  | Transcript                                                                 |
| ------ | -------------------------------------------------------------------------- |
| claude | `~/.claude/projects/*/<session-id>.jsonl` and `<session-id>/subagents/`    |
| codex  | `$CODEX_HOME/sessions/YYYY/MM/DD/*<session-id>.jsonl` (default `~/.codex`) |

Sessions come from `herdr pane list` (or `rex-agent list --json` under Rex). Other agents are listed without usage.

## Development

```sh
cargo build --release
herdr plugin link .
```
