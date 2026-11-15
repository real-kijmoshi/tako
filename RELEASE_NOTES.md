tako gives your AI tools one endpoint, multiple providers, and automatic fallback.

### Get started

```sh
curl -fsSL https://raw.githubusercontent.com/real-kijmoshi/tako/main/install.sh | sh
tako web
```

Connect a provider, build a fallback combo, and try your first request in the playground.
The terminal dashboard is available with `tako`.

### Host on a VPS

```sh
tako setup-vps --proxy
tako run
```

Use the included Caddy and systemd examples to serve the panel over HTTPS. Set your
admin password in the terminal, then create named client keys under **Access & keys**.
Keys are shown once and can be renamed or revoked without restarting the router.

### Included

- OpenAI and Anthropic APIs, streaming, tool calls, format translation, and token tracking.
- Ordered fallback combos, provider key rotation, and automatic cooldowns.
- Embedded web panel with first-run guidance, provider setup, playground, logs, and client recipes.
- Password protection, expiring sessions, and separate API keys for client apps.
- macOS and Linux binaries for x86_64 and ARM64; static musl builds on Linux.
- `SHA256SUMS` for all four assets; installer and updater verify downloads before replacement.

### Updates

Run `tako update` to install a newer published release. Opt into daily verified
updates with `tako auto-update on`; inspect or disable them with `status` / `off`.
macOS uses launchd and Linux uses a systemd user timer. Enable from the installed
binary at its permanent path. The scheduling user needs write access there.
Updates restart a running router and may interrupt requests. Settings and client
keys are retained. Automatic updates are off by default.

### Landing page

The `landing/` folder contains a standalone static page ready for Cloudflare Pages.
It needs no build command, dependencies, external fonts, or backend.

### Deployment notes

tako serves HTTP. Use an HTTPS reverse proxy for internet hosting. Public listeners
require an admin password. Provider and authentication data stay in `TAKO_HOME`
(default: `~/.config/tako`). Back up this directory. Browser sessions are cleared
on restart; API keys and provider settings persist. Token counters reset on restart;
remaining-token values are provider snapshots rather than account balances.

This is release **0.1.0**. Development builds previously labeled **0.2.0** should be
replaced manually with this release: the updater will not downgrade a higher version.
Keep the same configuration directory to preserve settings and keys.
