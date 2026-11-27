//! The App icon's Customize sheet (#417), laid out as a Look Preset's: a ☀ Light and a ☾ Dark
//! column, each drawn in its half's look. Its rows: the icon as each half draws it, then a swatch
//! per half for each of its color settings (Tile), which opens the color picker beside it. The
//! picker's buttons are Original (the icon's own colors) and Follow the look, off with why on
//! hover where that half's look isn't the icon's theme's.

use rune::{El, col, el, row};

use super::HEX_WIDTH;
use crate::model::settings::SettingsWindow;
use crate::model::settings::customize::{
    self, Customize, can_follow, close_picker, escape_picker, follow, open_picker, original, pick, shown, submit_hex,
};
use crate::theme::icon::{IconSetting, ThemeIcon, Tint, find};
use crate::theme::preset::Appearance;
use crate::ui::{FIELD_HEIGHT, Halves, PopupParts, Ui};

/// How wide the sheet gets.
const WIDTH: f32 = 600.0;

/// How big each half's icon is drawn.
const PREVIEW: f32 = 64.0;

/// The sheet, while it's open: the overlays to draw it with.
pub(super) fn sheet(s: &SettingsWindow, open: &Customize, ui: &Ui) -> Vec<El> {
    let Some((_, icon)) = find(open.icon) else { return Vec::new() };
    let looks = s.looks();
    let [light, dark] = Appearance::BOTH.map(|a| looks.shown(a).clone());
    drop(looks);
    let uis = [&light.ui, &dark.ui];
    let mut layer = Vec::new();
    let header = ui.halves_header(uis[0].half_heading(false), uis[1].half_heading(true), uis);
    let pictures = Appearance::BOTH.map(|half| Some(picture(s, icon, half, uis[index(half)])));
    let [l, d] = pictures;
    let mut lines =
        vec![(header, false), (ui.halves_row("customize-icon", "Icon", Halves::Split(l, d), uis, None, Vec::new(), false), false)];
    for setting in icon.settings {
        let [l, d] = Appearance::BOTH.map(|half| Some(swatch(s, open, icon, setting, half, uis[index(half)], &mut layer)));
        let key = format!("customize-{}", setting.key);
        lines.push((ui.halves_row(&key, setting.label, Halves::Split(l, d), uis, None, Vec::new(), false), false));
    }
    let body = col().w_full().child(ui.halves_card(lines, uis));
    let mut layers = ui.over_sheet(
        "customize",
        "Customize App icon",
        WIDTH,
        body,
        customize::escape,
        Some(Box::new(customize::cancel)),
        customize::done,
    );
    layers.extend(layer);
    layers
}

fn index(half: Appearance) -> usize {
    usize::from(half == Appearance::Dark)
}

/// The icon as `half` draws it.
fn picture(s: &SettingsWindow, icon: &ThemeIcon, half: Appearance, ui: &Ui) -> El {
    let tile = el().key(format!("customize-icon-{}-picture", half.key())).size(PREVIEW, PREVIEW).a11y_hidden();
    let tile = match s.app_icon.image_in(icon.id, half) {
        Some(image) => tile.pixels(image.pixels()),
        None => tile.radius(14.0).fill(ui.tokens().c.layer_02),
    };
    row().py(ui.spacing().s3).child(tile)
}

/// What the swatch of `setting` in `half` says on hover: its color and where it comes from.
fn help(s: &SettingsWindow, icon: &str, half: Appearance, key: &str) -> String {
    let hex = shown(s, icon, half, key).map(|c| c.hex()).unwrap_or_default();
    match s.app_icon.tint(icon, half, key) {
        Tint::Original => "Original".to_owned(),
        Tint::Follow if can_follow(s, icon, half) => format!("{hex} · follows the look"),
        Tint::Follow => format!("Original · {}", not_followed(half)),
        Tint::Color(_) => format!("{hex} · set by hand"),
    }
}

/// Why `half` can't follow the look.
fn not_followed(half: Appearance) -> String {
    format!("Your {} look isn\u{2019}t Phantom", half.label())
}

/// The swatch of `setting` in `half`, and its picker into `layer` while it's open.
fn swatch(
    s: &SettingsWindow,
    open: &Customize,
    icon: &ThemeIcon,
    setting: &'static IconSetting,
    half: Appearance,
    ui: &Ui,
    layer: &mut Vec<El>,
) -> El {
    let key = setting.key;
    let control = format!("customize-{key}-{}", half.key());
    let swatch_key = format!("{control}-swatch");
    let color = shown(s, icon.id, half, key).unwrap_or(crate::theme::setting::Rgb([0; 3]));
    let tint = s.app_icon.tint(icon.id, half, key);
    let label = format!("{} {}", half.label(), setting.label);
    let swatch = ui.color_swatch(
        swatch_key.clone(),
        color.color(),
        matches!(tint, Tint::Color(_)),
        &label,
        help(s, icon.id, half, key),
        move |s: &mut SettingsWindow| open_picker(s, half, key),
    );
    let Some(picker) = open.picker.as_ref().filter(|p| p.half == half && p.key == key) else { return swatch };
    let field = ui
        .text_field(control.clone(), &picker.field, &format!("{label} hex"))
        .on_edit(customize::field)
        .on_submit(|s: &mut SettingsWindow, _| submit_hex(s))
        .width(HEX_WIDTH)
        .height(FIELD_HEIGHT)
        .build();
    let followable = can_follow(s, icon.id, half);
    let original_button =
        ui.button(format!("{control}-original"), "Original").disabled(tint == Tint::Original).tab_stop().on_press(original).build();
    let follow_button = ui
        .button(format!("{control}-follow"), "Follow the look")
        .disabled(tint == Tint::Follow || !followable)
        .tab_stop()
        .on_press(follow)
        .build();
    // A button that's off takes no hover: the reason is on what holds it.
    let follow_button = match followable {
        true => follow_button,
        false => row().key(format!("{control}-follow-why")).help(not_followed(half)).child(follow_button),
    };
    let buttons = row().gap(ui.spacing().s3).child(original_button).child(follow_button);
    let parts = PopupParts { field, button: buttons, lines: Vec::new(), most: Vec::new() };
    ui.color_popup(
        &control,
        &swatch_key,
        picker.at,
        parts,
        |s: &mut SettingsWindow, p| pick(s, p),
        close_picker,
        |s: &mut SettingsWindow| {
            escape_picker(s);
        },
        layer,
    );
    swatch
}
