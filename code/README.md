# Tako Code

A separate PC runner for coding swarms controlled from the Tako web UI. Repositories, Git worktrees, worker processes and tests run on your PC. Your VPS queues jobs and routes model requests. The runner connects outward over HTTPS; no PC port needs exposing.

Tako Code is a single executable for macOS (Apple silicon and Intel), Linux (x86_64 and ARM64) and Windows (x86_64). No Python is needed; Git is. The **Code swarms** page on your VPS shows the download and setup commands for your PC. If you already have Tako on the PC, put `tako-code` next to `tako` or on your PATH and `tako code` runs it.

## Setup

Update Tako on the VPS to a version with Code swarms. Connect eligible free model providers there and create a Tako client key under **Access & keys**. Provider keys stay on the VPS.

On the PC, using your own URL (pick the asset for your system: `tako-code-macos-aarch64`, `tako-code-macos-x86_64`, `tako-code-linux-x86_64`, `tako-code-linux-aarch64`):

```sh
curl -fL https://github.com/real-kijmoshi/tako/releases/latest/download/tako-code-macos-aarch64 -o tako-code
chmod +x tako-code
./tako-code setup --url https://tako.example.com
# Enter the Tako client key at the hidden prompt.
./tako-code project add /path/to/repository --test-json '["npm","test"]'
./tako-code run
```

On Windows (PowerShell):

```powershell
Invoke-WebRequest https://github.com/real-kijmoshi/tako/releases/latest/download/tako-code-windows-x86_64.exe -OutFile tako-code.exe
.\tako-code.exe setup --url https://tako.example.com
```

Downloading with `curl` avoids macOS Gatekeeper's quarantine. If you download in a browser, macOS may refuse to open the unsigned file; run `xattr -d com.apple.quarantine tako-code` once. Windows SmartScreen may also warn about an unsigned download. The executables are built from `code/tako_code.py` by the release workflow; checksums are in `SHA256SUMS`. Prefer Python? `tako-code.pyz` (release) and `tako-code.py` (Code swarms page) are the same client for Python 3.11+.

Use a Windows repository path on Windows. Shell quoting for `--test-json` varies; the value must be a JSON argv array. You can omit tests, but the UI will clearly show that none were configured. Repeat `--test-json` for up to eight commands. Commands run directly, without a shell, with a five-minute limit each. Only configure commands you trust to execute generated code locally.

The repository needs an initial commit and a clean checkout when a job begins. `project add` is the local allowlist: a web job cannot choose arbitrary filesystem paths. Only project IDs and display names are registered with the VPS. Restart the runner after changing projects or installed engines.

Open **Code swarms** in the hosted web UI, select the PC, project, engine and 1–6 workers, and describe the task. The planner can use fewer workers when work cannot be split safely. Each worker gets an independent branch and worktree. The coordinator combines commits on a review branch, runs configured tests and requests a model review. Conflicts, failed tests and interrupted jobs preserve worktrees for inspection.

Review the patch, actual test output and model review in the UI. To apply a ready job on the PC:

```sh
./tako-code apply JOB_ID
```

Apply works while the runner remains connected. It requires the original checkout to be clean and still at the starting commit, and only fast-forwards it. It does not push or deploy. Changed branches require a manual merge instead. The complete patch is also saved under `~/.config/tako-code/jobs/JOB_ID/changes.patch`; the web preview is limited to 50,000 characters.

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
