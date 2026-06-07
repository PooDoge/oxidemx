import re

with open("iced_gtk_themer/src/widgets/grid.rs", "r") as f:
    text = f.read()

# remove double copyright and everything until the first pub fn resolve
text = text.split("pub fn resolve<Message>")[1]

# extract Assignment
assignment_impl = """
#[derive(Copy, Clone, Debug)]
#[must_use]
pub struct Assignment {
    pub(super) column: u16,
    pub(super) row: u16,
    pub(super) width: u16,
    pub(super) height: u16,
}

impl Default for Assignment {
    fn default() -> Self {
        Self::new()
    }
}

impl Assignment {
    pub const fn new() -> Self {
        Self {
            column: 0,
            row: 0,
            width: 1,
            height: 1,
        }
    }
    
    pub fn column(mut self, column: u16) -> Self {
        self.column = column;
        self
    }
    
    pub fn row(mut self, row: u16) -> Self {
        self.row = row;
        self
    }
    
    pub fn width(mut self, width: u16) -> Self {
        self.width = width;
        self
    }
    
    pub fn height(mut self, height: u16) -> Self {
        self.height = height;
        self
    }
}

impl From<(u16, u16)> for Assignment {
    fn from((column, row): (u16, u16)) -> Self {
        Self {
            column,
            row,
            width: 1,
            height: 1,
        }
    }
}

impl From<(u16, u16, u16, u16)> for Assignment {
    fn from((column, row, width, height): (u16, u16, u16, u16)) -> Self {
        Self {
            column,
            row,
            width,
            height,
        }
    }
}
"""

grid_impl = """
use iced::{Element, Renderer, Length, Padding, Alignment, Point, Size, Event, Rectangle, Vector};
use iced::advanced::{Widget, Shell, Clipboard, Layout, layout::{self, Limits, Node}, widget::{Tree, Operation}, mouse, renderer, overlay};
use taffy::geometry::{Line, Rect};
use taffy::style::{AlignItems, Dimension, Display, GridPlacement, Style};
use taffy::style_helpers::{auto, length};
use taffy::{AlignContent, TaffyTree};

pub fn grid<'a, Message>() -> Grid<'a, Message> {
    Grid::new()
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)]
pub fn resolve<Message>(
    renderer: &Renderer,
    limits: &Limits,
    items: &mut [Element<'_, Message>],
    assignments: &[Assignment],
    width: Length,
    height: Length,
    padding: Padding,
    column_alignment: Alignment,
    row_alignment: Alignment,
    justify_content: Option<AlignContent>,
    column_spacing: f32,
    row_spacing: f32,
    tree: &mut [Tree],
) -> Node {""" + text.split("pub struct Grid<'a, Message>")[0]

grid_struct = """
/// Responsively generates rows and columns of widgets based on its dimmensions.
#[must_use]
pub struct Grid<'a, Message> {
    children: Vec<Element<'a, Message>>,
    assignments: Vec<Assignment>,
    padding: Padding,
    column_alignment: Alignment,
    row_alignment: Alignment,
    justify_content: Option<AlignContent>,
    column_spacing: u16,
    row_spacing: u16,
    width: Length,
    height: Length,
    max_width: f32,
    column: u16,
    row: u16,
}

impl<Message> Default for Grid<'_, Message> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a, Message> Grid<'a, Message> {
    pub const fn new() -> Self {
        Self {
            children: Vec::new(),
            assignments: Vec::new(),
            padding: Padding::ZERO,
            column_alignment: Alignment::Start,
            row_alignment: Alignment::Start,
            justify_content: None,
            column_spacing: 4,
            row_spacing: 4,
            width: Length::Shrink,
            height: Length::Shrink,
            max_width: f32::INFINITY,
            column: 1,
            row: 1,
        }
    }
    
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }
    
    pub fn column_alignment(mut self, alignment: Alignment) -> Self {
        self.column_alignment = alignment;
        self
    }
    
    pub fn row_alignment(mut self, alignment: Alignment) -> Self {
        self.row_alignment = alignment;
        self
    }
    
    pub fn justify_content(mut self, justify_content: impl Into<Option<AlignContent>>) -> Self {
        self.justify_content = justify_content.into();
        self
    }
    
    pub fn column_spacing(mut self, spacing: u16) -> Self {
        self.column_spacing = spacing;
        self
    }
    
    pub fn row_spacing(mut self, spacing: u16) -> Self {
        self.row_spacing = spacing;
        self
    }
    
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }
    
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }
    
    pub fn max_width(mut self, max_width: f32) -> Self {
        self.max_width = max_width;
        self
    }

    /// Attach a new element with a given grid assignment.
    pub fn push(mut self, widget: impl Into<Element<'a, Message>>) -> Self {
        self.children.push(widget.into());

        self.assignments.push(Assignment {
            column: self.column,
            row: self.row,
            width: 1,
            height: 1,
        });

        self.column += 1;

        self
    }

    /// Attach a new element with custom properties
    pub fn push_with<W, S>(mut self, widget: W, setup: S) -> Self
    where
        W: Into<Element<'a, Message>>,
        S: Fn(Assignment) -> Assignment,
    {
        self.children.push(widget.into());

        self.assignments.push(setup(Assignment {
            column: self.column,
            row: self.row,
            width: 1,
            height: 1,
        }));

        self.column += 1;

        self
    }

    #[inline]
    pub fn insert_row(mut self) -> Self {
        self.row += 1;
        self.column = 1;
        self
    }
}

impl<Message: 'static + Clone> Widget<Message, iced::Theme, Renderer> for Grid<'_, Message> {
    fn children(&self) -> Vec<Tree> {
        self.children.iter().map(Tree::new).collect()
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(self.children.as_mut_slice());
    }

    fn size(&self) -> Size<Length> {
        Size::new(self.width, self.height)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let size = self.size();
        let limits = limits
            .max_width(self.max_width)
            .width(size.width)
            .height(size.height);

        resolve(
            renderer,
            &limits,
            &mut self.children,
            &self.assignments,
            self.width,
            self.height,
            self.padding,
            self.column_alignment,
            self.row_alignment,
            self.justify_content,
            f32::from(self.column_spacing),
            f32::from(self.row_spacing),
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
        clipboard: &mut dyn Clipboard,
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
        theme: &iced::Theme,
        style: &renderer::Style,
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
    ) -> Option<overlay::Element<'b, Message, iced::Theme, Renderer>> {
        overlay::from_children(
            &mut self.children,
            tree,
            layout,
            renderer,
            viewport,
            translation,
        )
    }

    #[cfg(feature = "a11y")]
    fn a11y_nodes(
        &self,
        layout: Layout<'_>,
        state: &Tree,
        p: mouse::Cursor,
    ) -> iced_accessibility::A11yTree {
        use iced_accessibility::A11yTree;
        A11yTree::join(
            self.children
                .iter()
                .zip(layout.children())
                .zip(state.children.iter())
                .map(|((c, c_layout), state)| {
                    c.as_widget().a11y_nodes(
                        c_layout.with_virtual_offset(layout.virtual_offset()),
                        state,
                        p,
                    )
                }),
        )
    }

    fn drag_destinations(
        &self,
        state: &Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        dnd_rectangles: &mut iced::advanced::clipboard::DndDestinationRectangles,
    ) {
        for ((e, c_layout), state) in self
            .children
            .iter()
            .zip(layout.children())
            .zip(state.children.iter())
        {
            e.as_widget().drag_destinations(
                state,
                c_layout.with_virtual_offset(layout.virtual_offset()),
                renderer,
                dnd_rectangles,
            );
        }
    }
}

impl<'a, Message: 'static + Clone> From<Grid<'a, Message>> for Element<'a, Message> {
    fn from(flex_row: Grid<'a, Message>) -> Self {
        Element::new(flex_row)
    }
}
"""

with open("iced_gtk_themer/src/widgets/grid.rs", "w") as f:
    f.write(grid_impl + grid_struct + assignment_impl)
