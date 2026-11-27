//! The App icon's Customize sheet (#417): the settings of the icon shown, a Light and a Dark value
//! each, set as they're picked, drawn at once, and kept in `gui.json` once by Done in the App icon's row and
//! tiles. A color setting's swatch opens the color picker beside it, whose buttons set the icon's
//! own colors (Original) or the colors of the look that half shows (Follow the look, for a look of
//! the icon's theme only). Done keeps what was set; Cancel, Escape once the picker is closed and the
//! window closing put back what the sheet opened on, writing nothing; so does quitting.

use rune::TextEdit;

use super::{SettingsWindow, follow_focus};
use crate::changes::Was;
use crate::changes::machine::AppSetting;
use crate::theme::icon::{IconSettings, ThemeIcon, Tint, find, of_theme};
use crate::theme::oklab::Okhsl;
use crate::theme::preset::Appearance;
use crate::theme::setting::Rgb;
use crate::ui::Pick;

/// The sheet, while it's open.
pub struct Customize {
    /// The id of the icon whose settings it shows.
    pub icon: &'static str,
    /// The settings as it opened, which Cancel puts back.
    pub opened: IconSettings,
    pub picker: Option<TintPicker>,
}

/// A color setting's picker, open beside the swatch of `key` in `half`.
pub struct TintPicker {
    pub half: Appearance,
    pub key: &'static str,
    /// Where its markers are.
    pub at: Okhsl,
    /// Its hex field: the color shown, or what's typed over it until it's applied.
    pub field: TextEdit,
    /// The icons' settings as a drag began: the drag is one step of Undo, from these, and one
    /// called off puts them back.
    drag: Option<IconSettings>,
}

/// The icon with settings each Appearance shows, Light's first: what Customize… customizes.
pub fn customizable(icon: &super::AppIcon) -> Option<&'static ThemeIcon> {
    let [dark, light] = icon.sides()?;
    [light, dark].iter().find_map(|id| find(id)).map(|(_, icon)| icon)
}

/// The color the setting `key` of `icon` comes to in `half`, as its swatch shows it.
pub fn shown(s: &SettingsWindow, icon: &str, half: Appearance, key: &str) -> Option<Rgb> {
    let (theme, of) = find(icon)?;
    let looks = s.looks.borrow();
    let values = s.app_icon.tints.get(icon).map(|h| h.get(half).clone()).unwrap_or_default();
    Some((of.color)(key, &values, of_theme(theme, looks.shown(half)), half))
}

/// Whether `icon` can follow the look `half` shows: it's of the icon's theme.
pub fn can_follow(s: &SettingsWindow, icon: &str, half: Appearance) -> bool {
    find(icon).is_some_and(|(theme, _)| of_theme(theme, s.looks.borrow().shown(half)).is_some())
}

/// Customize… pressed: the sheet opens on the icon shown, if it has settings.
pub fn open(s: &mut SettingsWindow) {
    follow_focus(s);
    let Some(icon) = customizable(&s.app_icon) else { return };
    let opened = s.app_icon.tints.clone();
    if let Some(sheet) = &mut s.sheet {
        sheet.customize = Some(Customize { icon: icon.id, opened, picker: None });
        // What it sets is shown, and kept once Done.
        s.app_icon.hold_tints();
        // Its picks are its own for ⌘Z, until it closes.
        s.main.post(|s, _| crate::changes::sheet_opened(s, crate::changes::session::Sheet::Customize));
    }
}

/// What a visit left of the sheet, which `opened` it: one step of Undo if it changed anything.
fn visit(s: &SettingsWindow, opened: IconSettings) -> Option<(AppSetting, Was)> {
    (s.app_icon.tints != opened).then_some((AppSetting::Tints, Was::Tints(opened)))
}

/// Done, Return, ⌘W: it closes, keeping what was set (a hex typed in the picker too). What it
/// changed since it opened is one step of Undo.
pub fn done(s: &mut SettingsWindow) {
    close_picker(s);
    let Some(open) = s.sheet.as_mut().and_then(|sheet| sheet.customize.take()) else { return };
    // What's left of the visit once a ⌘Z pressed just before has taken its step.
    super::end_sheet(s, move |s| {
        // Done keeps it: written once.
        s.app_icon.release_tints();
        visit(s, open.opened)
    });
}

/// Cancel, or Escape with the picker closed: what it opened on is back, and it closes, none of its
/// steps left for ⌘Z.
pub fn cancel(s: &mut SettingsWindow) {
    let Some(open) = s.sheet.as_mut().and_then(|sheet| sheet.customize.take()) else { return };
    // Put back once a ⌘Z pressed just before has taken its step, which would otherwise come last.
    super::end_sheet(s, move |s| {
        s.app_icon.put_tints(open.opened);
        s.app_icon.redraw(&s.looks.borrow());
        s.app_icon.release_tints();
        None
    });
}

/// Escape in the sheet: a hex being typed goes back to the color shown, else the picker closes,
/// else the sheet is cancelled.
pub fn escape(s: &mut SettingsWindow) {
    if escape_picker(s) {
        return;
    }
    cancel(s);
}

fn open_sheet(s: &mut SettingsWindow) -> Option<&mut Customize> {
    s.sheet.as_mut()?.customize.as_mut()
}

fn picker(s: &SettingsWindow) -> Option<(&'static str, Appearance, &'static str)> {
    let open = s.sheet.as_ref()?.customize.as_ref()?;
    open.picker.as_ref().map(|p| (open.icon, p.half, p.key))
}

/// A swatch pressed: its picker opens beside it, its hex field focused (another one closes).
pub fn open_picker(s: &mut SettingsWindow, half: Appearance, key: &'static str) {
    close_picker(s);
    let Some(icon) = s.sheet.as_ref().and_then(|sheet| sheet.customize.as_ref()).map(|c| c.icon) else { return };
    let Some(rgb) = shown(s, icon, half, key) else { return };
    let mut field = TextEdit::new(rgb.hex());
    field.focused = true;
    if let Some(open) = open_sheet(s) {
        open.picker = Some(TintPicker { half, key, at: Okhsl::of(rgb.color()), field, drag: None });
    }
}

/// The picker closes (a press outside it, Done): a hex typed in it is applied first.
pub fn close_picker(s: &mut SettingsWindow) {
    if picker(s).is_some() {
        submit_hex(s);
    }
    if let Some(open) = open_sheet(s) {
        open.picker = None;
    }
}

/// Escape in the picker: a hex being typed goes, the color shown again; else it closes. Whether
/// there was a picker to take it.
pub fn escape_picker(s: &mut SettingsWindow) -> bool {
    let Some((icon, half, key)) = picker(s) else { return false };
    let shown = shown(s, icon, half, key).map(Rgb::hex).unwrap_or_default();
    let open = open_sheet(s).expect("a picker is open");
    let picker = open.picker.as_mut().expect("a picker is open");
    if picker.field.text != shown {
        picker.field = TextEdit::new(shown);
        picker.field.focused = true;
    } else {
        open.picker = None;
    }
    true
}

/// `tint` set on the open picker's setting, drawn at once, one step of Undo unless it's part of a
/// drag; its field and markers show the color it comes to.
fn set(s: &mut SettingsWindow, tint: Tint, at: Option<Okhsl>) {
    let Some((icon, half, key)) = picker(s) else { return };
    let was = s.app_icon.tints.clone();
    s.app_icon.set_tint(icon, half, key, tint);
    s.app_icon.redraw(&s.looks.borrow());
    if s.app_icon.tints != was && open_sheet(s).and_then(|o| o.picker.as_ref()).is_some_and(|p| p.drag.is_none()) {
        super::app_changed(s, AppSetting::Tints, Was::Tints(was));
    }
    show_color(s, at);
}

/// After an undo: the picker's field and markers show the color its setting came back to.
pub fn refresh(s: &mut SettingsWindow) {
    show_color(s, None);
}

fn show_color(s: &mut SettingsWindow, at: Option<Okhsl>) {
    let Some((icon, half, key)) = picker(s) else { return };
    let rgb = shown(s, icon, half, key);
    if let (Some(picker), Some(rgb)) = (open_sheet(s).and_then(|o| o.picker.as_mut()), rgb) {
        picker.at = at.unwrap_or_else(|| Okhsl::of(rgb.color()));
        let focused = picker.field.focused;
        picker.field = TextEdit::new(rgb.hex());
        picker.field.focused = focused;
    }
}

/// Original: the icon's own colors in that half.
pub fn original(s: &mut SettingsWindow) {
    set(s, Tint::Original, None);
}

/// Follow the look: the colors of the look that half shows.
pub fn follow(s: &mut SettingsWindow) {
    set(s, Tint::Follow, None);
}

/// A pick in the open picker: the color, set by hand as the pointer goes down and moves; one
/// called off puts back what was there.
pub fn pick(s: &mut SettingsWindow, pick: Pick) {
    if picker(s).is_none() {
        return;
    }
    match pick {
        Pick::Down(at) => {
            let tints = s.app_icon.tints.clone();
            if let Some(picker) = open_sheet(s).and_then(|o| o.picker.as_mut()) {
                picker.drag = Some(tints);
            }
            set(s, Tint::Color(Rgb::of(at.color())), Some(at));
        }
        Pick::Move(at) => set(s, Tint::Color(Rgb::of(at.color())), Some(at)),
        Pick::Up(at) => {
            set(s, Tint::Color(Rgb::of(at.color())), Some(at));
            // The drag ends: what it changed is one step.
            if let Some(was) = open_sheet(s).and_then(|o| o.picker.as_mut()).and_then(|p| p.drag.take())
                && s.app_icon.tints != was
            {
                super::app_changed(s, AppSetting::Tints, Was::Tints(was));
            }
        }
        Pick::Cancelled => {
            if let Some(was) = open_sheet(s).and_then(|o| o.picker.as_mut()).and_then(|p| p.drag.take()) {
                s.app_icon.put_tints(was);
                s.app_icon.redraw(&s.looks.borrow());
                refresh(s);
            }
        }
    }
}

/// The picker's hex field, for its edits.
pub fn field(s: &mut SettingsWindow) -> &mut TextEdit {
    let sheet = s.sheet.as_mut().expect("the sheet is open");
    match sheet.customize.as_mut().and_then(|o| o.picker.as_mut()) {
        Some(picker) => &mut picker.field,
        None => &mut sheet.gone,
    }
}

/// Return in the hex field, or the picker closing: a hex typed is set by hand; anything else
/// goes back to the color shown.
pub fn submit_hex(s: &mut SettingsWindow) {
    let Some((icon, half, key)) = picker(s) else { return };
    let shown = shown(s, icon, half, key);
    let Some(text) = open_sheet(s).and_then(|o| o.picker.as_ref()).map(|p| p.field.text.clone()) else { return };
    match Rgb::parse(&text) {
        Some(rgb) if Some(rgb) != shown => set(s, Tint::Color(rgb), None),
        _ => {
            let kept = s.app_icon.tint(icon, half, key);
            set(s, kept, None);
        }
    }
}
