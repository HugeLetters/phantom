//! Tables: screens describe the columns and cells with rune-kit's [`Table`] builder and hand it to
//! [`Ui::table`], which draws it in the current look. Cells that need the look's colors come
//! from here too.

use rune::{Align, El, PointerPress, Tx, icon, row, text};

use rune_kit::TableStyle;
pub use rune_kit::{Cell, Table, TableColumn};

use super::{Icon, Tone, Ui};

/// The toggle button's size: compact, so a row keeps its height.
const TOGGLE_BUTTON: (f32, f32) = (24.0, 20.0);
/// How much of the toggle shows while it is disabled, as the kit's neutral button does.
const DISABLED: f32 = 0.5;

impl Ui {
    /// The table, filling the height its parent gives it.
    pub fn table<S: 'static>(&self, s: &S, table: Table<S>) -> El {
        table.style(self.table_style()).build(s, &self.k)
    }

    /// The kit's table style, with the look's row and header heights and cell padding.
    fn table_style(&self) -> TableStyle {
        let sp = &self.spacing;
        TableStyle { row_height: sp.row, header_height: sp.table_header, cell_pad: sp.cell_pad, ..TableStyle::from_kit(&self.k) }
    }

    /// Text as a table draws it, in `tone`: for a cell that is an element, see [`Ui::menu_cell`].
    pub fn cell_text(&self, text_: impl Into<String>, tone: Tone, mono: bool) -> El {
        let style = self.table_style();
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

    /// How wide a cell holding `text_` is, padding included, as [`Ui::cell_text`] sets it.
    pub fn cell_width(&self, text_: &str) -> f32 {
        let style = self.table_style();
        rune::text::measure(text_, &[], style.font, None).0 + 2.0 * style.cell_pad
    }

    /// The room a header's sort mark takes beside its title.
    const SORT_MARK: f32 = 16.0;

    /// How wide a column's header is with its sort mark, padding included.
    pub fn header_width(&self, title: &str) -> f32 {
        let style = self.table_style();
        rune::text::measure(title, &[], style.header_font, None).0 + 2.0 * style.cell_pad + Self::SORT_MARK
    }

    /// What a drag moves a column by, over its header cell: clear, tinted on hover and while its
    /// column is being moved (`active`).
    pub fn column_grip(&self, active: bool) -> El {
        let tint = self.colors.grip_active;
        let grip = rune::el().grow(1.0).h_full().radius(self.k.radius.sm).cursor(rune::Cursor::Pointer).hover(self.k.c.hover);
        if active { grip.fill(tint) } else { grip }
    }

    /// Text in `tone` that explains itself on hover: `help`.
    pub fn text_with_help(&self, s: impl Into<String>, tone: Tone, help: impl Into<String>) -> El {
        let color = if tone == Tone::Normal { self.k.c.text_base } else { self.tone_color(tone) };
        text(s.into()).font(self.k.ty.text_small).color(color).truncate().shrink().min_w(0.0).help(help)
    }

    /// A status that carries a warning: its text and a warning icon after it, both in the warning
    /// color, the whole thing explaining itself on hover with `why`. `key` names the icon.
    pub fn warned_text(&self, text_: impl Into<String>, why: impl Into<String>, key: impl Into<String>) -> El {
        let color = self.tone_color(Tone::Warning);
        let why = why.into();
        let icon = icon(self.icon(Icon::Warning), 14.0).color(color).key(key.into());
        row()
            .gap(self.spacing.s2)
            .center_y()
            .min_w(0.0)
            .child(text(text_.into()).font(self.k.ty.text_small).color(color).truncate().shrink().min_w(0.0))
            .child(icon)
            .help(why)
    }

    /// A row's toggle (#33): the icon for what a click does, `pausing` (❚❚) or resuming (▶), in a
    /// small rounded button: the look's neutral button fill at rest, a step stronger on hover,
    /// stronger again pressed. `off` is why it can't be pressed now (a torrent moving), when it can't:
    /// then it's the same button greyed, with that as its hover tip. The icon is muted, in full strength while `strong` (its row hovered
    /// or selected). It's a little under the row's height; the screen centres it in its cell. It's drawn
    /// through the look's icon lookup, so a theme can swap it. The screen gives it its press.
    pub fn row_toggle(&self, pausing: bool, strong: bool, off: Option<&str>) -> El {
        let (icon_, label) = if pausing { (Icon::PauseSolid, "Pause") } else { (Icon::ResumeSolid, "Resume") };
        let c = &self.k.c;
        let color = if strong { c.text_base } else { c.text_muted };
        let button = row()
            .key("toggle-button")
            .role("button")
            .label(label)
            .w(TOGGLE_BUTTON.0 - 2.0)
            .h(TOGGLE_BUTTON.1 - 2.0)
            .mx(1.0)
            .my(1.0)
            .center()
            .fill(c.bg_button_neutral)
            // Around the fill, not over it, so it takes in the row's own color (selected, hovered).
            .box_shadow(rune::Shadow { spread: 1.0, color: c.border_muted, ..Default::default() })
            .radius((self.k.radius.sm - 1.0).max(0.0))
            .child(icon(self.icon(icon_), 12.0).key("toggle-glyph").color(color));
        match off {
            None => button.cursor(rune::Cursor::Pointer).hover(c.hover).press(c.pressed).help(label),
            // Greyed to half, as the kit's neutral button is while disabled: no hover or press, and the
            // hover tip says why.
            Some(why) => button.disabled(true).opacity(DISABLED).help(why),
        }
    }

    /// [`Ui::menu_cell`], that also tells `on_hover` when the pointer comes into the cell or leaves
    /// it, and has it say whether that changed anything: the view is only rebuilt when it did.
    pub fn hover_cell<S: 'static>(
        &self,
        content: El,
        align: Align,
        on_menu: impl Fn(&mut S, PointerPress, &mut Tx) + 'static,
        on_hover: impl Fn(&mut S, bool) -> bool + 'static,
    ) -> Cell {
        let cell = row()
            .w_full()
            .h_full()
            .center_y()
            .on_secondary_press(on_menu)
            .on_hover_change(move |s: &mut S, inside, tx: &mut Tx| {
                if !on_hover(s, inside) {
                    tx.live_only();
                }
            })
            .child(content);
        Cell::el(match align {
            Align::Start => cell,
            Align::Center => cell.justify_center(),
            Align::End => cell.justify_end(),
        })
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

    /// `content`, pressable where `on_press` is given (the whole cell, keyed `press`), and answering a
    /// right-click anywhere in it with `on_menu`.
    pub fn press_menu_cell<S: 'static>(
        &self,
        content: El,
        on_press: Option<impl Fn(&mut S) + 'static>,
        on_menu: impl Fn(&mut S, PointerPress, &mut Tx) + 'static,
    ) -> Cell {
        let cell = row().w_full().h_full().center_y().on_secondary_press(on_menu).child(content);
        Cell::el(match on_press {
            Some(f) => cell.key("press").on_press(f),
            None => cell,
        })
    }
}
