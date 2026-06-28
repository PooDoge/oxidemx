//! `MenuSection` (group header) and `MenuRow` (interactive item) for OxideMX
//! floating menus.
//!
//! Both use `menu_theme()` so they stay visually consistent with any `MenuSurface`
//! that hosts them.
use freya::prelude::*;

use crate::tokens::{Theme, Tone};
use crate::components::composer::icons::icon;
use super::theme::menu_theme;
use super::surface::MenuDismiss;

// ── MenuSection ───────────────────────────────────────────────────────────────

/// A non-interactive group header row shown above a section of `MenuRow`s.
///
/// Renders an optional leading icon and a bold tinted label (font 11,
/// `FontWeight::BOLD`). Color resolves via `theme.tone(tone)` when present,
/// or `theme.subtext()` when `tone` is `None`.
#[derive(Clone, PartialEq)]
pub struct MenuSection {
    theme: Theme,
    title: String,
    tone:  Option<Tone>,
    icon:  Option<&'static str>,
}

impl MenuSection {
    pub fn new(theme: Theme, title: impl Into<String>, tone: Option<Tone>) -> Self {
        Self { theme, title: title.into(), tone, icon: None }
    }

    pub fn icon(mut self, name: &'static str) -> Self {
        self.icon = Some(name);
        self
    }
}

impl Component for MenuSection {
    fn render(&self) -> impl IntoElement {
        let th = self.theme;
        let color = match self.tone {
            Some(t) => th.tone(t),
            None    => th.subtext(),
        };

        let mut row = rect()
            .direction(Direction::Horizontal)
            .cross_align(Alignment::Center)
            .spacing(6.)
            .padding(Gaps::new(10., 14., 4., 14.));

        if let Some(icon_name) = self.icon {
            row = row.child(icon(icon_name, 14., color));
        }

        row.child(
            label()
                .text(self.title.clone())
                .font_size(11.)
                .font_weight(FontWeight::BOLD)
                .color(color),
        )
    }
}

// ── MenuRow ───────────────────────────────────────────────────────────────────

/// An interactive `MenuButton` row with optional leading icon, title + sub
/// label column, optional trailing element, and optional selected state.
///
/// The `Content::Flex` layout lives here so callers don't repeat it.
///
/// Builder usage:
/// ```ignore
/// MenuRow::new(theme)
///     .icon(Some("gear"))
///     .title("Settings")
///     .subtitle(Some("App preferences".to_string()))
///     .selected(false)
///     .on_press(move |()| { /* handler */ })
/// ```
#[derive(Clone, PartialEq)]
pub struct MenuRow {
    theme:    Theme,
    icon_name: Option<&'static str>,
    title:    String,
    sub:      Option<String>,
    trailing: Option<Element>,
    selected: bool,
    on_press: Option<EventHandler<()>>,
    auto_dismiss: bool,
}

impl MenuRow {
    pub fn new(theme: Theme) -> Self {
        Self {
            theme,
            icon_name: None,
            title:    String::new(),
            sub:      None,
            trailing: None,
            selected: false,
            on_press: None,
            auto_dismiss: true,
        }
    }

    pub fn icon(mut self, name: Option<&'static str>) -> Self {
        self.icon_name = name;
        self
    }

    pub fn title(mut self, t: impl Into<String>) -> Self {
        self.title = t.into();
        self
    }

    pub fn subtitle(mut self, s: Option<String>) -> Self {
        self.sub = s;
        self
    }

    pub fn trailing(mut self, el: Option<Element>) -> Self {
        self.trailing = el;
        self
    }

    pub fn selected(mut self, v: bool) -> Self {
        self.selected = v;
        self
    }

    pub fn on_press(mut self, h: impl Into<EventHandler<()>>) -> Self {
        self.on_press = Some(h.into());
        self
    }

    /// Whether activating this row also dismisses the menu. Default true (a
    /// regular item). Set false for toggle / submenu / nav rows that should keep
    /// the menu open. Only has effect under a `MenuSurface::light_dismiss(true)`
    /// (otherwise there is no `MenuDismiss` context and Freya's any-click-close applies).
    pub fn auto_dismiss(mut self, v: bool) -> Self {
        self.auto_dismiss = v;
        self
    }
}

impl Component for MenuRow {
    fn render(&self) -> impl IntoElement {
        let th = self.theme;
        let (_, item_theme) = menu_theme(th);
        let dismiss = use_try_consume::<MenuDismiss>();
        let auto_dismiss = self.auto_dismiss;
        let on_press = self.on_press.clone();

        // Title column: title + optional sub stacked vertically, takes flex space.
        let title_text = self.title.clone();
        let sub_text   = self.sub.clone();

        let mut title_col = rect()
            .direction(Direction::Vertical)
            .width(Size::flex(1.0))
            .child(
                label()
                    .text(title_text)
                    .font_size(13.)
                    .color(th.text())
                    .max_lines(1_usize),
            );

        if let Some(sub) = sub_text {
            title_col = title_col.child(
                label()
                    .text(sub)
                    .font_size(11.)
                    .color(th.subtext())
                    .max_lines(1_usize),
            );
        }

        // Inner row: [icon?] [title+sub] [trailing?]
        let mut inner = rect()
            .direction(Direction::Horizontal)
            .content(Content::Flex)
            .cross_align(Alignment::Center)
            .spacing(10.)
            .width(Size::fill());

        if let Some(icon_name) = self.icon_name {
            inner = inner.child(icon(icon_name, 16., th.subtext_hi()));
        }

        inner = inner.child(title_col.into_element());

        if let Some(trailing) = self.trailing.clone() {
            inner = inner.child(trailing);
        }

        MenuButton::new()
            .theme(item_theme)
            .on_press(move |_: Event<PressEventData>| {
                if let Some(h) = &on_press {
                    h.call(());
                }
                if auto_dismiss {
                    if let Some(d) = &dismiss {
                        if let Some(h) = &d.0 {
                            h.call(());
                        }
                    }
                }
            })
            .child(inner)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::menu::MenuSurface;
    use freya_testing::prelude::*;
    use freya_testing::TestingRunner;

    /// `menu_theme` returns without panic and a `MenuSurface`+`MenuRow` mount
    /// together (verifying the Menu context chain required by `MenuButton`).
    #[test]
    fn menu_theme_has_dark_hover() {
        let (_, item) = menu_theme(Theme::default());
        // Partial is opaque — we can only verify construction succeeds.
        drop(item);

        // MenuButton requires the Menu context provided by MenuSurface.
        fn app() -> impl IntoElement {
            MenuSurface::new(Theme::default())
                .child(MenuRow::new(Theme::default()).title("Smoke"))
        }
        let mut t = launch_test(app);
        t.sync_and_update();
        let found = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("Smoke"))
        });
        assert!(found.is_some(), "MenuRow smoke mount inside MenuSurface failed");
    }

    /// Mount a `MenuRow` with a title and trailing element inside a `MenuSurface`;
    /// assert both labels render.
    #[test]
    fn menu_row_renders_title_and_trailing() {
        fn app() -> impl IntoElement {
            MenuSurface::new(Theme::default())
                .child(
                    MenuRow::new(Theme::default())
                        .title("Hello")
                        .trailing(Some(label().text("✓").into_element())),
                )
        }
        let mut t = launch_test(app);
        t.sync_and_update();

        let hello = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("Hello"))
        });
        assert!(hello.is_some(), "MenuRow should render the title 'Hello'");

        let check = t.find(|_, el| {
            Label::try_downcast(el).filter(|l| l.text.as_ref().contains("✓"))
        });
        assert!(check.is_some(), "MenuRow should render the trailing '✓' element");
    }

    // Mount a light-dismiss surface whose on_close bumps a counter shown in a label.
    // Clicking an auto_dismiss(false) row must NOT bump it; an auto_dismiss(true) row must.
    fn dismiss_body(auto_dismiss: bool) -> Element {
        let mut closes = use_state(|| 0_u32);
        MenuSurface::new(Theme::default())
            .light_dismiss(true)
            .on_close(move |_| *closes.write() += 1)
            .child(
                rect()
                    .direction(Direction::Vertical)
                    .child(label().text(format!("closes={}", closes.read())).font_size(12.))
                    .child(
                        MenuRow::new(Theme::default())
                            .title("Row")
                            .auto_dismiss(auto_dismiss)
                            .on_press(move |_| {})
                            .into_element(),
                    )
                    .into_element(),
            )
            .into_element()
    }

    fn dismiss_count_after_row_click(auto_dismiss: bool) -> String {
        fn app_keep() -> Element { dismiss_body(false) }
        fn app_close() -> Element { dismiss_body(true) }
        let app: fn() -> Element = if auto_dismiss { app_close } else { app_keep };
        let (mut t, _) = TestingRunner::new(app, (320., 240.).into(), |_| {}, 1.);
        t.poll(std::time::Duration::from_millis(5), std::time::Duration::from_millis(60));
        t.sync_and_update();
        let center = t
            .find(|node, el| {
                Label::try_downcast(el)
                    .filter(|l| l.text.as_ref().contains("Row"))
                    .map(|_| {
                        let c = node.layout().visible_area().center();
                        (c.x as f64, c.y as f64)
                    })
            })
            .expect("Row label present");
        t.click_cursor(center);
        t.poll(std::time::Duration::from_millis(5), std::time::Duration::from_millis(60));
        t.sync_and_update();
        let txt = t
            .find(|_, el| {
                Label::try_downcast(el).filter(|l| l.text.as_ref().contains("closes="))
            })
            .expect("counter label present");
        txt.text.as_ref().to_string()
    }

    #[test]
    fn auto_dismiss_false_row_does_not_close_menu() {
        assert_eq!(dismiss_count_after_row_click(false), "closes=0");
    }

    #[test]
    fn auto_dismiss_true_row_closes_menu() {
        assert_eq!(dismiss_count_after_row_click(true), "closes=1");
    }
}
