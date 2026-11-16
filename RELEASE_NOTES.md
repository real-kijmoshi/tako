# Tako 0.1.9: install, type, swarm

Tako Code is now a terminal app. Install it, run it in your project, type what you want built, and it spins up a swarm of Claude Code agents.

```sh
curl -fsSL https://github.com/real-kijmoshi/tako/releases/latest/download/install-code.sh | sh
cd ~/code/my-app
tako-code
```

Windows (PowerShell): `irm https://github.com/real-kijmoshi/tako/releases/latest/download/install-code.ps1 | iex`

- **No setup steps.** The first run asks for your Tako server URL and a client key, once. The Git repository you are in is used automatically.
- **Type a task, get a swarm.** Agents default to Claude Code when it is installed (Codex, OpenCode and the built-in agent also work; change with `/engine`). Pick how many with `/agents` (1–6).
- **Watch it live.** Each agent's row shows what it is editing or running. When it finishes you get the changed files, test results and a model review.
- **Apply when you're happy.** `/apply` fast-forwards your branch; `/diff` shows the changes; Ctrl+C cancels. Nothing is pushed.
- **Still works with the web panel.** Terminal swarms appear on the Code swarms page, and swarms started there run on your PC while `tako-code` is open.
- **One-line installers** verify the download checksum and put `tako-code` on your PATH.
- Fixed agents occasionally failing to start when several Git worktrees were created at once.

Every model request still goes through your server to free models only. Using Claude Code or Codex as the agent does not give free access to Anthropic or OpenAI models; the models come from the free providers connected to your Tako server.

Update the server with `tako update`.
