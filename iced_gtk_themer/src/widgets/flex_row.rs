use iced::core::layout::{Limits, Node};
use iced::core::widget::{Tree, Widget, Operation};
use iced::core::{Element, Event, Layout, Length, Padding, Point, Rectangle, Size, Vector, mouse, overlay, clipboard, Shell};
use taffy::geometry::Rect;
use taffy::style::{AlignItems, Dimension, Display, Style, FlexDirection, FlexWrap, AlignContent};
use taffy::style_helpers::length;
use taffy::TaffyTree;

/// Responsively generates rows of widgets based on the dimensions of its children.
///
/// `FlexRow` will wrap its children onto new lines if they exceed the available width,
/// making it perfect for dynamic layouts where the number of elements or their sizes
/// might vary.
///
/// # Example
/// ```rust,no_run
/// use iced::widget::text;
/// use iced_gtk_themer::widgets::flex_row::flex_row;
/// 
/// let row = flex_row(vec![
///     text("Item 1").into(),
///     text("Item 2").into(),
///     text("Item 3").into(),
/// ])
/// .spacing(10)
/// .padding(20);
/// ```
pub struct FlexRow<'a, Message, Theme = iced::core::Theme, Renderer = iced::Renderer> {
    children: Vec<Element<'a, Message, Theme, Renderer>>,
    padding: Padding,
    column_spacing: u16,
    row_spacing: u16,
    width: Length,
    min_item_width: Option<f32>,
    max_width: f32,
    align_items: Option<AlignItems>,
    justify_items: Option<AlignItems>,
    justify_content: Option<AlignContent>,
}

pub fn flex_row<'a, Message, Theme, Renderer>(
    children: Vec<Element<'a, Message, Theme, Renderer>>,
) -> FlexRow<'a, Message, Theme, Renderer> {
    FlexRow::new(children)
}

impl<'a, Message, Theme, Renderer> FlexRow<'a, Message, Theme, Renderer> {
    pub fn new(children: Vec<Element<'a, Message, Theme, Renderer>>) -> Self {
        Self {
            children,
            padding: Padding::ZERO,
            column_spacing: 4,
            row_spacing: 4,
            width: Length::Shrink,
            min_item_width: None,
            max_width: f32::INFINITY,
            align_items: None,
            justify_items: None,
            justify_content: None,
        }
    }

    /// Sets the padding around the widget.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Sets the space between each column of items.
    pub fn column_spacing(mut self, spacing: u16) -> Self {
        self.column_spacing = spacing;
        self
    }

    /// Sets the space between each item in a row.
    pub fn row_spacing(mut self, spacing: u16) -> Self {
        self.row_spacing = spacing;
        self
    }

    /// Sets the space between each column and row.
    pub fn spacing(mut self, spacing: u16) -> Self {
        self.column_spacing = spacing;
        self.row_spacing = spacing;
        self
    }

    /// Sets the width of the row.
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// Sets the maximum width of the row.
    pub fn max_width(mut self, max_width: f32) -> Self {
        self.max_width = max_width;
        self
    }

    /// Sets the minimum width of items that grow.
    pub fn min_item_width(mut self, min_item_width: impl Into<Option<f32>>) -> Self {
        self.min_item_width = min_item_width.into();
        self
    }

    /// Defines how content will be aligned horizontally.
    pub fn align_items(mut self, alignment: iced::core::Alignment) -> Self {
        self.align_items = Some(match alignment {
            iced::core::Alignment::Center => AlignItems::Center,
            iced::core::Alignment::Start => AlignItems::Start,
            iced::core::Alignment::End => AlignItems::End,
        });
        self
    }

    /// Defines how content will be aligned vertically.
    pub fn justify_items(mut self, alignment: iced::core::Alignment) -> Self {
        self.justify_items = Some(match alignment {
            iced::core::Alignment::Center => AlignItems::Center,
            iced::core::Alignment::Start => AlignItems::Start,
            iced::core::Alignment::End => AlignItems::End,
        });
        self
    }
}

impl<'a, Message: 'a, Theme: 'a, Renderer> Widget<Message, Theme, Renderer>
    for FlexRow<'a, Message, Theme, Renderer>
where
    Renderer: iced::core::Renderer,
{
    fn children(&self) -> Vec<Tree> {
        self.children.iter().map(Tree::new).collect()
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(self.children.as_mut_slice());
    }

    fn size(&self) -> Size<Length> {
        Size::new(self.width, Length::Shrink)
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &Limits) -> Node {
        let size = self.size();
        let limits = limits
            .max_width(self.max_width)
            .width(size.width)
            .height(size.height);

        resolve(
            renderer,
            &limits,
            &mut self.children,
            self.padding,
            f32::from(self.column_spacing),
            f32::from(self.row_spacing),
            self.min_item_width,
            self.justify_items,
            self.align_items,
            self.justify_content,
            &mut tree.children,
        )
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation<()>,
    ) {
        operation.traverse(&mut |operation| {
            self.children
                .iter_mut()
                .zip(&mut tree.children)
                .zip(layout.children())
                .for_each(|((child, state), c_layout)| {
                    child.as_widget_mut().operate(
                        state,
                        c_layout.with_virtual_offset(layout.virtual_offset()),
                        renderer,
                        operation,
                    );
                });
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn clipboard::Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        for ((child, state), c_layout) in self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            child.as_widget_mut().update(
                state,
                event,
                c_layout.with_virtual_offset(layout.virtual_offset()),
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((child, state), c_layout)| {
                child.as_widget().mouse_interaction(
                    state,
                    c_layout.with_virtual_offset(layout.virtual_offset()),
                    cursor,
                    viewport,
                    renderer,
                )
            })
            .max()
            .unwrap_or_default()
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &iced::core::renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        for ((child, state), c_layout) in self
            .children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
        {
            child.as_widget().draw(
                state,
                renderer,
                theme,
                style,
                c_layout.with_virtual_offset(layout.virtual_offset()),
                cursor,
                viewport,
            );
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        overlay::from_children(
            &mut self.children,
            tree,
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message: 'a, Theme: 'a, Renderer: 'a> From<FlexRow<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Renderer: iced::core::Renderer,
{
    fn from(flex_row: FlexRow<'a, Message, Theme, Renderer>) -> Self {
        Self::new(flex_row)
    }
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)]
pub fn resolve<Message, Theme, Renderer>(
    renderer: &Renderer,
    limits: &Limits,
    items: &mut [Element<'_, Message, Theme, Renderer>],
    padding: Padding,
    column_spacing: f32,
    row_spacing: f32,
    min_item_width: Option<f32>,
    justify_items: Option<AlignItems>,
    align_items: Option<AlignItems>,
    justify_content: Option<AlignContent>,
    tree: &mut [Tree],
) -> Node
where
    Renderer: iced::core::Renderer,
{
    let max_size = limits.max();

    let mut leafs = Vec::with_capacity(items.len());
    let mut nodes = Vec::with_capacity(items.len());

    let mut taffy_tree = TaffyTree::<()>::with_capacity(items.len() + 1);

    let style = Style {
        display: Display::Flex,
        flex_direction: FlexDirection::Row,
        flex_wrap: FlexWrap::Wrap,

        gap: taffy::geometry::Size {
            width: length(column_spacing),
            height: length(row_spacing),
        },

        min_size: taffy::geometry::Size {
            width: length(max_size.width),
            height: Dimension::auto(),
        },

        align_items,
        justify_items,
        justify_content,

        padding: Rect {
            left: length(padding.left),
            right: length(padding.right),
            top: length(padding.top),
            bottom: length(padding.bottom),
        },

        ..Style::default()
    };

    for (child, tree) in items.iter_mut().zip(tree.iter_mut()) {
        let child_widget = child.as_widget_mut();
        let child_node = child_widget.layout(tree, renderer, limits);
        let size = child_node.size();

        nodes.push(child_node);

        let c_size = child_widget.size();
        let (width, flex_grow, justify_self) = match c_size.width {
            Length::Fill | Length::FillPortion(_) => {
                (Dimension::auto(), 1.0, Some(AlignItems::Stretch))
            }
            _ => (length(size.width), 0.0, None),
        };

        let child_style = Style {
            flex_grow,

            min_size: taffy::geometry::Size {
                width: match min_item_width {
                    Some(width) => length(size.width.min(width)),
                    None => Dimension::auto(),
                },
                height: Dimension::auto(),
            },

            size: taffy::geometry::Size {
                width,
                height: match c_size.height {
                    Length::Fill | Length::FillPortion(_) => Dimension::auto(),
                    _ => length(size.height),
                },
            },

            justify_self,

            ..Style::default()
        };

        leafs.push(match taffy_tree.new_leaf(child_style) {
            Ok(leaf) => leaf,
            Err(_) => continue,
        });
    }

    let root = match taffy_tree.new_with_children(style, &leafs) {
        Ok(root) => root,
        Err(_) => return Node::new(Size::ZERO),
    };

    if let Err(_) = taffy_tree.compute_layout(
        root,
        taffy::geometry::Size {
            width: length(max_size.width),
            height: length(max_size.height),
        },
    ) {
        return Node::new(Size::ZERO);
    }

    let flex_layout = match taffy_tree.layout(root) {
        Ok(layout) => layout,
        Err(_) => return Node::new(Size::ZERO),
    };

    leafs
        .into_iter()
        .zip(items.iter_mut())
        .zip(nodes.iter_mut())
        .zip(tree)
        .for_each(|(((leaf, child), node), tree)| {
            let Ok(leaf_layout) = taffy_tree.layout(leaf) else {
                return;
            };

            let child_widget = child.as_widget_mut();
            let c_size = child_widget.size();
            match c_size.width {
                Length::Fill | Length::FillPortion(_) => {
                    *node =
                        child_widget.layout(tree, renderer, &limits.width(leaf_layout.size.width));
                }
                _ => (),
            }

            node.move_to_mut(Point {
                x: leaf_layout.location.x,
                y: leaf_layout.location.y,
            });
        });

    let actual_height = nodes
        .iter()
        .map(|node| node.bounds().y + node.bounds().height)
        .fold(0.0f32, f32::max);

    let size = Size {
        width: flex_layout.size.width,
        height: actual_height.max(flex_layout.size.height),
    };

    Node::with_children(size, nodes)
}
