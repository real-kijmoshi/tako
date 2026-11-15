# Changelog

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
