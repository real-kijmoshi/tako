# Changelog

## 0.1.12 — 2026-11-16

- Explain why an agent stopped: Claude Code, Codex and OpenCode errors (for example `API Error: 429 …` or the turn limit) replace "claude exited with 1; inspect the worker events".
- Retry an agent once when it stops on a temporary API error (rate limit, overload, timeout, 5xx) before it changed anything.
- Keep the work of an agent that stopped midway, marked "partial", and raise Claude Code's turn limit from 30 to 60.
- Stop one failing agent from failing the whole swarm: the other agents' work is combined and reviewed, and the failed parts are listed with their reasons.
- Redesign the web Code swarms page: a single composer (Ctrl+Enter starts), setup steps only until a PC connects, a compact swarm list showing why something failed, and a detail view with a progress bar, agents, and Activity, Changes, Review and Tests tabs.

## 0.1.11 — 2026-11-16

- Stop agents failing with HTTP 524 behind Cloudflare and other proxies. Streaming requests to the Tako Code gateway are answered immediately and kept alive while a slow free model thinks; plain JSON answers and errors are delivered as stream events (OpenAI and Anthropic formats), so Claude Code and OpenCode retry them as usual.
- Make the built-in agent, planner and reviewer stream, and retry rate limits, overloaded models, timeouts and dropped connections three times (after 5, 20 and 60 seconds) before giving up.
- Redesign the terminal screen: a welcome box with project, agents and server; a status line above the prompt; a cleaner live view of agents and a clearer summary, including how many agents failed.
- Fix the live view occasionally repeating its header lines: it now redraws from a reserved block of lines instead of counting rows.
- Add self-update: `tako-code` checks for a new release at most every six hours, downloads and verifies it in the background, and installs it when you quit. `tako-code update` (or `/update`) updates now; `TAKO_CODE_NO_UPDATE=1` turns it off.
- Web panel: preselect Claude Code (then Codex, OpenCode) when the PC has it, and explain how to add a project when there is none.

## 0.1.10 — 2026-11-16

- Set up new projects from `tako-code`: in a folder that isn't a Git repository, or a repository with no commits (right after `git init`), it now offers to create the repository and the first commit, listing the files first. Previously it showed a raw `git rev-parse` error.
- Show a project in the web panel's **Local project** list as soon as `tako-code` opens it, instead of only after the first swarm typed in the terminal.
- `/project .` works in the current folder; the "no project" message says so.

## 0.1.9 — 2026-11-16

- Run `tako-code` with no arguments for an interactive swarm screen: the first run asks for the server URL and client key, the Git repository in the current directory is used automatically, and you type tasks instead of using the web panel. Agents default to Claude Code when it is installed.
- Show each agent live while a swarm runs (what it is editing or running), then a summary with changed files, test results and the model review. `/apply`, `/diff`, `/agents`, `/engine`, `/tests` and `/project` work from the same prompt; Ctrl+C cancels.
- Record swarms typed in the terminal on the server, so the web panel lists them; swarms started from the web panel still run on the PC while the terminal screen is open.
- Add one-line installers, `install-code.sh` and `install-code.ps1`, published with each release. They verify the checksum and put `tako-code` on your PATH.
- Fix agents occasionally failing to start when several Git worktrees were created at the same moment.

## 0.1.8 — 2026-11-16

- Ship Tako Code as standalone executables for macOS (Apple silicon, Intel), Linux (x86_64, ARM64) and Windows (x86_64). No Python is needed to run it.
- Show per-OS download and copyable install/setup commands on the Code swarms page; the Python script remains as an alternative.
- Make `tako code` run the `tako-code` executable found next to `tako` or on PATH, and explain where to get it when it is missing.
- Generate `tako-code apply JOB_ID` commands that work from the executable.

## 0.1.7 — 2026-11-16

- Add Tako Code, a separate PC client (`tako-code.pyz`, also `tako code`) that runs coding swarms on your own computer: a planner splits the task, 1–6 workers run in parallel Git worktrees, and the coordinator merges, runs locally configured tests and requests a review before you apply.
- Add a **Code swarms** page to the web UI to pair PCs, start swarms, watch each agent, read the patch, tests and review, cancel, and copy the apply command. Serve the client download from `/downloads/tako-code.py`.
- Support built-in, Claude Code, Codex CLI and OpenCode workers. Each external CLI gets a private home and is pointed at the Tako gateway.
- Add a free-only gateway under `/code/v1` (chat completions, Anthropic messages and the Responses API). It ignores requested model names, aliases and the default route, and never falls back to paid models.
- Add an OpenAI Responses API adapter so Codex CLI can work on free chat-completion models, including streaming events, function calls and Codex's freeform `apply_patch` tool.
- Show "Not reported" instead of "Unknown" when providers send no remaining-token limits, and explain why usage alone cannot determine what remains.
- Build and checksum `tako-code.pyz` with each release; run the Tako Code tests in CI and `scripts/check-release.sh`.

## 0.1.6 — 2026-11-16

- Correct Nous Portal’s API access classification: obtaining a key requires credits or a plan; exclude Nous targets from free routes even when their IDs end in :free.
- Mark Novita and Vikasit Check access and exclude them from free routes until account access is verified.
- Recheck all six additions for account requirements; document ModelScope’s verified Alibaba Cloud account and current Magicube point allocation, and SiliconFlow identity/eligibility restrictions.
- Add provider-specific setup guides to all 22 providers, inline connection instructions, official links, and copyable endpoints/first model targets.
- Bundle official icons for the six newly added providers and show pricing/access badges on connected providers.
- Add regression checks for account-gated free-route exclusion and guide/icon setup flows.

## 0.1.5 — 2026-11-16

- Isolate smart-route unit-test fixtures to remove a parallel file-write race caught during release verification. Version 0.1.4 was tagged but not published.

- Expand the catalog to 22 providers with Nous Portal, ModelScope, SiliconFlow China, BlockRun, Novita's Apodex promotion, and Vikasit Nova.
- Refresh free-model recommendations, highlight them in the web picker, and default to the free filter where known free models exist.
- Keep Mistral's direct API clearly marked paid and restrict Gemini free labels to documented text models.
- Accept numeric and decimal zero prices, exclude unavailable/non-text models from discovery, and label catalog suggestions when live discovery fails.
- Recheck provider pricing and live catalogs before release; remove unavailable BlockRun Muse Glimmer and retired Typhoon 2.1 recommendations. Record successful inference probes and untested accounts separately.
- Let users dismiss the dashboard setup checklist.

- Add smart routes: model names like `free:auto`, `free:coding` or `any:reasoning` that rank your models for each request by task fit and live health, with the ranking as the fallback order.
- Let `auto` routes pick a task from the request: images, long prompts, thinking requests, tools or code, and short questions.
- Accept smart routes as the default and as alias targets; list them in `/v1/models`, the playground, and a Smart routes card on the Combos page.
- Show where each routed request went on the live board (e.g. `free:auto → coding`).
- Make the live routing map show traffic at a glance: glowing and flowing lines, station stats, requests in flight, and a ticker of the latest requests.

## 0.1.3 — 2026-11-15

- Show requests on the dashboard as soon as they arrive, including in-flight progress.
- Reload open dashboards when a restarted router serves an updated UI, waiting while a dialog is open or a field holds text.
- Reorganize Connect into tool selection, route selection, and focused client settings with copyable recipes.
- Guide users to create a combo before copying connection settings.
- Show estimated tokens remaining from the latest provider report without adding potentially shared quotas.

## 0.1.2 — 2026-11-15

- Make settings transfer discoverable in command search: Move your setup, Export settings, and Import settings.
- Match transfer, migrate, backup, and restore searches; route results to the panel, export confirmation, or file picker.

## 0.1.1 — 2026-11-15

- Export and import portable settings from the CLI and web panel's Connect tab.
- Move providers, provider keys/headers, combos, aliases, and routing preferences while preserving destination access and authentication.
- Validate imports before writing, save private backups, and preview counts in the browser.
- Preserve environment-variable references rather than exporting resolved environment secrets.
- Refine the landing page, remove the base URL callout, add a sourced OmniRoute comparison, and improve the shared Tako logo.

## 0.1.0 — 2026-11-15

Initial release of tako, a self-hostable AI router with automatic fallback.

- Route OpenAI and Anthropic clients through providers, aliases, and ordered model combos.
- Rotate provider keys, respect cooldowns, translate streaming responses and tool calls, and track token usage.
- Manage providers and combos from the terminal dashboard or embedded web panel.
- Set an admin password in the terminal; sign in to the panel with expiring browser sessions.
- Create, rename, copy, and revoke named client API keys. Client keys grant model access without panel access.
- Follow first-run setup guidance, try the playground, and copy model-specific client configuration.
- Host behind an HTTPS reverse proxy with the included Caddy and systemd examples.
- Opt into daily verified updates using launchd (macOS) or systemd user timers (Linux).
- Deploy the standalone static landing page from `landing/`.
- Verify SHA-256 checksums before installing or updating release binaries.

Release binaries target macOS and Linux on x86_64 and ARM64. Linux builds use musl.
