//! Tables: screens describe the columns and cells with rune-kit's [`Table`] builder and hand it to
//! [`Ui::table`], which draws it in the current look. Cells that need the look's colours come
//! from here too.

use rune::El;

pub use rune_kit::{Cell, Table, TableColumn};

use super::Ui;

impl Ui {
    /// The table, filling the height its parent gives it.
    pub fn table<S: 'static>(&self, s: &S, table: Table<S>) -> El {
        table.build(s, &self.k)
    }

    /// Text in the look's error colour.
    pub fn error_cell(&self, text: impl Into<String>) -> Cell {
        Cell::text(text).color(self.k.c.danger.icon)
    }

    /// A progress bar with its percentage, `fraction` from 0.0 to 1.0.
    pub fn progress_cell(&self, fraction: f32) -> Cell {
        Cell::el(self.progress_bar(fraction))
    }
}
