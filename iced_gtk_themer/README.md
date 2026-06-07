# iced_gtk_themer

`iced_gtk_themer` is an advanced styling crate for [iced-rs](https://github.com/iced-rs/iced) that parses native GTK 3.0/4.0 CSS themes (like Adwaita, Libadwaita, or custom user themes) and dynamically generates high-fidelity `iced::Theme` and widget styling rules to perfectly match the Linux desktop environment. 

This crate also provides a large collection of high-level GTK/Libadwaita-inspired macro-widgets (like `BoxedList`, `PreferencesGroup`, `HeaderBar`, `SpinRow`, and more) designed to massively improve Developer Experience (DX) when building Linux applications.

## Features

- **Native GTK Theme Parsing**: Parses standard `gtk.css` files from `~/.themes` or `/usr/share/themes`.
- **Dynamic Semantic Color Mapping**: Maps GTK `@define-color` variables (e.g. `success_color`, `warning_color`, `destructive_color`) directly into `iced::Theme` palette colors.
- **Libadwaita Widget Gallery**: A massive collection of pre-built, ready-to-use widgets that perfectly mimic the Libadwaita design language:
  - `HeaderBar`
  - `BoxedList` & `ActionRow`
  - `PreferencesGroup`, `PreferencesPage`, `PreferencesDialog`
  - `ToggleGroup` / `SegmentedButton`
  - `SpinRow` & `ButtonRow`
  - `Dialog`, `Toaster`, `Banner`
  - `Spinner`, `Avatar`
  - `Carousel`, `ViewSwitcher`, `TabBar`
- **Extremely Ergonomic DX API**: Using extension traits, you can apply GTK styles easily using methods like `.gtk_style(gtk)`, `.primary(gtk)`, or `.card(gtk)`.

## Getting Started

1. Add `iced_gtk_themer` to your `Cargo.toml`.
2. Initialize the theme at startup.
3. Use the extension traits and custom widgets to build your UI.

```rust
use iced_gtk_themer::{GtkTheme, ext::*, HeaderBar, action_row, preferences_group, boxed_list};
use iced::widget::{column, text, button};

fn view(gtk: &GtkTheme) -> iced::Element<Message> {
    column![
        HeaderBar::new()
            .title("My App")
            .show_close_button(true)
            .gtk_style(gtk),
        
        boxed_list(
            gtk,
            vec![
                action_row("Network", Some("Configure network"), None::<iced::Element<Message>>),
                action_row("Bluetooth", Some("Configure bluetooth"), None::<iced::Element<Message>>),
            ]
        )
    ].into()
}
```

## DX Improvements included in this crate:

We have heavily focused on creating the best developer experience for Linux UI developers:

1. **Extension Traits**: No need to write complex `move |theme| style::...` closures everywhere. Just append `.gtk_style(gtk)` or `.primary(gtk)` to any standard `iced` widget to perfectly map its states (Hover, Active, Disabled) to the parsed GTK CSS colors.
2. **Zero Boilerplate Layouts**: Creating a Libadwaita boxed list manually takes hundreds of lines of `container` and `column` macros with custom borders. With `boxed_list()`, it's a single function call.
3. **Consistent Naming Conventions**: All widgets match standard GTK4/Libadwaita naming conventions (`ActionRow`, `HeaderBar`, etc.), making it easy for GTK developers to migrate to Rust + Iced.

## Architecture

This crate builds upon `pop_os_iced` (Cosmic's iced fork) to utilize advanced features like soft borders, rounded corners (`snap` rendering), and advanced `Text` layout features that are necessary to mimic GTK perfectly.

## License

MIT License
