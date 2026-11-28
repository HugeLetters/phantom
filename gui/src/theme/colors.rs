//! The colors Phantom's blocks draw that `Kit` has no field for: each a token named by what it's
//! for, which a theme sets ([`Colors::of`] derives Phantom's from its `Kit`). Blocks read these and
//! never compute a color themselves.

use rune::Color;
use rune_kit::Kit;

use crate::ui::Tone;

#[derive(Clone, Debug, PartialEq)]
pub struct Colors {
    /// A hairline over the page, made opaque so it doesn't tint the fill under it (popovers, the
    /// palette, a sheet's card).
    pub border_on_page: Color,
    /// The edge of a surface raised over the page (the notices popover).
    pub raised_edge: Color,
    /// A tooltip's edge: the raised edge in Light, about half as strong in Dark.
    pub tooltip_edge: Color,
    /// The fill of a surface raised over the page (the notices popover, a tooltip).
    pub raised_fill: Color,
    /// A dialog's surface, where it differs from the popover's: Light only.
    pub dialog_fill: Option<Color>,
    /// A wash over a dialog's surface that lifts it off the window: Dark only.
    pub dialog_wash: Option<Color>,
    /// A dialog's edge, over the scrim on the card behind.
    pub dialog_edge: Color,
    /// A notice count's oval, by tone.
    pub count_fill: [Color; 5],
    /// A column grip while it's dragged or hovered, over a table header.
    pub grip_active: Color,
    /// A quiet tile's fill in a sheet.
    pub tile_fill: Color,
    /// The press overlay that replaces the hover one (see [`pressed_over`]).
    pub pressed_over_hover: Color,
    /// The palette's highlighted row: the theme's Hover in Light; in Dark Hover is too faint on the
    /// layer, so Text at about the strength Hover has in Light.
    pub palette_selected: Color,
    /// A text field's sheen, top and bottom.
    pub field_sheen: (Color, Color),
}

impl Colors {
    /// Phantom's colors for `k`.
    pub fn of(k: &Kit) -> Self {
        let c = &k.c;
        let raised_edge_at = |alpha: f32| opaque_over(k.alpha(c.std_border, alpha), c.page);
        let raised_edge = raised_edge_at(if k.dark { 0.6 } else { 0.4 });
        let behind = opaque_over(c.scrim, c.bg_base);
        let dialog_edge = if k.dark { k.alpha(c.text_base, 0.18) } else { k.alpha(c.std_border, 0.5) };
        let count = |ink: Color, a: f32| k.alpha(ink, a);
        Self {
            border_on_page: opaque_over(c.border_weak, c.page),
            raised_edge,
            tooltip_edge: if k.dark { raised_edge_at(0.3) } else { raised_edge },
            raised_fill: if k.dark { c.page } else { c.surface_raised },
            dialog_fill: (!k.dark).then_some(c.layer_01),
            dialog_wash: k.dark.then(|| k.alpha(c.text_base, 0.06)),
            dialog_edge: opaque_over(dialog_edge, behind),
            count_fill: [
                count(c.text_base, 0.12),
                count(c.text_muted, 0.18),
                count(c.success.icon, 0.18),
                count(c.warning.icon, 0.18),
                count(c.danger.icon, 0.18),
            ],
            grip_active: k.alpha(c.text_accent, 0.16),
            tile_fill: k.alpha(c.text_base, 0.04),
            pressed_over_hover: pressed_over(c.hover, c.pressed),
            palette_selected: if k.dark { k.alpha(c.text_base, 0.09) } else { c.hover },
            field_sheen: (k.light(0.06), k.light(0.02)),
        }
    }

    /// A notice count's fill for `tone`.
    pub fn count_fill(&self, tone: Tone) -> Color {
        self.count_fill[match tone {
            Tone::Normal => 0,
            Tone::Muted => 1,
            Tone::Good => 2,
            Tone::Warning => 3,
            Tone::Error => 4,
        }]
    }
}

/// `color` as it looks over the opaque `behind`, in sRGB as Rune blends. For an edge: Rune draws a
/// translucent border over its own element's fill, so one that's to take in what's around it is
/// made opaque first.
pub(crate) fn opaque_over(color: Color, behind: Color) -> Color {
    let encode = |c: f32| if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    let decode = |c: f32| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
    let mix = |top: f32, under: f32| decode(encode(top) * color.a + encode(under) * (1.0 - color.a));
    Color { r: mix(color.r, behind.r), g: mix(color.g, behind.g), b: mix(color.b, behind.b), a: 1.0 }
}

/// The press overlay that, over the hover one the runtime already shows, gives `pressed` over the
/// fill (as the kit's own controls).
pub(crate) fn pressed_over(hover: Color, pressed: Color) -> Color {
    if hover.a >= 0.999 {
        return pressed;
    }
    pressed.alpha(((pressed.a - hover.a) / (1.0 - hover.a)).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each token is what its block computed on the spot before it was a token, in both Tones.
    #[test]
    fn tokens_equal_the_recipes_they_replace() {
        for dark in [false, true] {
            let k = Kit::neutral(dark);
            let c = &k.c;
            let t = Colors::of(&k);
            let edge = |a: f32| opaque_over(k.alpha(c.std_border, a), c.page);

            assert_eq!(t.border_on_page, opaque_over(c.border_weak, c.page));
            assert_eq!(t.raised_edge, edge(if dark { 0.6 } else { 0.4 }));
            assert_eq!(t.tooltip_edge, if dark { edge(0.3) } else { edge(0.4) });
            assert_eq!(t.raised_fill, if dark { c.page } else { c.surface_raised });
            assert_eq!(t.dialog_fill, if dark { None } else { Some(c.layer_01) });
            assert_eq!(t.dialog_wash, if dark { Some(k.alpha(c.text_base, 0.06)) } else { None });
            let dialog = if dark { k.alpha(c.text_base, 0.18) } else { k.alpha(c.std_border, 0.5) };
            assert_eq!(t.dialog_edge, opaque_over(dialog, opaque_over(c.scrim, c.bg_base)));
            for (tone, ink) in [
                (Tone::Normal, c.text_base),
                (Tone::Muted, c.text_muted),
                (Tone::Good, c.success.icon),
                (Tone::Warning, c.warning.icon),
                (Tone::Error, c.danger.icon),
            ] {
                assert_eq!(t.count_fill(tone), k.alpha(ink, if tone == Tone::Normal { 0.12 } else { 0.18 }));
            }
            assert_eq!(t.grip_active, k.alpha(c.text_accent, 0.16));
            assert_eq!(t.tile_fill, k.alpha(c.text_base, 0.04));
            assert_eq!(t.pressed_over_hover, pressed_over(c.hover, c.pressed));
            assert_eq!(t.field_sheen, (k.light(0.06), k.light(0.02)));
            // The palette's highlighted row: Hover in Light, stronger than it in Dark.
            assert_eq!(t.palette_selected, if dark { k.alpha(c.text_base, 0.09) } else { c.hover });
            assert!(dark.then_some(t.palette_selected.a > c.hover.a).unwrap_or(t.palette_selected == c.hover));
        }
    }
}
