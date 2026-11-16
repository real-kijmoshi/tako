# Tako 0.1.8: Tako Code without Python

Tako Code, the PC client for coding swarms added in 0.1.7, is now a single executable. Download it, make it executable, run it. No Python install needed; Git is still required.

| Your PC | Download |
| --- | --- |
| macOS, Apple silicon | `tako-code-macos-aarch64` |
| macOS, Intel | `tako-code-macos-x86_64` |
| Linux, x86_64 | `tako-code-linux-x86_64` |
| Linux, ARM64 | `tako-code-linux-aarch64` |
| Windows, x86_64 | `tako-code-windows-x86_64.exe` |

```sh
curl -fL https://github.com/real-kijmoshi/tako/releases/latest/download/tako-code-macos-aarch64 -o tako-code && chmod +x tako-code
./tako-code setup --url https://your-vps.example.com
./tako-code project add /path/to/repo --test-json '["npm","test"]'
./tako-code run
```

- The **Code swarms** page detects your system and shows the right download and setup commands (PowerShell on Windows).
- `tako code` runs the executable when it is next to `tako` or on PATH.
- The executables are unsigned. Download with `curl` to avoid macOS quarantine, or run `xattr -d com.apple.quarantine tako-code` after a browser download. Checksums are in `SHA256SUMS`.
- `tako-code.pyz` is still published for anyone who prefers Python 3.11+.

Workers can be the built-in agent, Claude Code, Codex CLI or OpenCode; every model request goes through your VPS to free models only. See the [Tako Code guide](https://github.com/real-kijmoshi/tako/blob/v0.1.8/code/README.md).

Update the VPS with `tako update`. Updating restarts a running router and can interrupt active requests; configuration and keys persist.
