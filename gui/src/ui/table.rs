//! Tables: screens describe the columns and cells with rune-kit's [`Table`] builder and hand it to
//! [`Ui::table`], which draws it in the current look. Cells that need the look's colours come
//! from here too.

use rune::{Align, El, PointerPress, Tx, row, text};

use rune_kit::TableStyle;
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

    /// Text as a table draws it, in `tone`: for a cell that is an element, see [`Ui::menu_cell`].
    pub fn cell_text(&self, text_: impl Into<String>, tone: Tone, mono: bool) -> El {
        let style = TableStyle::from_kit(&self.k);
        let color = match tone {
            Tone::Normal => style.text,
            Tone::Muted => style.muted,
            Tone::Good | Tone::Warning | Tone::Error => self.tone_color(tone),
        };
        let text_ = text_.into();
        text(text_.clone())
            .font(if mono { style.mono } else { style.font })
            .color(color)
            .truncate()
            .shrink()
            .min_w(0.0)
            .help_when_truncated(text_)
    }

    /// Text in `tone` that explains itself on hover: `help`.
    pub fn text_with_help(&self, s: impl Into<String>, tone: Tone, help: impl Into<String>) -> El {
        let color = if tone == Tone::Normal { self.k.c.text_base } else { self.tone_color(tone) };
        text(s.into()).font(self.k.ty.text_small).color(color).truncate().shrink().min_w(0.0).help(help)
    }

    /// A cell whose whole area answers a right-click with `on_menu`: the table has no hook for a
    /// row, so every cell carries it.
    pub fn menu_cell<S: 'static>(&self, content: El, align: Align, on_menu: impl Fn(&mut S, PointerPress, &mut Tx) + 'static) -> Cell {
        let cell = row().w_full().h_full().center_y().on_secondary_press(on_menu).child(content);
        Cell::el(match align {
            Align::Start => cell,
            Align::Center => cell.justify_center(),
            Align::End => cell.justify_end(),
        })
    }

    /// `content`, pressable where `on_press` is given: the whole cell, keyed `press`.
    pub fn press_cell<S: 'static>(&self, content: El, on_press: Option<impl Fn(&mut S) + 'static>) -> Cell {
        let cell = row().w_full().h_full().center_y().child(content);
        Cell::el(match on_press {
            Some(f) => cell.key("press").on_press(f),
            None => cell,
        })
    }

    /// A progress bar with its percentage, `fraction` from 0.0 to 1.0.
    pub fn progress_cell(&self, fraction: f32) -> Cell {
        Cell::el(self.progress_bar(fraction))
    }
}
