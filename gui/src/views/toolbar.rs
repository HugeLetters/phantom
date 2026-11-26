//! The toolbar: the magnet field, Add and Add file…. Resume and Pause aren't here: a row has its own
//! toggle ([`super::table`]) and the row menu its items. Remove and Delete have no button: ⌫ removes
//! the selection and ⌘⌫ deletes it, and the row menu has both.
//!
//! It sits in the titlebar band (#33), as Safari's: the link field centred in the window, Add at
//! its end and Add file… beside it; the traffic lights at the band's left (none in full screen). The
//! free room either side of the field moves the window, and holds what a theme puts there
//! ([`Ui::band`], a logo). The field (with Add) is in the window's middle, as wide as that leaves,
//! up to [`FIELD_MAX`], with [`DRAG_MIN`] of free room either side at the least. Where its
//! placeholder doesn't fit whole, a shorter one shows.

use client::display;
use rune::{El, FileDialogOptions, Tx, el, row};

use crate::commands::InApp;
use crate::model::{AddNote, Phantom, add_files, link_problem, submit_url};
use crate::notice::Notice;
use crate::ui::{BandSide, Ui};

/// The widest the link field gets; the narrowest, before the band's free room gives way.
pub const FIELD_MAX: f32 = 460.0;
const FIELD_LEAST: f32 = 120.0;
/// The least free room either side of the field, to drag the window by.
pub const DRAG_MIN: f32 = 12.0;

/// The field's placeholder, the longest that fits whole.
pub const PLACEHOLDERS: [&str; 3] =
    ["Paste a magnet link, .torrent URL or info hash", "Magnet link, .torrent URL or info hash", "Magnet link or info hash"];

/// The band's room, as last laid out: the window's width, where the traffic lights end (0 in full
/// screen), and how wide Add and Add file… are; its edge, past the traffic lights or the window's,
/// and the gap between its controls, as the look spaces them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub width: f32,
    pub inset: f32,
    pub add: f32,
    pub add_file: f32,
    pub edge: f32,
    pub gap: f32,
}

impl Band {
    /// Where the band's controls start: past the traffic lights, or the window's edge.
    fn lead(self) -> f32 {
        if self.inset > 0.0 { self.inset + self.edge } else { self.edge }
    }

    /// What the band holds besides the field and its free room: the edges, Add, Add file… and the
    /// gaps between.
    fn fixed(self) -> f32 {
        self.lead() + self.add + self.add_file + self.edge + 4.0 * self.gap
    }

    /// The field's width: as wide as keeping it (with Add at its end) in the window's middle
    /// leaves, up to [`FIELD_MAX`].
    pub fn field(self) -> f32 {
        let left = self.lead() + self.gap + DRAG_MIN;
        let right = 2.0 * self.gap + self.add_file + DRAG_MIN + self.edge;
        let centred = self.width - 2.0 * left.max(right) - self.gap - self.add;
        let room = self.width - self.fixed() - 2.0 * DRAG_MIN;
        centred.min(room).clamp(FIELD_LEAST, FIELD_MAX)
    }

    /// The free room left and right of the field, each [`DRAG_MIN`] at least: as much on the left as
    /// keeps the field and Add in the window's middle, the rest on the right.
    fn free(self) -> (f32, f32) {
        let free = (self.width - self.fixed() - self.field()).max(2.0 * DRAG_MIN);
        let middle = self.width / 2.0 - (self.field() + self.gap + self.add) / 2.0 - self.lead() - self.gap;
        let left = middle.clamp(DRAG_MIN, free - DRAG_MIN);
        (left, free - left)
    }
}

/// The toolbar, in the titlebar band.
pub fn toolbar(s: &Phantom, ui: &Ui, band: Band, overlays: &mut Vec<El>) -> El {
    let add = ui
        .button("add", "Add")
        .primary()
        .disabled(link_problem(&s.magnet.text).is_some())
        .on_press(|s: &mut Phantom| submit_url(&mut *s.service, &s.magnet, &mut s.add_note, &mut s.refocus_link, true))
        .on_press_disabled(|s: &mut Phantom| {
            s.add_note = link_problem(&s.magnet.text).map(|p| AddNote::about(&s.magnet, client::display::link_problem(p)));
        })
        .build();
    // The message about the text in the field, until the text changes.
    let note = s.add_note.as_ref().filter(|n| n.text == s.magnet.text);
    let close = |s: &mut Phantom| s.add_note = None;
    let add = ui.note("add-note", add, note.map_or("", |n| &n.message), note.is_some(), close, overlays);
    // While an add sent by Add is on its way, a marker beside it: when it goes, the field takes
    // the focus if it's still on Add (or nowhere, Add being off), and from nobody else.
    let marker = s.refocus_link.then(|| el().key("add-marker").w(0.0).h(0.0).focus_return("magnet-field"));
    let add = row().key("add-focus").children([add]).children(marker);
    let add_file = ui
        .button(InApp::AddFile.spec().id, InApp::AddFile.spec().name)
        .on_press_tx(add_torrent_file)
        .build()
        .help(display::add_file_help(&crate::shortcuts::hint(InApp::AddFile)));
    let placeholder =
        PLACEHOLDERS.into_iter().find(|p| ui.placeholder_fits(p, band.field())).unwrap_or(PLACEHOLDERS[PLACEHOLDERS.len() - 1]);
    let field = ui
        .text_field("magnet-field", &s.magnet, "Magnet link")
        .placeholder(placeholder)
        .on_edit(|s: &mut Phantom| &mut s.magnet)
        .on_submit(|s: &mut Phantom, _| submit_url(&mut *s.service, &s.magnet, &mut s.add_note, &mut s.refocus_link, false))
        .build();
    let field = row()
        .key("band-field")
        .gap(band.gap)
        .center_y()
        .child(row().w(band.field()).child(field.grow(1.0).min_w(0.0)))
        .child(add)
        .child(add_file);
    // Each side's free room is what keeps the field in the window's middle.
    let free = |key: &'static str, side: BandSide, width: f32| {
        row().key(key).w(width).min_w(DRAG_MIN).h_full().center().window_drag().child(ui.band(side))
    };
    let (left, right) = band.free();
    row()
        .key("toolbar")
        .w_full()
        .h(ui.spacing().toolbar)
        .pl(band.lead())
        .pr(band.edge)
        .gap(band.gap)
        .center_y()
        .window_drag()
        .child(free("band-left", BandSide::Left, left))
        .child(field)
        // The right side takes what rounding leaves, so it ends at the band's edge.
        .child(free("band-right", BandSide::Right, right).grow(1.0))
}

/// Add file…: picks a .torrent file and adds it.
pub fn add_torrent_file(_: &mut Phantom, tx: &mut Tx) {
    let options = FileDialogOptions { title: "Add torrent file".into(), ..Default::default() }.filter("Torrent", &["torrent"]);
    tx.open_file(options, |s: &mut Phantom, result, _| match result {
        Ok(Some(path)) => add_files(s, &[path]),
        Ok(None) => {}
        Err(e) => s.bar.tell(Notice::error(client::display::cant_ask_for_file()).with_text(client::display::sentence(&format!("{e:#}")))),
    });
}
