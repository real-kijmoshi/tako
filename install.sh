#!/bin/sh
# curl -fsSL https://raw.githubusercontent.com/real-kijmoshi/tako/main/install.sh | sh
set -eu
REPO="${TAKO_REPO:-real-kijmoshi/tako}"
DIR="${TAKO_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${TAKO_VERSION:-latest}"

case "$(uname -s)" in
  Darwin) os=macos ;;
  Linux)  os=linux ;;
  *) echo "unsupported OS: $(uname -s)" >&2; exit 1 ;;
esac
case "$(uname -m)" in
  arm64|aarch64) arch=aarch64 ;;
  x86_64|amd64)  arch=x86_64 ;;
  *) echo "unsupported CPU: $(uname -m)" >&2; exit 1 ;;
esac
command -v curl >/dev/null 2>&1 || { echo "curl is required" >&2; exit 1; }
if command -v sha256sum >/dev/null 2>&1; then checksum=sha256sum
elif command -v shasum >/dev/null 2>&1; then checksum=shasum
else echo "sha256sum or shasum is required to verify the download" >&2; exit 1
fi

asset="tako-$os-$arch"
case "$VERSION" in
  latest) base="https://github.com/$REPO/releases/latest/download" ;;
  v[0-9]*) base="https://github.com/$REPO/releases/download/$VERSION" ;;
  *) echo "TAKO_VERSION must be latest or a tag such as v0.1.0" >&2; exit 1 ;;
esac
mkdir -p "$DIR"
work=$(mktemp -d "$DIR/.tako-install.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM

echo "Downloading tako for ${os}/${arch}..."
curl -fsSL --retry 3 --connect-timeout 15 "$base/$asset" -o "$work/$asset"
curl -fsSL --retry 3 --connect-timeout 15 "$base/SHA256SUMS" -o "$work/SHA256SUMS"
# Check exactly one expected asset; never evaluate manifest contents as commands.
awk -v asset="$asset" '$2 == asset && NF == 2 {print; count++} END {if (count != 1) exit 1}' "$work/SHA256SUMS" > "$work/expected" || {
  echo "Release checksum missing or ambiguous. Existing installation kept." >&2; exit 1;
}
if [ "$checksum" = sha256sum ]; then
  (cd "$work" && sha256sum -c expected) || { echo "Checksum failed. Existing installation kept." >&2; exit 1; }
else
  (cd "$work" && shasum -a 256 -c expected) || { echo "Checksum failed. Existing installation kept." >&2; exit 1; }
fi
chmod 755 "$work/$asset"
mv "$work/$asset" "$DIR/tako"
echo "Installed $DIR/tako ($("$DIR/tako" version))"
case ":$PATH:" in
  *":$DIR:"*) ;;
  *) echo; echo "Add this to your shell profile:  export PATH=\"$DIR:\$PATH\"" ;;
esac
printf '\nNext steps:\n  tako web                 Open the web panel and connect a provider\n  tako                     Open the terminal dashboard\n  tako setup-vps --proxy    Set up a VPS behind an HTTPS reverse proxy\n'
