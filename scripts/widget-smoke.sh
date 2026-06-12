#!/usr/bin/env bash
# End-to-end smoke for the widget plugin runtime (Plan 2 Task 6):
# build the weather example for wasm32-wasip1, install it into a
# throwaway XDG_CONFIG_HOME with a config that places it on slot 4,
# then run the overlay's debug-only `--widget-smoke` flag — it boots
# the real host worker (registry scan → reconcile → wasm load/init/
# render → postcard decode) without iced and prints the first scene
# revision.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "==> building weather example (wasm32-wasip1)"
cargo build --release --target wasm32-wasip1 \
    --manifest-path examples/widgets/weather/Cargo.toml

echo "==> building overlay (debug — carries the --widget-smoke flag)"
cargo build -p oxidemx-overlay

TMP="$(mktemp -d /tmp/oxidemx-widget-smoke.XXXXXX)"
trap 'rm -rf "$TMP"' EXIT
export XDG_CONFIG_HOME="$TMP/config"

WIDGET_DIR="$XDG_CONFIG_HOME/oxidemx/widgets/weather"
mkdir -p "$WIDGET_DIR"
cp examples/widgets/weather/widget.json "$WIDGET_DIR/"
cp examples/widgets/weather/icon.svg "$WIDGET_DIR/"
cp examples/widgets/weather/target/wasm32-wasip1/release/weather.wasm \
    "$WIDGET_DIR/widget.wasm"

# One page, weather placed on slot 4 (instance key apps.slot4), the
# location set in the widget's global settings bag.
cat > "$XDG_CONFIG_HOME/oxidemx/config.json" <<'EOF'
{
  "radial_menu": {
    "pages": [
      {
        "name": "Apps",
        "slices": [
          { "label": "A", "type": "exec", "command": "true" },
          { "label": "B", "type": "exec", "command": "true" },
          { "label": "C", "type": "exec", "command": "true" },
          { "label": "D", "type": "exec", "command": "true" },
          {
            "label": "Weather",
            "type": "widget",
            "color": "teal",
            "widget": {
              "source": { "custom": "weather" },
              "instance_key": "apps.slot4"
            }
          }
        ]
      }
    ]
  },
  "widgets": {
    "global": {
      "weather": {
        "location": { "name": "Oslo", "lat": 59.91, "lon": 10.75 },
        "units": "c",
        "refresh": 900
      }
    }
  }
}
EOF

echo "==> running oxidemx-overlay --widget-smoke (XDG_CONFIG_HOME=$XDG_CONFIG_HOME)"
OUT="$(timeout 90 ./target/debug/oxidemx-overlay --widget-smoke)"
echo "$OUT"

if grep -q "revision=" <<< "$OUT"; then
    echo "==> widget-smoke OK"
else
    echo "==> widget-smoke FAILED: no scene revision in output" >&2
    exit 1
fi
