//! Tables: screens describe the columns and cells with rune-kit's [`Table`] builder and hand it to
//! [`Ui::table`], which draws it in the current look. Cells that need the look's colours come
//! from here too.

use rune::{El, text};

pub use rune_kit::{Cell, Table, TableColumn};

use super::{Tone, Ui};

impl Ui {
    /// The table, filling the height its parent gives it.
    pub fn table<S: 'static>(&self, s: &S, table: Table<S>) -> El {
        table.build(s, &self.k)
    }

    /// Text in `tone`.
    pub fn text_cell(&self, text: impl Into<String>, tone: Tone) -> Cell {
        match tone {
            Tone::Normal => Cell::text(text),
            Tone::Muted => Cell::text(text).muted(),
            Tone::Good | Tone::Warning | Tone::Error => Cell::text(text).color(self.tone_color(tone)),
        }
    }

    /// Text in `tone` that explains itself on hover: `help`.
    pub fn text_cell_with_help(&self, s: impl Into<String>, tone: Tone, help: impl Into<String>) -> Cell {
        let color = if tone == Tone::Normal { self.k.c.text_base } else { self.tone_color(tone) };
        Cell::el(text(s.into()).font(self.k.ty.text_small).color(color).truncate().shrink().min_w(0.0).help(help))
    }

    /// A progress bar with its percentage, `fraction` from 0.0 to 1.0.
    pub fn progress_cell(&self, fraction: f32) -> Cell {
        Cell::el(self.progress_bar(fraction))
    }
}
