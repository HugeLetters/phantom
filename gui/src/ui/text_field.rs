//! Single-line text field: `ui.text_field("magnet-field", &s.magnet, "Magnet link").on_edit(…)
//! .on_submit(…).build()`. It's as wide as its text unless it grows in its row (`.grow(1.0)` on
//! what's built) or is given a [`width`](TextField::width), which it fills whatever its text.

use rune::{El, TextEdit, Tx, el, row, text};
use rune_kit::TextInput;

use super::Ui;

pub struct TextField<'a> {
    ui: &'a Ui,
    key: String,
    edit: TextEdit,
    placeholder: String,
    width: Option<f32>,
    inner: TextInput,
}

impl Ui {
    /// `label` names the field for accessibility; `edit` is its text in the app's state.
    pub fn text_field(&self, key: impl Into<String>, edit: &TextEdit, label: &str) -> TextField<'_> {
        TextField {
            ui: self,
            key: key.into(),
            edit: edit.clone(),
            placeholder: String::new(),
            width: None,
            inner: TextInput::new(edit).label(label).width(None),
        }
    }
}

impl TextField<'_> {
    /// Shown while the field is empty.
    pub fn placeholder(mut self, text: &str) -> Self {
        self.placeholder = text.to_owned();
        self
    }

    /// Exactly this wide, as a setting's field is, whatever its text.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Its text set in `style` (the caret and selection follow it), in place of the kit's.
    pub fn text_style(mut self, style: rune::TextStyle) -> Self {
        self.inner = self.inner.text_style(style);
        self
    }

    /// No box of its own: no fill, border or focus ring, the caret and selection only. For a field
    /// that sits in something else, which draws how it shows the focus ([`Ui::in_place`]).
    pub fn plain(mut self) -> Self {
        self.inner = self.inner.plain();
        self
    }

    /// Exactly this tall, in place of the kit's 28 points.
    pub fn height(mut self, height: f32) -> Self {
        self.inner = self.inner.height(height);
        self
    }

    /// Where the field's text lives in the app's state.
    pub fn on_edit<S: 'static>(mut self, f: impl for<'a> Fn(&'a mut S) -> &'a mut TextEdit + 'static) -> Self {
        self.inner = self.inner.on_edit(f);
        self
    }

    /// Return was pressed in the field.
    pub fn on_submit<S: 'static>(mut self, f: impl Fn(&mut S, &mut Tx) + 'static) -> Self {
        self.inner = self.inner.on_submit(f);
        self
    }

    /// A key pressed in the field, before the rest of the app sees it; whether it took it.
    pub fn on_key<S: 'static>(mut self, f: impl Fn(&mut S, &rune::KeyEvent, &mut Tx) -> bool + 'static) -> Self {
        self.inner = self.inner.on_key(f);
        self
    }

    pub fn build(self) -> El {
        let width = self.width;
        let field = self.field();
        match width {
            Some(width) => row().w(width).child(field.grow(1.0).min_w(0.0)),
            None => field,
        }
    }

    fn field(self) -> El {
        let field = el().key(self.key).child(self.inner.build(&self.ui.k));
        if !self.edit.text.is_empty() || self.placeholder.is_empty() {
            return field;
        }
        // Drawn here rather than by the kit's field, whose placeholder wraps onto a second line
        // and spills out of the field when it's narrower than the text. Lines up with the text
        // the field would draw (its 2-point spacer and 6-point gap), and lets presses through.
        let hint = text(self.placeholder).font(self.ui.k.ty.ui_compact).color(self.ui.k.c.text_faint).truncate();
        field.child(row().pin(Some(8.0), Some(0.0), Some(8.0), Some(0.0)).center_y().inert().child(hint))
    }
}

/// Where a plain field puts its text's first character: its 2-point spacer and 6-point gap. A field
/// edited in place starts this far left of the text it replaces, so the text doesn't move.
pub const TEXT_INSET: f32 = 8.0;

/// How a field edited in place (a Name cell's) shows that it has the focus, drawn over `behind`,
/// the colour of what it sits on: Phantom's [`Blocks::field_focus`](super::Blocks) slot. The three
/// are drawn side by side for the user to pick one; `PHANTOM_FIELD_FOCUS` (underline, tint,
/// hairline) switches them, a debug switch until then.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusLook {
    /// An accent line under the text only, nothing around it.
    Underline,
    /// A soft wash of the accent behind it, no outline.
    Tint,
    /// A hairline outline in a muted colour.
    Hairline,
}

impl FocusLook {
    /// The look `PHANTOM_FIELD_FOCUS` names; the underline otherwise.
    pub fn from_env() -> FocusLook {
        match std::env::var("PHANTOM_FIELD_FOCUS").as_deref() {
            Ok("tint") => FocusLook::Tint,
            Ok("hairline") => FocusLook::Hairline,
            _ => FocusLook::Underline,
        }
    }
}

/// Phantom's field edited in place: opaque (it covers the cells beside it as it widens), the
/// colour of the row it's in, and `look` for its focus.
pub fn phantom_focus(ui: &Ui, look: FocusLook, field: El, behind: rune::Color) -> El {
    let k = &ui.k;
    let c = &k.c;
    let radius = k.radius.sm;
    let base = row().radius(radius).fill(c.bg_base);
    let face = row().w_full().h_full().radius(radius).fill(behind);
    let face = match look {
        FocusLook::Underline => {
            // Under the text, not the whole box: from the text's start to the field's end, less
            // the same inset there.
            let line = el().pin(Some(TEXT_INSET), None, Some(TEXT_INSET), Some(3.0)).h(1.5).fill(c.text_accent).inert();
            face.child(field).child(line)
        }
        FocusLook::Tint => face.child(row().w_full().h_full().radius(radius).fill(k.alpha(c.text_accent, 0.14)).child(field)),
        FocusLook::Hairline => face.border(1.0, k.alpha(c.text_muted, 0.45)).child(field),
    };
    base.key("focus-look").child(face)
}

impl Ui {
    /// `field`, built [`plain`](TextField::plain), as the look draws a field edited in place, over
    /// `behind`: the theme's [`Blocks::field_focus`](super::Blocks).
    pub fn in_place(&self, field: El, behind: rune::Color) -> El {
        (self.blocks.field_focus)(self, field, behind)
    }

    /// How wide a field edited in place is for all of `text` to show, set in a table's type.
    pub fn in_place_width(&self, text_: &str) -> f32 {
        let font = rune_kit::TableStyle::from_kit(&self.k).font;
        // A point to spare for the caret at the end, and as widths round.
        (rune::text::measure(text_, &[], font, None).0 + 2.0 * TEXT_INSET + 2.0).ceil()
    }

    /// A table's text style and a selected row's colour, for a field over a table's cell.
    pub fn table_cell_look(&self) -> (rune::TextStyle, rune::Color) {
        let style = rune_kit::TableStyle::from_kit(&self.k);
        (style.font, style.selected)
    }
}
