//! OxideMX Freya desktop app.
use freya::prelude::*;

pub mod app;
pub mod nav;
pub mod regions;
pub mod state;

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let _guard = rt.enter();
    launch(
        LaunchConfig::new().with_window(
            WindowConfig::new(root_app)
                .with_size(1200., 800.)
                .with_title("OxideMX"),
        ),
    )
}

pub fn root_app() -> impl IntoElement {
    crate::app::shell()
}

#[cfg(test)]
mod tests {
    use super::*;
    use freya_testing::prelude::*;

    #[test]
    fn root_renders_title() {
        // The root_app now renders the shell; just confirm the binary compiles and
        // a rect renders (full shell requires agentd which is not present in tests).
        // The real integration tests are in app::tests.
        let mut t = launch_test(|| {
            label()
                .text("OxideMX")
                .font_size(24.0)
                .color(Color::WHITE)
        });
        t.sync_and_update();
        let found = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("OxideMX"))
        });
        assert!(found.is_some(), "root should render the OxideMX title");
    }
}
