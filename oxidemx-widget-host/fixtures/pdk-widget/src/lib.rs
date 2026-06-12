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
//! - `mode` = `"stats"` (Str): SystemStats pushes update the rendered
//!   value to `cpu_pct` (rounded; `"--"` until the first push) — this is
//!   how the host tests observe the system-stats feed.

use oxidemx_widget_api::{tile, Ctx, Event, Scene, WedgeGeom, Widget};

#[derive(Default)]
pub struct PdkWidget {
    count: u32,
    echo: Option<String>,
    http_status: Option<u16>,
    http_body: Option<String>,
    stats_mode: bool,
    cpu_pct: Option<f32>,
}

impl Widget for PdkWidget {
    fn init(&mut self, ctx: &Ctx) {
        ctx.log("pdk-widget init");
        ctx.set_timer("refresh", 60);
        self.echo = ctx.setting_str("echo").map(str::to_string);
        self.stats_mode = ctx.setting_str("mode") == Some("stats");
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
            Event::SystemStats(snap) if self.stats_mode => {
                self.cpu_pct = snap.cpu_pct;
                true
            }
            _ => false,
        }
    }

    fn render(&self, geom: WedgeGeom) -> Scene {
        let value = if self.stats_mode {
            self.cpu_pct.map(|v| format!("{v:.0}")).unwrap_or_else(|| "--".into())
        } else if let Some(echo) = &self.echo {
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
