#!/usr/bin/env bash
# Build + pack every bundled built-in widget (plan 4 Task 4).
#
# For each widgets/builtin/<id>/ crate: compile to wasm32-wasip1, stage
# manifest + icon + wasm, and `oxidemx-widget pack` (dev-key signed) into
#
#   target/builtin-widgets/<id>-<version>.omxw
#
# install.sh ships that directory to <prefix>/share/oxidemx/widgets/,
# where overlay/settings startup seeding picks the bundles up (see
# tools/oxidemx-widget-cli/src/seed.rs for the trust model).
set -euo pipefail
cd "$(dirname "$0")/.."

OUT_DIR="target/builtin-widgets"

echo "==> building oxidemx-widget CLI (debug)"
cargo build -p oxidemx-widget-cli

mkdir -p "$OUT_DIR"
rm -f "$OUT_DIR"/*.omxw

for dir in widgets/builtin/*/; do
    [ -f "$dir/widget.json" ] || continue
    id="$(basename "$dir")"
    version="$(sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$dir/widget.json" | head -1)"

    echo "==> building $id v$version (wasm32-wasip1)"
    cargo build --release --target wasm32-wasip1 \
        --manifest-path "$dir/Cargo.toml" \
        --target-dir "$dir/target"

    stage="$(mktemp -d)"
    trap 'rm -rf "$stage"' EXIT
    cp "$dir/widget.json" "$dir/icon.svg" "$stage/"
    cp "$dir/target/wasm32-wasip1/release/${id}.wasm" "$stage/widget.wasm"

    ./target/debug/oxidemx-widget pack --output "$OUT_DIR/${id}-${version}.omxw" "$stage"
    rm -rf "$stage"
done

echo "==> bundles in $OUT_DIR:"
ls -l "$OUT_DIR"/*.omxw
