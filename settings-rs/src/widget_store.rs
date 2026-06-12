//! Widget store / downloader dialog (spec §11).
//!
//! v1 ships the dialog shell + the install pipeline — registry
//! catalog browsing lands later (the footer carries the STUB tag).
//! The list shows *installed* widgets (icon, name, author, version,
//! signature chip, Uninstall) and the footer offers the two manual
//! install paths: a native file picker and a URL download. Both
//! funnel into `oxidemx_widget_cli::install`, the same verified,
//! zip-slip-safe pipeline the CLI uses; sideloads (unsigned bundles
//! or unknown signing keys) bounce back as a consent prompt showing
//! the reason, fingerprint and the manifest's declared permissions,
//! and only proceed when the user clicks "Install anyway" (retry
//! with `force`).
//!
//! Rendered through `main.rs`'s `full_panel` chrome — the same
//! takeover pattern as the icon picker and theme customiser, so no
//! bespoke modal machinery.

use std::path::PathBuf;

use iced::widget::{button, column, container, row, text, text_input, Space};
use iced::{Alignment, Element, Length};
use oxidemx_widgets::{palette::Palette, style};

use crate::tabs::buttons::picker::{filter_registry, WidgetSummaryLite};
use crate::{Message, State};

// ============================================================================
// Dialog state
// ============================================================================

/// Everything the open store dialog tracks. `Some` on `State` while
/// the dialog is open; dropped wholesale on close.
#[derive(Debug, Clone, Default)]
pub struct WidgetStoreState {
    /// Live text of the list filter field.
    pub search: String,
    /// Live text of the "Install from URL…" field.
    pub url_input: String,
    /// Pending sideload consent prompt, if the last install attempt
    /// returned `NeedsConsent` (or hit an id collision → Replace).
    pub consent: Option<ConsentPrompt>,
    /// Widget id armed for the two-step uninstall confirm. First
    /// click arms (button flips to "Confirm removal"), second click
    /// on the same id removes.
    pub pending_uninstall: Option<String>,
    /// A download/install Task is in flight — buttons disabled.
    pub busy: bool,
}

/// Why the pending install needs explicit user consent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsentKind {
    /// No SIGNATURE entry in the bundle.
    Unsigned,
    /// Validly signed, but not by the pinned registry key. Carries
    /// the signer's hex fingerprint.
    UnknownKey(String),
    /// The widget id is already installed — replacing overwrites
    /// the current files (settings bags are untouched either way).
    Replace(String),
}

impl From<oxidemx_widget_cli::ConsentReason> for ConsentKind {
    fn from(r: oxidemx_widget_cli::ConsentReason) -> Self {
        match r {
            oxidemx_widget_cli::ConsentReason::Unsigned => ConsentKind::Unsigned,
            oxidemx_widget_cli::ConsentReason::UnknownKey(fp) => ConsentKind::UnknownKey(fp),
        }
    }
}

/// The consent sub-dialog's payload: which bundle to retry (with
/// `force`) on confirm, why it was refused, and the manifest's
/// self-declared permission list to show the user what they are
/// agreeing to.
#[derive(Debug, Clone, PartialEq)]
pub struct ConsentPrompt {
    pub bundle: PathBuf,
    pub kind: ConsentKind,
    pub permissions: Vec<String>,
}

/// What an install attempt came back with. Carried by
/// `Message::StoreInstallResult`.
#[derive(Debug, Clone)]
pub enum StoreInstallOutcome {
    Installed { id: String },
    NeedsConsent(ConsentPrompt),
    /// User dismissed the file picker.
    Cancelled,
    Failed(String),
}

// ============================================================================
// Install pipeline (async — driven via Task::perform)
// ============================================================================

/// "Install from file…": native file dialog → install. The dialog
/// filter accepts the spec's `.omxw` extension (§4) plus the
/// pre-standardization `.oxw` spelling for old bundles.
pub async fn pick_and_install() -> StoreInstallOutcome {
    let chosen = rfd::AsyncFileDialog::new()
        .set_title("Install widget bundle")
        .add_filter("Widget bundle", &["omxw", "oxw"])
        .pick_file()
        .await;
    let Some(handle) = chosen else {
        return StoreInstallOutcome::Cancelled;
    };
    install_bundle(handle.path().to_path_buf(), false).await
}

/// Run the CLI crate's verified install on a blocking thread and
/// fold the error space into the dialog's outcome shape. Sideload
/// refusals and id collisions both come back as consent prompts —
/// the retry path is `install_bundle(bundle, true)` for either.
pub async fn install_bundle(bundle: PathBuf, force: bool) -> StoreInstallOutcome {
    let res = tokio::task::spawn_blocking(move || {
        let outcome = oxidemx_widget_cli::install(&bundle, force, None);
        (bundle, outcome)
    })
    .await;
    use oxidemx_widget_cli::CliError;
    match res {
        Ok((_, Ok(r))) => StoreInstallOutcome::Installed { id: r.id },
        Ok((bundle, Err(CliError::NeedsConsent { reason, permissions }))) => {
            StoreInstallOutcome::NeedsConsent(ConsentPrompt {
                bundle,
                kind: reason.into(),
                permissions,
            })
        }
        Ok((bundle, Err(CliError::IdCollision(id)))) => {
            StoreInstallOutcome::NeedsConsent(ConsentPrompt {
                bundle,
                kind: ConsentKind::Replace(id),
                permissions: Vec::new(),
            })
        }
        Ok((_, Err(e))) => StoreInstallOutcome::Failed(e.to_string()),
        Err(e) => StoreInstallOutcome::Failed(format!("install task failed: {e}")),
    }
}

/// "Install from URL…": download to a fresh temp dir, then the same
/// install call as the file path.
pub async fn download_and_install(url: String) -> StoreInstallOutcome {
    let dl = tokio::task::spawn_blocking(move || download_to_temp(&url)).await;
    match dl {
        Ok(Ok(path)) => install_bundle(path, false).await,
        Ok(Err(e)) => StoreInstallOutcome::Failed(e),
        Err(e) => StoreInstallOutcome::Failed(format!("download task failed: {e}")),
    }
}

/// Fetch `url` into `$TMPDIR/oxidemx-store-<pid>-<ms>/<name>`.
/// Mirrors `geocode.rs`: curl on a blocking thread — settings has
/// no HTTP client dependency and this is a rare, user-initiated
/// call. `-f` turns HTTP errors into a nonzero exit.
fn download_to_temp(url: &str) -> Result<PathBuf, String> {
    let name = filename_from_url(url);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("oxidemx-store-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("temp dir: {e}"))?;
    let dest = dir.join(name);
    let out = std::process::Command::new("curl")
        .args(["-fsSL", "-m", "120", "-o"])
        .arg(&dest)
        .arg(url)
        .output()
        .map_err(|e| format!("curl failed to start: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let err = err.trim();
        return Err(if err.is_empty() {
            "download failed (offline? 404?)".to_string()
        } else {
            err.to_string()
        });
    }
    Ok(dest)
}

/// Remove an installed widget's directory. Settings bags
/// (`config.widgets.*`) are deliberately untouched (spec §9) — a
/// reinstall picks the old values straight back up. Slices that
/// referenced the widget fall back to the missing-widget chip.
pub fn uninstall(id: &str) -> Result<(), String> {
    // Belt-and-suspenders: ids come from our own registry scan, but
    // never feed anything path-like into remove_dir_all.
    if id.is_empty() || id.contains(['/', '\\']) || id == "." || id == ".." {
        return Err(format!("refusing to remove suspicious widget id {id:?}"));
    }
    let dir = oxidemx_widget_host::WidgetRegistry::widgets_dir()
        .ok_or_else(|| "cannot determine widgets dir".to_string())?;
    let target = dir.join(id);
    if !target.is_dir() {
        return Err(format!("\"{id}\" is not installed"));
    }
    std::fs::remove_dir_all(&target).map_err(|e| format!("remove failed: {e}"))
}

// ============================================================================
// Pure helpers (unit-tested below)
// ============================================================================

/// Derive a local filename from a download URL: last path segment,
/// query/fragment stripped, hostile characters filtered. Falls back
/// to `widget.omxw` when the URL has no usable path (bare host,
/// trailing slash, all-dots segment).
pub fn filename_from_url(url: &str) -> String {
    let stripped = url.split(['?', '#']).next().unwrap_or("");
    let after_scheme = stripped
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(stripped);
    let mut parts = after_scheme.split('/');
    let _host = parts.next();
    let last = parts.rfind(|s| !s.is_empty()).unwrap_or("");
    let safe: String = last
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || "-_.".contains(*c))
        .collect();
    if safe.trim_matches('.').is_empty() {
        "widget.omxw".to_string()
    } else {
        safe
    }
}

/// Signature chip label from `WidgetSummaryLite::signature`
/// ("registry" / "key <fp>" / "unsigned" — see `scan_registry_full`).
pub fn signature_chip_label(signature: &str) -> String {
    match signature {
        "registry" => "Pinned ✓".to_string(),
        "unsigned" => "Unsigned".to_string(),
        other => other.to_string(),
    }
}

// ============================================================================
// View (rendered inside main.rs's full_panel chrome)
// ============================================================================

pub fn view<'a>(state: &'a State, store: &'a WidgetStoreState) -> Element<'a, Message> {
    let pal = &state.palette;

    let mut col = column![text(
        "Community widgets — installed ones appear in the slice picker."
    )
    .size(12)
    .style(style::text_dim(pal))]
    .spacing(12);

    // Sideload consent sub-flow on top of everything else while
    // pending — the install is parked until the user decides.
    if let Some(prompt) = &store.consent {
        col = col.push(consent_view(pal, prompt, store.busy));
    }

    col = col.push(
        text_input("Search installed widgets…", &store.search)
            .on_input(Message::StoreSearch)
            .padding(6)
            .size(12),
    );

    if state.widget_registry.is_empty() {
        col = col.push(
            text("No community widgets installed yet — the built-in widgets are always available in the slice picker.")
                .size(11)
                .style(style::text_faint(pal)),
        );
    } else {
        let hits = filter_registry(&state.widget_registry, &store.search);
        if hits.is_empty() {
            col = col.push(
                text("No matches — clear the search to see everything.")
                    .size(11)
                    .style(style::text_faint(pal)),
            );
        }
        for w in hits {
            col = col.push(installed_row(state, store, w));
        }
    }

    if store.busy {
        col = col.push(
            text("Working…")
                .size(11)
                .style(style::text_accent(pal)),
        );
    }

    col = col.push(Space::new().height(Length::Fixed(4.0)));
    col = col.push(footer(pal, store));
    col.into()
}

/// One installed-widget row: icon tile, name + signature chip,
/// author · version, Settings jump (closes the dialog — the options
/// card lives on the slice editor) and the two-step Uninstall.
fn installed_row<'a>(
    state: &'a State,
    store: &'a WidgetStoreState,
    w: &'a WidgetSummaryLite,
) -> Element<'a, Message> {
    let pal = &state.palette;

    let icon: Element<Message> = w
        .icon_path
        .as_ref()
        .and_then(|p| {
            crate::radial_preview::resolve_icon_handle(
                &state.icons,
                &state.iced_handles,
                &p.to_string_lossy(),
                24,
                pal.text,
            )
        })
        .map(|h| -> Element<Message> {
            iced::widget::image(h)
                .width(Length::Fixed(24.0))
                .height(Length::Fixed(24.0))
                .into()
        })
        .unwrap_or_else(|| {
            text(w.name.chars().next().unwrap_or('?').to_uppercase().to_string())
                .size(14)
                .style(style::text_accent(pal))
                .into()
        });

    let pinned = w.signature == "registry";
    let sig_label = text(signature_chip_label(&w.signature)).size(9);
    let sig_chip = container(if pinned {
        sig_label.style(style::text_accent(pal))
    } else {
        sig_label.style(style::text_dim(pal))
    })
    .padding([2, 6])
    .style(style::chip(pal));

    let mut name_row = row![text(w.name.as_str()).size(13), sig_chip]
        .spacing(8)
        .align_y(Alignment::Center);
    if !w.ready {
        let reason = w.reason.as_deref().unwrap_or("incompatible");
        name_row = name_row.push(
            text(format!("incompatible — {reason}"))
                .size(9)
                .style(style::text_faint(pal)),
        );
    }

    // "Settings" jump: simplest v1 behaviour — close the dialog and
    // let the user open the widget's options card from its slice
    // (the card renders under the behavior chip).
    let settings_btn: Element<Message> = if w.has_options {
        button(text("Settings").size(11))
            .style(style::btn_secondary(pal))
            .on_press(Message::CloseWidgetStore)
            .into()
    } else {
        Space::new().width(Length::Shrink).into()
    };

    let armed = store.pending_uninstall.as_deref() == Some(w.id.as_str());
    let uninstall_btn = {
        // Two distinct `impl Fn` opaque types — pick the style in
        // two builder branches instead of one `if` expression.
        let b = button(text(if armed { "Confirm removal" } else { "Uninstall" }).size(11))
            .on_press_maybe((!store.busy).then(|| Message::StoreUninstall(w.id.clone())));
        if armed {
            b.style(style::btn_danger(pal))
        } else {
            b.style(style::btn_secondary(pal))
        }
    };

    let body = row![
        container(icon)
            .width(Length::Fixed(32.0))
            .height(Length::Fixed(32.0))
            .center_x(Length::Fixed(32.0))
            .center_y(Length::Fixed(32.0)),
        column![
            name_row,
            text(format!("by {} · v{}", w.author, w.version))
                .size(10)
                .style(style::text_faint(pal)),
        ]
        .spacing(2),
        Space::new().width(Length::Fill),
        settings_btn,
        uninstall_btn,
    ]
    .align_y(Alignment::Center)
    .spacing(10);

    container(body)
        .padding(10)
        .width(Length::Fill)
        .style(style::card_quiet(pal))
        .into()
}

/// The sideload consent card: reason (+ fingerprint), the manifest's
/// declared permissions, Install anyway / Cancel.
fn consent_view<'a>(
    pal: &'a Palette,
    prompt: &'a ConsentPrompt,
    busy: bool,
) -> Element<'a, Message> {
    let (title, detail) = match &prompt.kind {
        ConsentKind::Unsigned => (
            "This bundle is unsigned".to_string(),
            "It was not signed by the OxideMX widget registry or any key at all. \
             Only install widgets you trust."
                .to_string(),
        ),
        ConsentKind::UnknownKey(fp) => (
            "Signed by an unknown key".to_string(),
            format!(
                "The bundle is validly signed, but not by the pinned registry key. \
                 Signer fingerprint: {fp}"
            ),
        ),
        ConsentKind::Replace(id) => (
            format!("\"{id}\" is already installed"),
            "Installing replaces the current files. The widget's settings are kept."
                .to_string(),
        ),
    };

    let mut card = column![
        text(title).size(13).style(style::text_accent(pal)),
        text(detail).size(11).style(style::text_dim(pal)),
    ]
    .spacing(6);

    if prompt.permissions.is_empty() {
        card = card.push(
            text("Requests no permissions.")
                .size(10)
                .style(style::text_faint(pal)),
        );
    } else {
        card = card.push(
            text("Requests permissions:")
                .size(10)
                .style(style::text_dim(pal)),
        );
        for p in &prompt.permissions {
            card = card.push(
                text(format!("  • {p}"))
                    .size(10)
                    .style(style::text_dim(pal)),
            );
        }
    }

    card = card.push(
        row![
            button(text("Install anyway").size(11))
                .style(style::btn_danger(pal))
                .on_press_maybe((!busy).then_some(Message::StoreConsentAccept)),
            button(text("Cancel").size(11))
                .style(style::btn_secondary(pal))
                .on_press_maybe((!busy).then_some(Message::StoreConsentCancel)),
        ]
        .spacing(8),
    );

    container(card)
        .padding(12)
        .width(Length::Fill)
        .style(style::card(pal))
        .into()
}

/// Footer: STUB tag + drop-path hint + the two install actions.
fn footer<'a>(pal: &'a Palette, store: &'a WidgetStoreState) -> Element<'a, Message> {
    let stub_row = row![
        container(text("STUB").size(8).style(style::text_accent(pal)))
            .padding([2, 6])
            .style(style::chip(pal)),
        text("Registry browsing coming soon — for now drop a .omxw bundle into ~/.config/oxidemx/widgets/ or install below.")
            .size(10)
            .style(style::text_faint(pal)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let file_btn = button(text("Install from file…").size(11))
        .style(style::btn_primary(pal))
        .on_press_maybe((!store.busy).then_some(Message::StoreInstallFromFile));

    let url_ready = !store.busy && !store.url_input.trim().is_empty();
    let url_row = row![
        text_input("https://…/widget.omxw", &store.url_input)
            .on_input(Message::StoreUrlInput)
            .on_submit(Message::StoreInstallFromUrl)
            .padding(6)
            .size(11),
        button(text("Install from URL…").size(11))
            .style(style::btn_secondary(pal))
            .on_press_maybe(url_ready.then_some(Message::StoreInstallFromUrl)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    container(
        column![
            stub_row,
            row![file_btn, url_row].spacing(12).align_y(Alignment::Center),
        ]
        .spacing(10),
    )
    .padding(10)
    .width(Length::Fill)
    .style(style::card_quiet(pal))
    .into()
}

// ============================================================================
// Tests — pure helpers only (view code is exercised by cargo check)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- URL filename derivation ---

    #[test]
    fn filename_takes_last_path_segment() {
        assert_eq!(
            filename_from_url("https://widgets.oxidemx.org/v1/w/weather-1.4.0.omxw"),
            "weather-1.4.0.omxw"
        );
    }

    #[test]
    fn filename_strips_query_and_fragment() {
        assert_eq!(
            filename_from_url("https://x.org/a/b.omxw?token=abc#frag"),
            "b.omxw"
        );
    }

    #[test]
    fn filename_falls_back_for_bare_host_or_trailing_slash() {
        assert_eq!(filename_from_url("https://example.com"), "widget.omxw");
        assert_eq!(filename_from_url("https://example.com/"), "widget.omxw");
        assert_eq!(filename_from_url(""), "widget.omxw");
    }

    #[test]
    fn filename_filters_hostile_characters() {
        // Path traversal / separators / shell metachars never
        // survive into the temp-file name.
        assert_eq!(filename_from_url("https://x.org/a/..%2F..%2Fetc"), "..2F..2Fetc");
        assert_eq!(filename_from_url("https://x.org/.."), "widget.omxw");
        assert_eq!(filename_from_url("https://x.org/$(rm)/.."), "widget.omxw");
    }

    #[test]
    fn filename_without_scheme_still_works() {
        assert_eq!(filename_from_url("example.com/w.omxw"), "w.omxw");
    }

    // --- signature chip ---

    #[test]
    fn signature_chip_maps_states() {
        assert_eq!(signature_chip_label("registry"), "Pinned ✓");
        assert_eq!(signature_chip_label("unsigned"), "Unsigned");
        assert_eq!(signature_chip_label("key ab12cd34"), "key ab12cd34");
    }

    // --- list filtering (store list reuses the picker's helper) ---

    fn lite(id: &str, name: &str, author: &str) -> WidgetSummaryLite {
        WidgetSummaryLite {
            id: id.into(),
            name: name.into(),
            version: "1.0.0".into(),
            author: author.into(),
            ready: true,
            reason: None,
            has_options: false,
            icon_path: None,
            signature: "unsigned".into(),
        }
    }

    #[test]
    fn store_list_filters_by_name_author_or_id() {
        let reg = vec![
            lite("weather", "Weather", "JuhLabs"),
            lite("clock", "World clock", "Acme"),
        ];
        let by_name = filter_registry(&reg, "wea");
        assert_eq!(by_name.len(), 1);
        assert_eq!(by_name[0].id, "weather");

        let by_author = filter_registry(&reg, "acme");
        assert_eq!(by_author.len(), 1);
        assert_eq!(by_author[0].id, "clock");

        let by_id = filter_registry(&reg, "clock");
        assert_eq!(by_id.len(), 1);

        assert_eq!(filter_registry(&reg, "").len(), 2);
        assert!(filter_registry(&reg, "zzz").is_empty());
    }

    // --- consent mapping ---

    #[test]
    fn consent_kind_maps_from_cli_reason() {
        assert_eq!(
            ConsentKind::from(oxidemx_widget_cli::ConsentReason::Unsigned),
            ConsentKind::Unsigned
        );
        assert_eq!(
            ConsentKind::from(oxidemx_widget_cli::ConsentReason::UnknownKey("ab12".into())),
            ConsentKind::UnknownKey("ab12".into())
        );
    }

    // --- uninstall guard ---

    #[test]
    fn uninstall_rejects_suspicious_ids() {
        assert!(uninstall("").is_err());
        assert!(uninstall("..").is_err());
        assert!(uninstall("a/b").is_err());
        assert!(uninstall("a\\b").is_err());
    }
}
