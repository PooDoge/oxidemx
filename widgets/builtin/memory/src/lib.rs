//! Memory widget — bundled built-in (spec §16).
//!
//! Fed by the host's `Event::SystemStats` push (the `system-stats`
//! permission). Reproduces the native Memory wedge typography from
//! `overlay-rs/src/render/slices/widgets.rs::draw_widget_wedge`:
//! big "11.2" (used GiB, one decimal), sublabel "of 32 GB",
//! uppercase "MEMORY" label. No options.

use oxidemx_widget_api::{tile, Ctx, Event, Scene, WedgeGeom, Widget};

#[derive(Default)]
pub struct Memory {
    used_gb: Option<f32>,
    total_gb: Option<f32>,
}

/// Big value: used GiB with one decimal, or the native "—" placeholder.
fn fmt_value(used_gb: Option<f32>) -> String {
    used_gb.map(|u| format!("{u:.1}")).unwrap_or_else(|| "—".into())
}

/// Sublabel: "of 32 GB" (rounded total) or "" before data.
fn fmt_sublabel(total_gb: Option<f32>) -> String {
    total_gb.map(|t| format!("of {} GB", t.round() as u32)).unwrap_or_default()
}

impl Widget for Memory {
    fn init(&mut self, _ctx: &Ctx) {}

    fn on_event(&mut self, ev: Event, _ctx: &Ctx) -> bool {
        match ev {
            Event::SystemStats(snap) => {
                self.used_gb = snap.mem_used_gb;
                self.total_gb = snap.mem_total_gb;
                true
            }
            _ => false,
        }
    }

    fn render(&self, geom: WedgeGeom) -> Scene {
        let mut t = tile().value(fmt_value(self.used_gb)).label("MEMORY");
        let sub = fmt_sublabel(self.total_gb);
        if !sub.is_empty() {
            t = t.sublabel(sub);
        }
        t.into_scene(geom)
    }
}

oxidemx_widget_api::register_widget!(Memory);

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
        assert_eq!(fmt_value(Some(11.23)), "11.2");
        assert_eq!(fmt_value(Some(8.0)), "8.0");
        assert_eq!(fmt_value(None), "—");
    }

    #[test]
    fn sublabel_formats_like_the_native_wedge() {
        assert_eq!(fmt_sublabel(Some(31.8)), "of 32 GB");
        assert_eq!(fmt_sublabel(Some(16.2)), "of 16 GB");
        assert_eq!(fmt_sublabel(None), "");
    }

    #[test]
    fn stats_event_updates_state_and_requests_render() {
        let mut w = Memory::default();
        let ctx = Ctx::new(vec![]);
        let mut snap = SystemStatsSnapshot::default();
        snap.mem_used_gb = Some(11.2);
        snap.mem_total_gb = Some(32.0);
        assert!(w.on_event(Event::SystemStats(snap), &ctx));
        assert_eq!(texts(&w.render(geom())), vec!["11.2", "of 32 GB", "MEMORY"]);
    }

    #[test]
    fn placeholder_before_first_push() {
        let w = Memory::default();
        assert_eq!(texts(&w.render(geom())), vec!["—", "MEMORY"]);
    }
}
