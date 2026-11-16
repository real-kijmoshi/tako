# Tako Code

A swarm of coding agents on your PC. Type what you want built; Tako Code splits the work, runs several Claude Code (or Codex, OpenCode, built-in) agents at once, each in its own Git worktree, merges their work, runs your tests and asks a model to review it. Every model request goes through your Tako server to free models only. Repositories, worktrees, agents and tests stay on your PC; it connects outward over HTTPS, so no PC port needs opening.

## Install

macOS and Linux:

```sh
curl -fsSL https://github.com/real-kijmoshi/tako/releases/latest/download/install-code.sh | sh
```

Windows (PowerShell):

```powershell
irm https://github.com/real-kijmoshi/tako/releases/latest/download/install-code.ps1 | iex
```

The installer verifies the download against the release's `SHA256SUMS` and puts a single `tako-code` executable on your PATH (`~/.local/bin`, or `%LOCALAPPDATA%\Programs\tako` on Windows). No Python is needed. Git is. Install [Claude Code](https://claude.com/claude-code) to use Claude Code agents; without it, agents fall back to Codex, OpenCode or the built-in agent.

## Use

```sh
cd ~/code/my-app
tako-code
```

The first time, it asks for your Tako server URL and a client key (create one under **Access & keys** in the web panel). Then type what you want:

```
tako ❯ add rate-limit headers to the API and tests for them
▸ add rate-limit headers to the API and tests for them
  Claude Code × 3 · working · 1m 12s

  ✓ 1  Header middleware            done
  ⠹ 2  Config option                Edit src/config.rs
  ⠼ 3  Integration tests            $ cargo test

✓ Ready for review  5 files changed, 120 insertions(+) · 1 test command(s) passed
  /apply puts the changes on your branch · /diff shows them · or type a new task
```

| Command | What it does |
| --- | --- |
| *any text* | Start a swarm for that task |
| `/agents N` | Agents per swarm, 1–6 (default 3) |
| `/engine NAME` | `claude`, `codex`, `opencode` or `builtin` (default: Claude Code if installed) |
| `/tests CMD` | A command that checks the merged result, e.g. `/tests npm test` (`/tests off` to clear) |
| `/apply` | Fast-forward your branch to the reviewed result. Nothing is pushed. |
| `/diff` | Show the swarm's changes |
| `/project PATH` | Work in another Git repository |
| `/status`, `/help`, `/quit` | |

Ctrl+C cancels a running swarm. Agents start from your last commit, so your repository needs no uncommitted changes when a swarm starts; your checkout is untouched until `/apply`. Run `tako-code` in a folder that isn't a Git repository yet, or one with no commits, and it offers to set it up (`git init` and a first commit with the files there).

While `tako-code` is open, the **Code swarms** page of your web panel shows this PC. Swarms you start in the terminal appear there too, and swarms started from the web run on your PC (press Enter in the terminal to watch). To serve the web panel without the interactive screen, run `tako-code run`. If you have Tako on the PC as well, `tako code` runs the same program.

## More details

Tests run directly, without a shell, with a five-minute limit each. Only configure commands you trust to execute generated code locally. `tako-code project add PATH --test-json '["npm","test"]'` registers a repository and its tests without the interactive screen; repeat `--test-json` for up to eight commands.

The repository is the local allowlist: a web job can only target repositories you opened in `tako-code` or registered with `project add`. Only project IDs and display names are sent to the server.

The planner can use fewer agents when work cannot be split safely. Each agent gets an independent branch and worktree. Conflicts, failed tests and interrupted jobs preserve worktrees for inspection. From the web panel, copy the apply command (`tako-code apply JOB_ID`) instead of using `/apply`.

`apply` requires the original checkout to be clean and still at the starting commit, and only fast-forwards it. It does not push or deploy. Changed branches require a manual merge instead. The complete patch is saved under `~/.config/tako-code/jobs/JOB_ID/changes.patch`; the web preview is limited to 50,000 characters.

Prefer to download by hand? Each executable is attached to the release (`tako-code-macos-aarch64`, `tako-code-macos-x86_64`, `tako-code-linux-x86_64`, `tako-code-linux-aarch64`, `tako-code-windows-x86_64.exe`). Browser downloads are quarantined by macOS because the files are unsigned; run `xattr -d com.apple.quarantine tako-code` once. Windows SmartScreen may also warn. `tako-code.pyz` (release) and `tako-code.py` (Code swarms page) are the same client for Python 3.11+.

## Engines and model access

**Built-in** works immediately with no agent subscription. It can list, search, read and write coding files; it cannot execute arbitrary model-supplied shell commands. Existing files must be read before replacement. Secret paths, Git metadata, symlinks and paths outside the worktree are denied. Workers have a 30-turn and 20-minute limit.

**OpenCode** and **Claude Code** are optional installed command-line engines. Claude Code requires version 2.1.248+ for restricted mode. The runner discovers native executables and advertises them to the UI. It launches them with a private home, a limited environment, restricted file tools and the strict Tako free gateway. These integrations depend on installed CLI versions and provider tool-call compatibility. Windows `.cmd`/`.bat` shims are not supported. Their permission systems are not an operating-system sandbox; prefer the built-in engine for the most restricted first run.

**Codex CLI** is also supported. Each Codex worker runs `codex exec` with a private `CODEX_HOME` (your ChatGPT login and `~/.codex` config are not used), a custom provider pointing at the Tako gateway's Responses API, Codex's `workspace-write` sandbox with network access off, and approvals disabled. Unlike the other engines, Codex can run shell commands inside its OS sandbox (Seatbelt on macOS, Landlock on Linux), so workers can run the project's own checks while they work. That sandbox confines writes and network, not reads: a Codex worker's commands can read any file your user account can read, and whatever it reads can be sent to the free model provider. Use Codex only on a PC account without secrets you would not share with that provider, or choose the built-in engine, which is limited to the worktree. Codex prints a harmless "model metadata not found" notice for `free:coding`.

Claude Code and Codex here are agent interfaces using free provider models; they do not provide free access to Anthropic's Claude or OpenAI's GPT models.

All model requests use `/code/v1/chat/completions`, `/code/v1/messages` or `/code/v1/responses`. The Responses endpoint converts Codex requests to chat completions and back (hosted tools such as web search are dropped; Codex's freeform `apply_patch` tool becomes a one-string function). The server selects only catalog-classified free routes, ignores requested paid model names, aliases and the default route, and fails when no free route is available. It never falls back to a paid-model route. Free models still have provider quotas and account eligibility rules. Use free accounts or enforce zero spending at the provider: a provider may charge a billing-enabled account beyond a free allowance. Nous Portal requires paid access and is excluded. No credentials or free access are supplied by this download.

## Operation and privacy

`status` prints pairing information without keys. `run --once` polls once and finishes one assigned job. Cancel in the web UI; workers stop between tool calls, with in-flight model requests allowed up to three minutes. Ctrl+C stops the runner. On restart, interrupted jobs are marked failed rather than silently repeated; branches and local files remain for recovery.

The PC stores its client key, runner credential, project paths and job state in private files under `~/.config/tako-code` (`TAKO_CODE_HOME` overrides this location). Git worktrees live there too. Keep this directory outside registered repositories. `TAKO_CODE_API_KEY` can supply the key to setup; a manually configured `env:VARIABLE` key reference is also supported. HTTPS is required except on loopback.

Repository context is sent to selected model providers through your VPS. Task text, activity, patch previews, model reviews and bounded test output are stored on the VPS for the web UI. Registered project paths remain local, but paths printed in tool/test output may appear in logs. Tako credentials are redacted from uploaded events. Review repository contents and trusted test commands accordingly. Existing Git hooks are disabled for generated commits and merges. Worktrees provide branch isolation, not an OS sandbox for external CLIs or configured tests.

The VPS keeps at most 50 registered runners and 200 jobs. This first version has no history deletion UI; back up and maintain `code-state.json` with the router stopped if you reach those limits. Local branches/worktrees are retained until you remove them with Git's normal worktree and branch commands.

CLI references: [OpenCode](https://opencode.ai/docs/cli/), [Claude Code](https://code.claude.com/docs/en/cli-reference).
