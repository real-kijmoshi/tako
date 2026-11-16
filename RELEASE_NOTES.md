# Tako 0.1.13: HTTPS fix for macOS and Linux

The macOS and Linux Tako Code executables could not verify HTTPS certificates (`CERTIFICATE_VERIFY_FAILED`), so they could not connect to an `https://` Tako server or update themselves. They now use your system's certificate bundle, with a bundled fallback. Windows was not affected.

This release also includes 0.1.12: agents that explain why they stopped, fewer failed swarms, and the new Code swarms page.

**On macOS or Linux, install once more** (0.1.11 and 0.1.12 cannot update themselves there):

```sh
curl -fsSL https://github.com/real-kijmoshi/tako/releases/latest/download/install-code.sh | sh
```

Windows updates itself; or run `tako-code update`.
