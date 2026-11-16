# Changelog

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
