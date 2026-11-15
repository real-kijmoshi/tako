#!/bin/sh
# Local equivalent of the release verification job. Does not publish anything.
set -eu
cd "$(dirname "$0")/.."
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
python3 tests/dashboard.py
python3 tests/auth.py
python3 tests/installer.py
python3 tests/auto_update.py
python3 tests/settings.py
npm ci --prefix tests --no-audit --no-fund
node tests/web.cjs
node tests/landing.cjs
sh -n install.sh
cargo build --release --locked
target/release/tako version
printf '\nRelease checks passed. Review RELEASE_NOTES.md before tagging and publishing.\n'
