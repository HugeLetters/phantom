//! A modal dialog over the window: `ui.dialog("clash", title, body, foot, close, on_key, &mut layer)`.
//! Its title is the question it asks; there's no message under it: what each choice does is its
//! tooltip.

use rune::{El, KeyEvent, Tx, col, el, row, text};
use rune_kit::{ControlSize, IconButton, IconButtonVariant, popover};

use super::Ui;

/// The widest a dialog gets.
const WIDTH: f32 = 520.0;

impl Ui {
    /// A dialog keyed `<key>-dialog`, in `layer` (which the page hands to [`Ui::root`]): `title`,
    /// wrapped, the question it asks, then `body`, then `foot` along its bottom. It's modal: the
    /// window under it takes nothing, keys while the focus is in it go to `on_key` first, and
    /// Escape or its × calls `on_close`. In Dark it's lighter than the window, with a hairline
    /// border, so it stands out from what it covers.
    #[allow(clippy::too_many_arguments)]
    pub fn dialog<S: 'static>(
        &self,
        key: &str,
        title: &str,
        body: El,
        foot: El,
        on_close: impl Fn(&mut S) + Clone + 'static,
        on_key: impl Fn(&mut S, &KeyEvent, &mut Tx) -> bool + 'static,
        layer: &mut Vec<El>,
    ) {
        let k = &self.k;
        let c = &k.c;
        let close = on_close.clone();
        let close_on_return = on_close.clone();
        let dismiss = IconButton::new(k.glyph.close)
            .variant(IconButtonVariant::Ghost)
            .control_size(ControlSize::Regular)
            .label("Close")
            .build(k)
            .key("close")
            .on_press(close)
            // With the focus, Return presses it, rather than the dialog's own default.
            .on_key(move |s: &mut S, ev: &KeyEvent, _: &mut Tx| {
                let enter = matches!(ev.key, rune::Key::Enter) && ev.mods == rune::Mods::default();
                if enter {
                    close_on_return(s);
                }
                enter
            })
            .size(20.0, 20.0)
            .mt(-2.0)
            .mr(-2.0);
        let title = text(title.to_owned()).key(format!("{key}-title")).font(k.ty.ui_medium.lh(20.0)).color(c.text_base).wrap();
        let head =
            row().key("header").w_full().gap(self.spacing.s6).items_start().child(col().grow(1.0).min_w(0.0).child(title)).child(dismiss);
        let panel = col()
            .key(format!("{key}-dialog"))
            .role("dialog")
            .w_full()
            .max_w(WIDTH)
            .padding(self.spacing.s7)
            .gap(self.spacing.dialog_gap)
            .child(head)
            .child(body)
            .child(foot)
            .on_key(on_key);
        // Dark: a wash of the text's color over the raised surface, lifting it off the window.
        let panel = match self.colors.dialog_wash {
            Some(wash) => panel.radius(k.radius.lg).fill(wash),
            None => panel,
        };
        let surface = popover::raised(k, panel, k.radius.lg);
        let surface = match self.colors.dialog_fill {
            Some(fill) => surface.fill(fill),
            None => surface,
        };
        let surface = surface.border(1.0, self.colors.dialog_edge);
        let surface = surface.modal(true, on_close).modal_click_outside(false).token_motion(k.motion.overlay);
        layer.push(el().key(format!("{key}-scrim")).absolute(0.0, 0.0).w_full().h_full().fill(c.scrim).a11y_hidden());
        layer.push(el().key(format!("{key}-layer")).absolute(0.0, 0.0).w_full().h_full().padding(self.spacing.s7).center().child(surface));
    }

    /// A labelled checkbox in a dialog; a press calls `on_toggle`, which flips it in the app's state.
    pub fn checkbox<S: 'static>(&self, key: &str, label: &str, on: bool, on_toggle: impl Fn(&mut S) + 'static) -> El {
        rune_kit::Checkbox::new(on).label(label).on_toggle(on_toggle).build(&self.k).key(key.to_owned())
    }

    /// A path in a dialog, muted: never cut, it wraps onto more lines when it's too long.
    pub fn dialog_path(&self, path: &str) -> El {
        let k = &self.k;
        text(path.to_owned()).font(k.ty.text_small).color(k.c.text_muted).wrap().shrink().min_w(0.0)
    }

    /// A line of a dialog's text, muted.
    pub fn dialog_line(&self, line: &str) -> El {
        let k = &self.k;
        text(line.to_owned()).font(k.ty.text_small).color(k.c.text_muted).max_lines(1)
    }
}
