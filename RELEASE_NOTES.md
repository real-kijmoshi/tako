# Tako 0.1.11: no more 524s, a nicer terminal, self-update

- **Agents no longer die with HTTP 524 behind Cloudflare.** Slow free models used to leave the connection silent for over 100 seconds, and Cloudflare cut it off. The Tako Code gateway now answers streaming requests immediately and keeps them alive while the model thinks. The built-in agent streams too, and temporary failures (rate limits, overloaded models, timeouts) are retried three times before an agent gives up.
- **A cleaner terminal screen.** A welcome box shows your project, agents and server; a status line sits above the prompt; the live view and summary are easier to read and say how many agents failed. The live view no longer repeats its header lines.
- **Tako Code updates itself.** It checks for new releases in the background, verifies the download against `SHA256SUMS`, and installs it when you quit. `tako-code update` updates right away; `TAKO_CODE_NO_UPDATE=1` turns it off.
- **Web panel:** Claude Code is preselected when your PC has it.

**Update both sides this time:**

- On the server: `tako update` (this is where the 524 fix lives).
- On your PC, run the installer once more (later versions update themselves):

```sh
curl -fsSL https://github.com/real-kijmoshi/tako/releases/latest/download/install-code.sh | sh
```

Windows (PowerShell): `irm https://github.com/real-kijmoshi/tako/releases/latest/download/install-code.ps1 | iex`
