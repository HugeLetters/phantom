//! Themes, as properties: a block resolves whole through theme → parent → Phantom; every theme
//! there is is well formed; a Look Preset reads back what it keeps, and whatever `gui.json`
//! holds reads as a preset with what was reset named; a change rebuilds only the half it changes.

use std::cell::RefCell;
use std::rc::Rc;

use proptest::prelude::*;
use rune_kit::Kit;
use serde_json::{Value as Json, json};

use super::preset::{Appearance, LookPreset, Looks, SETTING, Stored, StoredLook};
use super::setting::tests::{SAMPLE as SAMPLE_SETTINGS, values_of};
use super::setting::{Kind, Value};
use super::*;
use crate::ui::{Icon, ProgressBar, StatusDot};

/// A theme other than Phantom, with a setting of every kind: it draws its own status dot and
/// copy icon, a progress bar that follows its Motion setting, and its App icon follows its Shape.
pub struct Sample;

pub static SAMPLE: Sample = Sample;

static SAMPLE_COPY: rune::Icon = rune::Icon { name: "sample-copy", view: 24.0, body: "" };

/// The key Sample's progress bar draws with, as its Motion setting says.
pub fn sample_bar_key(motion: bool) -> &'static str {
    if motion { "sample-bar-moving" } else { "sample-bar-still" }
}

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
        let motion = settings.get("motion") == Some(Value::Toggle(true));
        Built {
            tokens: Kit::neutral(false),
            blocks: BlockOverrides {
                icons: Some(Rc::new(|icon| (icon == Icon::Copy).then_some(&SAMPLE_COPY))),
                progress_bar: Some(Rc::new(move |_: &Ui, _| rune::el().key(sample_bar_key(motion)))),
                status_dot: Some(Rc::new(|_: &Ui, _| rune::el())),
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

// SAFETY: a link is leaked by the test case that builds it and only ever reached from that
// case's thread; `Sync` is only for the registry's statics, which never hold a link.
unsafe impl Sync for Link {}

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
        Built { tokens: Kit::neutral(false), blocks: self.blocks.clone(), screens: self.screens.clone(), app_icon: "phantom" }
    }
}

static ICON_A: rune::Icon = rune::Icon { name: "a", view: 24.0, body: "" };
static ICON_B: rune::Icon = rune::Icon { name: "b", view: 24.0, body: "" };

/// What one link of a chain draws itself: each block or not, and which icons.
#[derive(Clone, Copy, Debug)]
struct Draws {
    bar: bool,
    dot: bool,
    /// None, the copy icon, or every icon but the check.
    icons: u8,
    details: bool,
}

fn draws() -> impl Strategy<Value = Draws> {
    (any::<bool>(), any::<bool>(), 0..3u8, any::<bool>()).prop_map(|(bar, dot, icons, details)| Draws { bar, dot, icons, details })
}

impl Draws {
    /// What the link draws: a closure of its own for each block it draws.
    fn overrides(self) -> (BlockOverrides, ScreenOverrides) {
        let icons: Option<crate::ui::blocks::Icons> = match self.icons {
            0 => None,
            1 => Some(Rc::new(|icon| (icon == Icon::Copy).then_some(&ICON_A))),
            _ => Some(Rc::new(|icon| (icon != Icon::Check).then_some(&ICON_B))),
        };
        let blocks = BlockOverrides {
            icons,
            progress_bar: self.bar.then(|| Rc::new(|_: &Ui, _| rune::el()) as ProgressBar),
            status_dot: self.dot.then(|| Rc::new(|_: &Ui, _| rune::el()) as StatusDot),
        };
        let screens =
            ScreenOverrides { details: self.details.then(|| Rc::new(|_: &Phantom, _: &Ui, _: &TorrentInfo| rune::el()) as DetailsScreen) };
        (blocks, screens)
    }
}

/// The chain `links` (the theme first) ending at Phantom, as themes, and what each link draws.
fn chain(links: &[Draws]) -> (&'static dyn Theme, Vec<(BlockOverrides, ScreenOverrides)>) {
    let drawn: Vec<_> = links.iter().map(|d| d.overrides()).collect();
    let mut parent: &'static dyn Theme = &phantom::PHANTOM;
    for (blocks, screens) in drawn.iter().rev().cloned() {
        parent = Box::leak(Box::new(Link { blocks, screens, parent: Some(parent) }));
    }
    (parent, drawn)
}

/// That `resolved` is the first link's own along the chain, or, none drawing it, no link's.
fn nearest<T: ?Sized>(resolved: &Rc<T>, own: Vec<Option<&Rc<T>>>) -> bool {
    match own.iter().flatten().next() {
        Some(first) => Rc::ptr_eq(resolved, first),
        None => own.iter().flatten().all(|o| !Rc::ptr_eq(resolved, o)),
    }
}

proptest! {
    /// Each block, each icon and each screen is drawn whole by the first theme along the chain
    /// that draws it, else by Phantom.
    #[test]
    fn a_block_resolves_whole_through_theme_then_parent_then_phantom(links in proptest::collection::vec(draws(), 0..5)) {
        let (theme, drawn) = chain(&links);
        let look = Look::build(&ThemeLook::defaults(theme));
        let blocks = look.ui.blocks();
        prop_assert!(nearest(&blocks.progress_bar, drawn.iter().map(|(b, _)| b.progress_bar.as_ref()).collect()), "the progress bar");
        prop_assert!(nearest(&blocks.status_dot, drawn.iter().map(|(b, _)| b.status_dot.as_ref()).collect()), "the status dot");
        prop_assert!(nearest(&look.screens.details, drawn.iter().map(|(_, s)| s.details.as_ref()).collect()), "the details pane");
        for icon in <Icon as lifecycle::machine::Every>::every() {
            let expected = drawn.iter().find_map(|(b, _)| b.icons.as_ref().and_then(|f| f(icon))).unwrap_or(phantom::icon(icon));
            prop_assert!(std::ptr::eq(blocks.icon(icon), expected), "{:?}", icon);
        }
    }

    /// A block draws with its Theme Look's settings: Sample's progress bar follows its Motion.
    #[test]
    fn a_block_follows_its_theme_looks_settings(settings in values_of(&SAMPLE_SETTINGS)) {
        let motion = settings.get("motion") == Some(Value::Toggle(true));
        let look = Rc::new(Look::build(&ThemeLook { theme: &SAMPLE, settings }));
        let view = |look: &Rc<Look>, _: &mut rune::Cx<Rc<Look>>| (look.ui.blocks().progress_bar)(&look.ui, 0.5);
        let mut rt = rune::Runtime::new(look, view, (200.0, 100.0), rune::Env { system_dark: false, scale: 1.0 });
        rt.frame(0.0);
        prop_assert!(rt.find(sample_bar_key(motion)).is_some(), "drawn as Motion {} says", motion);
        prop_assert!(rt.find(sample_bar_key(!motion)).is_none());
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
        // Each in words, as the settings-reset notice lists them.
        for name in &reset {
            let words = super::preset::reset_label(name).expect("a name of the preset");
            let expected = match name.strip_prefix(&format!("{SETTING}.light.")) {
                Some(key) => format!("{} in Light", theme.settings().iter().find(|s| s.key == key).map_or(key, |s| s.label)),
                None => "Theme in Dark".to_owned(),
            };
            prop_assert_eq!(words, expected);
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
