<p align="center"><img src="assets/tako.svg" width="88" alt="Tako octopus logo"></p>

# Tako

**Many arms. One endpoint.**

One endpoint for all your AI providers. Run locally or host on your own VPS. Point Claude Code, Cursor, OpenCode,
or any OpenAI/Anthropic SDK at it. tako routes each request to the provider you
choose and falls back to the next one when a provider is rate-limited or down.

- **Self-contained**: one binary with an embedded web panel, with no frontend
  build or external assets. The dashboard is a separate terminal UI that only runs while it's open.
- **Both API formats**: clients can speak OpenAI (`/v1/chat/completions`) or
  Anthropic (`/v1/messages`). Providers can be either kind, and tako converts
  between them, including streaming and tool calls.
- **Fallback and key rotation**: combos try targets in order. Multiple keys per
  provider are rotated, and a key that fails sits out for a cooldown.
- **Live config**: edit one TOML file and changes apply on the next request.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/real-kijmoshi/tako/main/install.sh | sh
```

The installer requires a published GitHub release. Until the first release is
published, build from source:

```sh
git clone https://github.com/real-kijmoshi/tako.git
cd tako
cargo install --path . --locked
```

The installer verifies release checksums before replacing an existing binary.
To install this specific release, use `TAKO_VERSION=v0.1.0 sh install.sh`.

## Use

```sh
tako            # opens the dashboard
```

The dashboard walks you through it:

1. **Providers** tab: pick a free provider, paste a key (click the API key link or press `o` to open it; in the key dialog use `Ctrl+O`).
   Click provider rows to select them, or use the arrow keys.
2. **Pick models**: tako fetches the provider's live model list, with the free ones marked.
   They go into the `free` combo, which becomes the default.
3. **Connect** tab: launch Claude Code through tako, or copy settings for OpenCode, Aider, Cursor, etc.

**Chat** tab (`6`): talk to your default combo directly in the TUI. Press Enter to
send, Alt+Enter for a new line, Ctrl+Left/Right to change combo and start fresh,
and Ctrl+N for a new conversation. Tab or Esc leaves Chat. History lasts until
you reset it or close the dashboard. Start the router with `s` before chatting.

Or from a terminal:

```sh
tako claude     # Claude Code, routed through your default combo
```

Connect several free providers and put them all in one combo. When one hits its
daily limit, the next one takes over, so together they give you far more free
requests than any single free tier.

### Recommended providers

| | free tier (as of Nov 2026, these change often) |
|---|---|
| Z.ai (GLM) | only GLM-4.7-Flash / GLM-4.5-Flash are free (newer Flash/FlashX are paid); often overloaded |
| OpenRouter | `:free` models and the `openrouter/free` router: 50/day, 1000/day after $10 credit |
| Groq | gpt-oss: ~1000 requests/day, 8K tokens/min (too small for long agent prompts) |
| NVIDIA NIM | free dev access to DeepSeek, Kimi, GLM, MiniMax…; ~40/min (its list includes retired models) |
| Google Gemini | Flash models with small daily caps |
| Kilo Gateway | free models and `kilo-auto/free`; 200 requests/hour per IP; no key needed |
| Cloudflare Workers AI | 10,000 neurons/day shared; setup needs an API token and account ID; some models require payment |
| LLM7 | free-token allowance up to 100K tokens per rolling 24h on selected models; educational/research use |
| Typhoon | free light usage; Thai/English text models, up to 200 requests/min |
| Cohere | evaluation only: 1,000 calls/month; production/commercial use needs a paid key |
| Vercel AI Gateway | $5/month on eligible models; purchasing credits ends the recurring free credit |
| Mistral | paid API; free mode excludes API access and requires upgrading to create keys |
| Ollama, LM Studio | local, unlimited |
| DeepSeek, Z.ai Coding Plan | paid but cheap |

Kilo, NVIDIA, Groq, Gemini and Cloudflare are useful additions for a coding
fallback pool. Cohere and Typhoon are secondary options. Vercel supplies an
allowance for eligible paid models, so its model list does not mark them as
universally free or preselect them. LLM7's free badges cover only the checked
free-token models. Free tiers can change; confirm your account's current limits.

Provider documentation: [Kilo](https://kilo.ai/docs/gateway/usage-and-billing),
[Cloudflare](https://developers.cloudflare.com/workers-ai/platform/pricing/),
[LLM7](https://github.com/chigwell/llm7.io/blob/main/TERMS.md),
[Typhoon](https://docs.opentyphoon.ai/en/rate-limits/),
[Cohere](https://docs.cohere.com/v1/docs/rate-limits),
[Vercel](https://vercel.com/docs/ai-gateway/pricing).
GitHub Models was retired on July 30, 2026 and is no longer offered in the picker.

Any other OpenAI- or Anthropic-compatible API works too: add it in the config.

Clients choose a model by name: a **combo** (`free`), a **provider/model**
(`openrouter/qwen/qwen3-coder:free`), or anything else, which goes to the
`default` combo. That's how Claude Code's `claude-*` model names end up on your free models.

## Config

If you used the project under its former name, copy your configuration directory
to `~/.config/tako/` (including `auth.json`) or set `TAKO_HOME` to its existing
location. Update service definitions and client URLs/settings to use `tako`.
Browser sessions need a fresh sign-in.

`~/.config/tako/config.toml` (`tako config path` prints the path):

```toml
listen = "127.0.0.1:20129"
cooldown = 60                       # seconds a failing key/model sits out
long_cooldown = 1800                # out of credits, quota used up, bad key, missing model

[providers.anthropic]
type = "anthropic"
keys = ["sk-ant-..."]

[providers.openrouter]
type = "openai"
base_url = "https://openrouter.ai/api/v1"
keys = ["sk-or-...", "sk-or-..."]   # rotated

[providers.ollama]
type = "openai"
base_url = "http://localhost:11434/v1"

[combos]
smart = ["anthropic/claude-sonnet-5-5", "openrouter/anthropic/claude-sonnet-5.5", "ollama/qwen3:8b"]

default = "smart"                   # put it above the [tables]: used for unknown model names
```

A provider can set `max_tokens = 8192` if it rejects the large `max_tokens` some clients send.

Fallback rules: tako reads each failure and moves on to the next key or target:

| failure | example | key sits out for |
|---|---|---|
| out of credits | 402, "credit balance is too low", `insufficient_quota` | `long_cooldown` (30 min) |
| daily/monthly quota used up | OpenRouter `free-models-per-day`, Groq "tokens per day" | until the provider's reset time, else `long_cooldown` |
| rate limited | 429 | the provider's `retry-after` / `x-ratelimit-reset` / "try again in 7m12s", else `cooldown` |
| key rejected | 401, 403 | `long_cooldown` |
| model gone | 404, "model does not exist" | `long_cooldown`, all keys of that target |
| overloaded, 5xx, timeout, unreachable | 503, 529 | `cooldown` |
| 200 with an error inside, or an empty stream | some gateways do this | `cooldown` |
| request too large / rejected | 400, 413, 422 | nobody benched; next provider tries |

Errors hidden in a `200` response are caught before anything reaches the client, so
those fall back too. If every target is cooling down, tako retries the rate-limited
ones anyway, but not keys that are out of credits or rejected. Editing the config
clears credit and auth cooldowns, so a topped-up key is used right away. When nothing
works, the client gets one error listing every target and why it failed (a 429 with
`retry-after` when it's rate limits).

## Web dashboard

`tako web` opens `http://127.0.0.1:20129/ui` in your browser. It includes:

- **Overview**: live traffic, searchable requests and fallback traces, success and
  latency metrics, per-target health, and cooldown countdowns with retry controls.
- **Providers**: connect built-in or custom APIs, add rotation keys, test a provider,
  and search live model lists with a free-model filter.
- **Combos**: create fallback chains, add or remove models, change their priority,
  and choose the default route. Changes apply live.
- **Playground**: chat through a combo with the same fallback and key rotation as
  your clients. Conversations stay in the current browser tab until reset or reload.
- **Connect**: copy setup recipes for Claude Code, OpenCode, Aider, compatible
  clients, and curl. Authenticated routers show a placeholder for your router key.
- **Logs**: search, follow, and copy the latest log lines. Configured keys are redacted.
- **Access & keys**: check password protection, create and copy named client keys,
  rename them, and confirm revocation before disconnecting an app.

The overview guides first-time setup from provider to combo to your first request.
Connection recipes use the model you select and work with remote routers too.

Token stats appear in the web overview, TUI, `tako status`, and `/status`:
input (including cached tokens), output, total used, and per-target usage since
router start. Streaming usage is recorded when the stream ends or disconnects.
When a provider omits counters, tako estimates at roughly one token per four
bytes of prompt/generated content and marks the response as estimated. Failed
attempts without usage reports are excluded. Remaining tokens show the latest
provider rate-limit headers per target/key, with reset and observation time in
the web overview; absent limits are shown as unknown. These are window snapshots,
not account balances, and quotas may be shared across targets. Stats reset when
the router restarts.

The dashboard is built into the binary, needs no external assets, supports light
and dark themes, and adapts to mobile screens. Provider keys stay on the server;
config writes use the same validated editor as the TUI. Without an admin password, the dashboard only answers on localhost or IP hosts.
After password setup, it also supports domain names behind a reverse proxy. Starting, stopping, and updating the router remain
available through the CLI or TUI.

To check dashboard routing and configuration against an isolated mock provider:
`cargo build --locked && python3 tests/dashboard.py && python3 tests/auth.py`.
Installer verification is tested with `python3 tests/installer.py`.
UI interactions run against an isolated router with `npm ci --prefix tests && node tests/web.cjs`.
These DOM checks cover behavior; visual layout needs a real-browser review.

## Host on a VPS

Set up a router behind an HTTPS reverse proxy in the server terminal (password input is hidden):

```sh
tako setup-vps --proxy
tako run
```

Open `/ui` and sign in. In **Access & keys**, create a named key for
each client. Copy new keys immediately; they are shown only once. Rename them as
your setup changes, or revoke them with confirmation to stop access immediately. Use `Authorization: Bearer tako_…` for OpenAI clients
or `x-api-key: tako_…` for Anthropic clients. Client keys cannot administer the
panel. Provider keys continue to be configured under **Providers**.

For direct network binding, `tako setup-vps` prompts for a password and sets
`listen = "0.0.0.0:20129"` (or pass a specific IP:port). Restart an existing
router afterward. Public listeners refuse to start without an admin password.
For an internet deployment, use HTTPS through a reverse proxy and keep tako
bound to `127.0.0.1:20129`; there is no built-in TLS.

A Linux system service example (install your built binary first):

```sh
sudo install -m 755 target/release/tako /usr/local/bin/tako
sudo useradd --system --home-dir /var/lib/tako --shell /usr/sbin/nologin tako
sudo install -d -o tako -g tako -m 700 /var/lib/tako
sudo -u tako env TAKO_HOME=/var/lib/tako /usr/local/bin/tako password
sudo install -m 644 deploy/tako.service /etc/systemd/system/tako.service
sudo systemctl daemon-reload
sudo systemctl enable --now tako
```

Build with `cargo build --release` on the VPS or use the matching Linux release
binary. The service uses its own `/var/lib/tako` configuration; configure providers
and combos in its web panel. Leave its listen address at the loopback default.

Install Caddy, replace `router.example.com` in [deploy/Caddyfile](deploy/Caddyfile)
with your domain, and use it as `/etc/caddy/Caddyfile`. Point the domain's DNS at
your VPS, allow ports 80/443, then reload Caddy. Keep port 20129 closed to public
traffic. Visit `https://your-domain/ui`. The proxy must preserve the original Host
header. HTTPS logins receive Secure, HttpOnly, SameSite cookies. The panel checks
request origins and requires a custom header for mutations.

Browser sessions expire after 12 hours; **Sign out** invalidates the current
session. `tako password` resets the password and invalidates all browser sessions
without restarting. Logins are limited to five attempts per minute across the
server. Restarting also signs out all sessions. API keys survive restarts.
Back up `auth.json` with `config.toml` in `TAKO_HOME`; both contain sensitive data.
A password-enabled router requires a valid client key for all model API requests,
even before the first key is created. Creating the first managed key also enables
client authentication on a local router; revoking the last key keeps it enabled. Existing `api_key` config values remain
valid for model API access; remove that setting to retire the legacy shared key.
Local CLI/TUI clients use a private, per-process `local.token` file and keep working
without a browser login. That token rotates on restart; use managed keys for durable
client configurations. `/health` remains a minimal public liveness endpoint;
`/status` requires an admin session or the private local CLI credential.

## Commands

| | |
|---|---|
| `tako` | the dashboard |
| `tako claude [combo] [args]` | Claude Code through tako |
| `tako password` | set/reset the web admin password |
| `tako setup-vps [IP:port / --proxy]` | set password and VPS listen address |
| `tako web` | the web dashboard in your browser |
| `tako start` / `stop` / `restart` | run in the background |
| `tako status` | state, providers, traffic, cooldowns |
| `tako logs [-f]` | log file |
| `tako config` / `check` | edit / validate config |
| `tako enable` / `disable` | start at login (launchd on macOS, systemd user unit on Linux) |
| `tako update` | download the latest release and restart |
| `tako uninstall` | stop, remove autostart, delete binary (asks about config) |
| `tako run` | foreground, for debugging or Docker |

Everything lives in `~/.config/tako/` (config, log, pid). Set `TAKO_HOME` to use another directory.

## Releasing 0.1.0

The package and lockfile use version `0.1.0`. Release notes are in
[RELEASE_NOTES.md](RELEASE_NOTES.md), with changes in [CHANGELOG.md](CHANGELOG.md).
Run the local release checks (Rust, Python 3, Node.js 24, and npm are needed):

```sh
./scripts/check-release.sh
```

In a Git checkout, commit the prepared files, tag the commit `v0.1.0`, and push
the tag. The release workflow verifies that the tag matches the package version,
runs format, lint, unit, integration, and installer checks, then builds all four
platform binaries. A single packaging job creates `SHA256SUMS` and a **draft**
GitHub release. Review the draft and publish it when ready. The installer and
`tako update` select the latest published release.

Development binaries labeled `0.2.0` need a manual replacement with `0.1.0`;
`tako update` intentionally does not downgrade. Preserve `TAKO_HOME` to keep
providers, combos, passwords, and client keys.
