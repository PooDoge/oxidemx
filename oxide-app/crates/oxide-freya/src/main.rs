//! OxideMX Freya desktop app.
use freya::prelude::*;

pub mod nav;

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let _guard = rt.enter();
    launch(
        LaunchConfig::new().with_window(
            WindowConfig::new(app)
                .with_size(1200., 800.)
                .with_title("OxideMX"),
        ),
    )
}

pub fn app() -> impl IntoElement {
    rect()
        .expanded()
        .main_align(Alignment::Center)
        .cross_align(Alignment::Center)
        .background((5u8, 7u8, 11u8))
        .child(
            label()
                .text("OxideMX — hello Freya")
                .font_size(24.0)
                .color(Color::WHITE),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use freya_testing::prelude::*;

    #[test]
    fn root_renders_title() {
        let mut t = launch_test(app);
        t.sync_and_update();
        let found = t.find(|_, el| {
            Label::try_downcast(el)
                .filter(|l| l.text.as_ref().contains("OxideMX"))
        });
        assert!(found.is_some(), "root should render the OxideMX title");
    }
}
