<!-- Pulled from the OxideMX Design System project (claude.ai/design, projectId
686a723e-0412-4e94-870e-b4e32ae465f2, file design_handoff_ai_chat_ui/TOKENS.md).
The agent-bubbles UI MUST consume these via oxidemx-widgets::Palette / Kit — never hardcode hex. -->

# TOKENS.md — OxideMX design tokens (the `Palette` contract)

> Source of truth: **`oxidemx-widgets::palette::Palette`** (`palette.rs`), built per active theme
> from `oxidemx_shared::Theme` JSON, consumed by `settings-rs` / `popup-rs` / `overlay-rs`.
> **Never hardcode hex. Never reintroduce `--jr-*` CSS vars.** Use `Palette` field names.
> Values below are the Mocha fallback (`Palette::hardcoded_mocha`); real values come from the theme.

## Surfaces — increasing elevation
| field | mocha | role |
|---|---|---|
| `crust` | #11111b | window / wells / footer / header bg |
| `mantle` | #181825 | cards, sidebar rail |
| `base` | #1e1e2e | page body |
| `surface0` | #313244 | inputs, chips, slider track |
| `surface1` | #45475a | borders (chat cards) |
| `surface2` | #585b70 | strong border / hover |

## Text / ink
| field | mocha | role |
|---|---|---|
| `text` | #cdd6f4 | primary |
| `subtext1` | #bac2de | secondary |
| `subtext0` | #a6adc8 | muted / meta / placeholder |
| `overlay0` | #6c7086 | disabled / faint / idle dot |

## Accent + derived
| field | role |
|---|---|
| `accent` | active, links, focus, primary fill |
| `accent_dim` | muted accent |
| `accent_06` | background wash (6%) |
| `accent_15` | chip/active fill (15%) |
| `accent_40` | focus ring, active border (40%) |

## Hairlines + row washes
`hairline` .07 default border · `hairline_strong` .12 input/button/chip · `hairline_faint` .04 quiet card ·
`row_hover` .03 · `row_active` .05.

## Semantic + slice palette
`success`/`danger`/`warning` = `green`/`red`/`yellow`. Slice colors `mauve pink peach teal sapphire lavender`
(`peach` = approval/amber, `mauve` = memory/undo). **Agent-bubble tones map to slice colors**
(researcher→blue, shell→peach, writer→mauve, summarizer→teal, browser→green, coordinator→accent).

## Scales
- **Type:** 18/600 display · 16/600 heading · 14.5/600 title · 13 body · 12 body-sm · 11.5/500 label ·
  11 meta · 10.5 caption · 10 mono-micro. Inter for prose, `Font::MONOSPACE` for code/IDs/timestamps.
- **Spacing** (4px base): 2 · 4 · 6 · 8 · 10 · 12 · 16 · 20 · 24.
- **Radii:** control/code 6 · button 9 · settings-card 10 · chat-card 12 · panel 16 · pill 999.
- **Elevation** (`iced::Shadow`): e1 card `0·2·8 /.35` · e2 overlay `0·12·32 /.5` · e3 modal `0·24·60 /.6`.
  Focus = 1px accent border + faint shadow.
