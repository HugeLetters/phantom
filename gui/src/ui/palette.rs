//! A command palette: `ui.palette("palette", field, rows, at, on_pick, on_close, &mut layer)`, a
//! panel over the top of the window with a search field and the commands under it, each with its
//! icon and key, one highlighted, a group of them under a muted heading ([`PaletteRow::heading`]). Screens say what's listed and what a pick or a dismissal does;
//! the look is the theme's.

use rune::{El, Tx, col, el, icon, row, text};

use super::{Icon, Ui};

/// One command as the palette lists it.
pub struct PaletteRow {
    /// Some on the first row of a group: the muted heading drawn above it.
    pub heading: Option<String>,
    pub label: String,
    pub hint: Option<String>,
    /// None: an empty slot, so every label starts at the same x.
    pub icon: Option<Icon>,
    /// Muted after the label: where a setting lives, "Appearance ▸ Edit… ▸ Look".
    pub place: Option<String>,
    /// A setting's value, muted, at the right of the row, before its key.
    pub value: Option<String>,
    /// The current one of a setting's choices: highlighted for assistive technology too.
    pub selected: bool,
    /// What assistive technology reads for the row, where it's more than the label: its name,
    /// value and place.
    pub read: Option<String>,
}

/// The narrowest the panel is, however short its commands: it's placed so that, at this width, it's
/// centred in the window, and it keeps its left edge as it widens, so only its right edge moves.
const MIN_WIDTH: f32 = 440.0;
/// The widest it grows to, for a long name; a longer label is cut.
const MAX_WIDTH: f32 = 720.0;
/// How far from the window's sides it stays.
const SIDE: f32 = 16.0;
/// How far below the window's top the panel is: under the toolbar, as Spotlight sits high.
const TOP: f32 = 48.0;

impl Ui {
    /// The panel keyed `<key>-panel` in `layer` (which the page hands to [`Ui::root`]), `field`
    /// at its top, `rows` under it with the `at`-th highlighted, `empty` in their place when
    /// there are none. It's modal: the window under it takes nothing; Escape and a press outside
    /// call `on_close`, a press on a row `on_pick` with its index. The rows scroll when the window
    /// is too short for them; row `i` is keyed by `row_key(i)`. `choices`: the list's label while it
    /// lists one setting's choices ("Corners, 3 choices"). It is as wide as its widest row, between
    /// [`MIN_WIDTH`] and [`MAX_WIDTH`], inside the `window` (its width, when known), left edge fixed.

    #[allow(clippy::too_many_arguments)]
    pub fn palette<S: 'static>(
        &self,
        key: &str,
        field: El,
        choices: Option<String>,
        rows: Vec<PaletteRow>,
        at: usize,
        empty: &str,
        window: Option<f32>,
        row_key: impl Fn(usize) -> String,
        on_pick: impl Fn(&mut S, usize, &mut Tx) + Clone + 'static,
        on_close: impl Fn(&mut S, &mut Tx) + Clone + 'static,
        layer: &mut Vec<El>,
    ) {
        let k = &self.k;
        let c = &k.c;
        let (left, width) = self.palette_frame(&rows, window);
        let search = row()
            .key("search")
            .w_full()
            .px(self.spacing.s6)
            .py(self.spacing.s5)
            .gap(self.spacing.s4)
            .center_y()
            .child(icon(self.icon(Icon::Search), 16.0).color(c.icon_base).a11y_hidden())
            .child(field.grow(1.0).min_w(0.0));
        let mut list = col().key("rows").role("listbox").w_full().items_stretch().padding(self.spacing.s2);
        if let Some(label) = choices {
            list = list.label(label);
        }
        if rows.is_empty() {
            list = list.child(
                row()
                    .key("empty")
                    .h(self.spacing.palette_row)
                    .px(self.spacing.s5)
                    .center_y()
                    .child(text(empty.to_owned()).font(k.ty.ui_compact).color(c.text_faint)),
            );
        }
        for (i, r) in rows.into_iter().enumerate() {
            if let Some(heading) = r.heading {
                list = list.child(
                    row()
                        .key(format!("heading-{i}"))
                        .role("heading")
                        .label(heading.clone())
                        .w_full()
                        .h(self.spacing.palette_section)
                        .px(self.spacing.s5)
                        .pt(self.spacing.s4)
                        .center_y()
                        .child(text(heading).font(k.ty.small_medium).color(c.text_faint).truncate().grow(1.0).shrink()),
                );
            }
            let pick = on_pick.clone();
            let mut item = row()
                .key(row_key(i))
                .role("option")
                .label(r.read.unwrap_or_else(|| r.label.clone()))
                .focus_selected(r.selected)
                .h(self.spacing.palette_row)
                .px(self.spacing.s5)
                .gap(self.spacing.s5)
                .radius(k.radius.sm)
                .center_y()
                .child(match r.icon {
                    Some(i) => icon(self.icon(i), 14.0).key("icon").color(c.icon_base).a11y_hidden(),
                    // An empty slot the icon's size.
                    None => el().w(14.0).h(14.0),
                })
                .child(text(r.label).key("label").font(k.ty.ui_compact).color(c.text_base).truncate().shrink())
                .on_press_tx(move |s: &mut S, tx: &mut Tx| pick(s, i, tx));
            // What's left of the row is the place, which gives way first; the value and the key
            // keep their width.
            item = item.child(match r.place {
                Some(place) => {
                    text(place).key("place").font(k.ty.small_medium).color(c.text_faint).truncate().grow(1.0).shrink().a11y_hidden()
                }
                None => el().grow(1.0),
            });
            if let Some(value) = r.value {
                item = item.child(text(value).key("value").font(k.ty.ui_compact).color(c.text_muted).a11y_hidden());
            }
            if let Some(hint) = r.hint {
                item = item
                    .shortcut_hint(hint.clone())
                    .child(text(hint).key("hint").font(k.ty.small_medium).color(c.text_faint).a11y_hidden());
            }
            if i == at {
                item = item.fill(c.hover);
            }
            list = list.child(item);
        }
        let rows = col().key("scroll").w_full().shrink().min_h(0.0).scroll(rune::ScrollSpec { stick_bottom: false }).child(list);
        let mut panel = col()
            .key(format!("{key}-panel"))
            .role("dialog")
            .label("Commands")
            .w(width)
            .shrink()
            .min_h(0.0)
            .radius(k.radius.lg)
            .fill(c.layer_01)
            .border(1.0, self.colors.border_on_page)
            .child(search)
            .child(el().key("rule").w_full().h(1.0).fill(c.border_muted).a11y_hidden())
            .child(rows)
            .token_motion(k.motion.overlay);
        for shadow in k.shadow_layers(&k.elevation.overlay) {
            panel = panel.box_shadow(shadow);
        }
        let close = on_close.clone();
        layer.push(rune_kit::scrim_tx(format!("{key}-scrim"), move |s: &mut S, tx: &mut Tx| close(s, tx)));
        layer.push(
            col()
                .key(format!("{key}-layer"))
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .pt(TOP)
                .pb(self.spacing.s7)
                .pl(left)
                .pr(SIDE)
                .items_start()
                .child(panel.modal_tx(true, move |s: &mut S, tx: &mut Tx| on_close(s, tx))),
        );
    }

    /// Where the panel's left edge is in the window, and how wide it is: as wide as its widest row
    /// needs (its padding, icon, label and key, with the gaps), no less than [`MIN_WIDTH`], no more
    /// than [`MAX_WIDTH`] or the window leaves; its left edge is where it's centred at the minimum.
    fn palette_frame(&self, rows: &[PaletteRow], window: Option<f32>) -> (f32, f32) {
        let k = &self.k;
        let hint_style = rune::TextStyle { line_height: 11.0, ..k.ty.small_medium };
        let widest = rows
            .iter()
            .map(|r| {
                // The row's padding (10 a side), icon slot and gap (14 and 10), the key and its gap,
                // the list's padding (4 a side).
                let hint = r.hint.as_deref().map_or(0.0, |h| 10.0 + rune::text::measure(h, &[], hint_style, None).0);
                let place = r.place.as_deref().map_or(0.0, |p| 10.0 + rune::text::measure(p, &[], hint_style, None).0);
                let value = r.value.as_deref().map_or(0.0, |v| 10.0 + self.label_width(v));
                8.0 + 20.0 + 14.0 + 10.0 + self.label_width(&r.label) + place + value + hint
            })
            .fold(0.0, f32::max)
            .ceil()
            + 2.0;
        // Centred at the minimum (or the window's width less its sides, when narrower), and growing
        // right to the window's side at most.
        let at_minimum = window.map_or(MIN_WIDTH, |w| MIN_WIDTH.min(w - 2.0 * SIDE));
        let left = window.map_or(SIDE, |w| (w - at_minimum) / 2.0);
        let room = window.map_or(f32::MAX, |w| w - left - SIDE);
        (left, widest.clamp(MIN_WIDTH, MAX_WIDTH).min(room))
    }
}
