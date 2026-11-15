# Tako 0.1.1

Move your setup between machines, with a refined landing page and shared branding.

## Export and import settings

Use **Connect → Move your setup** in the web panel. Export on one machine, then
import the JSON file on another. The panel previews provider and combo counts
before you confirm.

Or use the CLI:

```sh
tako export tako-settings.json
tako import tako-settings.json
```

The file contains provider API keys and custom headers; transfer it privately.
Environment-variable references stay as references and need to be set on the
other machine. Imports validate before writing and save a private backup of the
previous configuration. The destination's address, router key, admin password,
client keys, sessions, and update schedule are preserved.

## Landing page

The standalone `landing/` folder is ready for Cloudflare Pages: no build command,
dependencies, or backend. This release removes the base URL callout, refines the
layout and logo, adds dark-theme support, and includes a sourced OmniRoute
comparison focused on documented features.

## Install or update

```sh
curl -fsSL https://raw.githubusercontent.com/real-kijmoshi/tako/main/install.sh | sh
# Or update an existing installation:
tako update
```

macOS and Linux binaries are available for Intel and ARM. The installer and updater
verify release SHA-256 checksums. Daily automatic updates are available with
`tako auto-update on` and remain off by default. Updating restarts a running router
and may interrupt active requests; configuration and client keys persist.
