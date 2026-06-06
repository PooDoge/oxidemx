//! Force-feedback servicing for the virtual gamepad.
//!
//! A uinput device cannot service `EVIOCSFF` itself — when a game
//! uploads an FF effect, the kernel parks the game's syscall and
//! hands the request back to *us* as a `UI_FF_UPLOAD` event on the
//! uinput fd. We must complete the three-ioctl handshake
//! (`UI_BEGIN_FF_UPLOAD` → inspect → `UI_END_FF_UPLOAD`) or the
//! game hangs forever in `D` state. The `evdev` crate wraps the
//! handshake in [`FFUploadEvent`] / [`FFEraseEvent`] RAII guards:
//! the `UI_END_*` ioctl fires on `Drop`, so the rule is simply
//! "set the retval, then let the guard drop".
//!
//! See `HAPTIC_GAMEPAD_BRIDGE_DESIGN.md` §6 for the protocol detail.
//!
//! ## Phase status
//!
//! **Phase 4 (this commit):** the handshake is serviced and every
//! upload / erase / play / stop is logged. No haptic output yet —
//! Phase 5 adds the magnitude → MX-4-pattern translator and calls
//! it from [`process_event`]'s play branch.

use std::collections::{BTreeMap, BTreeSet};

use evdev::{
    uinput::{VirtualDevice, VirtualEventStream},
    AbsoluteAxisCode, Device, EventSummary, EventStream, EventType, FFEffectCode,
    FFEffectData, FFEffectKind, InputEvent, UInputCode,
};
use oxidemx_shared::HapticRedirectConfig;
use tokio::sync::oneshot;

use super::translator::RumbleTranslator;
use super::virtual_pad;
use crate::hidpp::SharedHapticManager;

/// Effect-slot bookkeeping: which FF effect ids are free, and the
/// effect data the game uploaded for each live id.
///
/// We assign ids ourselves from a free pool (the kernel hands us
/// `effect.id == -1` for a new upload and expects us to fill it).
/// Storing the [`FFEffectData`] keyed by id means that when an
/// `EV_FF` *play* event arrives later — which carries only the
/// effect id — we can recover the magnitudes. Phase 5's translator
/// consumes exactly this map.
struct EffectSlots {
    /// Unused effect ids, `0..FF_EFFECTS_MAX`.
    free: BTreeSet<u16>,
    /// Effect data for every currently-uploaded id.
    active: BTreeMap<u16, FFEffectData>,
}

impl EffectSlots {
    fn new() -> Self {
        Self {
            free: (0..virtual_pad::FF_EFFECTS_MAX as u16).collect(),
            active: BTreeMap::new(),
        }
    }

    /// Take the lowest free id, or `None` if all slots are in use.
    fn allocate(&mut self) -> Option<u16> {
        let id = self.free.iter().next().copied()?;
        self.free.remove(&id);
        Some(id)
    }

    /// Record (or replace) the effect data stored for `id`.
    fn store(&mut self, id: u16, data: FFEffectData) {
        self.active.insert(id, data);
    }

    /// Return `id` to the free pool and forget its effect data.
    fn release(&mut self, id: u16) {
        self.active.remove(&id);
        self.free.insert(id);
    }

    /// Look up the effect data for a currently-uploaded id.
    fn get(&self, id: u16) -> Option<&FFEffectData> {
        self.active.get(&id)
    }
}

/// Service the virtual gamepad's force-feedback traffic until the
/// `shutdown` future resolves (which happens when the owning
/// [`super::GamepadHapticsService`] is dropped).
///
/// Takes ownership of `device`; when this returns, the event
/// stream — and therefore the kernel uinput device — is dropped.
/// `config` + `haptics` build the [`RumbleTranslator`] that turns
/// captured rumble into MX-4 haptic pulses.
///
/// `real` is the proxied controller in `Proxy` mode (`None` in
/// `Standalone` mode). Its input is forwarded onto the virtual pad
/// so the game sees a single controller.
pub async fn serve(
    device: VirtualDevice,
    real: Option<Device>,
    config: HapticRedirectConfig,
    haptics: SharedHapticManager,
    mut shutdown: oneshot::Receiver<()>,
) {
    let mut stream = match device.into_event_stream() {
        Ok(stream) => stream,
        Err(e) => {
            tracing::error!(
                error = %e,
                "haptic redirect: could not start the FF event stream"
            );
            return;
        }
    };

    // The proxied controller's input stream, if any.
    let mut real_stream: Option<EventStream> = match real {
        Some(controller) => match controller.into_event_stream() {
            Ok(stream) => Some(stream),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "haptic redirect: could not stream the proxied controller — \
                     its input will not be forwarded"
                );
                None
            }
        },
        None => None,
    };

    let mut slots = EffectSlots::new();
    // Wake-up pulse period — captured before the config moves into
    // the translator. 0 = disabled; > 60 is clamped to 60.
    let wake_period = match config.keep_gamepad_active_secs {
        0 => None,
        secs => Some(std::time::Duration::from_secs(u64::from(secs.min(60)))),
    };
    let mut translator = RumbleTranslator::new(config, haptics);

    // Paces sustained-rumble re-pulsing. The first `tick()` fires
    // immediately and is a harmless no-op (nothing is sustaining).
    let mut ticker = tokio::time::interval(translator.tick_period());
    // Wake-up ticker — pends forever when wake_period is None, so
    // the select arm simply never fires.
    let mut wake_ticker: Option<tokio::time::Interval> = wake_period.map(|p| {
        let mut iv = tokio::time::interval(p);
        // First tick fires immediately by default; skip it so the
        // bridge doesn't pulse the trigger the instant it starts.
        iv.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        iv.reset();
        iv
    });
    if wake_period.is_some() {
        tracing::info!(
            period_secs = wake_period.map(|p| p.as_secs()),
            "haptic redirect: gamepad wake-up enabled"
        );
    }

    // Forwarded input is batched between the proxied controller's
    // SYN_REPORT frames so we don't inject extra sync events.
    let mut fwd_batch: Vec<InputEvent> = Vec::with_capacity(16);

    tracing::info!("haptic redirect: FF servicing loop started");

    loop {
        tokio::select! {
            // Prefer shutdown so teardown is prompt even under a
            // flood of FF events.
            biased;

            _ = &mut shutdown => {
                tracing::debug!("haptic redirect: FF loop received shutdown");
                break;
            }

            _ = ticker.tick() => translator.tick(),

            _ = wake_tick(wake_ticker.as_mut()) => emit_wake_pulse(&mut stream),

            event = stream.next_event() => {
                match event {
                    Ok(event) => {
                        process_event(&mut stream, &mut slots, &mut translator, event)
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "haptic redirect: virtual pad event stream ended"
                        );
                        break;
                    }
                }
            }

            real_event = next_real_event(&mut real_stream) => {
                match real_event {
                    Ok(event) => forward_event(&mut stream, &mut fwd_batch, event),
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "haptic redirect: proxied controller stream ended — \
                             input forwarding stopped"
                        );
                        real_stream = None;
                    }
                }
            }
        }
    }

    tracing::info!("haptic redirect: FF servicing loop stopped");
}

/// Await the next event from the proxied controller. When there is
/// no controller (standalone mode) this pends forever, so the
/// `select!` arm simply never fires.
async fn next_real_event(stream: &mut Option<EventStream>) -> std::io::Result<InputEvent> {
    match stream {
        Some(stream) => stream.next_event().await,
        None => std::future::pending().await,
    }
}

/// Await the next wake-up tick. Pends forever when the wake-up
/// feature is disabled, so the matching `select!` arm never fires.
async fn wake_tick(interval: Option<&mut tokio::time::Interval>) -> tokio::time::Instant {
    match interval {
        Some(iv) => iv.tick().await,
        None => std::future::pending().await,
    }
}

/// Emit a sub-deadzone pulse on the right trigger of the virtual
/// pad, then immediately release it. The pulse value (1 / 255) is
/// well below any "trigger pressed" threshold games use, so this
/// won't fire weapons or zoom — but the event itself is what trips
/// the "last input was a gamepad" heuristic some engines use to
/// gate `FF_UPLOAD`. See design §14.6 and `feedback_haptic_bridge_diagnostics`.
fn emit_wake_pulse(stream: &mut VirtualEventStream) {
    let events = [
        InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_RZ.0, 1),
        InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_RZ.0, 0),
    ];
    if let Err(e) = stream.device_mut().emit(&events) {
        tracing::debug!(error = %e, "haptic redirect: wake pulse emit failed");
    } else {
        tracing::trace!("haptic redirect: gamepad wake pulse emitted");
    }
}

/// Forward one proxied-controller event onto the virtual pad.
///
/// Buttons and axes are batched and flushed on each `SYN_REPORT`
/// (mirroring the source's report framing). A device's *read* side
/// never produces `EV_FF` / `EV_UINPUT`, so only `KEY` + `ABS` are
/// forwarded; `MSC` and the rest are dropped.
fn forward_event(
    vstream: &mut VirtualEventStream,
    batch: &mut Vec<InputEvent>,
    event: InputEvent,
) {
    match event.event_type() {
        EventType::SYNCHRONIZATION => {
            if !batch.is_empty() {
                if let Err(e) = vstream.device_mut().emit(batch.as_slice()) {
                    tracing::debug!(
                        error = %e,
                        "haptic redirect: forwarding emit failed"
                    );
                }
                batch.clear();
            }
        }
        EventType::KEY | EventType::ABSOLUTE => batch.push(event),
        _ => {}
    }
}

/// Handle one event off the virtual pad's stream.
fn process_event(
    stream: &mut VirtualEventStream,
    slots: &mut EffectSlots,
    translator: &mut RumbleTranslator,
    event: InputEvent,
) {
    match event.destructure() {
        // ── A game is uploading (or updating) an FF effect ──────────
        EventSummary::UInput(uinput_event, UInputCode::UI_FF_UPLOAD, _) => {
            // process_ff_upload returns an owned guard; its Drop runs
            // UI_END_FF_UPLOAD, so the syscall is only unparked once
            // the guard falls out of scope below.
            let mut upload = match stream.device_mut().process_ff_upload(uinput_event) {
                Ok(upload) => upload,
                Err(e) => {
                    tracing::warn!(error = %e, "haptic redirect: UI_FF_UPLOAD failed");
                    return;
                }
            };

            let effect = upload.effect();
            let prior_id = upload.effect_id();

            // effect.id >= 0 means the game is updating an effect it
            // already owns — keep the id. -1 means a fresh upload —
            // allocate from the free pool.
            let id = if prior_id >= 0 {
                prior_id as u16
            } else {
                match slots.allocate() {
                    Some(id) => id,
                    None => {
                        tracing::warn!(
                            "haptic redirect: FF effect slots exhausted — rejecting upload"
                        );
                        // -ENOSPC: the game's EVIOCSFF returns the error
                        // and falls back gracefully (usually drops rumble).
                        upload.set_retval(-libc::ENOSPC);
                        return;
                    }
                }
            };

            upload.set_effect_id(id as i16);
            upload.set_retval(0);
            slots.store(id, effect);
            log_upload(id, prior_id >= 0, &effect);
        }

        // ── A game is discarding an FF effect ───────────────────────
        EventSummary::UInput(uinput_event, UInputCode::UI_FF_ERASE, _) => {
            let mut erase = match stream.device_mut().process_ff_erase(uinput_event) {
                Ok(erase) => erase,
                Err(e) => {
                    tracing::warn!(error = %e, "haptic redirect: UI_FF_ERASE failed");
                    return;
                }
            };
            let id = erase.effect_id() as u16;
            erase.set_retval(0);
            slots.release(id);
            translator.on_erase(id);
            tracing::info!(effect_id = id, "haptic redirect: FF effect erased");
        }

        // ── play / stop / gain ──────────────────────────────────────
        // For play/stop the `code` carries the effect id; FF_GAIN and
        // FF_AUTOCENTER are control codes (0x60 / 0x61, well clear of
        // our 0..16 effect-id range).
        EventSummary::ForceFeedback(_, code, value) => {
            if code == FFEffectCode::FF_GAIN {
                tracing::debug!(gain = value, "haptic redirect: FF master gain set");
                translator.on_gain(value);
            } else if code == FFEffectCode::FF_AUTOCENTER {
                tracing::trace!(
                    autocenter = value,
                    "haptic redirect: FF autocenter set (ignored — no wheel)"
                );
            } else {
                let id = code.0;
                if value == 0 {
                    tracing::info!(effect_id = id, "haptic redirect: FF effect stopped");
                    translator.on_stop(id);
                } else {
                    log_play(id, value, slots.get(id));
                    if let Some(effect) = slots.get(id) {
                        translator.on_play(id, effect, value);
                    }
                }
            }
        }

        // Ordinary input events: a standalone virtual pad never
        // emits any, and we don't forward them. Phase 6 (proxy mode)
        // pumps real-controller input through a separate path.
        _ => {}
    }
}

/// Log an FF effect upload, breaking out the fields that matter for
/// the eventual translation (and for the Phase 4 gate — `fftest`
/// should show up here).
fn log_upload(id: u16, is_update: bool, effect: &FFEffectData) {
    let verb = if is_update { "updated" } else { "uploaded" };
    match effect.kind {
        FFEffectKind::Rumble {
            strong_magnitude,
            weak_magnitude,
        } => {
            tracing::info!(
                effect_id = id,
                kind = "rumble",
                strong = strong_magnitude,
                weak = weak_magnitude,
                length_ms = effect.replay.length,
                delay_ms = effect.replay.delay,
                "haptic redirect: FF effect {verb}"
            );
        }
        FFEffectKind::Periodic {
            magnitude, period, ..
        } => {
            tracing::info!(
                effect_id = id,
                kind = "periodic",
                magnitude,
                period_ms = period,
                length_ms = effect.replay.length,
                "haptic redirect: FF effect {verb}"
            );
        }
        other => {
            tracing::info!(
                effect_id = id,
                kind = ?other,
                length_ms = effect.replay.length,
                "haptic redirect: FF effect {verb} (non-rumble — \
                 will be coarsely mapped)"
            );
        }
    }
}

/// Log an FF effect starting to play, including the stored
/// magnitudes when we still have the effect on file.
fn log_play(id: u16, repeat: i32, effect: Option<&FFEffectData>) {
    match effect.map(|e| e.kind) {
        Some(FFEffectKind::Rumble {
            strong_magnitude,
            weak_magnitude,
        }) => {
            tracing::info!(
                effect_id = id,
                repeat,
                strong = strong_magnitude,
                weak = weak_magnitude,
                "haptic redirect: FF effect playing"
            );
        }
        Some(kind) => {
            tracing::info!(
                effect_id = id,
                repeat,
                kind = ?kind,
                "haptic redirect: FF effect playing"
            );
        }
        None => {
            // Play for an effect we never saw uploaded — unusual,
            // but log it rather than silently dropping.
            tracing::warn!(
                effect_id = id,
                repeat,
                "haptic redirect: FF play for an unknown effect id"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_start_all_free() {
        let mut slots = EffectSlots::new();
        // FF_EFFECTS_MAX distinct ids should be allocatable.
        let mut seen = Vec::new();
        while let Some(id) = slots.allocate() {
            seen.push(id);
        }
        assert_eq!(seen.len(), virtual_pad::FF_EFFECTS_MAX as usize);
        // Allocated lowest-first.
        assert_eq!(seen.first(), Some(&0));
        assert_eq!(seen.last(), Some(&(virtual_pad::FF_EFFECTS_MAX as u16 - 1)));
    }

    #[test]
    fn exhausted_pool_returns_none() {
        let mut slots = EffectSlots::new();
        for _ in 0..virtual_pad::FF_EFFECTS_MAX {
            assert!(slots.allocate().is_some());
        }
        assert!(slots.allocate().is_none());
    }

    #[test]
    fn release_returns_id_to_pool() {
        let mut slots = EffectSlots::new();
        // Drain the pool.
        let ids: Vec<u16> = std::iter::from_fn(|| slots.allocate()).collect();
        assert!(slots.allocate().is_none());

        // Releasing one makes exactly that id allocatable again.
        let freed = ids[5];
        slots.release(freed);
        assert_eq!(slots.allocate(), Some(freed));
        assert!(slots.allocate().is_none());
    }

    /// The automated form of the Phase 4 gate ("`fftest` runs
    /// without hanging"): create the virtual pad, run [`serve`],
    /// then upload + play + stop a real `FF_RUMBLE` effect against
    /// it. If the `UI_FF_UPLOAD` handshake is broken the uploading
    /// `EVIOCSFF` syscall hangs forever — the 3-second timeout
    /// turns that hang into a clean test failure.
    ///
    /// Ignored by default: needs `/dev/uinput` write access *and*
    /// access to the created event node. Without the udev rule the
    /// node is `root:input 0660`, so the upload half is skipped
    /// (not failed) on `PermissionDenied`. Run with:
    ///
    /// ```text
    /// cargo test -p oxidemxd --lib -- --ignored ff_upload
    /// ```
    #[tokio::test]
    #[ignore = "creates a real uinput device + uploads FF; needs /dev/uinput access"]
    async fn ff_upload_handshake_completes_without_hanging() {
        use evdev::{Device, FFEffectData, FFEffectKind, FFReplay, FFTrigger};
        use std::io::ErrorKind;
        use std::time::Duration;

        let mut device =
            super::virtual_pad::build_virtual_pad().expect("build virtual pad");
        let node = device
            .enumerate_dev_nodes_blocking()
            .expect("enumerate dev nodes")
            .flatten()
            .next()
            .expect("virtual pad should expose an event node");

        // Run the servicing loop — it owns the device from here.
        let haptics = crate::hidpp::new_shared_haptic_manager(
            &crate::config::HapticConfig::default(),
        );
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let serve_task = tokio::spawn(serve(
            device,
            None, // standalone — no proxied controller
            HapticRedirectConfig::default(),
            haptics,
            shutdown_rx,
        ));

        // Let udev + logind settle the node (the uaccess ACL takes
        // ~1s to land) and the loop reach its select.
        tokio::time::sleep(Duration::from_millis(1500)).await;

        // Sender side: open the node and drive a full upload → play
        // → stop cycle. Runs on the blocking pool because EVIOCSFF
        // is a synchronous syscall (and the one that would hang).
        let node_for_upload = node.clone();
        let upload = tokio::task::spawn_blocking(move || {
            let mut dev = match Device::open(&node_for_upload) {
                Ok(dev) => dev,
                Err(e) if e.kind() == ErrorKind::PermissionDenied => return false,
                Err(e) => panic!("open virtual pad node: {e}"),
            };
            let data = FFEffectData {
                direction: 0,
                trigger: FFTrigger::default(),
                replay: FFReplay {
                    length: 250,
                    delay: 0,
                },
                kind: FFEffectKind::Rumble {
                    strong_magnitude: 0xC000,
                    weak_magnitude: 0x4000,
                },
            };
            let mut effect = dev.upload_ff_effect(data).expect("upload FF effect");
            effect.play(1).expect("play FF effect");
            std::thread::sleep(Duration::from_millis(50));
            effect.stop().expect("stop FF effect");
            true
        });

        let outcome = tokio::time::timeout(Duration::from_secs(3), upload).await;

        // Tear the servicing loop down regardless of the outcome.
        drop(shutdown_tx);
        let _ = tokio::time::timeout(Duration::from_secs(2), serve_task).await;

        match outcome {
            Err(_) => {
                panic!("FF upload hung — the UI_FF_UPLOAD handshake is broken")
            }
            Ok(joined) => match joined.expect("upload task panicked") {
                true => { /* handshake completed end-to-end */ }
                false => eprintln!(
                    "skipped the FF upload half: no access to the event node. \
                     Install packaging/udev/70-oxidemx-haptic-pad.rules."
                ),
            },
        }
    }

    /// Manual feel-test of the whole Phase 3-5 chain against the
    /// **real MX Master 4**: create the virtual pad, run [`serve`]
    /// with a live [`HapticManager`], then upload four escalating
    /// rumble effects + one continuous buzz. A human confirms the
    /// mouse clicks with rising intensity.
    ///
    /// Skips cleanly if no MX Master 4 is connected or the event
    /// node isn't accessible. Run with:
    ///
    /// ```text
    /// cargo test -p oxidemxd --lib -- --ignored --nocapture live_haptic_feel
    /// ```
    #[tokio::test]
    #[ignore = "drives the real MX Master 4 haptic actuator; needs the device + /dev/uinput"]
    async fn live_haptic_feel_walkthrough() {
        use evdev::{Device, FFEffectData, FFEffectKind, FFReplay, FFTrigger};
        use oxidemx_shared::HapticRedirectCurve;
        use std::io::ErrorKind;
        use std::time::Duration;

        // Connect a real haptic manager to the MX Master 4.
        let haptics = crate::hidpp::new_shared_haptic_manager(
            &crate::config::HapticConfig::default(),
        );
        {
            let mut hm = haptics.lock().unwrap();
            let connected = hm.connect().unwrap_or(false);
            if !connected {
                eprintln!("skipped: no MX Master 4 found — nothing to feel.");
                return;
            }
            hm.set_enabled(true);
        }

        let mut device =
            super::virtual_pad::build_virtual_pad().expect("build virtual pad");
        let node = device
            .enumerate_dev_nodes_blocking()
            .expect("enumerate dev nodes")
            .flatten()
            .next()
            .expect("virtual pad should expose an event node");

        // Linear curve so magnitude maps straight to tier — makes the
        // four steps land predictably.
        let config = HapticRedirectConfig {
            curve: HapticRedirectCurve::Linear,
            ..Default::default()
        };
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let serve_task = tokio::spawn(serve(device, None, config, haptics, shutdown_rx));

        tokio::time::sleep(Duration::from_millis(1500)).await;

        let node_for_upload = node.clone();
        let ran = tokio::task::spawn_blocking(move || {
            let mut dev = match Device::open(&node_for_upload) {
                Ok(dev) => dev,
                Err(e) if e.kind() == ErrorKind::PermissionDenied => return false,
                Err(e) => panic!("open virtual pad node: {e}"),
            };

            let rumble = |strong: u16, weak: u16, length: u16| FFEffectData {
                direction: 0,
                trigger: FFTrigger::default(),
                replay: FFReplay { length, delay: 0 },
                kind: FFEffectKind::Rumble {
                    strong_magnitude: strong,
                    weak_magnitude: weak,
                },
            };

            // Four single clicks (length 1 ms → one pulse each),
            // chosen to land in each tier under the Linear curve.
            let tiers = [
                ("Whisper — faint tick", 0x2000u16),
                ("Subtle — light click", 0x5A00),
                ("Damp   — medium thunk", 0xA600),
                ("Sharp  — strong knock", 0xF300),
            ];
            for (label, strong) in tiers {
                eprintln!(">>> {label}  (expect ONE haptic pulse now)");
                let mut effect = dev
                    .upload_ff_effect(rumble(strong, 0, 1))
                    .expect("upload FF effect");
                effect.play(1).expect("play FF effect");
                std::thread::sleep(Duration::from_millis(900));
            }

            // One continuous effect — sustained re-pulsing for ~1 s.
            eprintln!(">>> Continuous rumble  (expect a ~1 s buzz now)");
            let mut effect = dev
                .upload_ff_effect(rumble(0xC000, 0x4000, 0))
                .expect("upload continuous effect");
            effect.play(1).expect("play continuous effect");
            std::thread::sleep(Duration::from_millis(1000));
            effect.stop().expect("stop continuous effect");
            true
        });

        let outcome = tokio::time::timeout(Duration::from_secs(20), ran).await;
        drop(shutdown_tx);
        let _ = tokio::time::timeout(Duration::from_secs(2), serve_task).await;

        match outcome {
            Err(_) => panic!("feel-test hung"),
            Ok(joined) => match joined.expect("upload task panicked") {
                true => eprintln!(
                    "<<< feel-test finished — you should have felt 4 rising \
                     clicks then a buzz."
                ),
                false => eprintln!(
                    "skipped: no access to the event node — install \
                     packaging/udev/70-oxidemx-haptic-pad.rules."
                ),
            },
        }
    }

    /// Build a second virtual uinput gamepad to stand in for a real
    /// controller — a "simulated Xbox 360 pad". Same buttons/axes
    /// as our bridge pad, but a distinct name and no force feedback.
    fn build_simulated_controller() -> evdev::uinput::VirtualDevice {
        use evdev::{uinput::VirtualDevice, AttributeSet, KeyCode};

        let keys: AttributeSet<KeyCode> =
            virtual_pad::BUTTONS.iter().copied().collect();
        let mut builder = VirtualDevice::builder()
            .expect("uinput builder")
            .name("Simulated Xbox 360 pad")
            .with_keys(&keys)
            .expect("with_keys");
        for setup in virtual_pad::abs_setups() {
            builder = builder
                .with_absolute_axis(&setup)
                .expect("with_absolute_axis");
        }
        builder.build().expect("build simulated controller")
    }

    /// Proxy-mode forwarding, end to end, with **no physical
    /// controller** — a second virtual uinput device stands in for
    /// one. Emits a button press + a stick move on the simulated
    /// controller and asserts both arrive on the bridge pad.
    ///
    /// Ignored by default: creates two uinput devices and needs
    /// access to their event nodes (the udev rule grants ours; a
    /// generic joystick gets default-seat `uaccess`). Skips cleanly
    /// on `PermissionDenied`. Run with:
    ///
    /// ```text
    /// cargo test -p oxidemxd --lib -- --ignored --nocapture proxy_forwards
    /// ```
    #[tokio::test]
    #[ignore = "creates two uinput devices; needs /dev/uinput + event-node access"]
    async fn proxy_forwards_simulated_controller_input() {
        use evdev::{
            AbsoluteAxisCode, Device, EventType, InputEvent, KeyCode,
        };
        use std::io::ErrorKind;
        use std::time::{Duration, Instant};

        // 1. The simulated controller, and the bridge pad.
        let mut sim = build_simulated_controller();
        let sim_node = sim
            .enumerate_dev_nodes_blocking()
            .expect("enumerate sim nodes")
            .flatten()
            .next()
            .expect("simulated controller event node");

        let mut bridge = virtual_pad::build_virtual_pad().expect("build bridge pad");
        let bridge_node = bridge
            .enumerate_dev_nodes_blocking()
            .expect("enumerate bridge nodes")
            .flatten()
            .next()
            .expect("bridge pad event node");

        // 2. Let udev + logind settle both nodes' permissions.
        tokio::time::sleep(Duration::from_millis(1500)).await;

        // 3. Open the simulated controller as the "real" device the
        //    proxy wraps — exactly what discover_controller() hands
        //    back, grab included.
        let mut real = match Device::open(&sim_node) {
            Ok(dev) => dev,
            Err(e) if e.kind() == ErrorKind::PermissionDenied => {
                eprintln!(
                    "skipped: no access to the simulated controller node — \
                     install the udev rule / check seat uaccess."
                );
                return;
            }
            Err(e) => panic!("open simulated controller: {e}"),
        };
        let _ = real.grab(); // best-effort, mirrors discover_controller

        // 4. A reader on the bridge pad — this is what a game sees.
        let bridge_reader = match Device::open(&bridge_node) {
            Ok(dev) => dev,
            Err(e) if e.kind() == ErrorKind::PermissionDenied => {
                eprintln!("skipped: no access to the bridge pad node.");
                return;
            }
            Err(e) => panic!("open bridge pad: {e}"),
        };
        let mut bridge_stream =
            bridge_reader.into_event_stream().expect("bridge event stream");

        // 5. Run the bridge in proxy mode over the simulated pad.
        let haptics = crate::hidpp::new_shared_haptic_manager(
            &crate::config::HapticConfig::default(),
        );
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let serve_task = tokio::spawn(serve(
            bridge,
            Some(real),
            HapticRedirectConfig::default(),
            haptics,
            shutdown_rx,
        ));
        tokio::time::sleep(Duration::from_millis(400)).await;

        // 6. Press BTN_SOUTH and shove the left stick on the
        //    simulated controller. emit() appends the SYN_REPORT.
        sim.emit(&[
            InputEvent::new(EventType::KEY.0, KeyCode::BTN_SOUTH.0, 1),
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, 12345),
        ])
        .expect("emit on simulated controller");

        // 7. Collect what reaches the bridge pad (a short window).
        let mut collected: Vec<InputEvent> = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            match tokio::time::timeout(
                Duration::from_millis(250),
                bridge_stream.next_event(),
            )
            .await
            {
                Ok(Ok(event)) => collected.push(event),
                Ok(Err(_)) => break,
                Err(_) if !collected.is_empty() => break, // quiet gap
                Err(_) => {}
            }
        }

        // 8. Tear down before asserting.
        drop(shutdown_tx);
        let _ = tokio::time::timeout(Duration::from_secs(2), serve_task).await;

        let forwarded_button = collected.iter().any(|e| {
            e.event_type() == EventType::KEY
                && e.code() == KeyCode::BTN_SOUTH.0
                && e.value() == 1
        });
        let forwarded_axis = collected.iter().any(|e| {
            e.event_type() == EventType::ABSOLUTE
                && e.code() == AbsoluteAxisCode::ABS_X.0
                && e.value() == 12345
        });

        assert!(
            forwarded_button,
            "BTN_SOUTH press should forward to the bridge pad (got {collected:?})"
        );
        assert!(
            forwarded_axis,
            "ABS_X move should forward to the bridge pad (got {collected:?})"
        );
        eprintln!("proxy forwarding verified — button + axis reached the bridge pad");
    }

    #[test]
    fn store_and_get_round_trip() {
        use evdev::{FFEffectData, FFEffectKind, FFReplay, FFTrigger};

        let mut slots = EffectSlots::new();
        let id = slots.allocate().unwrap();
        let effect = FFEffectData {
            direction: 0,
            trigger: FFTrigger::default(),
            replay: FFReplay {
                length: 250,
                delay: 0,
            },
            kind: FFEffectKind::Rumble {
                strong_magnitude: 0xC000,
                weak_magnitude: 0x4000,
            },
        };
        slots.store(id, effect);

        match slots.get(id).map(|e| e.kind) {
            Some(FFEffectKind::Rumble {
                strong_magnitude,
                weak_magnitude,
            }) => {
                assert_eq!(strong_magnitude, 0xC000);
                assert_eq!(weak_magnitude, 0x4000);
            }
            other => panic!("expected stored rumble effect, got {other:?}"),
        }

        // After release the data is gone.
        slots.release(id);
        assert!(slots.get(id).is_none());
    }
}
