//! Disk free widget — bundled built-in (spec §16).
//!
//! Fed by the host's `Event::SystemStats` push (the `system-stats`
//! permission); the host samples the user's data filesystem (/home
//! first) globally, so this widget takes no path option in v1.
//! Reproduces the native Disk wedge typography from
//! `overlay-rs/src/render/slices/widgets.rs::draw_widget_wedge`:
//! big "412" (free decimal GB, rounded), sublabel "GB free",
//! uppercase "DISK" label.

use oxidemx_widget_api::{tile, Ctx, Event, Scene, WedgeGeom, Widget};

#[derive(Default)]
pub struct Disk {
    free_gb: Option<f32>,
}

/// Big value: rounded free GB or the native "—" placeholder.
fn fmt_value(free_gb: Option<f32>) -> String {
    free_gb.map(|f| format!("{}", f.round() as u32)).unwrap_or_else(|| "—".into())
}

impl Widget for Disk {
    fn init(&mut self, _ctx: &Ctx) {}

    fn on_event(&mut self, ev: Event, _ctx: &Ctx) -> bool {
        match ev {
            Event::SystemStats(snap) => {
                self.free_gb = snap.disk_free_gb;
                true
            }
            _ => false,
        }
    }

    fn render(&self, geom: WedgeGeom) -> Scene {
        // The native wedge keeps the "GB free" sublabel even before
        // data arrives.
        tile()
            .value(fmt_value(self.free_gb))
            .sublabel("GB free")
            .label("DISK")
            .into_scene(geom)
    }
}

oxidemx_widget_api::register_widget!(Disk);

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
        assert_eq!(fmt_value(Some(412.4)), "412");
        assert_eq!(fmt_value(Some(0.2)), "0");
        assert_eq!(fmt_value(None), "—");
    }

    #[test]
    fn stats_event_updates_state_and_requests_render() {
        let mut w = Disk::default();
        let ctx = Ctx::new(vec![]);
        let mut snap = SystemStatsSnapshot::default();
        snap.disk_free_gb = Some(412.0);
        assert!(w.on_event(Event::SystemStats(snap), &ctx));
        assert_eq!(texts(&w.render(geom())), vec!["412", "GB free", "DISK"]);
    }

    #[test]
    fn placeholder_before_first_push() {
        let w = Disk::default();
        assert_eq!(texts(&w.render(geom())), vec!["—", "GB free", "DISK"]);
    }
}
