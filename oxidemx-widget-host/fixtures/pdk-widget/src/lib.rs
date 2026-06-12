//! PDK-based fixture widget. Uses `oxidemx-widget-api` to prove that the
//! host can load and run PDK-built widgets end-to-end.
//!
//! Behaviour (ok-only):
//! - init: logs a message and sets a refresh timer
//! - on_event: returns true for Click (needs render), false for everything else
//! - render: returns a tile() scene

use oxidemx_widget_api::{tile, Ctx, Widget};
use oxidemx_widget_proto::{Event, Scene, WedgeGeom};

#[derive(Default)]
pub struct PdkWidget {
    count: u32,
}

impl Widget for PdkWidget {
    fn init(&mut self, ctx: &Ctx) {
        ctx.log("pdk-widget init");
        ctx.set_timer("refresh", 60);
    }

    fn on_event(&mut self, ev: Event, ctx: &Ctx) -> bool {
        match ev {
            Event::Click => {
                self.count += 1;
                ctx.log(&format!("click #{}", self.count));
                true // needs render
            }
            Event::Timer(_) => {
                ctx.log("timer fired");
                true
            }
            _ => false,
        }
    }

    fn render(&self, geom: WedgeGeom) -> Scene {
        tile()
            .value(&format!("{}", self.count))
            .sublabel("clicks")
            .label("PDK")
            .into_scene(geom)
    }
}

oxidemx_widget_api::register_widget!(PdkWidget);
