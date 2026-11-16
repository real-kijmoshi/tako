# Tako 0.1.10: start from any folder

- **New projects just work.** Run `tako-code` in a folder that isn't a Git repository yet, or right after `git init`, and it offers to create the repository and the first commit (it lists the files first). Before, it showed a raw `git rev-parse ... ambiguous argument 'HEAD'` error.
- **The web panel sees your project right away.** The **Local project** list on the Code swarms page fills in as soon as `tako-code` opens a repository, so you can start swarms from the web without first starting one in the terminal.
- `/project .` uses the current folder.

Update Tako Code by running the installer again:

```sh
curl -fsSL https://github.com/real-kijmoshi/tako/releases/latest/download/install-code.sh | sh
```

Windows (PowerShell): `irm https://github.com/real-kijmoshi/tako/releases/latest/download/install-code.ps1 | iex`

The server needs no update for this release (`tako update` is harmless).
