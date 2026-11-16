# Tako 0.1.7: Tako Code

Run swarms of coding agents on your own PC, controlled from your hosted Tako web UI, using free models only.

- **A separate PC client.** Download `tako-code.pyz` from this release (or from the new **Code swarms** page on your VPS). It needs Python 3.11+ and Git, connects outward to your VPS over HTTPS, and needs no open port on your PC.
- **Swarms, not one agent.** Describe a task in the web UI and pick 1–6 agents. A planner splits the work, each agent edits its own Git worktree in parallel, and the coordinator merges their commits on a review branch, runs your locally configured tests and asks a model for a review. Your checkout is untouched until you run `apply`.
- **Bring your favorite agent.** Workers can be the built-in agent, **Claude Code**, **Codex CLI** or **OpenCode**. Installed CLIs are detected automatically and shown in the engine picker.
- **Free models only.** Every worker request goes through `/code/v1` on your VPS, which picks only free routes, ignores requested model names and never falls back to paid models. Codex works through a new Responses API adapter.
- **Token remaining card** now says "Not reported" when a provider sends no limit headers.

Set up a PC:

```sh
python3 tako-code.pyz setup --url https://your-vps.example.com
python3 tako-code.pyz project add /path/to/repo --test-json '["npm","test"]'
python3 tako-code.pyz run
```

Free models still have provider quotas and account rules. Using Claude Code or Codex as the worker does not give free access to Anthropic or OpenAI models. See the [Tako Code guide](https://github.com/real-kijmoshi/tako/blob/v0.1.7/code/README.md).

Update the VPS with `tako update`. Updating restarts a running router and can interrupt active requests; configuration and keys persist.
