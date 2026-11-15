<p align="center"><img src="assets/tako.svg" width="92" alt="Tako octopus"></p>
<h1 align="center">Tako</h1>
<p align="center"><strong>Many arms. One endpoint.</strong><br>Your AI providers, working together.</p>
<p align="center"><a href="https://github.com/real-kijmoshi/tako/actions/workflows/ci.yml"><img alt="Checks" src="https://github.com/real-kijmoshi/tako/actions/workflows/ci.yml/badge.svg"></a> <a href="https://github.com/real-kijmoshi/tako/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/real-kijmoshi/tako?color=1f5fbf"></a> <a href="LICENSE"><img alt="MIT license" src="https://img.shields.io/badge/license-MIT-657169"></a></p>
<p align="center"><a href="#quick-start">Get started</a> · <a href="docs/guide.md">Full guide</a> · <a href="https://github.com/real-kijmoshi/tako/releases/latest">Downloads</a> · <a href="landing/">Landing page source</a></p>

Tako is a small, self-hosted AI router for Claude Code, Cursor, OpenCode, Aider,
and OpenAI or Anthropic SDKs. Connect your providers once, choose a fallback order,
and give your tools one endpoint. When a provider is rate limited or unavailable,
Tako tries the next one.

```text
Claude Code · Cursor · OpenCode · your app
                    │
                   tako
                    │
         first pick → backup → local model
```

- **Automatic fallback.** Ordered model combos, provider key rotation, and cooldowns for limits and outages.
- **Both API formats.** OpenAI and Anthropic requests, including streaming and tool calls, translated between providers.
- **One binary.** An embedded web panel and a terminal dashboard. No frontend build or external assets needed to run.
- **Your infrastructure.** Run on your laptop or a VPS. Keep provider keys on the machine hosting Tako.
- **Live configuration.** Change providers and routes without restarting. TOML for the terminal, a web panel for everything else.

## Quick start

Install on macOS or Linux (Intel or ARM):

```sh
curl -fsSL https://raw.githubusercontent.com/real-kijmoshi/tako/main/install.sh | sh
tako web
```

The installer verifies the release's SHA-256 checksums. It installs to
`~/.local/bin`; add that directory to your `PATH` if your shell cannot find `tako`.
You can also [download a binary](https://github.com/real-kijmoshi/tako/releases/latest)
or [build from source](#build-from-source).

1. Connect a provider in **Providers**.
2. Pick models and put them in a fallback **Combo**.
3. Open **Connect** for client settings, or try the **Playground**.

Then launch Claude Code through your default combo:

```sh
tako claude
```

Prefer the terminal? Run `tako` to open the TUI. It has provider setup, combo
selection, client recipes, and chat.

## Use your existing tools

| Client | Connection |
| --- | --- |
| Claude Code | `tako claude` or `tako claude your-combo` |
| Cursor, OpenCode, Aider | Copy the recipe from the **Connect** tab |
| OpenAI-compatible SDK | Base URL `http://127.0.0.1:20129/v1` |
| Anthropic-compatible SDK | Base URL `http://127.0.0.1:20129` |

Use a combo name as the model, or `provider/model` for a specific target.
Unknown model names route to the default combo. If client authentication is
enabled, use a named key from **Access & keys**. See the [full guide](docs/guide.md)
for configuration, fallback behavior, and authentication details.

Tako works with compatible hosted APIs and local servers such as Ollama or
LM Studio. It does not supply model credits or bypass provider quotas. Free-tier
availability and terms depend on your accounts and providers.

## Move your setup to another machine

Open **Connect → Move your setup** in the web panel. Export on the first machine,
then import the JSON file on the other. The panel previews the provider and combo
counts before you confirm.

Or use the terminal:

```sh
tako export tako-settings.json
# Transfer this file privately to the other machine, then:
tako import tako-settings.json
```

Exports include provider API keys, custom provider headers, combos, aliases,
and routing preferences. They preserve `env:VARIABLE` references; set those
variables separately on the destination. Keep export files private.

Import replaces the destination's provider/routing settings after validation and
saves a private `config.before-import-*.toml` backup. Its listen address, router
API key, admin password, client keys, browser sessions, and update schedule stay
on that machine. Invalid files leave the destination configuration unchanged.
CLI exports refuse to overwrite existing files. Export, import, and backup files
written by Tako use owner-only permissions. Maximum transfer file size: 256 KB.

## Updates

```sh
tako update                # check now, verify, install, restart if running
tako auto-update on        # opt into daily verified updates
tako auto-update status
tako auto-update off
```

Automatic updates are **off by default**. Enable them from the installed binary
at its permanent path. macOS uses a launchd user agent; Linux uses a systemd user
timer. The schedule retains the configuration directory used when you enable it.
The user service manager must be running; a headless Linux host may need lingering
configured by its administrator.

Only newer published stable releases are installed. Downloads are checked against
release checksums before replacing the executable. A successful update restarts a
running router and can interrupt active requests; configuration and client keys
are preserved. The binary's directory must be writable by the scheduling user.
For a root-owned installation, use a maintenance process with the appropriate
permissions instead.

On macOS, updater logs are in `~/.config/tako/updates.log` (or `TAKO_HOME`).
On Linux, use `journalctl --user -u tako-update.service`.

## Run on a VPS

```sh
tako setup-vps --proxy
tako run
```

Put an HTTPS reverse proxy in front, sign into `/ui`, and create a named key for
each client. Public listeners require an admin password. The repository includes
[Caddy](deploy/Caddyfile) and [systemd](deploy/tako.service) examples; follow the
[VPS guide](docs/guide.md#host-on-a-vps) for setup and security details.

## Configuration and commands

Settings live in `~/.config/tako/config.toml`. Use `TAKO_HOME` for another directory.

| Command | Purpose |
| --- | --- |
| `tako` / `tako web` | Terminal / web dashboard |
| `tako claude [combo] [args]` | Launch Claude Code through Tako |
| `tako start` / `stop` / `restart` | Manage the background router |
| `tako status` / `tako logs -f` | Inspect traffic and follow logs |
| `tako config` / `tako check` | Edit / validate configuration |
| `tako export FILE` / `tako import FILE` | Move provider and routing settings between machines |
| `tako password` | Set or reset the web admin password |
| `tako enable` / `tako disable` | Toggle router startup at login |
| `tako update` | Install the latest verified release |
| `tako auto-update on\|off\|status` | Manage daily automatic updates |
| `tako run` | Run in the foreground |

Moving from Poros? Stop the old router first (`poros stop`), copy `config.toml`
and any `auth.json` into `~/.config/tako/`, then start Tako. Keep a backup, update
old service definitions, and sign into the web panel again. Both names use port
`20129`, so they cannot run on that address together. Avoid overwriting a Tako
configuration you've already customized.

## Build from source

Requires a Rust toolchain:

```sh
git clone https://github.com/real-kijmoshi/tako.git
cd tako
cargo install --path . --locked
```

For development, use `cargo run --release -- web`. Run the full release checks
with `./scripts/check-release.sh` (Rust, Python 3, Node.js 24, and npm).

## Landing page

[`landing/`](landing/) is a complete static landing page ready for Cloudflare Pages.
No build step. Upload the folder directly, or use `landing` as the output directory
in a Git-connected Pages project. See its [deployment notes](landing/README.md).

## Releases and license

Tag a checked commit with the package version (for example `v0.1.1`) and push the
tag. The release workflow verifies it, builds four platform binaries, and prepares
a draft with SHA-256 checksums. Publish the draft after checking all assets.

[Changelog](CHANGELOG.md) · [Release notes](RELEASE_NOTES.md) · [MIT license](LICENSE)
