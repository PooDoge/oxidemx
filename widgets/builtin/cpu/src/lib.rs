//! CPU usage widget — bundled built-in (spec §16).
//!
//! Fed entirely by the host's `Event::SystemStats` push (the
//! `system-stats` permission): 1 Hz while the menu is open plus one
//! push on MenuOpened. No timers, no HTTP. The rendered typography
//! reproduces the native CPU wedge from
//! `overlay-rs/src/render/slices/widgets.rs::draw_widget_wedge`
//! exactly: big "23%", sublabel "8 cores · 52°C", uppercase "CPU"
//! label, and a 30-sample sparkline of recent load.

use std::collections::VecDeque;

use oxidemx_widget_api::{tile, Ctx, Event, Scene, WedgeGeom, Widget};

/// Sparkline sample count — matches the native wedge's SPARK_LEN
/// (~30 s of history at the 1 s stats cadence).
const SPARK_LEN: usize = 30;

#[derive(Default)]
pub struct Cpu {
    pct: Option<f32>,
    cores: Option<u32>,
    temp_c: Option<f32>,
    /// Raw cpu_pct samples (0..=100), newest last, capped at SPARK_LEN.
    history: VecDeque<f32>,
    show_sparkline: bool,
}

/// Push a sample, dropping the oldest beyond [`SPARK_LEN`].
fn push_capped(history: &mut VecDeque<f32>, v: f32) {
    history.push_back(v);
    while history.len() > SPARK_LEN {
        history.pop_front();
    }
}

/// Normalise raw samples to tile()'s 0..=1 sparkline space — the same
/// max-relative scaling as the native wedge (max floored at 1.0 so a
/// flat near-zero series doesn't blow up to full height).
fn normalised(history: &VecDeque<f32>) -> Vec<f32> {
    let max = history.iter().copied().fold(1.0_f32, f32::max);
    history.iter().map(|v| (v / max).clamp(0.0, 1.0)).collect()
}

/// Big value: "23%" (rounded) or the native "—" placeholder before the
/// first delta-bearing push (cpu_pct is None on the very first sample).
fn fmt_value(pct: Option<f32>) -> String {
    pct.map(|c| format!("{}%", c.round() as u32)).unwrap_or_else(|| "—".into())
}

/// Sublabel: "8 cores · 52°C" / "8 cores" / "" — native match arms.
fn fmt_sublabel(cores: Option<u32>, temp_c: Option<f32>) -> String {
    match (cores.unwrap_or(0), temp_c) {
        (n, Some(t)) if n > 0 => format!("{n} cores · {}°C", t.round() as i32),
        (n, None) if n > 0 => format!("{n} cores"),
        _ => String::new(),
    }
}

impl Widget for Cpu {
    fn init(&mut self, ctx: &Ctx) {
        self.show_sparkline = ctx.setting_bool("show_sparkline").unwrap_or(true);
    }

    fn on_event(&mut self, ev: Event, _ctx: &Ctx) -> bool {
        match ev {
            Event::SystemStats(snap) => {
                self.pct = snap.cpu_pct;
                self.cores = snap.cpu_cores;
                self.temp_c = snap.cpu_temp_c;
                if let Some(p) = snap.cpu_pct {
                    push_capped(&mut self.history, p);
                }
                true
            }
            _ => false,
        }
    }

    fn render(&self, geom: WedgeGeom) -> Scene {
        let mut t = tile().value(fmt_value(self.pct)).label("CPU");
        if self.show_sparkline && self.history.len() >= 2 {
            t = t.sparkline(&normalised(&self.history));
        }
        let sub = fmt_sublabel(self.cores, self.temp_c);
        if !sub.is_empty() {
            t = t.sublabel(sub);
        }
        t.into_scene(geom)
    }
}

oxidemx_widget_api::register_widget!(Cpu);

#[cfg(test)]
mod tests {
    use super::*;
    use oxidemx_widget_api::proto::{Prim, SystemStatsSnapshot};

    fn geom() -> WedgeGeom {
        WedgeGeom {
            width: 200.0,
            height: 160.0,
            inner_radius: 60.0,
            outer_radius: 160.0,
            angle_start: 0.0,
            angle_end: 0.785,
            hovered: 0.0,
        }
    }

    fn texts(scene: &Scene) -> Vec<&str> {
        scene
            .prims
            .iter()
            .filter_map(|p| match p {
                Prim::Text { content, .. } => Some(content.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn value_formats_like_the_native_wedge() {
        assert_eq!(fmt_value(Some(23.4)), "23%");
        assert_eq!(fmt_value(Some(99.6)), "100%");
        assert_eq!(fmt_value(Some(0.0)), "0%");
        assert_eq!(fmt_value(None), "—");
    }

    #[test]
    fn sublabel_formats_like_the_native_wedge() {
        assert_eq!(fmt_sublabel(Some(8), Some(51.7)), "8 cores · 52°C");
        assert_eq!(fmt_sublabel(Some(8), None), "8 cores");
        assert_eq!(fmt_sublabel(Some(0), Some(50.0)), "");
        assert_eq!(fmt_sublabel(None, None), "");
    }

    #[test]
    fn history_caps_at_thirty_samples() {
        let mut h = VecDeque::new();
        for i in 0..40 {
            push_capped(&mut h, i as f32);
        }
        assert_eq!(h.len(), SPARK_LEN);
        assert_eq!(h.front().copied(), Some(10.0), "oldest samples dropped");
        assert_eq!(h.back().copied(), Some(39.0));
    }

    #[test]
    fn normalised_scales_by_max_with_unit_floor() {
        let h: VecDeque<f32> = [25.0, 50.0, 100.0].into_iter().collect();
        assert_eq!(normalised(&h), vec![0.25, 0.5, 1.0]);
        // Max floored at 1.0: tiny values stay tiny instead of filling
        // the band.
        let low: VecDeque<f32> = [0.2, 0.4].into_iter().collect();
        assert_eq!(normalised(&low), vec![0.2, 0.4]);
    }

    #[test]
    fn stats_event_updates_state_and_requests_render() {
        let mut w = Cpu { show_sparkline: true, ..Default::default() };
        let ctx = Ctx::new(vec![]);
        let mut snap = SystemStatsSnapshot::default();
        snap.cpu_pct = Some(23.0);
        snap.cpu_cores = Some(8);
        snap.cpu_temp_c = Some(52.0);
        assert!(w.on_event(Event::SystemStats(snap.clone()), &ctx));
        snap.cpu_pct = Some(42.0);
        assert!(w.on_event(Event::SystemStats(snap), &ctx));

        let scene = w.render(geom());
        let t = texts(&scene);
        assert_eq!(t, vec!["42%", "8 cores · 52°C", "CPU"]);
        assert!(
            scene.prims.iter().any(|p| matches!(p, Prim::Sparkline { .. })),
            "two samples in history → sparkline drawn"
        );
    }

    #[test]
    fn sparkline_respects_option_and_sample_minimum() {
        let mut w = Cpu { show_sparkline: false, ..Default::default() };
        push_capped(&mut w.history, 10.0);
        push_capped(&mut w.history, 20.0);
        let scene = w.render(geom());
        assert!(!scene.prims.iter().any(|p| matches!(p, Prim::Sparkline { .. })));

        // Enabled but only one sample → still no sparkline (native
        // wedge requires len >= 2).
        let mut w = Cpu { show_sparkline: true, ..Default::default() };
        push_capped(&mut w.history, 10.0);
        let scene = w.render(geom());
        assert!(!scene.prims.iter().any(|p| matches!(p, Prim::Sparkline { .. })));
    }

    #[test]
    fn placeholder_before_first_push() {
        let w = Cpu { show_sparkline: true, ..Default::default() };
        let scene = w.render(geom());
        assert_eq!(texts(&scene), vec!["—", "CPU"], "no sublabel before data");
    }
}
