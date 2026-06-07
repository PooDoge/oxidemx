use iced::{Theme, Renderer};

// From iced_aw, license MIT

/// A widget that handles menu trees
use std::collections::HashMap;
use std::sync::Arc;

use super::menu_inner::{
    CloseCondition, Direction, ItemHeight, ItemWidth, Menu, MenuState, PathHighlight,
};
use super::menu_tree::MenuTree;

use super::style::StyleSheet;
use crate::widgets::wrapper::RcWrapper;
use iced::advanced::widget::tree::State;
use crate::widgets::menu::menu_inner::init_root_menu;

use iced::event::Status;
use iced::{Point, Shadow, Vector, window};
use iced::Border;
use iced::advanced::layout::{Limits, Node};
use iced::mouse::{self, Cursor};
use iced::advanced::renderer::{self, Renderer as IcedRenderer};
use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, overlay};
use iced::{Alignment, Element, Length, Padding, Rectangle, event, touch};


/// A `MenuBar` collects `MenuTree`s and handles all the layout, event processing, and drawing.
pub fn menu_bar<Message>(menu_roots: Vec<MenuTree<Message>>) -> MenuBar<Message>
where
    Message: Clone + 'static,
{
    MenuBar::new(menu_roots)
}

#[derive(Clone, Default)]
pub(crate) struct MenuBarState {
    pub(crate) inner: RcWrapper<MenuBarStateInner>,
}

pub(crate) struct MenuBarStateInner {
    pub(crate) tree: Tree,
        pub(crate) pressed: bool,
    pub(crate) bar_pressed: bool,
    pub(crate) view_cursor: Cursor,
    pub(crate) open: bool,
    pub(crate) active_root: Vec<usize>,
    pub(crate) horizontal_direction: Direction,
    pub(crate) vertical_direction: Direction,
    /// List of all menu states
    pub(crate) menu_states: Vec<MenuState>,
}
impl MenuBarStateInner {
    /// get the list of indices hovered for the menu
    pub(super) fn get_trimmed_indices(&self, index: usize) -> impl Iterator<Item = usize> + '_ {
        self.menu_states
            .iter()
            .skip(index)
            .take_while(|ms| ms.index.is_some())
            .map(|ms| ms.index.expect("No indices were found in the menu state."))
    }

    pub(crate) fn reset(&mut self) {
        self.open = false;
        self.active_root = Vec::new();
        self.menu_states.clear();
    }
}
impl Default for MenuBarStateInner {
    fn default() -> Self {
        Self {
            tree: Tree::empty(),
            pressed: false,
            view_cursor: Cursor::Available([-0.5, -0.5].into()),
            open: false,
            active_root: Vec::new(),
            horizontal_direction: Direction::Positive,
            vertical_direction: Direction::Positive,
            menu_states: Vec::new(),
                        bar_pressed: false,
        }
    }
}

pub(crate) fn menu_roots_children<Message>(menu_roots: &[MenuTree<Message>]) -> Vec<Tree>
where
    Message: Clone + 'static,
{
    /*
    menu bar
        menu root 1 (stateless)
            flat tree
        menu root 2 (stateless)
            flat tree
        ...
    */

    menu_roots
        .iter()
        .map(|root| {
            let mut tree = Tree::empty();
            let flat = root
                .flattern()
                .iter()
                .map(|mt| Tree::new(mt.item.clone()))
                .collect();
            tree.children = flat;
            tree
        })
        .collect()
}

#[allow(invalid_reference_casting)]
pub(crate) fn menu_roots_diff<Message>(menu_roots: &mut [MenuTree<Message>], tree: &mut Tree)
where
    Message: Clone + 'static,
{
    if tree.children.len() > menu_roots.len() {
        tree.children.truncate(menu_roots.len());
    }

    tree.children
        .iter_mut()
        .zip(menu_roots.iter())
        .for_each(|(t, root)| {
            let mut flat = root
                .flattern()
                .iter()
                .map(|mt| {
                    let widget = &mt.item;
                    let widget_ptr = widget as *const dyn Widget<Message, iced::Theme, Renderer>;
                    let widget_ptr_mut =
                        widget_ptr as *mut dyn Widget<Message, iced::Theme, Renderer>;
                    //TODO: find a way to diff_children without unsafe code
                    unsafe { &mut *widget_ptr_mut }
                })
                .collect::<Vec<_>>();

            t.diff_children(flat.as_mut_slice());
        });

    if tree.children.len() < menu_roots.len() {
        let extended = menu_roots[tree.children.len()..].iter().map(|root| {
            let mut tree = Tree::empty();
            let flat = root
                .flattern()
                .iter()
                .map(|mt| Tree::new(mt.item.clone()))
                .collect();
            tree.children = flat;
            tree
        });
        tree.children.extend(extended);
    }
}

pub fn get_mut_or_default<T: Default>(vec: &mut Vec<T>, index: usize) -> &mut T {
    if index < vec.len() {
        &mut vec[index]
    } else {
        vec.resize_with(index + 1, T::default);
        &mut vec[index]
    }
}

/// A `MenuBar` collects `MenuTree`s and handles all the layout, event processing, and drawing.
#[allow(missing_debug_implementations)]
pub struct MenuBar<Message> {
    width: Length,
    height: Length,
    spacing: f32,
    padding: Padding,
    bounds_expand: u16,
    main_offset: i32,
    cross_offset: i32,
    close_condition: CloseCondition,
    item_width: ItemWidth,
    item_height: ItemHeight,
    path_highlight: Option<PathHighlight>,
    menu_roots: Vec<MenuTree<Message>>,
    style: <iced::Theme as StyleSheet>::Style,
    }
impl<Message> Widget<Message, iced::Theme, Renderer> for MenuBar<Message>
where
    Message: Clone + 'static,
{
    fn size(&self) -> iced::Size<Length> {
        iced::Size::new(self.width, self.height)
    }

    fn diff(&mut self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<MenuBarState>();
        state
            .inner
            .with_data_mut(|inner| menu_roots_diff(&mut self.menu_roots, &mut inner.tree));
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<MenuBarState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(MenuBarState::default())
    }

    fn children(&self) -> Vec<Tree> {
        menu_roots_children::<Message>(&self.menu_roots)
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &Limits) -> Node {
        use super::flex;

        let limits = limits.width(self.width).height(self.height);
        let mut children = self
            .menu_roots
            .iter_mut()
            .map(|root| &mut root.item)
            .collect::<Vec<_>>();
        // the first children of the tree are the menu roots items
        let mut tree_children = tree
            .children
            .iter_mut()
            .map(|t| &mut t.children[0])
            .collect::<Vec<_>>();
        flex::resolve_wrapper(
            &flex::Axis::Horizontal,
            renderer,
            &limits,
            self.padding,
            self.spacing,
            Alignment::Center,
            &mut children,
            &mut tree_children,
        )
    }

    #[allow(clippy::too_many_lines)]
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &event::Event,
        layout: Layout<'_>,
        view_cursor: Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        use event::Event::{Mouse, Touch};
        use mouse::Button::Left;
        use mouse::Event::ButtonReleased;
        use touch::Event::{FingerLifted, FingerLost};

        process_root_events(
            &mut self.menu_roots,
            view_cursor,
            tree,
            event,
            layout,
            renderer,
            clipboard,
            shell,
            viewport,
        );

        let my_state = tree.state.downcast_mut::<MenuBarState>();

        // XXX this should reset the state if there are no other copies of the state, which implies no dropdown menus open.
        let reset = false
            && my_state
                .inner
                .with_data(|d| !d.open && !d.active_root.is_empty());

        let open = my_state.inner.with_data_mut(|state| {
            if reset {
                
            }
            state.open
        });

        match event {
            Mouse(mouse::Event::ButtonPressed(Left))
            | Touch(touch::Event::FingerPressed { .. })
                if view_cursor.is_over(layout.bounds()) =>
            {
                // TODO should we track that it has been pressed?
                shell.capture_event();
            }
            Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) | Touch(FingerLifted { .. } | FingerLost { .. }) => {
                let create_popup = my_state.inner.with_data_mut(|state| {
                    let mut create_popup = false;
                    if state.menu_states.is_empty() && view_cursor.is_over(layout.bounds()) {
                        state.view_cursor = view_cursor;
                        state.open = true;
                        create_popup = true;
                    
                        state.view_cursor = view_cursor;
                    }
                    create_popup
                });

                if !create_popup {
                    return;
                }
                shell.capture_event();
                #[cfg(all(
                    feature = "multi-window",
                    feature = "wayland",
                    target_os = "linux",
                    feature = "winit",
                    feature = "surface-message"
                ))]
                if matches!(WINDOWING_SYSTEM.get(), Some(WindowingSystem::Wayland)) {
                    self.create_popup(layout, view_cursor, renderer, shell, viewport, my_state);
                }
            }
            Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::CursorEntered)
                if open && view_cursor.is_over(layout.bounds()) =>
            {
                shell.capture_event();
                #[cfg(all(
                    feature = "multi-window",
                    feature = "wayland",
                    target_os = "linux",
                    feature = "winit",
                    feature = "surface-message"
                ))]
                if matches!(WINDOWING_SYSTEM.get(), Some(WindowingSystem::Wayland)) {
                    self.create_popup(layout, view_cursor, renderer, shell, viewport, my_state);
                }
            }
            _ => (),
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        view_cursor: Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<MenuBarState>();
        let cursor_pos = view_cursor.position().unwrap_or_default();
        state.inner.with_data_mut(|state| {
            let position = if state.open && (cursor_pos.x < 0.0 || cursor_pos.y < 0.0) {
                state.view_cursor
            } else {
                view_cursor
            };

            // draw path highlight
            if self.path_highlight.is_some() {
                let styling = theme.appearance(&self.style);
                if let Some(active) = state.active_root.first() {
                    let active_bounds = layout
                        .children()
                        .nth(*active)
                        .expect("Active child not found in menu?")
                        .bounds();
                    let path_quad = renderer::Quad {
                        bounds: active_bounds,
                        border: Border {
                            radius: styling.bar_border_radius.into(),
                            ..Default::default()
                        },
                        shadow: Shadow::default(),
                        snap: true,
                    };

                    renderer.fill_quad(path_quad, styling.path);
                }
            }

            self.menu_roots
                .iter()
                .zip(&tree.children)
                .zip(layout.children())
                .for_each(|((root, t), lo)| {
                    root.item.draw(
                        &t.children[root.index],
                        renderer,
                        theme,
                        style,
                        lo,
                        position,
                        viewport,
                    );
                });
        });
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        _renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, Renderer>> {
        #[cfg(all(
            feature = "multi-window",
            feature = "wayland",
            target_os = "linux",
            feature = "winit",
            feature = "surface-message"
        ))]
        if matches!(WINDOWING_SYSTEM.get(), Some(WindowingSystem::Wayland))
            && false
            && false
        {
            return None;
        }

        let state = tree.state.downcast_ref::<MenuBarState>();
        if state.inner.with_data(|state| !state.open) {
            return None;
        }

        Some(
            Menu {
                tree: state.clone(),
                menu_roots: std::borrow::Cow::Owned(self.menu_roots.clone()),
                bounds_expand: self.bounds_expand,
                menu_overlays_parent: false,
                close_condition: self.close_condition,
                item_width: self.item_width,
                item_height: self.item_height,
                bar_bounds: layout.bounds(),
                main_offset: self.main_offset,
                cross_offset: self.cross_offset,
                root_bounds_list: layout.children().map(|lo| lo.bounds()).collect(),
                path_highlight: self.path_highlight,
                style: std::borrow::Cow::Borrowed(&self.style),
                position: Point::new(translation.x, translation.y),
                is_overlay: true,
                                depth: 0,
                
            }
            .overlay(),
        )
    }
}
impl<Message> MenuBar<Message> {
    pub fn new(menu_roots: Vec<MenuTree<Message>>) -> Self {
        Self {
            menu_roots,
            width: Length::Shrink,
            height: Length::Shrink,
            spacing: 0.0,
            padding: iced::Padding::ZERO,
            main_offset: 0,
            cross_offset: 0,
            bounds_expand: 0,
            close_condition: CloseCondition {
                leave: true,
                click_outside: true,
                click_inside: true,
            },
            item_width: ItemWidth::Uniform(150),
            item_height: ItemHeight::Uniform(32),
            path_highlight: None,
            style: crate::widgets::menu::style::MenuBarStyle::Default,
        }
    }

}

impl<Message> From<MenuBar<Message>> for Element<'_, Message, iced::Theme, Renderer>
where
    Message: Clone + 'static,
{
    fn from(value: MenuBar<Message>) -> Self {
        Self::new(value)
    }
}

#[allow(unused_results, clippy::too_many_arguments)]
fn process_root_events<Message>(
    menu_roots: &mut [MenuTree<Message>],
    view_cursor: Cursor,
    tree: &mut Tree,
    event: &event::Event,
    layout: Layout<'_>,
    renderer: &Renderer,
    clipboard: &mut dyn Clipboard,
    shell: &mut Shell<'_, Message>,
    viewport: &Rectangle,
) {
    for ((root, t), lo) in menu_roots
        .iter_mut()
        .zip(&mut tree.children)
        .zip(layout.children())
    {
        // assert!(t.tag == tree::Tag::stateless());
        root.item.update(
            &mut t.children[root.index],
            event,
            lo,
            view_cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }
}
