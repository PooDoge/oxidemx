//! Network rate widget — bundled built-in (spec §16).
//!
//! Fed by the host's `Event::SystemStats` push (the `system-stats`
//! permission). Reproduces the native Network wedge typography from
//! `overlay-rs/src/render/slices/widgets.rs::draw_widget_wedge`:
//! big "84↓" (download Mbit/s, rounded), sublabel "12↑ Mb/s",
//! uppercase "NETWORK" label, and a 30-sample sparkline of the
//! download rate.

use std::collections::VecDeque;

use oxidemx_widget_api::{tile, Ctx, Event, Scene, WedgeGeom, Widget};

/// Sparkline sample count — matches the native wedge's SPARK_LEN.
const SPARK_LEN: usize = 30;

#[derive(Default)]
pub struct Network {
    down_mbps: Option<f32>,
    up_mbps: Option<f32>,
    /// Raw download-rate samples (Mbit/s), newest last, capped at
    /// [`SPARK_LEN`] — the native wedge sparklines download only.
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
/// max-relative scaling as the native wedge (max floored at 1.0).
fn normalised(history: &VecDeque<f32>) -> Vec<f32> {
    let max = history.iter().copied().fold(1.0_f32, f32::max);
    history.iter().map(|v| (v / max).clamp(0.0, 1.0)).collect()
}

/// Big value: "84↓" (rounded) or the native "—" placeholder before
/// the first delta-bearing push.
fn fmt_value(down_mbps: Option<f32>) -> String {
    down_mbps.map(|d| format!("{}↓", d.round() as u32)).unwrap_or_else(|| "—".into())
}

/// Sublabel: "12↑ Mb/s" (rounded) or "" before data.
fn fmt_sublabel(up_mbps: Option<f32>) -> String {
    up_mbps.map(|u| format!("{}↑ Mb/s", u.round() as u32)).unwrap_or_default()
}

impl Widget for Network {
    fn init(&mut self, ctx: &Ctx) {
        self.show_sparkline = ctx.setting_bool("show_sparkline").unwrap_or(true);
    }

    fn on_event(&mut self, ev: Event, _ctx: &Ctx) -> bool {
        match ev {
            Event::SystemStats(snap) => {
                self.down_mbps = snap.net_down_mbps;
                self.up_mbps = snap.net_up_mbps;
                if let Some(d) = snap.net_down_mbps {
                    push_capped(&mut self.history, d);
                }
                true
            }
            _ => false,
        }
    }

    fn render(&self, geom: WedgeGeom) -> Scene {
        let mut t = tile().value(fmt_value(self.down_mbps)).label("NETWORK");
        if self.show_sparkline && self.history.len() >= 2 {
            t = t.sparkline(&normalised(&self.history));
        }
        let sub = fmt_sublabel(self.up_mbps);
        if !sub.is_empty() {
            t = t.sublabel(sub);
        }
        t.into_scene(geom)
    }
}

oxidemx_widget_api::register_widget!(Network);

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
        assert_eq!(fmt_value(Some(84.3)), "84↓");
        assert_eq!(fmt_value(Some(0.4)), "0↓");
        assert_eq!(fmt_value(None), "—");
    }

    #[test]
    fn sublabel_formats_like_the_native_wedge() {
        assert_eq!(fmt_sublabel(Some(11.7)), "12↑ Mb/s");
        assert_eq!(fmt_sublabel(None), "");
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
    fn stats_event_updates_state_and_requests_render() {
        let mut w = Network { show_sparkline: true, ..Default::default() };
        let ctx = Ctx::new(vec![]);
        let mut snap = SystemStatsSnapshot::default();
        snap.net_down_mbps = Some(84.0);
        snap.net_up_mbps = Some(12.0);
        assert!(w.on_event(Event::SystemStats(snap.clone()), &ctx));
        assert!(w.on_event(Event::SystemStats(snap), &ctx));

        let scene = w.render(geom());
        assert_eq!(texts(&scene), vec!["84↓", "12↑ Mb/s", "NETWORK"]);
        assert!(
            scene.prims.iter().any(|p| matches!(p, Prim::Sparkline { .. })),
            "two samples in history → sparkline drawn"
        );
    }

    #[test]
    fn sparkline_respects_option() {
        let mut w = Network { show_sparkline: false, ..Default::default() };
        push_capped(&mut w.history, 10.0);
        push_capped(&mut w.history, 20.0);
        let scene = w.render(geom());
        assert!(!scene.prims.iter().any(|p| matches!(p, Prim::Sparkline { .. })));
        assert_eq!(normalised(&w.history), vec![0.5, 1.0]);
    }

    #[test]
    fn placeholder_before_first_push() {
        let w = Network { show_sparkline: true, ..Default::default() };
        assert_eq!(texts(&w.render(geom())), vec!["—", "NETWORK"]);
    }
}
