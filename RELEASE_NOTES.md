# Tako 0.1.12: agents that explain themselves, and a new Code swarms page

- **Real reasons when an agent stops.** Instead of "claude exited with 1", you see what Claude Code, Codex or OpenCode reported, for example `API Error: 429 rate limited` or "hit its 60 turn limit before finishing".
- **Fewer failed swarms.** An agent that stops on a temporary API error is retried once. An agent that stopped midway keeps its work, marked partial. And one failing agent no longer sinks the swarm: the rest is combined and reviewed, with the failed parts listed.
- **A new Code swarms page.** One composer at the top (Ctrl+Enter starts a swarm), setup steps only until your PC connects, a compact list that shows why a swarm failed, and a detail view with agents, progress, and Activity, Changes, Review and Tests tabs.

Update the server with `tako update`. Tako Code 0.1.11 and later update themselves; otherwise run the installer again:

```sh
curl -fsSL https://github.com/real-kijmoshi/tako/releases/latest/download/install-code.sh | sh
```

Windows (PowerShell): `irm https://github.com/real-kijmoshi/tako/releases/latest/download/install-code.ps1 | iex`
