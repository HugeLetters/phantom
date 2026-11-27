//! Phantom's App icon, the ghost, drawn from its own picture (`design/icons/picks/phantom.svg`)
//! with a Tile color (#417). Original is the shipped icon, untouched. A tile color, the look's
//! Accent (Follow the look) or one set by hand, tints it as mapping B of the #417 mockups has it:
//! the tile a gradient of that color, 10% lighter at the top and darker at the bottom (25% in a
//! Light look, 45% in a Dark one); the ghost the look's Surface in Light and its Text in Dark; the
//! eyes the tile color darkened (55%, 60%). A half whose look isn't Phantom takes the ghost from
//! a fresh install's look for that half, and can't follow the look.

use super::super::Look;
use super::super::icon::{IconSetting, IconValues, ThemeIcon, Tint};
use super::super::preset::Appearance;
use super::super::setting::Rgb;

/// The tile's color setting.
pub const TILE: &str = "tile";

pub static ICONS: [ThemeIcon; 1] = [ThemeIcon { id: "phantom", settings: &[IconSetting { key: TILE, label: "Tile" }], draw, color }];

/// The picture the shipped icon is rendered from.
const PICTURE: &str = include_str!("../../../../design/icons/picks/phantom.svg");

/// What the picture's colors are, as it's written: the tile's gradient, the ghost and the eyes.
const TILE_TOP: &str = r##"<stop offset="0" stop-color="#2a2a2c"/>"##;
const TILE_BOTTOM: &str = r##"<stop offset="1" stop-color="#0a0a0a"/></linearGradient>"##;
const GHOST: &str = r##"fill="#ededed" stroke="#ededed""##;
const EYE: &str = r##"fill="#0a0a0a"/>"##;

/// The tile's own color, as its swatch shows it while the icon is Original: the top of its gradient.
const ORIGINAL: Rgb = Rgb([0x2a, 0x2a, 0x2c]);

/// The tile color `values` give in `half`: `None` for the icon's own.
fn tile(values: &IconValues, look: Option<&Look>) -> Option<Rgb> {
    match values.get(TILE).copied().unwrap_or_default() {
        Tint::Original => None,
        Tint::Follow => look.and_then(|l| l.resolved.color("accent")),
        Tint::Color(rgb) => Some(rgb),
    }
}

fn color(key: &str, values: &IconValues, look: Option<&Look>, _: Appearance) -> Rgb {
    debug_assert_eq!(key, TILE);
    tile(values, look).unwrap_or(ORIGINAL)
}

fn draw(values: &IconValues, look: Option<&Look>, half: Appearance) -> Option<String> {
    let tile = tile(values, look)?;
    let fresh;
    let look = match look {
        Some(look) => look,
        None => {
            fresh = Look::build(&super::theme_look(half == Appearance::Dark));
            &fresh
        }
    };
    let dark = look.ui.tokens().dark;
    let ghost = look.resolved.color(if dark { "text" } else { "surface" }).unwrap_or(Rgb([0xed, 0xed, 0xed]));
    let top = mix(tile, Rgb([0xff; 3]), 0.10);
    let bottom = mix(tile, Rgb([0; 3]), if dark { 0.45 } else { 0.25 });
    let eye = mix(tile, Rgb([0; 3]), if dark { 0.60 } else { 0.55 });
    Some(
        PICTURE
            .replacen(TILE_TOP, &format!(r#"<stop offset="0" stop-color="{}"/>"#, top.hex()), 1)
            .replacen(TILE_BOTTOM, &format!(r#"<stop offset="1" stop-color="{}"/></linearGradient>"#, bottom.hex()), 1)
            .replacen(GHOST, &format!(r#"fill="{0}" stroke="{0}""#, ghost.hex()), 1)
            .replace(EYE, &format!(r#"fill="{}"/>"#, eye.hex())),
    )
}

/// `a` moved `t` of the way to `b`, byte by byte, as the mockups mixed them.
fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    Rgb(std::array::from_fn(|i| (f32::from(a.0[i]) + (f32::from(b.0[i]) - f32::from(a.0[i])) * t).round() as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(tint: Tint) -> IconValues {
        [(TILE.to_owned(), tint)].into()
    }

    /// Every color the tint changes is where the picture says: one tile top and bottom, one ghost,
    /// two eyes (and the tile's 1-px hem, `#0a0a0a` too, is no eye).
    #[test]
    fn the_picture_has_each_color_the_tint_changes() {
        assert_eq!(PICTURE.matches(TILE_TOP).count(), 1);
        assert_eq!(PICTURE.matches(TILE_BOTTOM).count(), 1);
        assert_eq!(PICTURE.matches(GHOST).count(), 1);
        assert_eq!(PICTURE.matches(EYE).count(), 2);
    }

    #[test]
    fn original_is_the_shipped_picture_and_its_swatch_the_tiles_own() {
        let look = super::super::look(false);
        assert_eq!(draw(&IconValues::new(), Some(&look), Appearance::Light), None);
        assert_eq!(draw(&values(Tint::Original), Some(&look), Appearance::Dark), None);
        assert_eq!(color(TILE, &IconValues::new(), Some(&look), Appearance::Light), ORIGINAL);
    }

    /// Following the look, the tile is its Accent and the ghost its Surface (Light) or Text (Dark);
    /// with no Phantom look to follow, it's the icon's own.
    #[test]
    fn follow_the_look_tints_with_its_accent() {
        for dark in [false, true] {
            let look = super::super::look(dark);
            let accent = look.resolved.color("accent").unwrap();
            let ghost = look.resolved.color(if dark { "text" } else { "surface" }).unwrap();
            let half = if dark { Appearance::Dark } else { Appearance::Light };
            let svg = draw(&values(Tint::Follow), Some(&look), half).expect("drawn");
            assert!(svg.contains(&format!(r#"stop-color="{}""#, mix(accent, Rgb([0xff; 3]), 0.1).hex())), "the tile's top");
            assert!(svg.contains(&format!(r#"fill="{0}" stroke="{0}""#, ghost.hex())), "the ghost");
            assert_eq!(color(TILE, &values(Tint::Follow), Some(&look), half), accent);
            assert_eq!(draw(&values(Tint::Follow), None, half), None, "nothing to follow");
        }
    }

    /// A color set by hand draws on any half, and the drawing is an SVG resvg (Rune's) renders.
    #[test]
    fn a_hand_set_tile_draws_and_renders() {
        let blue = Rgb([0x3f, 0x7e, 0xd0]);
        let svg = draw(&values(Tint::Color(blue)), None, Appearance::Dark).expect("drawn");
        assert!(svg.contains(&format!(r#"stop-color="{}""#, mix(blue, Rgb([0; 3]), 0.45).hex())), "Dark's bottom");
        assert_eq!(svg.matches(&format!(r#"fill="{}"/>"#, mix(blue, Rgb([0; 3]), 0.6).hex())).count(), 2, "both eyes");
        let (w, h, rgba) = rune::image::svg_rgba(svg.as_bytes(), 64).expect("renders");
        assert_eq!((w, h), (64, 64));
        // The tile's middle is blue-ish.
        let at = ((32 * 64 + 12) * 4) as usize;
        assert!(rgba[at + 2] > rgba[at], "blue over red at the tile's edge: {:?}", &rgba[at..at + 4]);
    }
}
