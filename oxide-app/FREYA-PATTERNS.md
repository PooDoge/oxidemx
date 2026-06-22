# Freya v0.4.0-rc.23 patterns (distilled from the cloned examples)

Source of truth: `/run/media/system/fastdrive/repos/freya` (examples/ + crates/freya-components/src/ +
crates/freya/src/_docs/). The developer's own best-practices skill is at
`.claude/skills/freya-gui-framework/SKILL.md` — run it before UI work. This file is the project-specific
distillation; cite the example, not memory.

## THE layout rule (the bug we kept hitting)

**`Size::flex(n)` is ONLY honoured when the parent container has `.content(Content::Flex)`.**
Default content is `Content::Normal`, which stacks children at their measured size and *ignores* flex.
A `ScrollView` (or any fill child) then takes its full measured height and pushes siblings off-screen.

- `Content::Normal` (default): children stack sequentially along the direction axis; flex ignored.
- `Content::Flex`: children with `Size::flex(n)` split the *remaining* space proportionally. **Required for flex.**
- `.expanded()` = `width(Size::fill()).height(Size::fill())` (both axes). `Size::fill()` takes all available
  *explicit* parent size but does NOT participate in flex distribution. Use `Size::flex(1.)` for "take the rest".
- `Size` variants: `auto()` (content-sized, default), `fill()` (all available), `flex(n)` (grow factor, needs
  Content::Flex), `px(n)`, `percent(n)` (of parent), `window_percent(n)` (of root).
  Source: `crates/torin/src/values/{size,content}.rs`; `examples/layout_content_flex.rs`.

### Canonical: scrollable body + pinned footer (chat thread + input)

From `examples/ai-chat/src/main.rs:82-160`:
```rust
rect()
    .expanded()
    .content(Content::Flex)                 // <-- THE missing piece
    .child(
        rect()
            .width(Size::fill())
            .height(Size::flex(1.0))        // engages because parent is Content::Flex
            .child(ScrollView::new().child(thread)),
    )
    .child(
        rect().width(Size::fill()).height(Size::px(60.))   // pinned, natural height
            .child(input_bar),
    )
```
Order matters: body first, footer second → footer sits at the bottom. The same rule applies to a vertical
column that wants a spacer (`rect().height(Size::flex(1.0))`) to push a trailing control to the bottom — the
column itself must be `.content(Content::Flex)`.

### 3-column shell (fixed sides, flex center)

From `examples/kanban.rs` / `examples/component_sidebar.rs`:
```rust
rect().expanded().horizontal().content(Content::Flex)
    .child(rect().width(Size::px(274.)).height(Size::fill()))   // left, fixed
    .child(rect().width(Size::flex(1.0)).height(Size::fill()))  // center, fills
    .child(rect().width(Size::px(300.)).height(Size::fill()))   // right, fixed
```

## Elements / builders

- `rect()` container; `label()` single-line; `&str`/`String: Into<Label>` so `rect().child("Hi")` is fine.
- Shorthands: `.expanded()`, `.center()`, `.horizontal()`, `.vertical()`, `.spacing(f)`, `.padding(Gaps::new_all(f))`.
- Builder rule: never store an element in a `let mut` to mutate later — chain, or use `.maybe(cond, |el| ...)`
  / `.map(opt, |el, v| ...)` / `.maybe_child(Option<_>)`. (skill: "Element Builder Pattern")
- Children on custom components: store `Vec<Element>` + implement `ChildrenExt`; or take `impl IntoElement`
  and `.into()` it. (`crates/freya-components/src/card.rs`)

## ScrollView (chat thread + lists)

`crates/freya-components/src/scrollviews/`.
- `ScrollView::new().direction(..).spacing(f).show_scrollbar(bool).child(..)`. Default size fill/fill.
- **Auto-scroll-to-bottom (chat):** `use_scroll_controller(|| ScrollConfig { default_vertical_position:
  ScrollPosition::End, ..default })` + `ScrollView::new_controlled(controller)`. New content re-measures and
  sticks to the end. (`use_scroll_controller.rs`)
- Long lists: `VirtualScrollView::new(|i,_| rect().key(i)...into()).length(n).item_size(h)`.

## Input

`crates/freya-components/src/input.rs`; styled in `examples/ai-chat/src/main.rs:136-142`.
- `Input::new(value: impl Into<Writable<String>>).placeholder("…").on_submit(handler).width(Size::flex(1.))`.
- **Make it visibly a box:** set `.background(..)` (+ `.focus_background(..)`); a transparent border + no bg
  renders an invisible field. `.leading(svg)` / `.trailing(svg)` for icons. `.on_submit` fires on Enter.

## Sidebar / collapsible / resizable

- Built-in `SideBarItem::new().child(..).on_press(..)` with active highlight via `ActivableRoute` (router) —
  `examples/component_sidebar.rs`. Optional; a hand-rolled `rect()` is fine for full control.
- **Collapse rail↔full:** `use_state(bool)` + conditional `.width(Size::px(if collapsed {60.} else {274.}))`;
  optionally animate the width with `use_animation(|_| AnimNum::new(274.,60.).time(300))`. A bottom-pinned
  toggle needs the column in `.content(Content::Flex)` + a `Size::flex(1.0)` spacer (see layout rule).
- Drag-resizable panels (future): `ResizableContainer::new().direction(..).panel(ResizablePanel::new(
  PanelSize::px(n)).min_size(m).child(..))`. (`examples/component_resizable_container.rs`)

## Theming (we should migrate oxide-ui to this)

`crates/freya-components/src/theming/`; `examples/theme_*.rs`.
- Install at root: `let mut theme = use_init_theme(dark_theme);` then switch with `theme.set(other())`.
- `ColorsSheet` fields: `background`, `surface_primary/secondary/tertiary`, `text_primary/secondary/placeholder/
  inverse`, `primary/secondary/tertiary` (brand/accent), `border/border_focus`, `success/warning/error/info`.
- Custom theme: start from `dark_theme()`, override `theme.colors = ColorsSheet { primary: cyan, ..DARK_COLORS }`.
- Pull theme colors on elements: `rect().theme_background().theme_color()`, `label().theme_color()` /
  `.title()`/`.subtitle()`/`.body()`. (`element_expansions.rs`)
- Themeable custom component: `define_theme! { %[component] pub Foo { %[fields] background: Color, ... } }`
  + `get_theme!(&self.theme, FooThemePreference, "foo")`; register `theme.set("foo", FooThemePreference { ...,
  background: Preference::Reference("secondary") })`. (`examples/theme_definition.rs`)
- **oxide-ui currently hand-rolls a `Theme` struct of RGB constants** — the skill says prefer the real system.
  Migration tracked as a 2b improvement.

## Components / state / async (confirmations)

- Component = `#[derive(PartialEq)] struct + impl Component { fn render(&self) -> impl IntoElement }`. Use
  `ComponentOwned` (+`#[derive(Clone)]`, `fn render(mut self)`) when you'd otherwise clone several `self` fields.
- `use_state` → `State<T>` (Copy): `*s.read()`, `s.write()`, `s.set()`, `s.set_if_modified()`, `s.toggle()`.
- Hooks only at the top of `render`; never in conditionals/loops/handlers/async. Capture state into `move` closures.
- Async: Freya's `spawn(async move {..})` (NOT tokio::spawn) for UI-updating tasks; or `use_future(|| async {..})`
  for managed tasks; `use_hook(|| spawn(..))` for one-shot on mount. Enter a tokio runtime in `main` for
  tokio-ecosystem crates. (`crates/freya/src/_docs/_async.rs`)
- `Readable<T>`/`Writable<T>` are type-erased reactive props (`state.into_writable()`).

## Testing

`freya-testing`: `let (mut runner, state) = TestingRunner::new(app, (w,h).into(), |r| r.provide_root_context(
|| State::create(v)), 1.);` then `runner.sync_and_update()`, `runner.click_cursor((x,y))`, assert via
`runner.find(|_, el| Label::try_downcast(el).filter(|l| l.text.as_ref()=="..."))`. `runner.render_to_file(p)`
snapshots.
