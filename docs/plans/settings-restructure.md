# Settings tab restructure + settings search (next iteration)

Spec from Jim, 2026-06-12. The Settings tab is getting crowded;
restructure it into a parent item with sub-categories, and add
search.

## Navigation

The Settings tab gains **sub-items** that appear when the parent
Settings tab item is selected:

- **Settings** (parent selected) → shows ALL settings, ordered as
  the category list below.
- **Theme + Visuals** — the theme card AND the visuals card
  (everything from today's Visuals tab EXCEPT the GPU shaders
  card).
- **GPU Shaders** — the GPU shaders card (incl. AI window effects).
- **Animations** — current Animation tab content.
- **AI Page** — every AI-chat-page-related setting:
  - **API key shown at the top**,
  - App Bindings,
  - Import/Export — when it is the only section displayed, re-lay
    the buttons + descriptions to use the full space (today's
    cramped layout assumes it shares the page).
- **Widgets** — weather location/units + widget sources.
- **About**.

Reorder the parent (all-settings) page to match exactly this
category order.

## Search

A search field over individual settings:

- Matches against setting **title first** (sort priority), then
  description text.
- Results jump to / reveal the owning category + card.

## Implementation notes (for whoever picks this up)

- Today's tab enum + per-tab `view()` fns live in
  `settings-rs/src/main.rs` + `settings-rs/src/tabs/*`. The
  cleanest path: a `SettingsCategory` enum (None = all), a slim
  sidebar sub-nav rendered only while the Settings parent is
  active, and category filters around the existing card builders
  (each card already is a discrete fn — reuse, don't rewrite).
- For search: a static registry `&[(&str title, &str blurb,
  SettingsCategory, CardId)]` built alongside the card fns; rank
  title `contains` (case-insensitive) above blurb hits.
- **Coordinate with the widget-plugins session** — it is actively
  reworking settings-rs (widget store, schema v3); land this after
  or rebase onto its branch.
