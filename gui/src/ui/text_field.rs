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
    plain: bool,
}

impl Ui {
    /// Whether `placeholder` fits a field `width` points wide, whole.
    pub fn placeholder_fits(&self, placeholder: &str, width: f32) -> bool {
        rune::text::measure(placeholder, &[], self.k.ty.ui_compact, None).0 <= width - 2.0 * PLACEHOLDER_INSET
    }

    /// `label` names the field for accessibility; `edit` is its text in the app's state.
    pub fn text_field(&self, key: impl Into<String>, edit: &TextEdit, label: &str) -> TextField<'_> {
        TextField {
            ui: self,
            key: key.into(),
            edit: edit.clone(),
            placeholder: String::new(),
            width: None,
            inner: TextInput::new(edit).label(label).width(None),
            plain: false,
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
        self.plain = true;
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
        let k = &self.ui.k;
        let mut inner = self.inner.build(k);
        if !self.plain {
            // The kit's field is flat, its focus ring inside its edge; Phantom's keeps a sheen, and
            // its ring around it.
            inner = inner.gradient(self.ui.colours.field_sheen.0, self.ui.colours.field_sheen.1);
            if self.edit.focused {
                inner = inner.outline(1.0, 0.0, k.c.border_focus);
            }
        }
        let field = el().key(self.key).child(inner);
        if !self.edit.text.is_empty() || self.placeholder.is_empty() {
            return field;
        }
        // Drawn here rather than by the kit's field, whose placeholder wraps onto a second line
        // and spills out of the field when it's narrower than the text. Lines up with the text
        // the field would draw (its 2-point spacer and 6-point gap), and lets presses through.
        let hint = text(self.placeholder).font(self.ui.k.ty.ui_compact).color(self.ui.k.c.text_faint).truncate();
        field.child(row().pin(Some(PLACEHOLDER_INSET), Some(0.0), Some(PLACEHOLDER_INSET), Some(0.0)).center_y().inert().child(hint))
    }
}

/// Where the placeholder starts and ends in its field.
const PLACEHOLDER_INSET: f32 = 8.0;
/// Where a plain field puts its text's first character: its 2-point spacer and 6-point gap. A field
/// edited in place starts this far left of the text it replaces, so the text doesn't move.
pub const TEXT_INSET: f32 = 8.0;

/// How a field edited in place (the details pane's name) shows that it has the focus, drawn over
/// `behind`, the colour of what it sits on: Phantom's [`Blocks::field_focus`](super::Blocks) slot,
/// an accent line under the text and nothing around it. Opaque: it covers the cell's own text, in
/// the colour of the row it's in.
pub fn phantom_focus(ui: &Ui, field: El, behind: rune::Color) -> El {
    let k = &ui.k;
    let c = &k.c;
    let radius = k.radius.sm;
    let base = row().radius(radius).fill(c.bg_base);
    let face = row().w_full().h_full().radius(radius).fill(behind);
    // Under the text, not the whole box: from the text's start to the field's end, less the same
    // inset there.
    let line = el().pin(Some(TEXT_INSET), None, Some(TEXT_INSET), Some(3.0)).h(1.5).fill(c.text_accent).inert();
    base.key("focus-look").child(face.child(field).child(line))
}

impl Ui {
    /// `field`, built [`plain`](TextField::plain), as the look draws a field edited in place, over
    /// `behind`: the theme's [`Blocks::field_focus`](super::Blocks).
    pub fn in_place(&self, field: El, behind: rune::Color) -> El {
        (self.blocks.field_focus)(self, field, behind)
    }

    /// A [`heading`](Self::heading)'s text style and the panel's colour, for a field in place of a
    /// heading (the details pane's name).
    pub fn heading_field_look(&self) -> (rune::TextStyle, rune::Color) {
        (self.k.ty.ui_medium, self.k.c.bg_base)
    }
}
