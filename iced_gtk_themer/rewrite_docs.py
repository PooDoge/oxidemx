import re

with open("src/widgets/grid.rs", "r") as f:
    text = f.read()

new_docs = """/// A Grid widget for arranging elements in a multi-dimensional row and column structure.
///
/// This widget provides advanced layout capabilities similar to CSS Grid, allowing you
/// to position widgets across explicit rows and columns using `Assignment` specifications.
/// 
/// # Usage
/// ```rust,no_run
/// use iced::{Length, Padding};
/// use iced::widget::text;
/// use iced_gtk_themer::widgets::grid::{grid, Assignment};
///
/// let grid_widget = grid()
///     .width(Length::Fill)
///     .height(Length::Fill)
///     .padding(Padding::from(10))
///     .column_spacing(8)
///     .row_spacing(8)
///     // Simple sequential assignment (advances column by 1 for each push)
///     .push(text("Row 1, Column 1"))
///     .push(text("Row 1, Column 2"))
///     .insert_row() // Advance to next row, reset column
///     // Explicit assignment
///     .push_with(text("Row 2, Spanning 2 Columns"), |assign| {
///         assign.column(1).row(2).width(2)
///     });
/// ```
#[must_use]"""

text = text.replace("/// Responsively generates rows and columns of widgets based on its dimmensions.\n#[must_use]", new_docs)

with open("src/widgets/grid.rs", "w") as f:
    f.write(text)
