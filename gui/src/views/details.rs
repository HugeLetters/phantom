//! The details pane, under the list while exactly one torrent is selected, as tall as its content
//! (#33: [`layout::pane`]). Its header is two lines, each with a left and a right side:
//!
//! ```text
//! Tears of Steel (2012)                                  3.3 GiB · 4.1 GiB Off
//! ~/Downloads/Tears of Steel (2012)                                The disk is full
//! ```
//!
//! The name, shortened in the middle as Finder does where it's too long (whole on hover, with the
//! original name after a Rename, #331; Rename… turns it into a field, in place, as wide), and on its right the Size and how much is Off when some
//! files are. Under it, where it is, always, shortened in the middle as well (whole on hover), and
//! on its right the problem it has, short and in the error colour, when it has one. Where the
//! window is short the header is one line (name · size · where), the problem on its hover. Then
//! two tabs: Files, its files with their progress (or Missing) and priority, which a click cycles;
//! and Info, what it uploaded, its ratio, the seeding limit it stopped at, its info hash and magnet
//! link (with Copy) and, when renamed, its original name. Everything else that can be done to it is
//! in its row's menu.
//!
//! [`layout::pane`]: crate::model::layout::pane

use std::borrow::Cow;
use std::path::Path;

use client::display;
use proto::{Action, File, Priority, TorrentInfo};
use rune::{Align, El, ScrollSpec, Tx, col, el, row};

use crate::commands::OnTorrents;
use crate::model::layout::{Header, INFO_LINE, INFO_PAD, LINE, NAME_LINE, NO_FILES, PaneContent, PaneTab, TABS};
use crate::model::{Phantom, copy_magnet, cycle_priority, renaming, reveal_target, size_label, sort_files};
use crate::shortcuts;
use crate::ui::{TEXT_INSET, Table, TableColumn, Tone, Ui};

/// How the pane is drawn: its header's shape and its width, for fitting the header's sides.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    pub header: Header,
    pub width: f32,
}

/// What the pane holds under its header and tabs, for its height: `files` its files as last
/// fetched (one until they arrive), none before its info; and the Info tab's lines.
pub fn content(t: &TorrentInfo, files: Option<usize>) -> PaneContent {
    PaneContent { files, info: info_lines(t).len() }
}

/// The Info tab's lines: what each is, its text, and its key. What it uploaded and its ratio
/// come once its info has arrived; the seeding limit only when it stopped at one.
fn info_lines(t: &TorrentInfo) -> Vec<(&'static str, String, &'static str)> {
    let labels = display::info_labels();
    let mut lines = Vec::new();
    if let Some(c) = t.content {
        lines.push((labels.uploaded, display::bytes(t.uploaded_bytes), "info-uploaded"));
        lines.extend(display::uploaded_ratio(t.uploaded_bytes, c.size_bytes).map(|ratio| (labels.ratio, ratio, "info-ratio")));
    }
    lines.extend(display::seeding_limit(&t.status).map(|limit| (labels.limit, limit, "info-limit")));
    lines.push((labels.hash, t.info_hash.clone(), "info-hash"));
    lines.push((labels.magnet, client::magnet_link(t), "info-magnet"));
    lines.extend(t.original_name.iter().map(|name| (labels.original, name.clone(), "info-original")));
    lines
}

/// The problem it has, short, with its longer wording if there's one.
fn reason(s: &Phantom, t: &TorrentInfo) -> Option<(String, Option<String>)> {
    let missing = s.pane.details.as_ref().filter(|d| d.id == t.id).map_or(0, |d| d.files.iter().filter(|f| f.missing).count());
    display::pane_reason(&t.status, missing)
}

/// The name and the path stop at this share of the header's width, however wide it is: shortened
/// in the middle to it, whole on hover.
const NAME_CAP: f32 = 0.6;

/// The share of the header's width the reason may take on the right.
const REASON_CAP: f32 = 0.6;

/// The header's side padding.
const HEADER_INSET: f32 = 10.0;

/// The room between a header line's two sides.
const SIDES_GAP: f32 = 16.0;

pub fn details_pane(s: &Phantom, ui: &Ui, t: &TorrentInfo, shape: Shape) -> El {
    // The header offers Rename…, as its name is where the field opens (ADR 0013).
    s.mounted.offer([OnTorrents::Rename]);
    let fetching = t.content.is_none();
    let name = t.display_name();
    let location = match reveal_target(t) {
        Some(path) => home_relative(path),
        None if fetching => "Not on disk until its info arrives".into(),
        None => String::new(),
    };
    let reason = reason(s, t);
    // The header's text runs from the pane's inset to its edge.
    let room = shape.width - 2.0 * HEADER_INSET;

    // The name, shortened in the middle to what the right side leaves; its hover is the whole of
    // it where it was cut, and after a Rename the original name under it, with no label (#331). rune's tooltip is one plain
    // text, so it isn't muted.
    let name_hover = |shown: &str| {
        let mut lines = Vec::new();
        if shown != name || t.original_name.is_some() {
            lines.push(name.to_owned());
        }
        lines.extend(t.original_name.as_deref().map(str::to_owned));
        (!lines.is_empty()).then(|| lines.join("\n"))
    };
    let heading = |room: f32| {
        if s.rename.editing(t.id) {
            return rename_field(s, ui, room);
        }
        let shown = shorten_middle(name, |text| ui.heading_width(text) <= room);
        let heading = ui.heading(shown.clone()).key("details-name").min_w(0.0).shrink();
        match name_hover(&shown) {
            Some(hover) => heading.help(hover),
            None => heading,
        }
    };
    let where_ = |room: f32| {
        let shown = shorten_middle(&location, |text| ui.caption_fits(text, room));
        ui.caption(shown).key("details-location").truncate().min_w(0.0).shrink().help(location.clone())
    };
    // Another torrent's name is a new title, not this one resizing: the sides land at once
    // instead of sliding over a longer name.
    let header = match shape.header {
        Header::Whole => {
            // The name's line: the size (and Off) whole, the name in what it leaves.
            let side = display::size_and_off(t.content.as_ref());
            let side_width = ui.caption_width(&side);
            let side = ui.caption(side).key("details-side");
            let gap = || el().grow(1.0).min_w(SIDES_GAP);
            let first = row()
                .w_full()
                .h(NAME_LINE)
                .center_y()
                .child(heading((room - side_width - SIDES_GAP).min(room * NAME_CAP)))
                .child(gap())
                .child(side);
            // The path's line: the problem whole, the path shortened to what it leaves.
            let reason = reason.map(|(line, more)| {
                let width = ui.caption_width(&line).min(room * REASON_CAP);
                let text = ui.error_caption(line).key("details-reason").truncate().min_w(0.0).max_w(width + 1.0);
                (
                    width,
                    match more {
                        Some(more) => text.help(more),
                        None => text,
                    },
                )
            });
            let path_room = room - SIDES_GAP - reason.as_ref().map_or(0.0, |(w, _)| *w);
            let second = row()
                .w_full()
                .h(LINE)
                .center_y()
                .child(where_(path_room.min(room * NAME_CAP)))
                .child(gap())
                .children(reason.map(|(_, r)| r));
            col().key("details-header").instant_changes().w_full().px(HEADER_INSET).pt(8.0).pb(6.0).child(first).child(second)
        }
        // One line: name · size · where; the problem it has on hover.
        Header::OneLine => {
            let name_room = (room * 0.45).max(120.0);
            let size = ui.caption(size_label(t)).key("details-side");
            let line = row()
                .w_full()
                .h(NAME_LINE)
                .gap(8.0)
                .center_y()
                .child(heading(name_room))
                .child(size)
                .child(row().gap(8.0).min_w(0.0).shrink().grow(1.0).child(ui.caption("·")).child(where_(room - name_room)));
            let line = col().key("details-header").instant_changes().w_full().px(HEADER_INSET).pt(8.0).pb(6.0).child(line);
            match reason {
                Some((line_, _)) => line.help(line_),
                None => line,
            }
        }
    };

    let tab = s.prefs.tab();
    let tabs = ui.tab_bar(
        PaneTab::ALL
            .into_iter()
            .map(|each| {
                ui.tab(each.title(), each == tab).key(format!("tab-{}", each.id())).on_press(move |s: &mut Phantom| s.prefs.set_tab(each))
            })
            .collect(),
    );
    let tabs = tabs.key("pane-tabs").w_full().h(TABS);
    let body = match tab {
        PaneTab::Files if fetching => {
            ui.empty_state("No files yet", "They show once the torrent's info arrives").key("no-files").h(NO_FILES)
        }
        PaneTab::Files if s.pane.details.is_some() => file_table(s, ui, t.actions.does(Action::SetPriority)),
        PaneTab::Files => el().grow(1.0),
        PaneTab::Info => info(ui, t),
    };

    ui.panel().key("details").w_full().child(col().w_full().h_full().child(header).child(tabs).child(body.grow(1.0).min_h(0.0)))
}

/// The rename field (#331), in place of the header's name: its text where the name was, `room`
/// wide as the name may be (the 60 % cap), a long name scrolling inside it. Its tooltip, while the
/// name typed is refused, is drawn by the window under the element keyed [`RENAME_FIELD`].
fn rename_field(s: &Phantom, ui: &Ui, room: f32) -> El {
    let (font, behind) = ui.heading_field_look();
    let field = ui
        .text_field("rename-field", &s.rename.field, "Name")
        .plain()
        .text_style(font)
        .height(NAME_LINE)
        .on_edit(|s: &mut Phantom| &mut s.rename.field)
        .on_submit(|s: &mut Phantom, _| renaming::submit(s))
        .on_key(|s: &mut Phantom, ev: &rune::KeyEvent, _: &mut Tx| match ev.key {
            rune::Key::Escape => {
                renaming::close(s);
                true
            }
            _ => false,
        })
        .build()
        .grow(1.0)
        .min_w(0.0);
    let field = ui.in_place(row().w_full().h(NAME_LINE).child(field), behind).w(room + TEXT_INSET).h(NAME_LINE);
    let row = row().key(RENAME_FIELD).ml(-TEXT_INSET).shrink().min_w(0.0).child(field).child(ui.focus_goes_nowhere());
    match s.rename.note() {
        Some(why) if crate::ui::a_mode() => row.help(client::display::name_problem(why)).help_open(true),
        _ => row,
    }
}

/// The key of the rename field's place in the header, which its tooltip hangs under.
pub const RENAME_FIELD: &str = "rename-over";

/// The Info tab: a line for each of [`info_lines`], what it is on the left, its text after, cut
/// with all of it on hover, scrolling where the pane is short; the magnet link's with a Copy
/// button, which tells the bar as the row menu's Copy magnet link does.
fn info(ui: &Ui, t: &TorrentInfo) -> El {
    let lines = info_lines(t).into_iter().map(|(label, text, key)| {
        let value = ui.caption(text.clone()).key(key).truncate().min_w(0.0).shrink().help(text);
        let line = row().w_full().h(INFO_LINE).gap(12.0).center_y().child(ui.caption(label).w(INFO_LABEL)).child(value);
        if key == "info-magnet" {
            line.child(el().grow(1.0)).child(ui.button("copy-magnet", display::copy()).on_press_tx(copy_magnet).build())
        } else {
            line
        }
    });
    // Scrolls where the pane is too short for all of it.
    col()
        .key("info")
        .w_full()
        .scroll(ScrollSpec { stick_bottom: false })
        .child(col().w_full().px(HEADER_INSET).py(INFO_PAD).children(lines))
}

/// The width of the Info tab's labels, so the texts line up.
const INFO_LABEL: f32 = 96.0;

/// `text` shortened in the middle until `fits` says it does, as Finder shortens a path: its end
/// (the folder's or file's own name) is kept whole while the start can give way, then the end gives
/// way too, from its middle.
pub fn shorten_middle(text: &str, fits: impl Fn(&str) -> bool) -> String {
    if fits(text) {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    // The last part, from its separator, kept whole while there's room for it.
    let tail_from = text.rfind('/').map_or(0, |i| text[..i].chars().count());
    let joined = |head: usize, tail: usize| -> String {
        chars[..head].iter().chain(['…'].iter()).chain(chars[chars.len() - tail..].iter()).collect()
    };
    let tail = chars.len() - tail_from;
    // As much of the start as fits before the whole end.
    if let Some(head) = (0..tail_from).rev().find(|&head| fits(&joined(head, tail))) {
        return joined(head, tail);
    }
    // Then the end too: its middle goes, as much of each side as fits.
    (0..chars.len()).rev().map(|n| joined(n / 2, n - n / 2)).find(|t| fits(t)).unwrap_or_else(|| "…".into())
}

/// The files; with `settable`, a click on a file's priority cycles it (Normal → Off → Normal):
/// for the selected files if it's one of them, else for it alone.
fn file_table(s: &Phantom, ui: &Ui, settable: bool) -> El {
    // The Files list offers the file commands, for its selection.
    s.mounted.offer(crate::commands::OnFiles::ALL);
    let cui = ui.clone();
    let table = Table::new(
        |s: &Phantom| &s.pane.files,
        |s: &mut Phantom| &mut s.pane.files,
        |s: &Phantom| s.pane.details.as_ref().map_or(0, |d| d.files.len()),
        |s: &Phantom, i| Cow::Owned(files(s)[i].index.to_string()),
    )
    .name("Files")
    .column(TableColumn::new("path", "File").flex(3.0).min(160.0).sortable())
    .column(TableColumn::new("progress", "Progress").width(150.0).min(110.0).max(260.0).sortable())
    .column(TableColumn::new("size", "Size").width(84.0).min(70.0).align(Align::End).sortable())
    .column(TableColumn::new("priority", "Priority").width(72.0).min(60.0).sortable())
    .cell(move |s: &Phantom, i, column| {
        let f = &files(s)[i];
        let index = f.index;
        // A right-click anywhere in a row opens the file menu.
        let menu = move |s: &mut Phantom, press, tx: &mut Tx| super::file_menu::open(s, index, press, tx);
        match column {
            "path" => cui.menu_cell(cui.cell_text(f.path.to_string_lossy(), Tone::Normal, false), Align::Start, menu),
            "progress" if f.missing => cui.menu_cell(cui.cell_text("Missing", Tone::Error, false), Align::Start, menu),
            "progress" => cui.menu_cell(cui.progress_bar(f.progress()), Align::Start, menu),
            "size" => cui.menu_cell(cui.cell_text(display::bytes(f.size_bytes), Tone::Normal, true), Align::End, menu),
            _ => {
                let tone = if f.priority == Priority::Off { Tone::Muted } else { Tone::Normal };
                let label = cui.cell_text(client::display::priority(f.priority), tone, false);
                cui.press_menu_cell(label, settable.then_some(move |s: &mut Phantom| cycle_priority(&mut *s.service, &s.pane, index)), menu)
            }
        }
    })
    .on_sort(|s: &mut Phantom, column, dir, _| {
        if let Some(details) = &mut s.pane.details {
            sort_files(details, column, dir);
        }
    });
    // Where the file list has focus, the file shortcuts are its own: a key it leaves goes up to
    // here before the app's, so ⌘⌫ moves files to the Trash and never deletes the torrent.
    col()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .on_key(|s: &mut Phantom, ev: &rune::KeyEvent, _: &mut Tx| shortcuts::on_file_key(s, ev))
        .child(ui.table(s, table).grow(1.0).min_h(0.0))
}

fn files(s: &Phantom) -> &[File] {
    s.pane.details.as_ref().map_or(&[], |d| &d.files)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::shorten_middle;

    proptest! {
        /// The text as it is when it fits; otherwise it fits, has one "…", and keeps its last part
        /// whole whenever "…" and that part alone fit.
        #[test]
        fn a_path_is_shortened_in_the_middle(parts in proptest::collection::vec("[a-zA-Z ()0-9]{1,16}", 1..6), room in 1usize..80) {
            let path = format!("~/{}", parts.join("/"));
            let fits = |t: &str| t.chars().count() <= room;
            let shown = shorten_middle(&path, fits);
            if path.chars().count() <= room {
                prop_assert_eq!(shown, path);
            } else {
                prop_assert!(fits(&shown) || shown == "…");
                prop_assert_eq!(shown.matches('…').count(), 1);
                let last = &path[path.rfind('/').unwrap()..];
                if last.chars().count() + 1 <= room {
                    prop_assert!(shown.ends_with(last), "{} keeps {}", shown, last);
                }
            }
        }
    }
}

/// `~/Downloads/…` rather than `/Users/name/Downloads/…`.
fn home_relative(path: &Path) -> String {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    match home.as_deref().and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) => Path::new("~").join(rest).display().to_string(),
        None => path.display().to_string(),
    }
}
