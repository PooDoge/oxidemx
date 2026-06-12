//! PDK-based fixture widget. Uses `oxidemx-widget-api` to prove that the
//! host can load and run PDK-built widgets end-to-end.
//!
//! Base behaviour (no settings — keeps `pdk_widget_round_trips` green):
//! - init: logs a message and sets a 60s refresh timer
//! - on_event: Click increments a counter (needs render), Timer needs render
//! - render: 3-prim tile() scene with the click count as the value
//!
//! Settings-driven extras for the worker tests (Task 5):
//! - `echo` (Str): render shows it as the big value instead of the count
//! - `timer_secs` (Num): init sets an extra timer `"t"` with that period
//! - `fetch_url` (Str): init issues `http_get("h", url)`; the response is
//!   re-rendered as value `h<status>` with the body as the sublabel — this
//!   is how the host tests observe permission denials and cache hits.

use oxidemx_widget_api::{tile, Ctx, Widget};
use oxidemx_widget_proto::{Event, Scene, WedgeGeom};

#[derive(Default)]
pub struct PdkWidget {
    count: u32,
    echo: Option<String>,
    http_status: Option<u16>,
    http_body: Option<String>,
}

impl Widget for PdkWidget {
    fn init(&mut self, ctx: &Ctx) {
        ctx.log("pdk-widget init");
        ctx.set_timer("refresh", 60);
        self.echo = ctx.setting_str("echo").map(str::to_string);
        if let Some(secs) = ctx.setting_u64("timer_secs") {
            ctx.set_timer("t", secs);
        }
        if let Some(url) = ctx.setting_str("fetch_url") {
            ctx.http_get("h", url);
        }
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
            Event::HttpResponse { id, status, body } if id == "h" => {
                self.http_status = Some(status);
                self.http_body = Some(String::from_utf8_lossy(&body).into_owned());
                true
            }
            _ => false,
        }
    }

    fn render(&self, geom: WedgeGeom) -> Scene {
        let value = if let Some(echo) = &self.echo {
            echo.clone()
        } else if let Some(status) = self.http_status {
            format!("h{status}")
        } else {
            format!("{}", self.count)
        };
        let sublabel = self.http_body.clone().unwrap_or_else(|| "clicks".into());
        tile()
            .value(&value)
            .sublabel(&sublabel)
            .label("PDK")
            .into_scene(geom)
    }
}

oxidemx_widget_api::register_widget!(PdkWidget);
