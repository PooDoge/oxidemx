#!/usr/bin/env bash
# End-to-end smoke for the widget plugin runtime (Plan 2 Task 6 +
# Plan 4 Task 6): build the bundled builtin widgets, install weather
# by hand into a throwaway XDG_CONFIG_HOME, let startup SEEDING
# install cpu (OXIDEMX_BUILTIN_WIDGETS_DIR → target/builtin-widgets),
# place weather on slot 4 and cpu on slot 2, then run the overlay's
# debug-only `--widget-smoke` flag — it boots the real host worker
# (registry scan → reconcile → wasm load/init/render → MenuOpened →
# system-stats push → postcard decode) without iced and waits for a
# scene from every placed instance (stats-fed cpu must render twice:
# init + the stats-driven render).
set -euo pipefail
cd "$(dirname "$0")/.."

echo "==> building + packing the bundled builtin widgets"
scripts/build-builtin-widgets.sh

echo "==> building overlay (debug — carries the --widget-smoke flag)"
cargo build -p oxidemx-overlay

TMP="$(mktemp -d /tmp/oxidemx-widget-smoke.XXXXXX)"
trap 'rm -rf "$TMP"' EXIT
export XDG_CONFIG_HOME="$TMP/config"
# Startup seeding installs every target/builtin-widgets/*.omxw bundle
# that isn't already present (weather is hand-installed below at the
# same version, exercising the SkippedUpToDate path).
export OXIDEMX_BUILTIN_WIDGETS_DIR="$PWD/target/builtin-widgets"

WIDGET_DIR="$XDG_CONFIG_HOME/oxidemx/widgets/weather"
mkdir -p "$WIDGET_DIR"
cp widgets/builtin/weather/widget.json "$WIDGET_DIR/"
cp widgets/builtin/weather/icon.svg "$WIDGET_DIR/"
cp widgets/builtin/weather/target/wasm32-wasip1/release/weather.wasm \
    "$WIDGET_DIR/widget.wasm"

# One page: cpu on slot 2 (seeded bundle, fed by the host's
# system-stats push) and weather on slot 4, the location set in the
# weather widget's global settings bag.
cat > "$XDG_CONFIG_HOME/oxidemx/config.json" <<'EOF'
{
  "radial_menu": {
    "pages": [
      {
        "name": "Apps",
        "slices": [
          { "label": "A", "type": "exec", "command": "true" },
          { "label": "B", "type": "exec", "command": "true" },
          {
            "label": "CPU",
            "type": "widget",
            "color": "peach",
            "widget": {
              "source": { "custom": "cpu" },
              "instance_key": "apps.slot2"
            }
          },
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

# Both placed instances must have rendered. Value content is NOT
# asserted (the real ProcStatsSource runs; cpu_pct is None on the
# first push, so cpu legitimately renders the "—" placeholder).
grep -q "scene instance=weather/apps.slot4 revision=" <<< "$OUT" || {
    echo "==> widget-smoke FAILED: no weather scene" >&2
    exit 1
}
grep -Eq "scene instance=cpu/apps\.slot2 revision=[1-9]" <<< "$OUT" || {
    echo "==> widget-smoke FAILED: no cpu scene (system-stats push feed)" >&2
    exit 1
}
echo "==> widget-smoke OK"
