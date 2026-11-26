//! Every tooltip (#351) is one look, Phantom's, drawn by Rune: the card Settings and the main
//! window show on hover, in Dark and Light, whole (wrapped, never cut).

use crate::model::Phantom;
use crate::service::MemoryService;
use rune::Runtime;
use rune::render::{Instance, KIND_RECT};

use super::window::tears;
use super::{app_in, wait};

const PATH: &str = "Tears of Steel (2012) Open Movie Project by the Blender Foundation, the 4K Remastered Edition, \
    in a folder with a name long enough that no single line of a small tooltip could ever hold it whole.mkv";

/// The tooltip's card, as drawn, once the pointer has rested on `key`: its text, its box (points)
/// and its rectangle.
fn card_on<S: 'static>(rt: &mut Runtime<S>, key: &str) -> (String, [f32; 4], Instance) {
    let (text, tip, card, _) = card_over(rt, key);
    (text, tip, card)
}

/// [`card_on`], and the fill of the pane the card is drawn over (the last rectangle under its
/// centre, drawn before it).
fn card_over<S: 'static>(rt: &mut Runtime<S>, key: &str) -> (String, [f32; 4], Instance, [f32; 4]) {
    let at = super::generated::rect_of(rt, key).unwrap_or_else(|| panic!("no {key}"));
    rt.pointer_move(at[0] + at[2] / 2.0, at[1] + at[3] / 2.0);
    wait(rt, 0.1);
    wait(rt, 0.9);
    let (text, tip) = rt.help_tooltip().map(|(t, r)| (t.to_owned(), r)).unwrap_or_else(|| panic!("{key}: no tooltip"));
    let scale = 2.0;
    let card = rt
        .instances
        .iter()
        .rev()
        .find(|i| i.params[1] == KIND_RECT && (i.rect[0] - tip[0] * scale).abs() < 1.0 && (i.rect[1] - tip[1] * scale).abs() < 1.0)
        .copied()
        .unwrap_or_else(|| panic!("{key}: no card at {tip:?}"));
    let (cx, cy) = (card.rect[0] + card.rect[2] / 2.0, card.rect[1] + card.rect[3] / 2.0);
    let at = rt.instances.iter().position(|i| i.rect == card.rect && i.fill == card.fill).expect("the card");
    let behind = rt.instances[..at]
        .iter()
        .rev()
        .find(|i| {
            i.params[1] == KIND_RECT
                && i.fill[3] == 1.0
                && i.rect[0] <= cx
                && cx <= i.rect[0] + i.rect[2]
                && i.rect[1] <= cy
                && cy <= i.rect[1] + i.rect[3]
        })
        .map_or(card.fill, |i| i.fill);
    rt.pointer_move(1.0, 1.0);
    wait(rt, 0.3);
    (text, tip, card, behind)
}

fn main_cards(dark: bool) -> Vec<(String, [f32; 4], Instance)> {
    let mut rt = app_in(tears(PATH), dark);
    super::tap(&mut rt, "rows/1");
    super::settle(&mut rt);
    rt.set_viewport((700.0, 700.0), 2.0);
    wait(&mut rt, 0.2);
    ["add-file", "details-name", "details-location"].into_iter().map(|k| card_on(&mut rt, k)).collect()
}

/// One look: every tooltip's card has the same fill, edge and corners, and the edge shows in both
/// Appearances (opaque, a point wide), unlike Rune's own hairline of translucent white.
#[test]
fn every_tooltip_is_one_look_with_a_visible_edge_in_both_appearances() {
    let mut seen = Vec::new();
    for dark in [true, false] {
        let cards = main_cards(dark);
        let mut settings = super::settings::appearance_tab();
        let settings_card = card_on(&mut settings.settings, "app_icon-note");
        for (text, _, card) in cards.iter().chain([&settings_card]) {
            assert_eq!(card.border[3], 1.0, "dark {dark}: {text:?}: an opaque edge");
            assert!(card.params[0] >= 2.0, "dark {dark}: {text:?}: at least a point wide: {}", card.params[0]);
        }
        let first = cards[0].2;
        for (text, _, card) in &cards {
            assert_eq!((card.fill, card.border, card.radii), (first.fill, first.border, first.radii), "dark {dark}: {text:?}");
        }
        seen.push(first);
    }
    assert_ne!(seen[0].fill, seen[1].fill, "Dark and Light each have their own");
}

/// A long path is wrapped inside a card that's no wider than 320 points, shown whole.
#[test]
fn a_long_path_wraps_in_the_tooltip_whole() {
    for dark in [true, false] {
        let cards = main_cards(dark);
        let (short, short_box, _) = &cards[0];
        assert!(short.contains("Add"), "{short}");
        for (text, tip, _) in &cards[1..] {
            assert!(tip[2] <= 320.0 + 0.5, "{text:?}: no wider than a note: {tip:?}");
            assert!(tip[3] > short_box[3] * 1.5, "{text:?}: more than a line tall, so wrapped: {tip:?}");
        }
        assert!(cards.iter().any(|(t, _, _)| t.contains(PATH)), "whole, not cut");
    }
}

/// WCAG's contrast ratio of two opaque colors, as Rune holds them (linear).
fn contrast(a: [f32; 4], b: [f32; 4]) -> f32 {
    let lum = |c: [f32; 4]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    let (hi, lo) = (lum(a).max(lum(b)), lum(a).min(lum(b)));
    (hi + 0.05) / (lo + 0.05)
}

/// In Dark a tooltip's edge is fainter than the notices popover's, yet still shows: at least 2:1
/// against its own fill and 1.5:1 against the pane behind it (the page, as the list sits on it).
#[test]
fn in_dark_a_tooltips_edge_is_fainter_than_the_notices_popovers_but_shows() {
    let service = MemoryService::new().with_torrents(vec![super::torrent(0, "A"), super::torrent(1, "B")]).trash_refusing("A");
    let mut rt = app_in(service, true);
    wait(&mut rt, 0.5);
    rt.update(|s, _| crate::model::remove_ids(s, vec![0], true));
    wait(&mut rt, 1.0);
    let id = rt.state.bar.entries().last().expect("a notice").id;
    super::generated::show_notice(&mut rt, id);
    settle_popover(&mut rt);
    let panel = super::generated::rect_of(&rt, "notices-panel").expect("the panel");
    let popover = rt
        .instances
        .iter()
        .rev()
        .find(|i| i.params[1] == KIND_RECT && (i.rect[0] - panel[0] * 2.0).abs() < 1.0 && (i.rect[1] - panel[1] * 2.0).abs() < 1.0)
        .copied()
        .expect("the panel's card");
    // The panel is the page's own color in Dark.
    let page = popover.fill;
    let wanted = contrast(popover.border, page);
    assert!(wanted > 1.2, "the popover's edge shows: {wanted}");
    let mut tip = app_in(tears(PATH), true);
    let (_, _, card, pane) = card_over(&mut tip, "add-file");
    let (own, behind) = (contrast(card.border, card.fill), contrast(card.border, pane));
    assert!(own < wanted && behind < wanted, "fainter than the popover's {wanted}: {own}, {behind}");
    assert!(own >= 2.0, "against its fill: {own}");
    assert!(behind >= 1.5, "against the pane behind: {behind}");
}

fn settle_popover(rt: &mut Runtime<Phantom>) {
    wait(rt, 0.6);
}
