// Copyright 2024
// DX Improvements: Extension traits for fluent GTK styling.

use iced::widget::{Button, Container, TextInput, Checkbox, Radio, Slider, PickList};
use iced::Theme;
use crate::GtkTheme;
use std::borrow::Borrow;

pub trait ButtonExt<'a, Message> {
    fn primary(self, theme: &'a GtkTheme) -> Self;
    fn secondary(self, theme: &'a GtkTheme) -> Self;
    fn destructive(self, theme: &'a GtkTheme) -> Self;
}

impl<'a, Message: Clone + 'a> ButtonExt<'a, Message> for Button<'a, Message> {
    fn primary(self, theme: &'a GtkTheme) -> Self {
        self.style(move |_: &Theme, status| theme.button_primary(status))
    }

    fn secondary(self, theme: &'a GtkTheme) -> Self {
        self.style(move |_: &Theme, status| theme.button_secondary(status))
    }

    fn destructive(self, theme: &'a GtkTheme) -> Self {
        self.style(move |_: &Theme, status| theme.button_destructive(status))
    }
}

pub trait ContainerExt<'a, Message> {
    fn card(self, theme: &'a GtkTheme) -> Self;
}

impl<'a, Message: Clone + 'a> ContainerExt<'a, Message> for Container<'a, Message> {
    fn card(self, theme: &'a GtkTheme) -> Self {
        self.style(move |_: &Theme| theme.container_card())
    }
}

pub trait TextInputExt<'a, Message> {
    fn gtk_style(self, theme: &'a GtkTheme) -> Self;
}

impl<'a, Message: Clone + 'a> TextInputExt<'a, Message> for TextInput<'a, Message> {
    fn gtk_style(self, theme: &'a GtkTheme) -> Self {
        self.style(move |_: &Theme, status| theme.text_input(status))
    }
}

pub trait CheckboxExt<'a, Message> {
    fn gtk_style(self, theme: &'a GtkTheme) -> Self;
}

impl<'a, Message: Clone + 'a> CheckboxExt<'a, Message> for Checkbox<'a, Message> {
    fn gtk_style(self, theme: &'a GtkTheme) -> Self {
        self.style(move |_: &Theme, status| theme.checkbox(status))
    }
}

pub trait RadioExt<'a, Message> {
    fn gtk_style(self, theme: &'a GtkTheme) -> Self;
}

impl<'a, Message: Clone + 'a> RadioExt<'a, Message> for Radio<'a, Message> {
    fn gtk_style(self, theme: &'a GtkTheme) -> Self {
        self.style(move |_: &Theme, status| theme.radio(status))
    }
}

pub trait SliderExt<'a, T, Message> {
    fn gtk_style(self, theme: &'a GtkTheme) -> Self;
}

impl<'a, T: Copy + From<u8> + std::cmp::PartialOrd, Message: Clone + 'a> SliderExt<'a, T, Message> for Slider<'a, T, Message> {
    fn gtk_style(self, theme: &'a GtkTheme) -> Self {
        self.style(move |_: &Theme, status| theme.slider(status))
    }
}

pub trait PickListExt<'a, T, L, V, Message> {
    fn gtk_style(self, theme: &'a GtkTheme) -> Self;
}

impl<'a, T: std::fmt::Display + PartialEq + Clone + ToString + 'a, L: std::borrow::Borrow<[T]> + 'a, V: std::borrow::Borrow<T> + 'a, Message: Clone + 'a> PickListExt<'a, T, L, V, Message> for PickList<'a, T, L, V, Message> {
    fn gtk_style(self, theme: &'a GtkTheme) -> Self {
        self.style(move |_: &Theme, status| theme.pick_list(status))
    }
}
