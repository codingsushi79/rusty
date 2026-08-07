#!/usr/bin/env sh
# Rusty installer — macOS & Linux.
#
#   curl -fsSL https://raw.githubusercontent.com/codingsushi79/rusty/main/scripts/install.sh | sh
#
# Builds Rusty and installs the `rusty` command to ~/.local/bin, so you can run
# `rusty <dir>` in any terminal.
set -eu

REPO="${RUSTY_REPO:-https://github.com/codingsushi79/rusty}"
BRANCH="${RUSTY_BRANCH:-main}"
BINDIR="${RUSTY_BINDIR:-$HOME/.local/bin}"
say() { printf '\033[1;38;5;208m▸\033[0m %s\n' "$1"; }
die() { printf '\033[1;31m✗ %s\033[0m\n' "$1" >&2; exit 1; }

if ! command -v cargo >/dev/null 2>&1; then
  say "Rust not found — installing via rustup…"
  curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs | sh -s -- -y
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
fi
command -v git >/dev/null 2>&1 || die "git is required"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
say "Fetching source…"
git clone --depth 1 --branch "$BRANCH" "$REPO" "$WORK/rusty" >/dev/null 2>&1

say "Building (first build takes a few minutes)…"
( cd "$WORK/rusty" && cargo build --release )

mkdir -p "$BINDIR"
install -m 0755 "$WORK/rusty/target/release/rusty" "$BINDIR/rusty"
say "Installed rusty to $BINDIR/rusty"

case ":$PATH:" in
  *":$BINDIR:"*) ;;
  *) say "Add $BINDIR to your PATH:  export PATH=\"$BINDIR:\$PATH\"" ;;
esac
say "Done. Run:  rusty ."
