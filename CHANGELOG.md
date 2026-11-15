# Changelog

## 0.1.0 — 2026-11-15

Initial release of tako, a self-hostable AI router with automatic fallback.

- Route OpenAI and Anthropic clients through providers, aliases, and ordered model combos.
- Rotate provider keys, respect cooldowns, translate streaming responses and tool calls, and track token usage.
- Manage providers and combos from the terminal dashboard or embedded web panel.
- Set an admin password in the terminal; sign in to the panel with expiring browser sessions.
- Create, rename, copy, and revoke named client API keys. Client keys grant model access without panel access.
- Follow first-run setup guidance, try the playground, and copy model-specific client configuration.
- Host behind an HTTPS reverse proxy with the included Caddy and systemd examples.
- Verify SHA-256 checksums before installing or updating release binaries.

Release binaries target macOS and Linux on x86_64 and ARM64. Linux builds use musl.
