#!/usr/bin/env bash
# Generate shell completions without installing the binaries.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
out="${1:-target/completions}"
mkdir -p "$out"
cargo run --quiet -p lorgarctl --locked -- completions bash > "$out/lorgarctl.bash"
cargo run --quiet -p lorgarctl --locked -- completions fish > "$out/lorgarctl.fish"
cargo run --quiet -p lorgarctl --locked -- completions zsh > "$out/_lorgarctl"
cargo run --quiet -p lorgarctl --locked -- completions powershell > "$out/_lorgarctl.ps1"
cargo run --quiet -p lorgarctl --locked -- completions elvish > "$out/lorgarctl.elv"
echo "completions written to $out"
