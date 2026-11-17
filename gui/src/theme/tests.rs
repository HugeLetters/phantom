//! Themes, as properties: a block resolves whole through theme → parent → Phantom; every theme
//! there is is well formed; a Look Preset reads back what it keeps, and whatever `gui.json`
//! holds reads as a preset with what was reset named; a change rebuilds only the half it changes.

use std::cell::RefCell;
use std::rc::Rc;

use proptest::prelude::*;
use rune::El;
use rune_kit::Kit;
use serde_json::{Value as Json, json};

use super::preset::{Appearance, LookPreset, Looks, SETTING, Stored, StoredLook};
use super::setting::tests::{SAMPLE as SAMPLE_SETTINGS, values_of};
use super::setting::{Kind, Value};
use super::*;
use crate::ui::{Icon, ProgressBar, StatusDot, Tone};

/// A theme other than Phantom, with a setting of every kind: it draws its own status dot and
/// copy icon, and its App icon follows its Shape.
pub struct Sample;

pub static SAMPLE: Sample = Sample;

fn sample_dot(ui: &Ui, _: Tone) -> El {
    let _ = ui;
    rune::el()
}

static SAMPLE_COPY: rune::Icon = rune::Icon { name: "sample-copy", view: 24.0, body: "" };

impl Theme for Sample {
    fn id(&self) -> &'static str {
        "sample"
    }
    fn name(&self) -> &'static str {
        "Sample"
    }
    fn parent(&self) -> Option<&'static dyn Theme> {
        Some(&phantom::PHANTOM)
    }
    fn settings(&self) -> &'static [Setting] {
        &SAMPLE_SETTINGS
    }
    fn build(&self, settings: &Values) -> Built {
        let dark = settings.get("motion") == Some(Value::Toggle(false));
        Built {
            tokens: Kit::neutral(dark),
            blocks: BlockOverrides {
                icons: Some(|icon| (icon == Icon::Copy).then_some(&SAMPLE_COPY)),
                status_dot: Some(sample_dot),
                ..Default::default()
            },
            screens: Default::default(),
            app_icon: if settings.get("shape") == Some(Value::Choice("round")) { "full-moon" } else { "dark-hour" },
        }
    }
}

/// A theme in a generated chain: what it draws itself, and its parent.
struct Link {
    blocks: BlockOverrides,
    screens: ScreenOverrides,
    parent: Option<&'static dyn Theme>,
}

impl Theme for Link {
    fn id(&self) -> &'static str {
        "link"
    }
    fn name(&self) -> &'static str {
        "Link"
    }
    fn parent(&self) -> Option<&'static dyn Theme> {
        self.parent
    }
    fn settings(&self) -> &'static [Setting] {
        &[]
    }
    fn build(&self, _: &Values) -> Built {
        Built { tokens: Kit::neutral(false), blocks: self.blocks, screens: self.screens, app_icon: "phantom" }
    }
}

// Drawings to override with, told apart by address.
fn bar_a(_: &Ui, _: f32) -> El {
    rune::el().key("a")
}
fn bar_b(_: &Ui, _: f32) -> El {
    rune::el().key("b")
}
fn dot_a(_: &Ui, _: Tone) -> El {
    rune::el().key("a")
}
fn details_a(_: &Phantom, _: &Ui, _: &TorrentInfo) -> El {
    rune::el().key("a")
}
static ICON_A: rune::Icon = rune::Icon { name: "a", view: 24.0, body: "" };
static ICON_B: rune::Icon = rune::Icon { name: "b", view: 24.0, body: "" };
fn icons_a(icon: Icon) -> Option<&'static rune::Icon> {
    (icon == Icon::Copy).then_some(&ICON_A)
}
fn icons_b(icon: Icon) -> Option<&'static rune::Icon> {
    (icon != Icon::Check).then_some(&ICON_B)
}

/// What one link of a chain draws, by number: 0 is nothing, then one of two drawings.
#[derive(Clone, Copy, Debug)]
struct Draws {
    bar: u8,
    dot: u8,
    icons: u8,
    details: u8,
}

fn draws() -> impl Strategy<Value = Draws> {
    (0..3u8, 0..2u8, 0..3u8, 0..2u8).prop_map(|(bar, dot, icons, details)| Draws { bar, dot, icons, details })
}

impl Draws {
    fn bar(self) -> Option<ProgressBar> {
        [None, Some(bar_a as ProgressBar), Some(bar_b)][self.bar as usize]
    }
    fn dot(self) -> Option<StatusDot> {
        [None, Some(dot_a as StatusDot)][self.dot as usize]
    }
    fn icons(self) -> Option<fn(Icon) -> Option<&'static rune::Icon>> {
        [None, Some(icons_a as fn(Icon) -> Option<&'static rune::Icon>), Some(icons_b)][self.icons as usize]
    }
    fn details(self) -> Option<DetailsScreen> {
        [None, Some(details_a as DetailsScreen)][self.details as usize]
    }
}

/// The chain `links` (the theme first) ending at Phantom, as themes.
fn chain(links: &[Draws]) -> &'static dyn Theme {
    let mut parent: &'static dyn Theme = &phantom::PHANTOM;
    for draws in links.iter().rev() {
        let blocks = BlockOverrides { icons: draws.icons(), progress_bar: draws.bar(), status_dot: draws.dot() };
        let screens = ScreenOverrides { details: draws.details() };
        parent = Box::leak(Box::new(Link { blocks, screens, parent: Some(parent) }));
    }
    parent
}

/// The first `Some` along `links`, else Phantom's.
fn nearest<T>(links: &[Draws], of: impl Fn(Draws) -> Option<T>, phantom: T) -> T {
    links.iter().find_map(|d| of(*d)).unwrap_or(phantom)
}

proptest! {
    /// Each block, each icon and each screen is drawn whole by the first theme along the chain
    /// that draws it, else by Phantom.
    #[test]
    fn a_block_resolves_whole_through_theme_then_parent_then_phantom(links in proptest::collection::vec(draws(), 0..5)) {
        let look = Look::build(&ThemeLook::defaults(chain(&links)));
        let blocks = look.ui.blocks();
        let phantoms = Blocks::phantom(phantom::icon);
        prop_assert!(std::ptr::fn_addr_eq(blocks.progress_bar, nearest(&links, Draws::bar, phantoms.progress_bar)));
        prop_assert!(std::ptr::fn_addr_eq(blocks.status_dot, nearest(&links, Draws::dot, phantoms.status_dot)));
        prop_assert!(std::ptr::fn_addr_eq(look.screens.details, nearest(&links, Draws::details, crate::views::details_pane as DetailsScreen)));
        for icon in <Icon as lifecycle::machine::Every>::every() {
            let expected = links.iter().find_map(|d| d.icons().and_then(|f| f(icon))).unwrap_or(phantom::icon(icon));
            prop_assert!(std::ptr::eq(blocks.icon(icon), expected), "{:?}", icon);
        }
    }
}

/// Every theme there is: a unique id, its parent chain ending at Phantom, its settings' keys
/// unique and each choice's default one of its choices.
#[test]
fn every_theme_is_well_formed() {
    let ids: Vec<_> = themes().iter().map(|t| t.id()).collect();
    for (i, theme) in themes().iter().enumerate() {
        assert!(!ids[..i].contains(&theme.id()), "{} twice", theme.id());
        assert_eq!(find(theme.id()).map(|t| t.id()), Some(theme.id()));
        let mut at = *theme;
        for _ in 0..themes().len() {
            match at.parent() {
                Some(parent) => at = parent,
                None => break,
            }
        }
        assert_eq!(at.id(), "phantom", "{}'s chain ends at Phantom", theme.id());
        let keys: Vec<_> = theme.settings().iter().map(|s| s.key).collect();
        for (j, setting) in theme.settings().iter().enumerate() {
            assert!(!keys[..j].contains(&setting.key), "{}.{} twice", theme.id(), setting.key);
            if let Kind::Choice { choices, default } = setting.kind {
                assert!(choices.iter().any(|c| c.id == default), "{}.{}'s default", theme.id(), setting.key);
            }
        }
    }
}

proptest! {
    /// Every theme's App icon, whatever its settings, is one that ships, so Follow theme always
    /// shows it.
    #[test]
    fn every_themes_app_icon_ships(look in theme_look()) {
        let m = crate::platform::app_icon::Manifest::load(&crate::platform::app_icon::manifest_dir()).expect("the shipped icons");
        let icon = Look::build(&look).app_icon;
        prop_assert!(m.icon(icon).is_some(), "{}'s App icon {} doesn't ship", look.theme.id(), icon);
    }
}

/// Today's look: the fresh preset is Phantom's tokens as they were, rune-kit's neutral kit,
/// light in Light and dark in Dark, its icons and its App icon the bundle's own.
#[test]
fn a_fresh_install_shows_phantom_light_in_light_and_dark_in_dark() {
    let looks = Looks::default();
    for (appearance, dark) in [(Appearance::Light, false), (Appearance::Dark, true)] {
        let look = looks.shown(appearance);
        let shown = looks.preset().get(appearance);
        assert_eq!(shown.theme.id(), "phantom");
        assert_eq!(shown.settings.get(phantom::TONE), Some(Value::Choice(if dark { "dark" } else { "light" })));
        assert_eq!(look.ui.tokens().c.page, Kit::neutral(dark).c.page);
        assert_eq!(look.ui.tokens().dark, dark);
    }
    assert_eq!(looks.app_icons(), crate::platform::app_icon::Icons::One { icon: "phantom".into() });
}

/// Any theme with any values for its settings.
fn theme_look() -> impl Strategy<Value = ThemeLook> {
    (0..themes().len()).prop_flat_map(|i| {
        let theme = themes()[i];
        values_of(theme.settings()).prop_map(move |settings| ThemeLook { theme, settings })
    })
}

pub(crate) fn preset() -> impl Strategy<Value = LookPreset> {
    (theme_look(), theme_look()).prop_map(|(light, dark)| LookPreset { light, dark })
}

/// JSON of any shape, small.
fn junk() -> impl Strategy<Value = Json> {
    prop_oneof![
        Just(json!(null)),
        any::<bool>().prop_map(Json::Bool),
        any::<i32>().prop_map(|n| json!(n)),
        "[a-z#0-9]{0,8}".prop_map(Json::String),
        Just(json!([])),
        Just(json!({ "x": 1 })),
    ]
}

proptest! {
    #[test]
    fn a_preset_reads_back_what_it_keeps(preset in preset()) {
        let stored: Stored = serde_json::from_value(serde_json::to_value(preset.stored()).unwrap()).unwrap();
        let (read, reset) = LookPreset::read(&stored);
        prop_assert_eq!(read, preset);
        prop_assert!(reset.is_empty());
    }

    /// Whatever a half's settings hold, it reads as its theme with each key kept, reset to its
    /// default or dropped, and each one not kept named; a half naming no theme there is reads as a
    /// fresh install's, named; the other half is as it was.
    #[test]
    fn a_stored_preset_reads_with_what_was_reset_named(
        preset in preset(),
        light in proptest::collection::btree_map("[a-z]{1,6}", junk(), 0..4),
        unknown_dark in any::<bool>(),
    ) {
        let mut stored = preset.stored();
        stored.light.settings.extend(light.clone());
        if unknown_dark {
            stored.dark = StoredLook { theme: "gone".into(), settings: Default::default() };
        }
        let (read, reset) = LookPreset::read(&stored);
        let theme = preset.light.theme;
        prop_assert_eq!(read.light.theme.id(), theme.id());
        let mut named = Vec::new();
        for (key, json) in &stored.light.settings {
            let kept = read.light.settings.get(key);
            match theme.settings().iter().find(|s| s.key == key) {
                None => named.push(format!("{SETTING}.light.{key}")),
                Some(setting) => match setting.kind.read(json) {
                    Some(value) => prop_assert_eq!(kept, Some(value), "{} kept", key),
                    None => {
                        prop_assert_eq!(kept, Some(setting.kind.default()), "{} back to its default", key);
                        named.push(format!("{SETTING}.light.{key}"));
                    }
                },
            }
        }
        if unknown_dark {
            prop_assert_eq!(&read.dark, &LookPreset::fresh().dark);
            named.push(format!("{SETTING}.dark"));
        } else {
            prop_assert_eq!(&read.dark, &preset.dark);
        }
        let mut reset = reset;
        reset.sort();
        named.sort();
        prop_assert_eq!(reset, named);
    }

    /// Setting a half's Theme Look rebuilds that half's look and only it, and keeps the preset;
    /// the same look again rebuilds nothing and keeps nothing. Each half's look is always built
    /// from what the preset holds for it.
    #[test]
    fn a_change_rebuilds_only_the_half_it_changes(start in preset(), changes in proptest::collection::vec((any::<bool>(), theme_look()), 1..8)) {
        let kept = Rc::new(RefCell::new(Vec::new()));
        let mut looks = Looks::new(start, { let kept = kept.clone(); move |p: &LookPreset| kept.borrow_mut().push(p.clone()) });
        for (dark, look) in changes {
            let appearance = Appearance::of(dark);
            let before = Appearance::BOTH.map(|a| looks.shown(a).clone());
            let changed = *looks.preset().get(appearance) != look;
            let kept_before = kept.borrow().len();
            looks.set(appearance, look.clone());
            prop_assert_eq!(looks.preset().get(appearance), &look);
            for (a, before) in Appearance::BOTH.into_iter().zip(before) {
                let rebuilt = !Rc::ptr_eq(looks.shown(a), &before);
                prop_assert_eq!(rebuilt, changed && a == appearance, "{:?} rebuilt", a);
                let (shown, built) = (looks.shown(a), Look::build(looks.preset().get(a)));
                prop_assert_eq!(shown.ui.tokens().c.page, built.ui.tokens().c.page, "{:?} drawn from what the preset holds", a);
                prop_assert_eq!(shown.ui.tokens().dark, built.ui.tokens().dark);
                prop_assert_eq!(shown.app_icon, built.app_icon);
            }
            prop_assert_eq!(kept.borrow().len(), kept_before + usize::from(changed));
            if changed {
                let last = kept.borrow().last().cloned();
                prop_assert_eq!(last.as_ref(), Some(looks.preset()));
            }
        }
    }
}
