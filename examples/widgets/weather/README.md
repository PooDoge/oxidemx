# Weather widget

Current conditions from [Open-Meteo](https://open-meteo.com/) (keyless, fair-use).
Displays temperature and WMO condition label; scroll the slice to cycle through
a 3-day forecast; tap the slice to open open-meteo.com.

## Build

```
cargo build --release --target wasm32-wasip1 \
    --manifest-path examples/widgets/weather/Cargo.toml
```

The compiled module lands at:

```
examples/widgets/weather/target/wasm32-wasip1/release/weather.wasm
```

## Pack and install (requires the CLI — Task 7)

```
# Pack the widget directory into a .omxw bundle:
oxidemx-widget pack examples/widgets/weather/

# Install the bundle:
oxidemx-widget install ./weather-1.4.0.omxw
```

## Manual install (no CLI)

```
mkdir -p ~/.config/oxidemx/widgets/weather
cp examples/widgets/weather/widget.json ~/.config/oxidemx/widgets/weather/
cp examples/widgets/weather/icon.svg    ~/.config/oxidemx/widgets/weather/
cp examples/widgets/weather/target/wasm32-wasip1/release/weather.wasm \
       ~/.config/oxidemx/widgets/weather/widget.wasm
```

The `entry` field in `widget.json` must match the copied filename (`widget.wasm`).

## Options

| Key        | Type     | Default          | Description                                             |
|------------|----------|------------------|---------------------------------------------------------|
| `location` | location | *(required)*     | City name or `lat,lon` — used to build the API URL.     |
| `units`    | enum     | `c`              | Temperature unit: `c` (Celsius) or `f` (Fahrenheit).   |
| `display`  | enum     | `temp_condition` | Slice layout: `temp`, `temp_condition`, or `full`.      |
| `refresh`  | select   | `900` s          | Fetch interval in seconds: 300, 900, 1800, or 3600.     |
