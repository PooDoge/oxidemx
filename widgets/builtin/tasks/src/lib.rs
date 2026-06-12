//! Tasks-due widget — bundled built-in (spec §16).
//!
//! Fed by the host's `Event::SystemStats` push (the `system-stats`
//! permission); the host counts scheduled `oxidemx-task-*` systemd
//! timers due within 24 h. Reproduces the native TasksDue wedge
//! typography from
//! `overlay-rs/src/render/slices/widgets.rs::draw_widget_wedge`:
//! big "3", sublabel "due in 24h", uppercase "TASKS" label —
//! "—" / "scheduled tasks" before data.

use oxidemx_widget_api::{tile, Ctx, Event, Scene, WedgeGeom, Widget};

#[derive(Default)]
pub struct Tasks {
    due: Option<u32>,
}

/// `(big value, sublabel)` — the native wedge's TasksDue match arms.
fn fmt(due: Option<u32>) -> (String, &'static str) {
    match due {
        Some(n) => (n.to_string(), "due in 24h"),
        None => ("—".into(), "scheduled tasks"),
    }
}

impl Widget for Tasks {
    fn init(&mut self, _ctx: &Ctx) {}

    fn on_event(&mut self, ev: Event, _ctx: &Ctx) -> bool {
        match ev {
            Event::SystemStats(snap) => {
                self.due = snap.tasks_due;
                true
            }
            _ => false,
        }
    }

    fn render(&self, geom: WedgeGeom) -> Scene {
        let (value, sub) = fmt(self.due);
        tile().value(value).sublabel(sub).label("TASKS").into_scene(geom)
    }
}

oxidemx_widget_api::register_widget!(Tasks);

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
        assert_eq!(fmt(Some(3)), ("3".to_string(), "due in 24h"));
        assert_eq!(fmt(Some(0)), ("0".to_string(), "due in 24h"));
        assert_eq!(fmt(None), ("—".to_string(), "scheduled tasks"));
    }

    #[test]
    fn stats_event_updates_state_and_requests_render() {
        let mut w = Tasks::default();
        let ctx = Ctx::new(vec![]);
        let mut snap = SystemStatsSnapshot::default();
        snap.tasks_due = Some(3);
        assert!(w.on_event(Event::SystemStats(snap), &ctx));
        assert_eq!(texts(&w.render(geom())), vec!["3", "due in 24h", "TASKS"]);
    }

    #[test]
    fn placeholder_before_first_push() {
        let w = Tasks::default();
        assert_eq!(texts(&w.render(geom())), vec!["—", "scheduled tasks", "TASKS"]);
    }
}
