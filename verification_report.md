# Verification Report for Iced 0.14 Advanced Guides

I have carefully reviewed the core architecture and theming guides against the Iced 0.14 source code. Most of the concepts and code snippets are highly accurate. However, there is a notable inaccuracy related to window dragging.

## Findings

### 1. `architecture_0.14.md`
**Status:** ✅ **Accurate**
- `iced::run` correctly takes `update` and `view` functions.
- The `Task` signature and `Task::perform(future, message)` usage correctly align with Iced 0.14's architecture.
- `From<()>` is correctly implemented for `Task`, so returning `()` in `update` functions automatically converts into `Task::none()`.
- `Subscription` and `time::every` are correctly referenced.

### 2. `shaders_guide.md`
**Status:** ✅ **Accurate**
- The `Pipeline` trait is indeed strongly-typed in Iced 0.14, and caching is built into the `Primitive` trait automatically.
- `Primitive::prepare` and `Primitive::draw` correctly match the `wgpu` backend API signatures.
- Reversing the Y-axis for the `Canvas` coordinate system in WGSL is a known and correct quirk.

### 3. `advanced_theming_and_windows.md`
**Status:** ❌ **Inaccurate** (Requires Update)
- **Inaccuracy**: The guide mentions `window::Id::MAIN` and `window::drag_window()`.
- **Reason**: In Iced 0.14, `window::Id::MAIN` no longer exists, and `window::drag_window()` has been removed in favor of `window::drag(id: window::Id)`.
- **Correction**: In modern Iced, you cannot assume a single constant `MAIN` ID. Instead, you need to either capture the window ID through `Event::Window(id, _)`, or retrieve it dynamically via `window::oldest()` when triggering window actions like dragging.

### 4. `custom_widgets.md`
**Status:** ✅ **Accurate**
- The custom widget lifecycle correctly references the `iced::advanced::widget::Widget` trait.
- `size`, `layout`, `draw`, `update` (formerly `on_event`), and `mouse_interaction` correctly correspond to the trait methods and their expected arguments (like `layout::Limits` and `widget::Tree`).

---

## Suggested Additional Documentation Topics

To further improve these areas, I recommend adding documentation covering the following topics:

1. **Managing Multiple Windows in Iced 0.14:**
   Since `window::Id::MAIN` is gone, users often struggle to figure out how to manage window identifiers. A dedicated guide explaining how to spawn multiple windows, cache their `window::Id`s, route messages to specific windows using `window::run`, and handle `Event::Window(id, _)` is essential.

2. **Migrating from `Command` to `Task` (Async/Await Workflows):**
   While `architecture_0.14.md` briefly explains `Task`, many users struggle with complex async state machines (e.g., streaming data, channels). A deep-dive guide on leveraging `Task::run`, integrating external `tokio` channels, and handling asynchronous errors cleanly would be highly beneficial.

3. **Advanced `widget::Tree` State Management:**
   `custom_widgets.md` touches upon `widget::Tree`, but building complex stateful widgets (like infinite scrolling lists or collapsible trees) requires deeper knowledge of `tree.state` and `tree.children`. An advanced guide explaining how to initialize and persist custom widget state across frames without polluting the application's global `State` would solve a major pain point for widget developers.
