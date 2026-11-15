# Tako 0.1.2

Settings transfer is now discoverable in the command menu (⌘K / Ctrl+K).
Search for **move**, **export**, **import**, **transfer**, **migrate**, **backup**,
or **restore**. Results open the transfer panel, export confirmation, or import
file picker. The panel is also under **Connect → Move your setup**.

Update with `tako update`, then reload the browser tab. Updating restarts a
running router and can interrupt active requests; configuration and keys persist.

This patch retains the portable settings, landing page, and comparison improvements
from 0.1.1. Exports contain provider keys; imports validate before replacing provider
settings and save a private backup while preserving destination authentication.
