//! The virtual evdev gamepad — capability spec + builder.
//!
//! We expose a uinput device that games treat as an ordinary
//! Xbox-360 controller, so their rumble (`FF_RUMBLE` uploaded via
//! `EVIOCSFF`) lands on *us* instead of a real pad. The daemon then
//! re-renders that rumble as MX Master 4 haptic patterns.
//!
//! Identity and capability choices are explained in
//! `HAPTIC_GAMEPAD_BRIDGE_DESIGN.md` §5. The short version:
//!
//!   * VID/PID `045E:028E` (wired Xbox 360) — SDL2 ships a built-in
//!     `gamecontrollerdb` mapping for this GUID, so every
//!     SDL_GameController game treats us as a fully-mapped pad with
//!     no extra config.
//!   * `BUS_VIRTUAL` (not `BUS_USB`) — keeps SDL2's HIDAPI backend
//!     off our device (HIDAPI parents to a `usb_device`), so games
//!     fall through to the evdev rumble path we actually capture.
//!   * `FF_RUMBLE` + the `FF_PERIODIC` family + `FF_GAIN` — covers
//!     both modern `SDL_GameControllerRumble` callers and the older
//!     `SDL_Haptic` API (which uploads `FF_SINE`, not `FF_RUMBLE`).
//!   * Full Xbox-360 button + axis set — required for udev's
//!     `input_id` builtin to tag the node `ID_INPUT_JOYSTICK=1`;
//!     without that tag SDL2 won't enumerate it as a controller.

use evdev::{
    uinput::VirtualDevice, AbsInfo, AbsoluteAxisCode, AttributeSet, BusType, FFEffectCode,
    InputId, KeyCode, UinputAbsSetup,
};

/// Device name as it appears in `evtest` / Steam's controller list.
/// Deliberately *not* the literal "Microsoft X-Box 360 pad" string —
/// SDL maps by GUID (derived from bus+VID+PID+version), so a
/// distinct name keeps the device identifiable to humans without
/// breaking the auto-mapping.
pub const NAME: &str = "MX Master 4 Haptic Gamepad";

/// Microsoft vendor id.
pub const VENDOR: u16 = 0x045E;
/// Xbox 360 wired pad product id — matches SDL's built-in mapping.
pub const PRODUCT: u16 = 0x028E;
/// Arbitrary version; folded into the SDL GUID.
pub const VERSION: u16 = 0x0114;

/// Simultaneous FF effect slots advertised via `EVIOCGEFFECTS`.
/// Matches `xpad`'s claim and far exceeds what any game uses
/// (Wine/SDL touch 1-4 slots in practice).
pub const FF_EFFECTS_MAX: u32 = 16;

/// Xbox-360 button set. The kernel aliases line up as
/// `BTN_SOUTH=A`, `BTN_EAST=B`, `BTN_NORTH=X`, `BTN_WEST=Y` — the
/// historical xpad layout. `BTN_SOUTH` (== `BTN_GAMEPAD`) being
/// present is what makes udev tag the node as a joystick.
pub const BUTTONS: [KeyCode; 11] = [
    KeyCode::BTN_SOUTH,  // A
    KeyCode::BTN_EAST,   // B
    KeyCode::BTN_NORTH,  // X
    KeyCode::BTN_WEST,   // Y
    KeyCode::BTN_TL,     // left shoulder
    KeyCode::BTN_TR,     // right shoulder
    KeyCode::BTN_SELECT, // back / view
    KeyCode::BTN_START,  // start / menu
    KeyCode::BTN_MODE,   // guide
    KeyCode::BTN_THUMBL, // left stick click
    KeyCode::BTN_THUMBR, // right stick click
];

/// Force-feedback effect types we advertise. `FF_RUMBLE` is the one
/// games actually use; the `FF_PERIODIC` family is advertised so
/// SDL2's older `SDL_Haptic` probe (which checks for `FF_PERIODIC`
/// + a waveform) considers the device haptic-capable. `FF_GAIN`
/// lets games set a master amplitude we honour as a multiplier.
pub const FF_CODES: [FFEffectCode; 8] = [
    FFEffectCode::FF_RUMBLE,
    FFEffectCode::FF_PERIODIC,
    FFEffectCode::FF_SINE,
    FFEffectCode::FF_SQUARE,
    FFEffectCode::FF_TRIANGLE,
    FFEffectCode::FF_CONSTANT,
    FFEffectCode::FF_RAMP,
    FFEffectCode::FF_GAIN,
];

/// Analogue sticks — signed 16-bit, the Xbox-360 range SDL expects.
pub const STICK_AXES: [AbsoluteAxisCode; 4] = [
    AbsoluteAxisCode::ABS_X,
    AbsoluteAxisCode::ABS_Y,
    AbsoluteAxisCode::ABS_RX,
    AbsoluteAxisCode::ABS_RY,
];

/// Triggers — unsigned, 0..1023.
pub const TRIGGER_AXES: [AbsoluteAxisCode; 2] =
    [AbsoluteAxisCode::ABS_Z, AbsoluteAxisCode::ABS_RZ];

/// D-pad — reported as a hat, -1..1 per axis.
pub const HAT_AXES: [AbsoluteAxisCode; 2] =
    [AbsoluteAxisCode::ABS_HAT0X, AbsoluteAxisCode::ABS_HAT0Y];

/// `AbsInfo` for a stick axis. `(value, min, max, fuzz, flat, resolution)`.
fn stick_absinfo() -> AbsInfo {
    AbsInfo::new(0, -32768, 32767, 16, 128, 0)
}

/// `AbsInfo` for a trigger axis.
fn trigger_absinfo() -> AbsInfo {
    AbsInfo::new(0, 0, 1023, 0, 0, 0)
}

/// `AbsInfo` for a hat axis.
fn hat_absinfo() -> AbsInfo {
    AbsInfo::new(0, -1, 1, 0, 0, 0)
}

/// The full ordered list of absolute-axis setups for the builder.
pub fn abs_setups() -> Vec<UinputAbsSetup> {
    let mut v = Vec::with_capacity(8);
    for code in STICK_AXES {
        v.push(UinputAbsSetup::new(code, stick_absinfo()));
    }
    for code in TRIGGER_AXES {
        v.push(UinputAbsSetup::new(code, trigger_absinfo()));
    }
    for code in HAT_AXES {
        v.push(UinputAbsSetup::new(code, hat_absinfo()));
    }
    v
}

/// Create the virtual gamepad as a live uinput device.
///
/// Requires write access to `/dev/uinput` (the `uinput` kernel
/// module must be loaded and a udev rule must grant the daemon's
/// user access — see `HAPTIC_GAMEPAD_BRIDGE_DESIGN.md` §5.3).
///
/// The returned [`VirtualDevice`] owns the kernel device: dropping
/// it destroys the `/dev/input/eventN` node.
pub fn build_virtual_pad() -> std::io::Result<VirtualDevice> {
    let keys: AttributeSet<KeyCode> = BUTTONS.iter().copied().collect();
    let ff: AttributeSet<FFEffectCode> = FF_CODES.iter().copied().collect();

    let mut builder = VirtualDevice::builder()?
        .name(NAME)
        .input_id(InputId::new(BusType::BUS_VIRTUAL, VENDOR, PRODUCT, VERSION))
        .with_keys(&keys)?
        .with_ff(&ff)?
        .with_ff_effects_max(FF_EFFECTS_MAX);

    for setup in abs_setups() {
        builder = builder.with_absolute_axis(&setup)?;
    }

    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    // The capability spec is data, so it can be checked without a
    // live uinput device — guards against silent drift from §5.2 of
    // HAPTIC_GAMEPAD_BRIDGE_DESIGN.md.

    #[test]
    fn identity_matches_xbox_360_wired() {
        // SDL's built-in mapping keys off exactly this fingerprint.
        assert_eq!(VENDOR, 0x045E);
        assert_eq!(PRODUCT, 0x028E);
        assert_eq!(NAME, "MX Master 4 Haptic Gamepad");
    }

    #[test]
    fn advertises_eleven_xbox_buttons() {
        assert_eq!(BUTTONS.len(), 11);
        // BTN_SOUTH == BTN_GAMEPAD — its presence is what makes udev
        // tag the node ID_INPUT_JOYSTICK=1.
        assert!(BUTTONS.contains(&KeyCode::BTN_SOUTH));
        assert!(BUTTONS.contains(&KeyCode::BTN_MODE));
        assert!(BUTTONS.contains(&KeyCode::BTN_THUMBR));
    }

    #[test]
    fn ff_set_covers_rumble_periodic_and_gain() {
        assert_eq!(FF_CODES.len(), 8);
        // FF_RUMBLE: the effect games actually upload.
        assert!(FF_CODES.contains(&FFEffectCode::FF_RUMBLE));
        // FF_PERIODIC + FF_SINE: makes SDL2's SDL_Haptic probe pass.
        assert!(FF_CODES.contains(&FFEffectCode::FF_PERIODIC));
        assert!(FF_CODES.contains(&FFEffectCode::FF_SINE));
        // FF_GAIN: master-amplitude events we honour.
        assert!(FF_CODES.contains(&FFEffectCode::FF_GAIN));
    }

    #[test]
    fn ff_effects_max_is_sixteen() {
        assert_eq!(FF_EFFECTS_MAX, 16);
    }

    /// End-to-end check: actually create the kernel device and read
    /// its capabilities back. Ignored by default because it needs
    /// write access to `/dev/uinput` — run as root, or after
    /// installing `packaging/udev/70-juhradial-haptic-pad.rules`:
    ///
    /// ```text
    /// cargo test -p juhradiald --lib -- --ignored virtual_pad
    /// ```
    ///
    /// This is the programmatic form of the Phase 3 gate ("evtest
    /// sees the virtual pad"): if the capabilities read back match
    /// the spec, udev will also tag the node `ID_INPUT_JOYSTICK=1`
    /// (it keys that off `BTN_GAMEPAD` + `ABS_X`/`ABS_Y`, which we
    /// assert below).
    ///
    /// The capability read-back step re-opens the freshly created
    /// `/dev/input/eventN`. Without the udev rule installed that
    /// node is `root:input 0660`, so the read-back is *skipped*
    /// (not failed) on `PermissionDenied` — device creation itself,
    /// the part that proves Phase 3, is still hard-asserted.
    #[test]
    #[ignore = "creates a real uinput device; needs /dev/uinput write access"]
    fn smoke_creates_device_with_expected_capabilities() {
        use evdev::Device;
        use std::io::ErrorKind;

        let mut pad = build_virtual_pad().expect("build virtual pad");
        let node = pad
            .enumerate_dev_nodes_blocking()
            .expect("enumerate dev nodes")
            .flatten()
            .next()
            .expect("virtual pad should expose at least one event node");

        // Give udev (and logind, for the uaccess ACL) time to
        // finish processing the new node.
        std::thread::sleep(std::time::Duration::from_millis(1500));

        let dev = match Device::open(&node) {
            Ok(dev) => dev,
            Err(e) if e.kind() == ErrorKind::PermissionDenied => {
                let facl = std::process::Command::new("getfacl")
                    .arg("-p")
                    .arg(&node)
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                    .unwrap_or_else(|e| format!("(getfacl failed: {e})"));
                eprintln!(
                    "virtual pad created at {} — skipping capability read-back: \
                     no access to the node. Install \
                     packaging/udev/70-juhradial-haptic-pad.rules to grant it.\n\
                     node ACL:\n{facl}",
                    node.display()
                );
                return;
            }
            Err(e) => panic!("unexpected error re-opening the virtual pad: {e}"),
        };

        assert_eq!(dev.name(), Some(NAME));
        assert_eq!(dev.input_id().vendor(), VENDOR);
        assert_eq!(dev.input_id().product(), PRODUCT);

        let ff = dev.supported_ff().expect("device advertises force feedback");
        assert!(ff.contains(FFEffectCode::FF_RUMBLE));
        assert!(ff.contains(FFEffectCode::FF_GAIN));

        let keys = dev.supported_keys().expect("device advertises keys");
        assert!(keys.contains(KeyCode::BTN_SOUTH)); // == BTN_GAMEPAD

        let axes = dev
            .supported_absolute_axes()
            .expect("device advertises absolute axes");
        assert!(axes.contains(AbsoluteAxisCode::ABS_X));
        assert!(axes.contains(AbsoluteAxisCode::ABS_Y));
    }

    #[test]
    fn abs_setup_count_and_ranges() {
        let setups = abs_setups();
        // 4 sticks + 2 triggers + 2 hat axes.
        assert_eq!(setups.len(), 8);

        let find = |code: AbsoluteAxisCode| {
            setups
                .iter()
                .find(|s| s.code() == code.0)
                .unwrap_or_else(|| panic!("missing axis {code:?}"))
                .absinfo()
        };

        // Sticks: signed 16-bit.
        let lx = find(AbsoluteAxisCode::ABS_X);
        assert_eq!(lx.minimum(), -32768);
        assert_eq!(lx.maximum(), 32767);

        // Triggers: unsigned 0..1023.
        let lt = find(AbsoluteAxisCode::ABS_Z);
        assert_eq!(lt.minimum(), 0);
        assert_eq!(lt.maximum(), 1023);

        // Hat: -1..1.
        let hat = find(AbsoluteAxisCode::ABS_HAT0X);
        assert_eq!(hat.minimum(), -1);
        assert_eq!(hat.maximum(), 1);
    }
}
