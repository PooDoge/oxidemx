# Haptic Gamepad Bridge — Design Document

**Status:** Pre-implementation draft. Research complete. No code yet.
**Date:** 2026-05-14
**Owner:** `daemon/src/gamepad_haptics.rs` (to be added)
**Settings tab:** `settings-rs/src/tabs/gaming.rs` (extend existing Gaming tab)

> **One line:** When Game Mode is on, expose a virtual Xbox-360 evdev gamepad that
> captures any rumble a game sends to it (`EVIOCSFF` + `EV_FF play`) and re-renders
> the rumble as haptic patterns on the MX Master 4's piezo actuator.

---

## 1. Goal, scope, and novelty

### Goal

Redirect gamepad rumble that would normally play on a controller (or be silently
dropped because the user is playing keyboard-and-mouse) onto the MX Master 4's
haptic actuator. The mouse becomes a coarse, single-channel haptic surface for
the game — gunshots, impacts, UI bumps land as crisp piezo clicks; engine-rev or
continuous "feel" rumble lands as a rapid-tick buzz.

### Scope

In:

- A daemon-managed virtual evdev gamepad with `FF_RUMBLE` + `FF_PERIODIC` + `FF_GAIN`.
- Capture of `UI_FF_UPLOAD`/`UI_FF_ERASE` requests and `EV_FF` play/stop events.
- Translation of `(strong_magnitude, weak_magnitude, gain, duration)` tuples into
  calls on the existing `SharedHapticManager` (HID++ haptic patterns over
  `0x1B04` / `0x2150`).
- Optional **proxy mode**: forward events from a real controller into our virtual
  one, so games see one pad (ours) and the user keeps their controller working.
- Gating: only active while `GamingMode::is_enabled()`.
- All tunables in the existing Gaming settings tab.

Out:

- Hooking SDL's HIDAPI hidraw path (DualSense / Xbox-One / Switch Pro etc.).
  These bypass evdev entirely (see §3). We document the workaround
  (`SDL_JOYSTICK_HIDAPI=0`, `PROTON_NO_HIDRAW`) but do not ship a hidraw
  interceptor.
- LD_PRELOAD or DLL-injection capture (cf. Intiface Game Haptics Router on
  Windows). Out of scope on Linux given the evdev path covers 85–90 % of titles.
- Steam Controller / Steam Deck HIDAPI emulation. Possible later via `uhid`
  with a Steam Controller HID descriptor; reverse-engineering territory.

### Novelty (per prior-art research)

**Net-new on Linux.** No project bridges gamepad rumble to a Logitech mouse
haptic actuator. Closest prior art, in decreasing overlap:

| Project | Overlap | What it teaches |
|---|---|---|
| [sc-controller](https://github.com/C0rn3j/sc-controller) | ~50% | Same shape: capture `FF_RUMBLE` from a virtual XInput pad, re-render as haptic pulses on a non-rumble surface (Steam Controller trackpads). Closest open-source template. |
| [Intiface Game Haptics Router](https://github.com/intiface/intiface-game-haptics-router) | ~40% | Same intent (route rumble to a third-party actuator) but Windows-only, DLL-injection capture, different sink ecosystem. Reuse: magnitude-blending heuristics. |
| [SCUF Envision Pro V2 Linux driver](https://github.com/Tealdragon204/scuf-envision-pro-V2-Linux) | ~25% | Clean current example of uinput-virtual-pad + `FF_RUMBLE`+`FF_GAIN` capture/passthrough on Linux. Reuse: source-code template for the capture loop. |
| [mx4notifications](https://github.com/lukasfri/mx4notifications) + [Solaar](https://github.com/pwr-Solaar/Solaar) | ~30% (sink only) | The HID++ MX Master 4 haptic path is already solved by us and these. No source side. |

Steam Deck's own trackpad-haptic emulation in Steam Input is the conceptual
north star: take a coarse two-motor rumble signal and synthesize discrete
patterns on a non-rumble actuator. Our project is the same shape for a different
sink.

---

## 2. The Linux rumble stack — current state-of-the-world

Linux games can deliver rumble through **five distinct paths**, only three of
which we can catch with an evdev/uinput device:

```
Game (Linux-native or Windows-via-Proton)
  │
  ├─► SDL2/SDL3 evdev backend  ────────► ioctl(EVIOCSFF) + write(EV_FF) ──► /dev/input/eventN   [ CATCH ]
  │                                                                          (our virtual pad)
  │
  ├─► SDL2/SDL3 HIDAPI driver  ────────► write(/dev/hidrawN, vendor_rpt) ──► real controller    [ BYPASS ]
  │     (xbox360/xboxone/ps3/ps4/ps5/switch/steam/...)
  │
  ├─► Proton: winebus + bus_sdl.c  ────► (uses SDL above) ─────────────────► see SDL branches
  │
  ├─► Proton: winebus + bus_udev.c ────► write(/dev/hidrawN, raw_report) ──► real controller    [ BYPASS ]
  │     (when PROTON_ENABLE_HIDRAW=VID/PID is set)
  │
  ├─► Proton: winebus + lnxev path ────► ioctl(EVIOCSFF, FF_RUMBLE)    ────► /dev/input/eventN   [ CATCH ]
  │     (evdev fallback for unknown VID/PID)
  │
  ├─► Steam Input virtual pad  ────────► EVIOCSFF on Steam's 28DE:11FF uinput ─► Steam translates ─► HID  [ MAYBE CATCH ]
  │     (Steam writes FF back to its own virtual pad; game reads our pad if we ARE the wrapped device)
  │
  ├─► libmanette (GNOME/Flatpak)  ─────► ioctl(EVIOCSFF) ───────────────────► /dev/input/eventN   [ CATCH ]
  │
  └─► Hand-rolled hidraw or direct ────► write(/dev/hidrawN) ──────────────► real controller     [ BYPASS ]
        (some Unity titles, emulators with custom drivers, <2% of corpus)
```

### What this means at a single glance

- **If our virtual pad is the only gamepad the game sees**, we catch ~85–90 % of
  top-100 Linux/Proton titles. The remaining 10–15 % is "user has a real
  Xbox/PS/Switch plugged in and SDL HIDAPI claimed it".
- **The HIDAPI bypass is the principal danger path.** SDL2 has dedicated
  per-vendor HIDAPI drivers for Xbox 360/One, PS3/4/5, Switch Pro, Steam
  Controller, Steam Deck, Shield, Stadia, Luna, Wii. For VID/PIDs in
  `controller_list.h`, SDL opens `/dev/hidrawN` directly and `FF_RUMBLE` never
  touches our virtual evdev.
- **Wine's evdev fallback collapses everything to `FF_RUMBLE`.** Wine's
  `lnxev_device_haptics_start()` does not pass through `FF_PERIODIC`/`FF_CONSTANT`
  even when the Windows game uploaded a DInput periodic — strong_magnitude
  becomes the periodic-effect magnitude, weak_magnitude the trigger. We only
  need to handle `FF_RUMBLE` correctly to catch all Proton XInput + DInput
  traffic.

### The Steam Input wrinkle

Steam, when "Steam Input" is on for a game, **grabs the physical controller**
(`EVIOCGRAB`) and exposes a uinput evdev device at VID `28DE` PID `11FF` named
`"Microsoft X-Box 360 pad"` with `EV_FF` + `FF_RUMBLE`. The game writes
rumble to *that* virtual pad; Steam reads the upload and translates to the real
device's vendor protocol on hidraw.

So if Steam Input is on:
- The game's rumble goes to **Steam's** virtual pad, not ours.
- Two virtual pads stack badly. Our daemon is invisible to the game.

Workaround paths, in order of preference:
1. **Be the controller Steam wraps.** If our virtual pad has a generic
   gamepad-class VID/PID, Steam can wrap *it* and write rumble *to us* (which is
   exactly what we want). Empirical question — needs `evtest` validation.
2. **Tell users to disable Steam Input for our pad** in Steam's
   `Controller Settings → Detected Controllers`. Our pad becomes a "plain
   gamepad" Steam doesn't wrap. Game's SDL will fall through to our pad's
   evdev FF.
3. **Tell users to disable Steam Input entirely for game X** if it doesn't need
   Steam Input remapping. Common for non-Steam-Input titles.

The settings UI should surface this as a one-paragraph help text plus a
diagnostic button (§11).

---

## 3. Where to sit — chosen catch surface

**Catch at the evdev/uinput layer.** Create a uinput virtual device that
advertises `EV_FF` + `FF_RUMBLE` + `FF_PERIODIC` + `FF_GAIN`. Implement the
`UI_FF_UPLOAD` / `UI_FF_ERASE` userspace-service protocol. Read `EV_FF` events
to know when effects start/stop. Translate the upload's
`strong_magnitude`/`weak_magnitude` (modulated by `FF_GAIN` and `ff_replay`) into
calls on `SharedHapticManager`.

We **do not** intercept hidraw. The HIDAPI bypass is documented as a known gap
(§14) with environment-flag workarounds.

---

## 4. Compatibility matrix

| Path                                                  | Catch?     | Top-100 share | Notes |
|-------------------------------------------------------|------------|---------------|-------|
| Native SDL2/SDL3 evdev backend                        | ✓          | high          | Default for any controller not in `controller_list.h`. Our virtual pad will fall into this bucket. |
| Native SDL2/SDL3 **HIDAPI** for real Xbox/PS/Switch   | ✗          | medium        | Bypass via `/dev/hidrawN`. Mitigation: user sets `SDL_JOYSTICK_HIDAPI=0` per game, or removes the real controller, or our pad is the *only* gamepad enumerated. |
| Native UE5 (uses SDL2 internally)                     | depends    | medium        | Same as SDL2; depends on which controller SDL picked. |
| Native Godot 4.5+ (uses SDL3 internally)              | depends    | low           | Same as SDL3. |
| Native Godot ≤ 4.4 (hand-rolled evdev)                | ✓          | very low      | Direct `EVIOCSFF`. |
| Native libmanette (GNOME/Flatpak)                     | ✓          | low           | Pure evdev. |
| Native Unity (legacy `UnityEngine.Input`)             | ✓          | medium        | Falls through to SDL evdev on desktop. |
| Native Unity (new InputSystem)                        | mostly ✗   | medium        | Most Unity titles don't rumble on Linux at all today. |
| Native indie hand-rolled evdev / emulators            | ✓          | very low      | Direct `EVIOCSFF`. |
| Native indie hand-rolled hidraw                       | ✗          | < 2 %         | Uncatchable without an LD_PRELOAD shim. |
| Proton XInput (default `bus_sdl.c`)                   | depends    | very high     | Uses Proton's bundled SDL2; HIDAPI for known VID/PIDs, evdev for the rest. |
| Proton XInput w/ `PROTON_ENABLE_HIDRAW=VID/PID`       | ✗          | opt-in        | User explicitly bypasses us. Default off. |
| Proton DInput (legacy)                                | ✓          | low           | Wine `joystick_hid.c` ends in evdev `FF_RUMBLE` (waveform collapsed). |
| Proton RawInput                                       | depends    | very low      | Routes through `hidclass.sys` → same three-backend split as XInput. |
| Steam Input ON for game, our pad **not** wrapped      | ✗          | high (Steam)  | Game writes to Steam's virtual pad. Mitigation: disable Steam Input for game / our pad. |
| Steam Input ON for game, our pad **is** the wrapped device | ✓     | uncertain     | Empirically needs testing. |

**Realistic estimate (user has plugged the MX Master 4 only, no controller):**
~90 % of rumble-enabled titles will land in our daemon. With a real controller
also plugged in, drops to ~70–80 % unless the user follows the Steam Input
guidance.

---

## 5. Virtual device specification

### 5.1 Identity

```
uinput_setup {
    id.bustype = BUS_VIRTUAL   (0x06)
    id.vendor  = 0x045E        (Microsoft)
    id.product = 0x028E        (Xbox 360 wired pad)
    id.version = 0x0114
    name       = "MX Master 4 Haptic Gamepad"
    ff_effects_max = 16
}
```

**Why these values:**

- `BUS_VIRTUAL`: SDL2 evdev backend accepts any bus, but SDL2 HIDAPI parents to a
  `usb_device` and skips non-USB nodes — exactly what we want, since
  `0x045E / 0x028E` is in `controller_list.h`. Using `BUS_VIRTUAL` keeps SDL
  HIDAPI off our device while letting SDL evdev still recognise the
  Xbox-360-style fingerprint.
- `0x045E:0x028E`: SDL2 ships a built-in `gamecontrollerdb` entry for this
  GUID (`030000005e0400008e02000010010000`), so every SDL_GameController-using
  game treats us as a fully-mapped standard pad with no extra config.
- Distinct device **name** `"MX Master 4 Haptic Gamepad"` (not the literal
  Microsoft string) so users can identify us in `evtest` / Steam controller
  settings while the SDL mapping still hits by GUID.
- `ff_effects_max = 16`: matches `xpad`'s claim, exceeds Wine/SDL usage
  (typically 1–4 slots).

### 5.2 Capability bits

Required for SDL2/3 to mark the joystick "rumble-capable" *and* for udev's
`input_id` builtin to set `ID_INPUT_JOYSTICK=1`:

```
EV_SYN
EV_KEY:  BTN_A BTN_B BTN_X BTN_Y BTN_TL BTN_TR BTN_SELECT BTN_START BTN_MODE
         BTN_THUMBL BTN_THUMBR
EV_ABS:  ABS_X   ABS_Y   ABS_RX  ABS_RY  (sticks; -32768..32767, flat=128)
         ABS_Z   ABS_RZ                  (triggers; 0..1023)
         ABS_HAT0X ABS_HAT0Y             (dpad; -1..1)
EV_FF:   FF_RUMBLE FF_PERIODIC FF_SINE FF_SQUARE FF_TRIANGLE
         FF_CONSTANT FF_RAMP FF_GAIN
```

**Why FF_PERIODIC + FF_SINE/SQUARE/TRIANGLE despite Wine only sending FF_RUMBLE:**
SDL2's older `SDL_Haptic` path uploads `FF_SINE`, not `FF_RUMBLE`. Some games
still use that API. Advertising the periodic family costs us nothing — we
collapse all of it to magnitude in the translator.

**Why FF_CONSTANT + FF_RAMP:** rare but games upload them. Honour as a
"continuous magnitude until stopped". `SPRING/DAMPER/FRICTION/INERTIA` we
reject with `retval = -EINVAL`; games fall back gracefully.

### 5.3 udev rule

`/etc/udev/rules.d/70-juhradial-haptic-pad.rules`:

```udev
# Allow non-root to open /dev/uinput
KERNEL=="uinput", SUBSYSTEM=="misc", OPTIONS+="static_node=uinput",
    TAG+="uaccess", MODE="0660", GROUP="input"

# Tag our virtual device as a joystick + grant logged-in user access
SUBSYSTEM=="input", ATTRS{name}=="MX Master 4 Haptic Gamepad",
    ENV{ID_INPUT_JOYSTICK}="1", TAG+="uaccess"
```

Ship via the existing `packaging/` tree alongside other udev rules. `install.sh`
already handles dropping rules into `/usr/local/share/...` on Bazzite; this
follows the same pattern.

---

## 6. uinput FF protocol — exact handshake

This is the part most easy to get wrong. The kernel hands FF uploads from games
*back to our daemon* via the uinput fd; we must service them with a strict
three-ioctl handshake or the game's `EVIOCSFF` syscall hangs forever in `D` state.

```
Game side                       Kernel                              Our daemon
─────────                       ──────                              ──────────
ioctl(EVIOCSFF, &eff) ─►   parks calling task,
                            queues upload request
                            emits event on uinput fd:
                              type=EV_UINPUT
                              code=UI_FF_UPLOAD
                              value=<request_id>       ─►   loop reads event
                                                            upload = uinput_ff_upload{0}
                                                            upload.request_id = ev.value
                                                            ioctl(UI_BEGIN_FF_UPLOAD, &upload)
                                                            // kernel fills upload.effect (new) +
                                                            // upload.old (previous, if replacing)
                                                            // ─── inspect, store, translate ───
                                                            upload.retval = 0   // or -ENOSPC etc.
                                                            ioctl(UI_END_FF_UPLOAD, &upload)
              ◄── EVIOCSFF returns retval; effect.id allocated
              game now calls write() with EV_FF/code=effect_id/value=1 to play
```

`UI_FF_ERASE` is the mirror: `EV_UINPUT/UI_FF_ERASE/value=request_id` →
`UI_BEGIN_FF_ERASE` → set `retval` → `UI_END_FF_ERASE`. The struct is
`uinput_ff_erase { request_id, retval, effect_id }`.

**Iron rule:** every `UI_BEGIN_*` MUST be matched by a `UI_END_*` with a
populated `retval`, even on error paths. Skipping the END unparks no one;
`EVIOCSFF` hangs forever.

### Replay timing

Crucially, **on a uinput-owned device the kernel does NOT emit a "stop" event
when `ff_replay.length` elapses**. Our daemon must time it ourselves:

- On `EV_FF play (value≥1)`: read the stored effect's `replay.length` (ms) and
  `replay.delay` (ms before start). Schedule a tokio sleep that fires the
  haptic engine pattern.
- On `EV_FF stop (value=0)` or on the timer firing: stop emitting haptic pulses
  for that slot.

### FF_GAIN

`type=EV_FF, code=FF_GAIN, value=u16 (0..0xFFFF)` is a master multiplier. Honour
as a global scale on `intensity_scale` (§7).

### Rust crate verdict

`evdev = "0.13"` — has `VirtualDevice::process_ff_upload()` returning an RAII
guard that exposes `effect()` / `old()` / `set_retval()` and runs
`UI_END_FF_UPLOAD` on `Drop`. Pin to `0.13.x`. `input-linux` is a fallback;
`nix` for any raw ioctl we need that `evdev` doesn't wrap (e.g.
`UI_GET_SYSNAME` for devnode resolution).

### Devnode race

After `UI_DEV_CREATE` returns, `/dev/input/eventN` may not yet exist (udev
hasn't processed the event). Standard idiom — implemented exactly like
`libevdev-uinput.c`:

1. `ioctl(uinput_fd, UI_GET_SYSNAME, buf[64])` → e.g. `"input42"`.
2. Walk `/sys/devices/virtual/input/input42/` for `eventN` child.
3. Poll `/dev/input/<eventN>` for existence with 50 ms steps, give up at 2 s.

`udevadm settle --timeout=2` is an acceptable fallback at daemon startup; not
inline per-create.

---

## 7. Magnitude → haptic translation

Inputs to the translator:

```
strong_magnitude    u16     0..0xFFFF   "left/low-freq motor"
weak_magnitude      u16     0..0xFFFF   "right/high-freq motor"
ff_gain             u16     0..0xFFFF   master gain (default 0xFFFF)
duration_ms         u32     replay.length (0 = until stopped)
delay_ms            u32     replay.delay
effect_type         enum    FF_RUMBLE | FF_PERIODIC | FF_CONSTANT | FF_RAMP
```

### 7.1 The combined-intensity scalar

```
intensity = (strong_weight * strong + weak_weight * weak) / 0xFFFF
intensity *= (ff_gain / 0xFFFF)
intensity *= intensity_scale          // user setting, default 1.0
intensity = clamp(intensity, 0.0, 1.0)
```

Default weights: `strong_weight = 1.0`, `weak_weight = 0.4`. The MX Master 4's
piezo actuator only renders one channel, so we collapse both motors but
emphasise the low-frequency (game's "big rumble" signal).

### 7.2 Deadzone

If `intensity < min_intensity` (default `0.08` ≈ magnitude `0x1500`): emit
nothing. Filters quiet continuous rumble that'd otherwise become a non-stop
buzz on the piezo.

### 7.3 Curve

Three curves, user-selectable:

| Curve     | Behaviour                                                         | Best for           |
|-----------|-------------------------------------------------------------------|--------------------|
| `Linear`  | Pattern intensity = linear in `intensity`.                        | Faithful           |
| `Eventy`  | Quadratic accent on peaks. Short bursts feel sharper, sustained mid-magnitude rumble feels lighter than linear. | Action games (gunfire, impacts) |
| `Subtle`  | Logarithmic compression; ceiling at ~70 % of strongest pattern.   | Long sessions, ambient |

`Eventy` is the default — best feel for the dominant rumble shape in modern
games.

### 7.4 Pattern selection

The existing `daemon/src/hidpp/patterns.rs` already defines `HapticPattern`
(intensity tier + waveform ID) and `HapticPulse` (one-shot). The translator
maps `intensity` to an existing pattern band:

```
intensity < 0.20  →  HapticPattern::Tick    (subtle)
intensity < 0.50  →  HapticPattern::Click   (medium)
intensity < 0.80  →  HapticPattern::Bump    (strong)
intensity ≥ 0.80  →  HapticPattern::Pulse   (heaviest)
```

Names are illustrative; final names should reuse whatever `haptic_profiles::`
constants already exist (`MENU_APPEAR`, `SLICE_CHANGE`, `CONFIRM`, `INVALID`,
`PAGE_CHANGE`, `SUBMENU_OPEN/CLOSE` — see `patterns.rs:18-104`). We define a
*new* set of "game rumble" intensity tiers (so they're tunable per Game-Mode
config) but reuse the same underlying MX4 waveform IDs.

### 7.5 Throttling and continuous-rumble handling

The MX Master 4's actuator doesn't want kHz updates; over-driving causes the
piezo to feel "buzzy and ugly". The translator emits a haptic pulse no more
often than every `throttle_ms` (default **30 ms** — matches SDL2 HIDAPI's
`RUMBLE_WRITE_FREQUENCY_MS`).

For **continuous rumble** (FF_CONSTANT or FF_RUMBLE with `replay.length=0`):
re-emit a pattern every `throttle_ms` until stopped. Intensity is sampled at
each tick; magnitude changes mid-flight (game updates rumble while it's
playing) are picked up at the next tick.

For **short transient rumble** (`replay.length < throttle_ms`, e.g. a 16 ms
"gunshot"): fire exactly one pulse at the chosen tier and ignore the duration.

### 7.6 FF_PERIODIC handling

Wine collapses to `FF_RUMBLE`, so most of the time we never see a periodic.
But native SDL_Haptic users do upload `FF_SINE`. We treat the periodic's
`magnitude` as a single u16 intensity (mapped to combined intensity), ignore
the waveform / period / phase (we have no way to render a sine on a piezo).
Envelope `attack_length` / `fade_length` could theoretically modulate over
time, but **MVP ignores envelopes** — they're rare in practice, and the
patterns are already short enough that an envelope on a 30 ms tick adds little.

### 7.7 Per-event vs continuous mode

Two operating philosophies the user can pick:

- **Event mode** (default): every rumble play is one haptic pulse at the
  intensity tier; continuous rumble re-pulses on `throttle_ms`. Crisp,
  Mario-Kart-like.
- **Stream mode**: model the actuator as a low-rate vibration source; on
  continuous rumble emit `Tick` patterns spaced inversely to intensity (high
  intensity = 20 ms spacing, low intensity = 80 ms spacing). Sounds like a
  rapidly-clicking ratchet. Some users will prefer it for racing games.

Implement Event first; Stream is a `Mode` enum in config that can land later.

---

## 8. Modes of operation

### 8.1 Standalone mode (`Mode::Standalone`)

Daemon just creates the virtual gamepad. No real controller is proxied. Games
see only our pad; user plays keyboard-and-mouse (or with a real pad
side-by-side but only ours sources rumble — useful when a game lets the user
pick the "rumble device" explicitly, e.g. some racing sims).

Trivial. Implement first.

### 8.2 Proxy mode (`Mode::Proxy`) — recommended default

Daemon discovers a real gamepad via udev (`ID_INPUT_JOYSTICK=1`), mirrors its
capabilities onto the virtual pad, `EVIOCGRAB`s the real one, and pumps
events real→virtual. Game sees only the virtual pad; user keeps their physical
controller working. Rumble FF lands on our virtual pad; we translate.

#### 8.2.1 Discovery and adoption

On Game Mode enable:

1. Scan `/dev/input/by-id/` for `*-event-joystick` entries.
2. For each, read VID/PID via `EVIOCGID`. If multiple, prefer:
   - Configured controller (settings: "preferred controller GUID"); else
   - First-discovered.
3. `open(O_RDWR)` + `ioctl(EVIOCGRAB, 1)` to take exclusive ownership.
4. Read source's `EV_KEY` + `EV_ABS` bits via `EVIOCGBIT`, and `ABS_*` axis
   `input_absinfo` via `EVIOCGABS(code)` — mirror onto the virtual setup so
   the virtual pad looks like a copy of the real pad input-wise (rumble caps
   we override to ours).
5. Stop the existing `evdev::EvdevHandler` if it had bound to the same device
   (we already filter virtual / uinput devices in `daemon/src/evdev.rs:363`;
   when we *create* a virtual pad we need to make sure other handlers don't
   grab it, and we need to flag the real pad as "already proxied" so existing
   gesture detection doesn't try to read it).

#### 8.2.2 Event forwarding

Single tokio task:

```rust
loop {
    let ev = real_device.fetch_events().await?;
    for e in ev {
        match e.event_type() {
            EV_KEY | EV_ABS | EV_SYN => virtual_device.emit(&[e])?,
            EV_FF => continue,  // ignore; rumble path is reversed
            _    => continue,
        }
    }
}
```

A second tokio task listens for `EV_UINPUT` on the virtual fd and runs the FF
upload/erase protocol (§6).

A third tokio task watches the virtual fd for `EV_FF play/stop` and drives the
haptic translator (§7).

#### 8.2.3 Hiding the real device from games and the desktop

When in proxy mode we want **only** the virtual pad to be enumerable. The
`EVIOCGRAB` we did takes input ownership but does **not** hide the node from
`SDL_NumJoysticks()`.

Two options:

- **Soft hide:** set an env var Steam reads — `SDL_JOYSTICK_DEVICE` /
  `SDL_GAMECONTROLLER_IGNORE_DEVICES` (comma-separated `vid:pid` list).
  Documented, works for SDL-using games, doesn't survive across non-SDL
  paths. Daemon writes a launcher script that exports these.
- **Hard hide:** a transient udev rule on Game Mode enable that sets
  `ENV{ID_INPUT_JOYSTICK}="0"` for the real device's VID/PID. Survives across
  all paths but requires a `udevadm trigger` cycle that can briefly
  disconnect the device. **Not** the default — daemon takes the soft path
  and surfaces the hard-hide option in the settings UI for power users.

#### 8.2.4 Rumble passthrough to the real pad

When `passthrough_to_pad = true`: in addition to firing the mouse haptic,
forward the FF effect to the real device (upload via `EVIOCSFF` on the
ungrabbed FF channel — `EVIOCGRAB` covers input, not FF). Result: the user
feels rumble on both the controller and the mouse. Default off (most users
will set their mouse as primary haptic precisely *because* they're playing
keyboard-and-mouse, not controller).

### 8.3 Steam-Wrap mode (future, not MVP)

Position our pad so Steam Input *wraps* it as a controller, and we receive
Steam-translated rumble back via the same FF upload path. Requires empirical
testing to determine which VID/PIDs Steam will adopt. Defer; ship Proxy +
Standalone first.

---

## 9. Steam Input interaction — explicit handling

### 9.1 Detection

On Game Mode enable, daemon checks:

- Is the Steam client running? (`pgrep -x steam` is a coarse check; better
  is `/run/user/$UID/steam_input/*.pipe` existence.)
- Is there a Steam-created virtual pad? Scan
  `/sys/class/input/event*/device/id/vendor` for `28DE` + `product=11FF`.
- Was our virtual pad created **before** Steam's? Order matters for which pad
  the game picks as "controller 0".

### 9.2 Three-button diagnostic in settings UI

The settings tab includes a "Diagnose Game Compatibility" button. Daemon
returns a structured report:

```
Steam running:              yes
Steam Input virtual pad:    yes (vid=28DE pid=11FF on event23)
Our virtual pad:            yes (event19)
Real controllers detected:  1 (Xbox Wireless 045E:02FD on event12)
Recommended action:         For game X: open Steam → Settings → Controller
                            → Detected Controllers → uncheck Steam Input
                            for "MX Master 4 Haptic Gamepad".
```

This shifts the "Steam Input swallowed my rumble" debugging burden off the
user.

### 9.3 No automatic Steam config writes

We **never** edit Steam's `controller_configs.vdf` or any
Steam-owned file. Users can break their setups; we surface guidance only.

---

## 10. Config schema

Extends `juhradial-shared/src/config.rs` under the existing `GamingConfig`
(if it doesn't exist there yet, the existing Gaming tab probably reads from
`config.haptic` and a `gaming` block — match the surrounding style).

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HapticRedirectConfig {
    /// Master enable. Even when Game Mode is on, this stays off by default
    /// so the user opts in explicitly.
    pub enabled: bool,

    /// Standalone | Proxy
    pub mode: HapticRedirectMode,

    /// Preferred real controller for proxy mode. Identified by SDL GUID
    /// hex string. None = pick first discovered.
    pub preferred_controller_guid: Option<String>,

    /// 0.0 .. 2.0
    pub intensity_scale: f32,

    /// 0.0 .. 1.0 — magnitude below this is dropped.
    pub min_intensity: f32,

    /// Mix weights for the two rumble channels into one haptic intensity.
    pub strong_weight: f32,
    pub weak_weight: f32,

    /// Linear | Eventy | Subtle
    pub curve: HapticRedirectCurve,

    /// Event | Stream
    pub event_mode: HapticEventMode,

    /// ms — minimum gap between haptic pulses (rate-limits piezo)
    pub throttle_ms: u16,

    /// Forward rumble to the real controller too (so user feels both).
    pub passthrough_to_pad: bool,

    /// Hard-hide the real pad via transient udev rule. Requires re-trigger.
    pub hard_hide_real_controller: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum HapticRedirectMode { Standalone, Proxy }

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum HapticRedirectCurve { Linear, Eventy, Subtle }

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum HapticEventMode { Event, Stream }
```

### Default values

```rust
impl Default for HapticRedirectConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: HapticRedirectMode::Proxy,
            preferred_controller_guid: None,
            intensity_scale: 1.0,
            min_intensity: 0.08,
            strong_weight: 1.0,
            weak_weight: 0.4,
            curve: HapticRedirectCurve::Eventy,
            event_mode: HapticEventMode::Event,
            throttle_ms: 30,
            passthrough_to_pad: false,
            hard_hide_real_controller: false,
        }
    }
}
```

### Hot reload

Settings UI writes via the existing inotify-watched `config.json` path used
by overlay + macros. Daemon reloads `HapticRedirectConfig` on every change.
**Most changes hot-apply** (intensity / curve / weights / throttle / mode).
A handful require a virtual-device rebuild:

- `enabled` toggle
- `mode` swap (Standalone ↔ Proxy)
- `preferred_controller_guid` change while in Proxy mode
- `hard_hide_real_controller` toggle

These tear down and re-create the virtual device. Document this in the UI
("Changing the mode briefly re-creates the virtual gamepad").

---

## 11. Settings UI — extends `tabs/gaming.rs`

The existing tab (`settings-rs/src/tabs/gaming.rs:18-89`) has the master Game
Mode toggle and the DPI Cycle card. Append a **"Game Rumble → Haptic"**
section below the DPI card.

### 11.1 Layout

```
Gaming Mode  [● ON]
  G  Gaming Mode
     On — DPI raised, overlay suppressed.

[Gaming DPI                                     [Cycle preset]]
  Walks through the preset DPI list (e.g. 1600 → 3200 → 4800)
  on each click. Useful for hot-binding to a side button via
  the macro recorder.

[Game Rumble → Haptic                                       ▾]
  Redirect gamepad rumble from games onto the MX Master 4's
  haptic actuator. Active only while Game Mode is on.

  Enable                                            [○ OFF → ● ON]

  ── Behaviour ────────────────────────────────────────────────
  Mode      [⦿ Proxy a real controller] [○ Standalone (no pad)]
  Curve     [⦿ Eventy] [○ Linear] [○ Subtle]
  Event mode[⦿ Event ] [○ Stream]

  ── Strength ─────────────────────────────────────────────────
  Intensity         ──◆────────────────  1.00×
  Min intensity     ◆────────────────── 0.08
  Strong weight     ────◆──────────────  1.00
  Weak weight       ──◆────────────────  0.40
  Throttle          ────◆────────────── 30 ms

  ── Compatibility ────────────────────────────────────────────
  ☐  Passthrough rumble to real controller too
  ☐  Hard-hide real controller from games (advanced)

  Preferred controller (Proxy mode):
   [auto-detect: Xbox Wireless 045E:02FD] [Change ▾]

  [Diagnose game compatibility]   [Test haptic]
```

### 11.2 Help text — Steam Input warning

Inline note rendered when Steam is detected and Steam Input is enabled:

> Steam Input is active. If a game's rumble isn't reaching the mouse,
> open Steam → Settings → Controller → Detected Controllers and uncheck
> Steam Input for "MX Master 4 Haptic Gamepad". Run **Diagnose** for a
> per-game recommendation.

### 11.3 Test haptic button

Pumps a synthetic FF_RUMBLE upload (`strong=0xC000 weak=0x4000
length=500ms`) through our own translator and into the haptic manager — so
the user can verify the chain end-to-end without launching a game. Calls a
new D-Bus method `TestHapticRedirect()`.

### 11.4 Diagnose button

Calls D-Bus `DiagnoseHapticRedirect()` which returns a structured report
(§9.2) and the UI renders as a side panel.

---

## 12. Daemon module structure

New module **`daemon/src/gamepad_haptics/`** (directory, not flat — it has
sub-files):

```
daemon/src/gamepad_haptics/
  mod.rs            — top-level GamepadHapticsService struct + lifecycle
  virtual_pad.rs    — uinput device creation + capability spec
  ff_protocol.rs    — UI_FF_UPLOAD / UI_FF_ERASE handshake loop
  translator.rs     — magnitude → HapticPattern (curve, throttle, mix)
  proxy.rs          — real-device discovery + EVIOCGRAB + event forwarder
  diagnostics.rs    — Steam / virtual-pad / controller introspection
```

### Lifecycle integration with `GamingMode`

Edit `daemon/src/gaming.rs:30-37` to add:

```rust
pub struct GamingMode {
    enabled: bool,
    suppress_overlay: bool,
    dpi_manager: DpiManager,
    haptic_manager: SharedHapticManager,
    haptic_redirect: Option<GamepadHapticsService>,   // NEW
}
```

`GamingMode::enable()` (current `gaming.rs:54-76`) spawns the service if
`config.gaming.haptic_redirect.enabled`. `disable()` aborts the service +
ungrabs the real pad + destroys the virtual pad. The current double-enable
and double-disable no-op guards (tests at `gaming.rs:191-208`) stay.

### D-Bus surface

Extend `daemon/src/dbus/interface.rs` with:

```
TestHapticRedirect() -> ()
DiagnoseHapticRedirect() -> (a{sv})        # property bag
ReloadHapticRedirectConfig() -> ()         # for explicit nudge if inotify missed
```

`SetGamingMode(bool)` (already exists per the settings tab at
`tabs/gaming.rs:41`) is the entry that drives the whole thing on/off.

### Cargo workspace

`daemon/Cargo.toml` adds:

```toml
evdev = "0.13"          # already a dep? confirm; else add
nix = "0.29"            # already a dep
tokio = { version = "1", features = ["macros", "rt-multi-thread", "fs", "io-util", "time"] }
udev = "0.9"            # for real-device discovery
```

---

## 13. Testing strategy

### 13.1 Unit tests

| Layer            | Tests                                                                 |
|------------------|-----------------------------------------------------------------------|
| `translator.rs`  | Magnitude curves on representative inputs; deadzone; gain; throttle; mix weights; curve enum coverage; FF_GAIN modulation. |
| `ff_protocol.rs` | Mock kernel side: feed `EV_UINPUT` event + canned `uinput_ff_upload`; assert `UI_BEGIN`/`UI_END` ioctls called; verify `retval` set; verify timeout-on-missing-END (poison detection). |
| `virtual_pad.rs` | Snapshot the capability bits we declare; regression-test against the §5.2 spec list. |
| `proxy.rs`       | Mock evdev source emitting a button + axis stream; assert virtual pad emits the same events; assert `EVIOCGRAB` was called. |

### 13.2 Integration tests

| Tool         | What                                                                  |
|--------------|-----------------------------------------------------------------------|
| `evtest`     | Open our virtual pad node, read button presses (proxied), and uploaded FF effects (`evtest --query`). Manual but scripted. |
| `fftest`     | The classic `fftest /dev/input/eventN` — uploads a canned FF_RUMBLE and plays it. We should see haptic on the mouse. |
| `xpadneo`-style python evdev rumble script | `xpadneo/misc/examples/python_evdev_rumble/rumble.py` — known-good FF_RUMBLE test loop. |
| Steam: `Big Buck Bunny`-equivalent rumble title | Any free game with rumble (e.g. *Risk of Rain 2*, *Brawlhalla*, *Crab Game*). Verify end-to-end. |
| Proton: known-rumble Windows game | *Hades*, *Halo MCC*, anything with native XInput. Verify XInput → winebus → evdev → us path. |

### 13.3 Manual compatibility matrix

Spreadsheet (or `tests/MANUAL_RUMBLE_COMPAT.md`) tracking actual results
per-title: game, engine, rumble path observed (evdev / HIDAPI / hidraw),
whether we caught it, any per-game env-var workaround required. Build up
over time as users report.

### 13.4 What we explicitly skip

- No mocked HID++ at the wire level for these tests; the existing
  `daemon/src/hidpp/tests.rs` covers that. We test `gamepad_haptics` against
  the `SharedHapticManager` *trait* (existing) and trust it.

---

## 14. Caveats and unsolved problems

### 14.1 SDL HIDAPI bypass — known gap

For any user who has an Xbox/PS/Switch controller plugged in **and** the game
uses SDL HIDAPI for it (default for those VID/PIDs), the game's rumble goes
straight to `/dev/hidrawN` and never touches our virtual pad. Mitigations,
all documented in the UI:

- Per-game env var: `SDL_JOYSTICK_HIDAPI=0` forces SDL to evdev for that game.
- For Steam-launched games: same env via Steam launch options:
  `SDL_JOYSTICK_HIDAPI=0 %command%`.
- Unplug the real controller while playing.
- Future: a hidraw shim via `LD_PRELOAD` overriding `hid_open`/`hid_write` —
  big lift, deferred.

### 14.2 Steam Input wrapping

When Steam Input is on for a game and Steam wraps the real controller, the
game writes rumble to Steam's `28DE:11FF` virtual pad — not ours. We don't
catch it. Workaround: disable Steam Input for the game, OR (future)
position our pad to be the one Steam wraps.

### 14.3 Proton hidraw mode

If the user has set `PROTON_ENABLE_HIDRAW=0xVID/0xPID` (rare, manual),
Proton bypasses evdev for that specific controller. Documented as a
"disable for our daemon to work" hint.

### 14.4 No FF_PERIODIC waveform rendering

Piezo can't render a sine. We collapse periodic to magnitude. Some
sim-racing wheel-style effects (custom waveforms, deep envelopes) will lose
their character. Acceptable: those games target wheels, not pads anyway.

### 14.5 No envelope honouring

`ff_envelope.attack_length` / `fade_length` are ignored in MVP. Easy to add
once the throttle scheduler exists (modulate intensity over time). Tracked
as a v2 feature.

### 14.6 Concurrent FF effects

We advertise 16 slots; if a game uploads 16 effects and plays them
simultaneously we'd be expected to mix. Piezo doesn't mix — we play the
highest-intensity active effect (`max` over all currently-playing slot
intensities). Document this behaviour.

### 14.7 Native Unity games on Linux

Most Unity titles on Linux desktop **don't rumble at all** because Unity
InputSystem's Linux backend lacks an HID FF write path. We can't fix this
from our side. Affected: many indie Unity ports.

### 14.8 Wayland / non-Wayland

No relevance — we operate at the kernel input layer below the display
server. Confirmed: no Wayland-specific concerns.

### 14.9 Bazzite / rpm-ostree

The udev rule goes under `/usr/local/share/...` per the existing project
convention (memory: `user_system.md`). `install.sh` already follows this.
No additional Bazzite handling required for the daemon binary (matches the
existing `juhradiald` install path).

---

## 15. Implementation phases

Strict order, with verification gates between phases:

### Phase 1 — Config schema (no behaviour)
- Add `HapticRedirectConfig` to `juhradial-shared/src/config.rs`.
- Add defaults; bump config schema version; write migration in
  `daemon/src/config.rs` (if there's a version-bump path; otherwise rely
  on serde defaults for absent fields).
- Add tests around serialisation round-trip and default values.
- **Gate:** existing config-load tests still pass; default settings JSON
  round-trips.

### Phase 2 — Settings UI skeleton
- Extend `settings-rs/src/tabs/gaming.rs` with the new card (enabled
  toggle + curve + sliders + mode picker). Wire `Message::SetHapticRedirect*`
  variants in `settings-rs/src/main.rs`. **Don't** wire to daemon yet —
  just write to config.
- **Gate:** `cargo build -p juhradial-settings` clean; UI renders;
  config.json updates as the user moves sliders.

### Phase 3 — Virtual pad creation
- New `daemon/src/gamepad_haptics/virtual_pad.rs`. Implement Mode::Standalone
  device-creation only.
- Hook into `GamingMode::enable()` / `disable()` lifecycle.
- **Gate:** with Game Mode on + Haptic Redirect on, `evtest` sees the virtual
  pad with the §5.2 caps. `udevadm info /dev/input/eventN` shows
  `ID_INPUT_JOYSTICK=1`.

### Phase 4 — FF upload/erase protocol (no haptic yet)
- New `daemon/src/gamepad_haptics/ff_protocol.rs`. Implement the three-ioctl
  handshake. Log each upload/erase at `tracing::info!`.
- **Gate:** `fftest /dev/input/eventN` runs without hanging; daemon logs
  show "FF effect uploaded id=X type=FF_RUMBLE strong=0xC000 weak=0x4000".

### Phase 5 — Translation + haptic dispatch
- New `daemon/src/gamepad_haptics/translator.rs`. Implement curve,
  deadzone, throttle, FF_GAIN, replay timing.
- Wire to `SharedHapticManager`.
- **Gate:** `fftest` causes the MX Master 4 to click. Different magnitudes
  pick different patterns. Settings sliders affect output in real time.

### Phase 6 — Proxy mode
- New `daemon/src/gamepad_haptics/proxy.rs`. udev discovery + EVIOCGRAB +
  event forwarding.
- **Gate:** plug in an Xbox pad, Game Mode on, Proxy mode on. Game sees only
  the virtual pad; controller inputs work; rumble lands on the mouse.

### Phase 7 — Diagnostics + Test button
- `diagnostics.rs` + D-Bus `TestHapticRedirect()` + `DiagnoseHapticRedirect()`.
- Settings tab "Test haptic" / "Diagnose" buttons wired.
- **Gate:** Diagnose returns plausible structured output across two
  controller states (plugged / unplugged) and with Steam running / not.

### Phase 8 — Manual game compatibility sweep
- Test top-20 candidate titles per the compatibility matrix. Document
  per-game findings in `tests/MANUAL_RUMBLE_COMPAT.md`.
- **Gate:** README updated with the compatibility tier estimate, with a
  per-API table the user can sanity-check.

### Phase 9 — Polish & docs
- Update CHANGELOG, CONTRIBUTING-BAZZITE-PR, README haptic section.
- Add a memory entry summarising the gamepad bridge for future Claude
  sessions (auto-memory `project_gamepad_haptic_bridge.md`).

Total estimate: **2–3 weeks of focused work** for a working MVP through
Phase 7. Phase 8 sweep stretches with title coverage.

---

## 16. Future extensions (not MVP)

- **Per-game profiles.** Detect window class / Steam appid; swap
  `HapticRedirectConfig` profile. UI: per-game override list.
- **Stream mode.** §7.7 rapid-tick alternative.
- **Envelope honouring.** Modulate intensity over time per
  `ff_envelope.attack/fade`.
- **Steam-wrap mode.** Position to be the controller Steam Input wraps.
  Empirical.
- **Hidraw shim.** LD_PRELOAD hook for `hid_write` on `/dev/hidrawN`
  matching known controller VID/PIDs; mirror writes into our translator.
  Big lift.
- **Audio-reactive haptic** when no rumble signal is present (game has
  no rumble support but we still want some feedback). Out of scope of the
  gamepad bridge — separate feature.
- **Network-multipeer**: forward rumble from a remote machine to a local
  mouse. Niche.

---

## 17. References

### Linux kernel
- [Force Feedback documentation (ff.rst)](https://github.com/torvalds/linux/blob/master/Documentation/input/ff.rst)
- [uinput documentation](https://docs.kernel.org/input/uinput.html)
- [linux/include/uapi/linux/uinput.h](https://github.com/torvalds/linux/blob/master/include/uapi/linux/uinput.h)
- [linux/include/uapi/linux/input.h (struct ff_effect)](https://github.com/torvalds/linux/blob/master/include/uapi/linux/input.h)
- [Linux Gamepad Specification](https://www.kernel.org/doc/html/latest/input/gamepad.html)

### SDL
- [SDL_JoystickRumble](https://wiki.libsdl.org/SDL2/SDL_JoystickRumble) /
  [SDL_GameControllerRumble](https://wiki.libsdl.org/SDL2/SDL_GameControllerRumble) /
  [SDL_Haptic](https://wiki.libsdl.org/SDL2/SDL_Haptic) /
  [SDL3 SDL_RumbleJoystick](https://wiki.libsdl.org/SDL3/SDL_RumbleJoystick)
- [HIDAPI and Device-Specific Drivers (DeepWiki)](https://deepwiki.com/libsdl-org/SDL/5.2-hidapi-and-device-specific-drivers)
- [SDL_HINT_JOYSTICK_HIDAPI_STEAM](https://wiki.libsdl.org/SDL3/SDL_HINT_JOYSTICK_HIDAPI_STEAM)
- [SDL_GameControllerDB](https://github.com/mdqinc/SDL_GameControllerDB)
- [SDL_sysjoystick.c (linux)](https://github.com/libsdl-org/SDL/blob/main/src/joystick/linux/SDL_sysjoystick.c)

### Steam Input
- [Steam Input Getting Started](https://partner.steamgames.com/doc/features/steam_controller/getting_started_for_devs)
- [ISteamInput Steamworks docs](https://partner.steamgames.com/doc/api/isteaminput)
- [Steam Input General Concepts](https://partner.steamgames.com/doc/features/steam_controller/concepts)
- [steam-devices udev rules](https://github.com/ValveSoftware/steam-devices/blob/master/60-steam-input.rules)
- [Steam Deck HID + libmanette (Alice Mikhaylenko, GNOME, 2024)](https://blogs.gnome.org/alicem/2024/10/24/steam-deck-hid-and-libmanette-adventures/)
- [ValveSoftware/steam-for-linux #8590 — Steam Input rumble passthrough broken](https://github.com/ValveSoftware/steam-for-linux/issues/8590)

### Wine / Proton
- [winebus.sys bus_udev.c](https://github.com/wine-mirror/wine/blob/master/dlls/winebus.sys/bus_udev.c)
- [winebus.sys bus_sdl.c](https://github.com/wine-mirror/wine/blob/master/dlls/winebus.sys/bus_sdl.c)
- [Wine xinput1_3 main.c](https://github.com/wine-mirror/wine/blob/master/dlls/xinput1_3/main.c)
- [Wine dinput joystick_hid.c](https://github.com/wine-mirror/wine/blob/master/dlls/dinput/joystick_hid.c)
- [Proton issue #4707 — duplicate evdev+rawhid](https://github.com/ValveSoftware/Proton/issues/4707)
- [Proton issue #9034 — hidraw vs evdev fallback](https://github.com/ValveSoftware/Proton/issues/9034)
- [Proton issue #8672 — PROTON_ENABLE_HIDRAW](https://github.com/ValveSoftware/Proton/issues/8672)
- [Wine + DualSense via hidraw (Nicholas Tay, 2024)](https://nick.tay.blue/2024/01/21/wine-dualsense/)

### Prior art
- [sc-controller fork (C0rn3j)](https://github.com/C0rn3j/sc-controller)
- [Intiface Game Haptics Router](https://github.com/intiface/intiface-game-haptics-router)
- [SCUF Envision Pro V2 Linux driver](https://github.com/Tealdragon204/scuf-envision-pro-V2-Linux)
- [MoltenGamepad](https://github.com/jgeumlek/MoltenGamepad)
- [Solaar (HID++ reference)](https://github.com/pwr-Solaar/Solaar)
- [mx4notifications (HID++ haptic sink reference)](https://github.com/lukasfri/mx4notifications)
- [Project Aurora (DualSense → smart-light prior art)](https://www.project-aurora.com/Docs/devices/dualsense/)
- [rumble-test (FF ioctl snippet)](https://github.com/sre/rumble-test)
- [Haptic Retargeting (Microsoft Research, CHI 2016)](https://www.microsoft.com/en-us/research/publication/haptic-retargeting-dynamic-repurposing-passive-haptics-enhanced-virtual-reality-experiences/)

### Rust crates
- [evdev 0.13](https://docs.rs/evdev/) — uinput `VirtualDevice::process_ff_upload`
- [input-linux](https://docs.rs/input-linux/) — fallback ioctl crate
- [nix](https://docs.rs/nix/) — raw ioctl macros
- [udev](https://docs.rs/udev/) — device discovery

### In-repo
- `daemon/src/gaming.rs` — existing GamingMode lifecycle
- `daemon/src/evdev.rs` — existing uinput usage (mouse pass-through)
- `daemon/src/hidpp/patterns.rs` — existing haptic patterns
- `daemon/src/hidpp/mod.rs` — SharedHapticManager
- `settings-rs/src/tabs/gaming.rs` — existing Gaming tab
- `juhradial-shared/src/config.rs` — config schema
