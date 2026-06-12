#!/usr/bin/env bash
# Settings-app side of the widget-system demo (Plan 3 Task 4) —
# the sibling of widget-smoke.sh. Builds the weather example, installs
# it into a throwaway XDG_CONFIG_HOME via the *real* CLI install
# pipeline, seeds a config that places it on slot 4, then validates
# headlessly what can be validated without a window:
#   * the settings binary builds,
#   * `oxidemx-settings --check-config` loads the seeded config and
#     sees the installed widget (registry scan + config parse).
# It does NOT auto-launch a GUI — the final line prints the exact
# command to walk the picker → options card flow by hand.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "==> building weather example (wasm32-wasip1)"
cargo build --release --target wasm32-wasip1 \
    --manifest-path widgets/builtin/weather/Cargo.toml

echo "==> building widget CLI + settings app (debug)"
cargo build -p oxidemx-widget-cli -p oxidemx-settings

TMP="$(mktemp -d /tmp/oxidemx-settings-widget-demo.XXXXXX)"
# NOTE: no EXIT trap — the temp home must outlive this script so the
# printed launch command works. Old demo dirs are cleaned up here.
find /tmp -maxdepth 1 -name 'oxidemx-settings-widget-demo.*' -user "$(id -un)" \
    -mmin +120 -exec rm -rf {} + 2>/dev/null || true
export XDG_CONFIG_HOME="$TMP/config"
mkdir -p "$XDG_CONFIG_HOME/oxidemx"

echo "==> packing + installing the weather widget via the CLI"
BUNDLE="$TMP/weather-1.4.0.omxw"
STAGE="$TMP/weather"
mkdir -p "$STAGE"
cp widgets/builtin/weather/widget.json "$STAGE/"
cp widgets/builtin/weather/icon.svg "$STAGE/"
cp widgets/builtin/weather/target/wasm32-wasip1/release/weather.wasm \
    "$STAGE/widget.wasm"
./target/debug/oxidemx-widget pack --output "$BUNDLE" "$STAGE"
# Dev-key signed → unknown key → needs --force (the GUI surfaces the
# same refusal as its consent prompt).
./target/debug/oxidemx-widget install --force "$BUNDLE"

# One page, weather placed on slot 4 (instance key apps.slot4), the
# location set in the widget's global settings bag — same seed as
# widget-smoke.sh so both halves of the demo line up.
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

echo "==> validating headlessly (oxidemx-settings --check-config)"
OUT="$(./target/debug/oxidemx-settings --check-config)"
echo "$OUT"
grep -q 'slot 4: widget' <<< "$OUT" || {
    echo "==> demo FAILED: seeded widget slice not found by --check-config" >&2
    exit 1
}
grep -q 'installed: weather' <<< "$OUT" || {
    echo "==> demo FAILED: weather widget not in the registry scan" >&2
    exit 1
}

echo "==> settings-widget-demo OK"
echo
echo "Manual GUI walk (picker → options card → scope toggle):"
echo "  launch with: XDG_CONFIG_HOME=$XDG_CONFIG_HOME cargo run -p oxidemx-settings"
echo "  then: Menu tab → select slot 5 (Weather) → Change… → pick/options →"
echo "  verify writes: cat $XDG_CONFIG_HOME/oxidemx/config.json"
