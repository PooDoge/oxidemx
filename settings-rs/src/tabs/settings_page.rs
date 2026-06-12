//! "Settings" tab — overlay-specific knobs that don't fit any of
//! the legacy device-config sections. Hosts the Theme picker,
//! Visuals, and Animation sub-panels stacked.

use crate::{tabs, Message, State};
use iced::widget::{button, column, container, pick_list, row, rule, text, text_input, Space};
use iced::{Alignment, Element, Length};
use oxidemx_widgets::palette::theme_catalogue;
use oxidemx_widgets::style;
use oxidemx_widgets::widgets::section_header;

pub fn view(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    // Two-press reset: first click arms (button label flips to a
    // warning style); second click confirms. Style follows the
    // armed state so the user gets clear visual feedback that
    // their next click will wipe state.
    let now = std::time::Instant::now();
    let armed = state
        .reset_armed_at
        .map(|t| now.duration_since(t) <= crate::RESET_CONFIRM_WINDOW)
        .unwrap_or(false);
    let reset_label = if armed {
        "Click again to confirm"
    } else {
        "Reset all to defaults"
    };
    let reset_btn: Element<Message> = if armed {
        button(text(reset_label).size(11))
            .style(style::btn_danger(pal))
            .on_press(Message::ResetAll)
            .into()
    } else {
        button(text(reset_label).size(11))
            .style(style::btn_secondary(pal))
            .on_press(Message::ResetAll)
            .into()
    };
    // Config-bundle controls — Export / Import / Open folder. Live
    // alongside Reset so the full "manage my config" toolkit is in
    // one place at the top of the tab.
    let export_btn = button(text("Export…").size(11))
        .style(style::btn_secondary(pal))
        .on_press(Message::ExportConfig);
    let import_btn = button(text("Import…").size(11))
        .style(style::btn_secondary(pal))
        .on_press(Message::ImportConfig);
    let folder_btn = button(text("Open config folder").size(11))
        .style(style::btn_secondary(pal))
        .on_press(Message::OpenConfigFolder);

    let header = row![
        section_header("Overlay settings"),
        Space::new().width(Length::Fill),
        export_btn,
        import_btn,
        folder_btn,
        reset_btn,
    ]
    .align_y(Alignment::Center)
    .spacing(8);

    column![
        header,
        text(
            "Theme, visuals, and animations for the radial overlay + this \
             settings window. Edits autosave; the running overlay picks them \
             up via inotify within ~150 ms."
        )
        .size(12)
        .style(style::text_dim(pal)),
        rule::horizontal(1).style(style::rule_style(pal)),
        Space::new().height(Length::Fixed(8.0)),
        section_block(state, "Theme", theme_picker(state)),
        Space::new().height(Length::Fixed(16.0)),
        section_block(state, "Weather widget", weather_location(state)),
        Space::new().height(Length::Fixed(16.0)),
        section_block(state, "Visuals", tabs::visuals::view(state)),
        Space::new().height(Length::Fixed(16.0)),
        section_block(state, "Animation", tabs::animation::view(state)),
        Space::new().height(Length::Fixed(16.0)),
        section_block(state, "Application bindings", app_bindings(state)),
        Space::new().height(Length::Fixed(16.0)),
        section_block(state, "AI Assistant", ai_key_panel(state)),
        Space::new().height(Length::Fixed(16.0)),
        section_block(state, "About + shortcuts", about_panel(state)),
    ]
    .spacing(10)
    .into()
}

// ============================================================================
// AI Assistant — Gemini API key management
// ============================================================================

/// Write-only key field: the stored key is never loaded back into
/// the UI (only a "configured" indicator), so a screen-share or a
/// glance at the settings window can't leak it. Saving writes
/// `~/.config/oxidemx/gemini.key` with 0600 perms; the overlay
/// re-reads the file on every prompt, so there's nothing to restart.
fn ai_key_panel(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;

    let intro = text(
        "Google Gemini API key for the radial menu's AI Assistant page \
         (chat + settings agent). Stored outside config.json at \
         ~/.config/oxidemx/gemini.key — config exports and imports \
         never include it. Get a free key at aistudio.google.com.",
    )
    .size(11)
    .style(style::text_dim(pal));

    let status_line: Element<Message> = if state.ai_key_present {
        text("Key configured ✓ — paste a new one below to replace it.")
            .size(11)
            .style(style::text_accent(pal))
            .into()
    } else {
        text("No key configured — the AI page will answer with an error until one is set.")
            .size(11)
            .style(style::text_faint(pal))
            .into()
    };

    let input = text_input("Paste API key…", &state.ai_key_draft)
        .secure(true)
        .on_input(Message::AiKeyDraftChanged)
        .on_submit(Message::AiKeySave)
        .padding(6)
        .size(12)
        .width(Length::Fill);

    let save_btn = button(text("Save").size(11))
        .style(style::btn_primary(pal))
        .on_press(Message::AiKeySave);

    let mut form = row![input, save_btn].align_y(Alignment::Center).spacing(8);
    if state.ai_key_present {
        form = form.push(
            button(text("Remove").size(11))
                .style(style::btn_danger(pal))
                .on_press(Message::AiKeyRemove),
        );
    }

    column![intro, status_line, form].spacing(8).into()
}

// ============================================================================
// Per-app profile bindings — class → profile name
// ============================================================================

fn app_bindings(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let bindings: Vec<(String, String)> = state
        .config
        .app_profiles
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let intro = text(
        "Map a focused-window class (matched against WM_CLASS / \
         xdg-toplevel app_id) to a radial-menu profile. The daemon \
         loads ~/.config/oxidemx/profiles/<name>.json when that \
         class focuses; falls back to the main config otherwise. \
         Edit the per-profile slices by hand for now — the \
         in-app profile editor is on the roadmap.",
    )
    .size(11)
    .style(style::text_dim(pal));

    let mut list_col = column![].spacing(6);
    if bindings.is_empty() {
        list_col = list_col.push(
            text("No bindings configured.")
                .size(11)
                .style(style::text_faint(pal)),
        );
    } else {
        for (class, profile) in &bindings {
            list_col = list_col.push(binding_row(state, class, profile));
        }
    }

    let class = state.app_binding_draft.class.clone();
    let profile = state.app_binding_draft.profile.clone();
    let class_for_msg = class.clone();
    let profile_for_msg = profile.clone();

    let add_form = row![
        text_input("Window class (e.g. \"firefox\")", &class)
            .on_input(move |v| Message::SetAppBindingDraft {
                class: v,
                profile: profile_for_msg.clone(),
            })
            .padding(6)
            .size(12)
            .width(Length::FillPortion(2)),
        text("→").size(13).style(style::text_faint(pal)),
        text_input("Profile name", &profile)
            .on_input(move |v| Message::SetAppBindingDraft {
                class: class_for_msg.clone(),
                profile: v,
            })
            .padding(6)
            .size(12)
            .width(Length::FillPortion(2)),
        button(text("+ Add").size(11))
            .style(style::btn_secondary(pal))
            .on_press(Message::AddAppBinding),
    ]
    .align_y(Alignment::Center)
    .spacing(8);

    column![
        intro,
        rule::horizontal(1).style(style::rule_style(pal)),
        list_col,
        Space::new().height(Length::Fixed(8.0)),
        add_form,
    ]
    .spacing(8)
    .into()
}

fn binding_row<'a>(state: &'a State, class: &str, profile: &str) -> Element<'a, Message> {
    let pal = &state.palette;
    let class_for_remove = class.to_string();
    row![
        container(text(class.to_string()).size(12))
            .padding([3, 8])
            .style(style::chip(pal))
            .width(Length::FillPortion(2)),
        text("→").size(11).style(style::text_faint(pal)),
        text(profile.to_string())
            .size(12)
            .width(Length::FillPortion(2)),
        Space::new().width(Length::Fill),
        button(text("Remove").size(10))
            .style(style::btn_danger(pal))
            .on_press(Message::RemoveAppBinding(class_for_remove)),
    ]
    .align_y(Alignment::Center)
    .spacing(8)
    .into()
}

/// "About + shortcuts" card — version, mouse-button cheat sheet,
/// keyboard shortcuts, and links to the project's resources. Most
/// users won't need any of this once they're set up, but having
/// it parked at the bottom of the Settings tab gives a clean
/// reference for new users / when something feels unintuitive.
fn about_panel(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let version = env!("CARGO_PKG_VERSION");
    let device_name = state
        .daemon
        .device_name
        .clone()
        .unwrap_or_else(|| "(disconnected)".to_string());

    let about_col = column![
        row![
            text("OxideMX").size(15),
            text(format!("v{version}"))
                .size(11)
                .style(style::text_dim(pal)),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        text(
            "Logitech MX Master 4 configuration utility — \
              radial menu, haptics, macros, Easy-Switch."
        )
        .size(11)
        .style(style::text_dim(pal)),
        rule::horizontal(1).style(style::rule_style(pal)),
        text(format!("Connected device: {device_name}"))
            .size(11)
            .style(style::text_dim(pal)),
    ]
    .spacing(6);

    let mouse_col = column![
        text("Mouse").size(13),
        cheat_row("Gesture button (thumb)", "Open radial menu"),
        cheat_row("Centre puck + scroll wheel", "Cycle radial pages"),
        cheat_row(
            "Centre puck + click",
            "Toggle hold/click mode (configurable)"
        ),
        cheat_row("Hover slice + release gesture", "Activate slice action"),
        cheat_row("Drag past inner ring + release", "Reveal sub-items"),
    ]
    .spacing(4);

    let keys_col = column![
        text("Keyboard").size(13),
        cheat_row("Esc", "Close radial menu / cancel capture"),
        cheat_row("Click \"Capture\" + chord", "Bind a key chord to a slice"),
        cheat_row("D-Bus: ShowMenuAtCursor", "Open the radial from any script"),
    ]
    .spacing(4);

    let links_col = column![
        text("Resources").size(13),
        text("• Project repo: https://github.com/PooDoge/oxidemx")
            .size(11)
            .style(style::text_dim(pal)),
        text("• Daemon logs: journalctl --user -u oxidemxd -f")
            .size(11)
            .style(style::text_dim(pal)),
        text("• Config: ~/.config/oxidemx/config.json")
            .size(11)
            .style(style::text_dim(pal)),
        text("• User themes: ~/.local/share/oxidemx/themes/")
            .size(11)
            .style(style::text_dim(pal)),
    ]
    .spacing(4);

    container(
        column![
            about_col,
            Space::new().height(Length::Fixed(8.0)),
            row![
                container(mouse_col).width(Length::FillPortion(1)),
                Space::new().width(Length::Fixed(16.0)),
                container(keys_col).width(Length::FillPortion(1)),
            ],
            Space::new().height(Length::Fixed(8.0)),
            links_col,
        ]
        .spacing(8),
    )
    .padding(12)
    .style(style::card_quiet(pal))
    .into()
}

fn cheat_row<'a>(action: &'a str, effect: &'a str) -> Element<'a, Message> {
    row![
        text(action.to_string())
            .size(11)
            .width(Length::Fixed(220.0)),
        text(effect.to_string()).size(11),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn section_block<'a>(
    state: &'a State,
    title: &str,
    body: Element<'a, Message>,
) -> Element<'a, Message> {
    let pal = &state.palette;
    container(
        column![
            text(title.to_string()).size(16),
            rule::horizontal(1).style(style::rule_style(pal)),
            body,
        ]
        .spacing(10),
    )
    .padding(14)
    .style(style::card(pal))
    .into()
}

// ============================================================================
// Theme picker
// ============================================================================

fn theme_picker(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;
    let entries = theme_catalogue();
    let current_slug = state.config.theme.as_str().to_string();
    let options: Vec<ThemeChoice> = entries
        .iter()
        .map(|(slug, name)| ThemeChoice {
            slug: slug.clone(),
            display: name.clone(),
        })
        .collect();
    let selected = options.iter().find(|c| c.slug == current_slug).cloned();

    let picker = pick_list(options, selected, |choice: ThemeChoice| {
        Message::SetTheme(choice.slug)
    })
    .style(style::pick_list_style(pal))
    .text_size(13);

    let preview = swatch_row(pal);

    let customise_btn = button(
        text(if state.theme_editor.is_some() {
            "Close customiser"
        } else {
            "Customise…"
        })
        .size(11),
    )
    .style(style::btn_secondary(pal))
    .on_press(Message::ToggleThemeCustomiser);

    let mut col = column![
        text(
            "Pick a theme. Re-styles every settings widget instantly; the \
             radial overlay reads this field on next show too."
        )
        .size(12)
        .style(style::text_dim(pal)),
        row![
            text("Theme").size(13),
            Space::new().width(Length::Fixed(16.0)),
            picker,
            Space::new().width(Length::Fill),
            customise_btn,
        ]
        .align_y(Alignment::Center)
        .spacing(8),
        row![
            text("Palette preview")
                .size(11)
                .style(style::text_faint(pal)),
            Space::new().width(Length::Fixed(8.0)),
            preview,
        ]
        .align_y(Alignment::Center)
        .spacing(8),
    ]
    .spacing(10);

    // Saved-theme management — list every user-saved theme with a
    // delete button. Bundled themes (Catppuccin variants, etc.)
    // are not deletable so they don't appear here. Empty when the
    // user has never hit "Save as user theme".
    // Always render the saved-themes panel — even when the user
    // has no themes yet, the Import button gives them somewhere
    // to drop a downloaded community theme.
    let user_slugs = oxidemx_shared::theme::list_user_theme_slugs();
    col = col.push(saved_themes_panel(state, user_slugs));

    // The full-panel customiser takes over the entire content area
    // when `theme_editor.is_some()` (handled in main::view), so
    // we don't need to render an inline editor here. Clicking
    // "Customise…" just toggles the editor state and the shell
    // swaps the body.
    col.into()
}

/// Render the "Saved themes" panel — one row per user-saved
/// theme, each with a Delete button. Bundled themes are not
/// represented here because they live inside the binary and
/// can't be removed.
fn saved_themes_panel<'a>(state: &'a State, slugs: Vec<String>) -> Element<'a, Message> {
    let pal = &state.palette;
    let header_row = row![
        text("Saved themes").size(13).style(style::text_dim(pal)),
        Space::new().width(Length::Fill),
        button(text("Import theme JSON…").size(11))
            .style(style::btn_secondary(pal))
            .on_press(Message::ImportTheme),
    ]
    .align_y(Alignment::Center);
    let mut col = column![
        header_row,
        text(
            "Themes saved from the customiser live in \
             ~/.local/share/oxidemx/themes/. Delete to remove the \
             file from disk; bundled themes can't be deleted. \
             \"Import theme JSON…\" loads any saved Theme file from \
             disk into your themes folder + switches to it."
        )
        .size(11)
        .style(style::text_dim(pal)),
        rule::horizontal(1).style(style::rule_style(pal)),
    ]
    .spacing(6);

    let active_slug = state.config.theme.as_str().to_string();
    for slug in slugs {
        let is_active = slug == active_slug;
        let renaming = state.renaming_theme.as_ref().filter(|r| r.original == slug);
        let row_el = if let Some(r) = renaming {
            // Inline rename mode — replace the row with an input
            // + Save / Cancel pair so the rename feels in-place.
            let input_value = r.draft.clone();
            row![
                text("Rename:").size(11).style(style::text_dim(pal)),
                text_input("New slug", &input_value)
                    .on_input(Message::SetRenameThemeDraft)
                    .on_submit(Message::CommitRenameTheme)
                    .padding(4)
                    .size(12)
                    .width(Length::Fixed(220.0)),
                button(text("Save").size(11))
                    .style(style::btn_secondary(pal))
                    .on_press(Message::CommitRenameTheme),
                button(text("Cancel").size(11))
                    .style(style::btn_secondary(pal))
                    .on_press(Message::CancelRenameTheme),
                Space::new().width(Length::Fill),
            ]
            .align_y(Alignment::Center)
            .spacing(6)
        } else {
            let display_name = oxidemx_shared::theme::Theme::load(
                &oxidemx_shared::theme::ThemeName::from(slug.as_str()),
            )
            .map(|t| t.name)
            .unwrap_or_else(|| slug.clone());
            let active_chip = if is_active {
                text(" — active").size(10).style(style::text_faint(pal))
            } else {
                text("")
            };
            row![
                text(display_name).size(12),
                text(format!("({slug})"))
                    .size(10)
                    .style(style::text_faint(pal)),
                active_chip,
                Space::new().width(Length::Fill),
                button(text("Export").size(11))
                    .style(style::btn_secondary(pal))
                    .on_press(Message::ExportTheme(slug.clone())),
                button(text("Rename").size(11))
                    .style(style::btn_secondary(pal))
                    .on_press(Message::BeginRenameTheme(slug.clone())),
                button(text("Delete").size(11))
                    .style(style::btn_danger(pal))
                    .on_press(Message::DeleteUserTheme(slug.clone())),
            ]
            .align_y(Alignment::Center)
            .spacing(8)
        };
        col = col.push(row_el);
    }

    container(col)
        .padding(10)
        .style(style::card_quiet(pal))
        .into()
}

fn swatch_row(pal: &oxidemx_widgets::palette::Palette) -> Element<'static, Message> {
    let swatches = [
        pal.accent,
        pal.green,
        pal.yellow,
        pal.red,
        pal.blue,
        pal.mauve,
        pal.pink,
        pal.peach,
        pal.teal,
        pal.sapphire,
        pal.lavender,
    ];
    let mut row_w = row![].spacing(4);
    for c in swatches {
        row_w = row_w.push(swatch(c));
    }
    row_w.into()
}

fn swatch(color: iced::Color) -> Element<'static, Message> {
    container(
        Space::new()
            .width(Length::Fixed(18.0))
            .height(Length::Fixed(18.0)),
    )
    .style(move |_| iced::widget::container::Style {
        background: Some(iced::Background::Color(color)),
        border: iced::Border {
            color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.18),
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    })
    .into()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ThemeChoice {
    slug: String,
    display: String,
}

impl std::fmt::Display for ThemeChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display)
    }
}

/// Weather-widget location: shows the configured place, a city
/// search box backed by Open-Meteo's keyless geocoder, and the
/// result list. Picking a result persists
/// `overlay.weather_location` + `overlay.weather_place`; the
/// overlay's sampler reads both on its next launch of the menu.
fn weather_location(state: &State) -> Element<'_, Message> {
    let pal = &state.palette;

    let current: Element<Message> = match (
        &state.config.overlay.weather_place,
        state.config.overlay.weather_location,
    ) {
        (Some(place), _) => row![
            text(format!("Location: {place}")).size(12),
            button(text("Clear").size(11))
                .style(style::btn_secondary(pal))
                .on_press(Message::WeatherClearLocation),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into(),
        (None, Some((lat, lon))) => row![
            text(format!("Location: {lat:.3}, {lon:.3}")).size(12),
            button(text("Clear").size(11))
                .style(style::btn_secondary(pal))
                .on_press(Message::WeatherClearLocation),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into(),
        _ => text("No location set — the Weather wedge shows a placeholder.")
            .size(12)
            .style(style::text_dim(pal))
            .into(),
    };

    let search_btn = if state.weather_searching {
        button(text("Searching…").size(11)).style(style::btn_secondary(pal))
    } else {
        button(text("Search").size(11))
            .style(style::btn_secondary(pal))
            .on_press(Message::WeatherSearch)
    };
    let search = row![
        text_input("City name (e.g. Oslo)…", &state.weather_query)
            .size(12)
            .padding(6)
            .on_input(Message::SetWeatherQuery)
            .on_submit(Message::WeatherSearch)
            .width(Length::Fixed(260.0)),
        search_btn,
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    // Temperature unit — Fahrenheit default, Celsius opt-in.
    let celsius = state.config.overlay.weather_celsius;
    let unit_btn = |label: &'static str, is_celsius: bool| {
        let b = button(text(label).size(11)).on_press(Message::SetWeatherCelsius(is_celsius));
        if celsius == is_celsius {
            b.style(style::btn_primary(pal))
        } else {
            b.style(style::btn_secondary(pal))
        }
    };
    let units = row![
        text("Units:").size(12),
        unit_btn("°F", false),
        unit_btn("°C", true),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let mut col = column![current, units, search].spacing(8);
    for (i, place) in state.weather_results.iter().enumerate() {
        col = col.push(
            button(text(place.label.clone()).size(12))
                .style(style::btn_secondary(pal))
                .on_press(Message::WeatherPick(i)),
        );
    }
    col = col.push(
        text("Forecast data by Open-Meteo (no API key). Refreshes every 15 minutes while the menu is open.")
            .size(10)
            .style(style::text_dim(pal)),
    );
    col.into()
}
