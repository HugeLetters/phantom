//! The main window's layout rules (#33), as plain functions of its size: which of the list's columns
//! fit, and how tall the details pane is. `views` draws what these say; the person's own choices
//! (the columns' order, those hidden, the pane's height and tab) are kept in `gui.json`
//! ([`WindowPrefs`]).
//!
//! **Columns** are in the person's order (a drag on a header moves one). As the window narrows they
//! hide from the right end, one at a time, until the rest fit; the list always keeps at least one.
//! Name is a column like any other: it can be moved and hidden. Until a width is dragged it takes
//! the width the others leave, at least [`NAME_MIN`]; once its edge is dragged, or another column's
//! (which fixes Name where it was), its width is a width like the others', and the room the
//! columns leave stays empty at the list's right end. Status is as wide as its longest current word. A column either fits whole or isn't drawn: no column is ever cut, and the list
//! never scrolls sideways (only the one column left may be cut, in a window too narrow for it). The
//! room at the right end is empty until the next hidden column fits whole; no width changes to make
//! room but the one being dragged.
//!
//! **The pane** is as tall as its header, its tabs and its content, up to [`PANE_SHARE`] of the
//! room, or the room the list's rows leave when that's more (or the height the divider was dragged
//! to); the list always keeps its header and [`LIST_ROWS`] rows.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use serde::{Deserialize, Serialize};

/// One of the torrent list's columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Column {
    Name,
    Progress,
    Status,
    Size,
    Down,
    Up,
    Peers,
    Eta,
}

impl Column {
    /// Every column, in the order a fresh install draws them.
    pub const ALL: [Column; 8] =
        [Column::Name, Column::Progress, Column::Status, Column::Size, Column::Down, Column::Up, Column::Peers, Column::Eta];

    /// Its id in the table, in sorting and in `gui.json`.
    pub fn id(self) -> &'static str {
        match self {
            Column::Name => "name",
            Column::Progress => "progress",
            Column::Status => "status",
            Column::Size => "size",
            Column::Down => "down",
            Column::Up => "up",
            Column::Peers => "peers",
            Column::Eta => "eta",
        }
    }

    pub fn from_id(id: &str) -> Option<Column> {
        Column::ALL.into_iter().find(|c| c.id() == id)
    }

    /// Its header.
    pub fn title(self) -> &'static str {
        match self {
            Column::Name => "Name",
            Column::Progress => "Progress",
            Column::Status => "Status",
            Column::Size => "Size",
            Column::Down => "Down",
            Column::Up => "Up",
            Column::Peers => "Peers",
            Column::Eta => "ETA",
        }
    }

    /// Its width; Status's is its content's ([`fit`]), Name's the least it takes before the
    /// columns after it hide.
    pub fn width(self) -> f32 {
        match self {
            Column::Name => NAME_MIN,
            Column::Progress => 150.0,
            Column::Status => 0.0,
            Column::Size => 84.0,
            Column::Down | Column::Up => 92.0,
            Column::Peers => 60.0,
            Column::Eta => 72.0,
        }
    }
}

/// The columns in the person's order: every column, each once. `gui.json` keeps it as a list of
/// ids; one that isn't every column once is invalid, and reset.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<Column>", into = "Vec<Column>")]
pub struct ColumnOrder(Vec<Column>);

impl Default for ColumnOrder {
    fn default() -> Self {
        ColumnOrder(Column::ALL.to_vec())
    }
}

impl TryFrom<Vec<Column>> for ColumnOrder {
    type Error = String;
    fn try_from(columns: Vec<Column>) -> Result<Self, String> {
        let once = columns.len() == Column::ALL.len() && Column::ALL.iter().all(|c| columns.contains(c));
        if once { Ok(ColumnOrder(columns)) } else { Err("not every column once".into()) }
    }
}

impl From<ColumnOrder> for Vec<Column> {
    fn from(order: ColumnOrder) -> Self {
        order.0
    }
}

impl ColumnOrder {
    pub fn columns(&self) -> &[Column] {
        &self.0
    }

    /// This order with the columns `shown` (in it, in its order) put in the order `moved` (the
    /// same columns): the others keep their places.
    fn with_shown(&self, shown: &[Column], moved: &[Column]) -> ColumnOrder {
        let mut next = moved.iter();
        ColumnOrder(self.0.iter().map(|c| if shown.contains(c) { *next.next().expect("as many") } else { *c }).collect())
    }
}

/// The least Name gets before a column gives way to it, until its edge is dragged.
pub const NAME_MIN: f32 = 200.0;

/// What the list shows, in order: each column with its width, Name's `None` while it takes the rest.
pub type Fitted = Vec<(Column, Option<f32>)>;

/// The columns that fit `width` points of list, of those in `order` not `hidden`, each whole: they
/// hide from the right end until the rest fit, the first always kept (cut to the list, if it alone
/// is wider). `widths` gives each column's width (a dragged one's, Status's its longest current
/// word), Name's, while `name_flex` (it wasn't sized), the least it gets, [`NAME_MIN`]: it then
/// takes what's left, else it's a width like the others'. Room left over stays empty.
pub fn fit(width: f32, order: &ColumnOrder, hidden: &BTreeSet<Column>, name_flex: bool, widths: impl Fn(Column) -> f32) -> Fitted {
    let mut shown: Vec<Column> = order.columns().iter().copied().filter(|c| !hidden.contains(c)).collect();
    if shown.is_empty() {
        shown.push(order.columns()[0]);
    }
    let need = |shown: &[Column]| shown.iter().map(|c| widths(*c)).sum::<f32>();
    while shown.len() > 1 && need(&shown) > width {
        shown.pop();
    }
    let sole = shown.len() == 1;
    shown
        .into_iter()
        .map(|c| (c, (!(c == Column::Name && name_flex)).then(|| if sole { widths(c).min(width) } else { widths(c) })))
        .collect()
}

/// Where a column dragged along the header lands: `spans` are the columns shown with their widths,
/// in order, `dragged` one of them, `x` the pointer's distance from the header's start. Its index
/// among them such that, put there, its span holds the pointer (its own where it already does; the
/// ends past them), so a drag that crosses a wider neighbour's middle doesn't bounce.
pub fn drop_index(spans: &[(Column, f32)], dragged: Column, x: f32) -> usize {
    let Some(from) = spans.iter().position(|(c, _)| *c == dragged) else { return 0 };
    let w = spans[from].1;
    let others: Vec<f32> = spans.iter().filter(|(c, _)| *c != dragged).map(|(_, w)| *w).collect();
    // Put at `i`, it starts after the first `i` of the others.
    let start = |i: usize| others[..i].iter().sum::<f32>();
    let holds = |i: usize| (start(i)..=start(i) + w).contains(&x);
    let last = others.len();
    if holds(from) {
        from
    } else if x < 0.0 {
        0
    } else if x > start(last) + w {
        last
    } else {
        // Between two places (a neighbour wider than it): it stays until one holds the pointer.
        (0..=last).find(|&i| holds(i)).unwrap_or(from)
    }
}

/// The width of the toggle at the start of each row (#33): no column, so it is never moved, hidden
/// or counted in the order; the columns fit what it leaves.
pub const TOGGLE: f32 = 36.0;

/// A row of the list or of the files, and their headers.
pub const ROW: f32 = 28.0;
pub const TABLE_HEADER: f32 = 30.0;
/// The rows the list always keeps under its header, however tall the pane.
pub const LIST_ROWS: usize = 3;
/// The list's least height: its header and [`LIST_ROWS`] rows.
pub const LIST_MIN: f32 = TABLE_HEADER + LIST_ROWS as f32 * ROW;
/// The divider between the list and the pane, which drags.
pub const DIVIDER: f32 = 10.0;
/// The pane's share of the room, unless its divider was dragged or the list leaves more.
pub const PANE_SHARE: f32 = 0.4;
/// The pane's header: its padding, the name's line and the path's under it.
pub const HEADER_PAD: f32 = 14.0;
pub const NAME_LINE: f32 = 22.0;
pub const LINE: f32 = 18.0;
/// The pane's tab bar, Files · Info.
pub const TABS: f32 = 30.0;
/// The Info tab: a line each, and the padding around them.
pub const INFO_LINE: f32 = 26.0;
pub const INFO_PAD: f32 = 12.0;
/// What shows in place of the files before the info arrives.
pub const NO_FILES: f32 = 64.0;

/// How the pane's header is drawn: whole (the name's line, then the path's), or as one line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Header {
    Whole,
    OneLine,
}

impl Header {
    pub fn height(self) -> f32 {
        match self {
            Header::Whole => HEADER_PAD + NAME_LINE + LINE,
            Header::OneLine => HEADER_PAD + NAME_LINE,
        }
    }
}

/// The pane as drawn: its height and its header's shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pane {
    pub height: f32,
    pub header: Header,
}

/// What the pane holds under its header and tabs: `files` (`None` before its info arrives) and the
/// Info tab's `info` lines. The pane is as tall as the taller of the two, so a switch of tab never
/// moves the divider.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneContent {
    pub files: Option<usize>,
    pub info: usize,
}

impl PaneContent {
    fn height(self, header: Header) -> f32 {
        let files = self.files.map_or(NO_FILES, |n| TABLE_HEADER + n.max(1) as f32 * ROW);
        header.height() + TABS + files.max(2.0 * INFO_PAD + self.info as f32 * INFO_LINE)
    }
}

/// The most the pane may take of `room` (the height the list, the divider and the pane share).
pub fn pane_most(room: f32) -> f32 {
    (room - LIST_MIN - DIVIDER).max(0.0)
}

/// The least the divider can make the pane: a one-line header and the tabs over one file.
pub const PANE_LEAST: f32 = HEADER_PAD + NAME_LINE + TABS + TABLE_HEADER + ROW;

/// The pane in `room` points over a list of `rows` rows, holding `content`, and the height its
/// divider was dragged to, if it was: as tall as its content, up to [`PANE_SHARE`] of the room or
/// what the list's rows leave, whichever is more (or the dragged height), and never into the list's
/// least. Where the whole header would leave less than two files showing, it's one line.
pub fn pane(room: f32, content: PaneContent, dragged: Option<f32>, rows: usize) -> Pane {
    let most = pane_most(room);
    // At least a one-line header over two files where the room has it, so a small window still
    // shows files; at most what the list leaves.
    let least_useful = Header::OneLine.height() + TABS + TABLE_HEADER + 2.0 * ROW;
    let spare = room - DIVIDER - (TABLE_HEADER + rows.max(LIST_ROWS) as f32 * ROW);
    let cap = dragged.unwrap_or((room * PANE_SHARE).max(least_useful).max(spare)).min(most);
    let full = content.height(Header::Whole);
    let shows_two = cap - Header::Whole.height() - TABS - TABLE_HEADER >= 2.0 * ROW;
    let header = if full <= cap || shows_two { Header::Whole } else { Header::OneLine };
    Pane { height: content.height(header).min(cap), header }
}

/// The details pane's tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneTab {
    #[default]
    Files,
    Info,
}

impl PaneTab {
    pub const ALL: [PaneTab; 2] = [PaneTab::Files, PaneTab::Info];

    pub fn id(self) -> &'static str {
        match self {
            PaneTab::Files => "files",
            PaneTab::Info => "info",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            PaneTab::Files => "Files",
            PaneTab::Info => "Info",
        }
    }
}

/// A height the pane's divider was let go at: above zero and finite, as one `gui.json` keeps is;
/// anything else is invalid there, and reset.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "f32", into = "f32")]
pub struct PaneHeight(f32);

impl TryFrom<f32> for PaneHeight {
    type Error = String;
    fn try_from(height: f32) -> Result<Self, String> {
        if height.is_finite() && height > 0.0 { Ok(PaneHeight(height)) } else { Err("not a height above zero".into()) }
    }
}

impl From<PaneHeight> for f32 {
    fn from(height: PaneHeight) -> f32 {
        height.0
    }
}

/// The widths the person dragged columns to, in points: each above zero and finite, of a column
/// that resizes (not Status). `gui.json` keeps them as an object by column id; one that holds
/// anything else is invalid, and reset.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "BTreeMap<Column, f32>", into = "BTreeMap<Column, f32>")]
pub struct ColumnWidths(BTreeMap<Column, f32>);

impl TryFrom<BTreeMap<Column, f32>> for ColumnWidths {
    type Error = String;
    fn try_from(widths: BTreeMap<Column, f32>) -> Result<Self, String> {
        let good = widths.iter().all(|(c, w)| *c != Column::Status && w.is_finite() && *w > 0.0);
        if good { Ok(ColumnWidths(widths)) } else { Err("not widths above zero of resizable columns".into()) }
    }
}

impl From<ColumnWidths> for BTreeMap<Column, f32> {
    fn from(widths: ColumnWidths) -> Self {
        widths.0
    }
}

/// What `gui.json` keeps of the main window.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Kept {
    pub order: ColumnOrder,
    pub hidden: BTreeSet<Column>,
    pub widths: ColumnWidths,
    pub pane_height: Option<f32>,
    pub tab: PaneTab,
}

/// A column's edge being dragged: the width it's dragged to so far, and, while Name was taking
/// the rest, the width it was fixed at as another column's drag began.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Resizing {
    pub column: Column,
    pub width: f32,
    name: Option<f32>,
}

/// A column being dragged along the header: the order before the drag, put back if it's cancelled.
#[derive(Clone, Debug, PartialEq)]
pub struct Moving {
    pub column: Column,
    before: ColumnOrder,
}

/// How long a column a drag passes takes to slide into its place, in seconds.
pub const SLIDE: f32 = 0.15;

/// The columns sliding aside for `column`, which itself doesn't slide.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slide {
    pub column: Column,
    until: Option<f64>,
}

/// The person's choices about the main window, kept in `gui.json`: the columns' order and those
/// they hid, the height they dragged the details pane to and its tab. Each change is kept at once
/// through `keep` (nowhere in headless runs and tests, unless a test gives one).
#[derive(Clone, Default)]
pub struct WindowPrefs {
    pub kept: Kept,
    /// The divider's height while it's being dragged, not kept until it's let go.
    pub dragging: Option<f32>,
    /// The column being dragged along the header: the order changes as it goes, kept when let go.
    pub moving: Option<Moving>,
    /// The columns a drag passes slide while it's on and for [`SLIDE`] after it ends.
    pub slide: Option<Slide>,
    /// The last press on a header moved its column: it doesn't sort too.
    pub moved: bool,
    /// The column whose edge is being dragged: kept when let go.
    pub resizing: Option<Resizing>,
    keep: Option<Rc<dyn Fn(&'static str, serde_json::Value)>>,
}

/// The `gui.json` keys.
pub const COLUMN_ORDER: &str = "column_order";
pub const HIDDEN_COLUMNS: &str = "hidden_columns";
pub const COLUMN_WIDTHS: &str = "column_widths";
pub const PANE_HEIGHT: &str = "pane_height";
pub const PANE_TAB: &str = "details_tab";

impl WindowPrefs {
    /// As `kept` says, each change kept through `keep`.
    pub fn new(kept: Kept, keep: impl Fn(&'static str, serde_json::Value) + 'static) -> Self {
        Self { kept, dragging: None, moving: None, slide: None, moved: false, resizing: None, keep: Some(Rc::new(keep)) }
    }

    /// As `kept` says, kept nowhere.
    #[cfg(test)]
    pub fn unkept(kept: Kept) -> Self {
        Self { kept, ..Self::default() }
    }

    pub fn order(&self) -> &ColumnOrder {
        &self.kept.order
    }

    pub fn hidden(&self) -> &BTreeSet<Column> {
        &self.kept.hidden
    }

    /// Shows `column` if it's hidden, hides it if it shows, unless it's the last one `shown` (the
    /// list always keeps one).
    pub fn toggle(&mut self, column: Column, shown: &[Column]) {
        if !self.kept.hidden.remove(&column) {
            let wanted = Column::ALL.iter().filter(|c| !self.kept.hidden.contains(c) && **c != column).count();
            if shown == [column] || wanted == 0 {
                return;
            }
            self.kept.hidden.insert(column);
        }
        let hidden: Vec<Column> = self.kept.hidden.iter().copied().collect();
        self.kept(HIDDEN_COLUMNS, serde_json::to_value(hidden).expect("JSON"));
    }

    /// The width `column` was sized to, if it was: while its edge is being dragged, where the
    /// pointer is; Name's, while another's is, what it was fixed at.
    pub fn width(&self, column: Column) -> Option<f32> {
        match self.resizing {
            Some(r) if r.column == column => Some(r.width),
            Some(Resizing { name: Some(w), .. }) if column == Column::Name => Some(w),
            _ => self.kept.widths.0.get(&column).copied(),
        }
    }

    /// The edge of `column` is dragged to `width`. `name` is Name's drawn width if it takes the
    /// rest: it's fixed there, so it doesn't shrink as another column grows.
    pub fn resize_to(&mut self, column: Column, width: f32, name: Option<f32>) {
        let before = self.resizing.filter(|r| r.column == column).and_then(|r| r.name);
        let name = (column != Column::Name && self.kept.widths.0.get(&Column::Name).is_none()).then(|| name.or(before)).flatten();
        self.resizing = Some(Resizing { column, width, name });
    }

    /// The drag of the edge let go: the widths are kept, or, `cancelled`, forgotten.
    pub fn end_resize(&mut self, cancelled: bool) {
        let Some(r) = self.resizing.take() else { return };
        if cancelled {
            return;
        }
        let before = self.kept.widths.clone();
        self.kept.widths.0.insert(r.column, r.width);
        if let Some(name) = r.name {
            self.kept.widths.0.insert(Column::Name, name);
        }
        if self.kept.widths != before {
            self.kept_widths();
        }
    }

    /// A double-click on `column`'s edge: it goes back to its own width (Name to taking the rest).
    pub fn reset_width(&mut self, column: Column) {
        self.resizing = None;
        if self.kept.widths.0.remove(&column).is_some() {
            self.kept_widths();
        }
    }

    fn kept_widths(&self) {
        self.kept(COLUMN_WIDTHS, serde_json::to_value(&self.kept.widths).expect("JSON"));
    }

    /// A drag on `column`'s header began.
    pub fn start_move(&mut self, column: Column) {
        self.moving = Some(Moving { column, before: self.kept.order.clone() });
        self.slide = Some(Slide { column, until: None });
    }

    /// The dragged column is over index `to` of those `shown` (in order, the dragged one among
    /// them): it goes there, the others shown shifting, the rest keeping their places.
    pub fn move_to(&mut self, shown: &[Column], to: usize) {
        let Some(moving) = &self.moving else { return };
        let mut moved: Vec<Column> = shown.iter().copied().filter(|c| *c != moving.column).collect();
        moved.insert(to.min(moved.len()), moving.column);
        self.kept.order = self.kept.order.with_shown(shown, &moved);
    }

    /// The clock is at `now`: once the drag ended, its slide runs out [`SLIDE`] later.
    pub fn slide_passed(&mut self, now: f64) {
        let Some(slide) = &mut self.slide else { return };
        if self.moving.is_some() {
            return;
        }
        // A frame more than the slide, so nothing lands early.
        if now > *slide.until.get_or_insert(now + SLIDE as f64 + 0.05) {
            self.slide = None;
        }
    }

    /// The drag let go: the order is kept, or, `cancelled`, put back.
    pub fn end_move(&mut self, cancelled: bool) {
        let Some(moving) = self.moving.take() else { return };
        if cancelled {
            self.kept.order = moving.before;
        } else if self.kept.order != moving.before {
            self.kept(COLUMN_ORDER, serde_json::to_value(&self.kept.order).expect("JSON"));
        }
    }

    /// The height the pane takes up to: where the divider is, or was let go.
    pub fn dragged(&self) -> Option<f32> {
        self.dragging.or(self.kept.pane_height)
    }

    /// The divider was let go at `height`.
    pub fn set_pane_height(&mut self, height: f32) {
        self.dragging = None;
        self.kept.pane_height = Some(height);
        self.kept(PANE_HEIGHT, serde_json::json!(height));
    }

    pub fn tab(&self) -> PaneTab {
        self.kept.tab
    }

    pub fn set_tab(&mut self, tab: PaneTab) {
        if self.kept.tab != tab {
            self.kept.tab = tab;
            self.kept(PANE_TAB, serde_json::to_value(tab).expect("JSON"));
        }
    }

    fn kept(&self, key: &'static str, value: serde_json::Value) {
        if let Some(keep) = &self.keep {
            keep(key, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn hidden() -> impl Strategy<Value = BTreeSet<Column>> {
        proptest::collection::btree_set(proptest::sample::select(Column::ALL.to_vec()), 0..8)
    }

    fn order() -> impl Strategy<Value = ColumnOrder> {
        Just(Column::ALL.to_vec()).prop_shuffle().prop_map(ColumnOrder)
    }

    /// Each column's own width, Status's `status`, Name's least `name`.
    fn width_of(status: f32, name: f32) -> impl Fn(Column) -> f32 {
        move |c| match c {
            Column::Status => status,
            Column::Name => name,
            c => c.width(),
        }
    }

    proptest! {
        /// What shows is the start of the order's wanted columns (they hide from the right end),
        /// at least one; the columns shown fit whole, Name at its least, and the next one wanted
        /// would not have fitted.
        #[test]
        fn columns_hide_from_the_right_end(width in 100.0f32..2400.0, order in order(), hidden in hidden(), status in 60.0f32..200.0, name in 120.0f32..600.0) {
            let widths = width_of(status, name);
            let fitted = fit(width, &order, &hidden, true, &widths);
            let shown: Vec<Column> = fitted.iter().map(|(c, _)| *c).collect();
            let mut wanted: Vec<Column> = order.columns().iter().copied().filter(|c| !hidden.contains(c)).collect();
            if wanted.is_empty() {
                wanted.push(order.columns()[0]);
            }
            prop_assert!(!shown.is_empty(), "never no column");
            prop_assert_eq!(&shown[..], &wanted[..shown.len()], "in the order, the right end gone");
            let least = |cols: &[Column]| cols.iter().map(|c| widths(*c)).sum::<f32>();
            if shown.len() > 1 {
                prop_assert!(least(&shown) <= width, "whole columns, Name at its least");
            }
            if shown.len() < wanted.len() {
                prop_assert!(least(&wanted[..=shown.len()]) > width, "one hides only when it doesn't fit");
            }
            for (c, w) in &fitted {
                prop_assert_eq!(w.is_none(), *c == Column::Name, "only Name takes the rest");
            }
        }

        /// A narrower Name brings back what fits: with Name's least smaller, every column shown
        /// stays shown.
        #[test]
        fn a_narrower_name_keeps_every_column(width in 100.0f32..2400.0, order in order(), hidden in hidden(), a in 120.0f32..600.0, b in 120.0f32..600.0) {
            let (narrow, wide) = (a.min(b), a.max(b));
            let cols = |name| fit(width, &order, &hidden, true, width_of(100.0, name)).into_iter().map(|(c, _)| c).collect::<BTreeSet<_>>();
            prop_assert!(cols(wide).is_subset(&cols(narrow)));
        }

        /// A wider list shows every column a narrower one does.
        #[test]
        fn a_wider_list_shows_more(a in 100.0f32..2400.0, b in 100.0f32..2400.0, order in order(), hidden in hidden(), status in 60.0f32..200.0, name in 120.0f32..600.0) {
            let (narrow, wide) = (a.min(b), a.max(b));
            let widths = width_of(status, name);
            let cols = |w| fit(w, &order, &hidden, true, &widths).into_iter().map(|(c, _)| c).collect::<BTreeSet<_>>();
            prop_assert!(cols(narrow).is_subset(&cols(wide)));
        }

        /// Wherever the pointer is, the dragged column lands where its span holds it, at the end the
        /// pointer is past, or (between two places, by a wider neighbour) stays; landing changes
        /// nothing for a pointer still over its own span.
        #[test]
        fn a_dragged_column_lands_under_the_pointer(order in order(), n in 2usize..8, which in 0usize..8, x in -100.0f32..1400.0) {
            let spans: Vec<(Column, f32)> = order.columns()[..n].iter().map(|c| (*c, if *c == Column::Status { 90.0 } else { c.width() })).collect();
            let from = which % n;
            let dragged = spans[from].0;
            let to = drop_index(&spans, dragged, x);
            prop_assert!(to < n);
            let mut moved: Vec<(Column, f32)> = spans.iter().copied().filter(|(c, _)| *c != dragged).collect();
            moved.insert(to, spans[from]);
            let start: f32 = moved[..to].iter().map(|(_, w)| w).sum();
            let end = start + spans[from].1;
            let total: f32 = spans.iter().map(|(_, w)| w).sum();
            if (0.0..=total).contains(&x) {
                prop_assert!((start <= x && x <= end) || to == from, "{:?} at {} spans {}..{} for {}", dragged, to, start, end, x);
            } else {
                prop_assert_eq!(to, if x < 0.0 { 0 } else { n - 1 });
            }
            prop_assert_eq!(drop_index(&moved, dragged, x), to, "stable once there");
        }

        /// Moving a shown column keeps every column once, the others shown in their order, and the
        /// hidden ones where they were.
        #[test]
        fn moving_a_column_keeps_the_rest_in_place(order in order(), hidden in hidden(), which in 0usize..8, to in 0usize..8) {
            let shown: Vec<Column> = order.columns().iter().copied().filter(|c| !hidden.contains(c)).collect();
            prop_assume!(!shown.is_empty());
            let dragged = shown[which % shown.len()];
            let mut prefs = WindowPrefs { kept: Kept { order: order.clone(), hidden: hidden.clone(), ..Kept::default() }, ..WindowPrefs::default() };
            prefs.start_move(dragged);
            prefs.move_to(&shown, to);
            let after = prefs.order().clone();
            prop_assert!(ColumnOrder::try_from(after.columns().to_vec()).is_ok(), "every column once");
            let now_shown: Vec<Column> = after.columns().iter().copied().filter(|c| !hidden.contains(c)).collect();
            prop_assert_eq!(now_shown.iter().position(|c| *c == dragged), Some(to.min(shown.len() - 1)));
            let rest = |cols: &[Column]| cols.iter().copied().filter(|c| *c != dragged).collect::<Vec<_>>();
            prop_assert_eq!(rest(&now_shown), rest(&shown));
            for (i, c) in order.columns().iter().enumerate() {
                if hidden.contains(c) {
                    prop_assert_eq!(after.columns()[i], *c, "a hidden column keeps its place");
                }
            }
            prefs.end_move(true);
            prop_assert_eq!(prefs.order(), &order, "a cancelled drag puts it back");
        }

        /// The pane is never taller than its content nor than what the list leaves; it's as tall as
        /// its content when that fits in its share, or in what the list's rows leave; and its header
        /// is one line only when the whole header would leave fewer than two files showing.
        #[test]
        fn the_pane_is_sized_to_its_content(room in 150.0f32..1600.0, files in proptest::option::of(0usize..40), info in 2usize..4, dragged in proptest::option::of(PANE_LEAST..900.0), rows in 0usize..60) {
            let content = PaneContent { files, info };
            let p = pane(room, content, dragged, rows);
            prop_assert!(p.height <= content.height(p.header) + 0.01, "no blank under the files");
            prop_assert!(p.height <= pane_most(room) + 0.01, "the list keeps its header and three rows");
            let spare = room - DIVIDER - TABLE_HEADER - rows.max(LIST_ROWS) as f32 * ROW;
            let share = dragged.unwrap_or((room * PANE_SHARE).max(spare));
            let whole = content.height(Header::Whole);
            if whole <= share.min(pane_most(room)) {
                prop_assert_eq!(p, Pane { height: whole, header: Header::Whole }, "all of it, when it fits");
            }
            if p.header == Header::OneLine {
                prop_assert!(p.height - Header::Whole.height() - TABS - TABLE_HEADER < 2.0 * ROW);
            }
        }
    }

    #[test]
    fn a_one_file_torrent_takes_its_header_tabs_and_info() {
        let p = pane(800.0, PaneContent { files: Some(1), info: 3 }, None, 3);
        assert_eq!(p.height, Header::Whole.height() + TABS + 2.0 * INFO_PAD + 3.0 * INFO_LINE);
    }

    /// Over a list of two rows the pane takes the room the list doesn't need, past its 40 %.
    #[test]
    fn the_pane_takes_the_room_a_short_list_leaves() {
        let room = 500.0;
        let p = pane(room, PaneContent { files: Some(30), info: 3 }, None, 2);
        assert_eq!(p.height, room - DIVIDER - LIST_MIN);
        assert!(p.height > room * PANE_SHARE);
        let long = pane(room, PaneContent { files: Some(30), info: 3 }, None, 40);
        assert_eq!(long.height, room * PANE_SHARE, "a long list keeps the rest");
    }

    /// A sized Name is a width like the others': nothing takes the rest, so the room left over stays empty.
    #[test]
    fn a_sized_name_leaves_room_empty() {
        let order = ColumnOrder::default();
        let fitted = fit(900.0, &order, &BTreeSet::new(), false, |c| if c == Column::Status { 90.0 } else { c.width() });
        assert!(fitted.iter().all(|(_, w)| w.is_some()), "every width fixed: {fitted:?}");
        let total: f32 = fitted.iter().filter_map(|(_, w)| *w).sum();
        assert!(total < 900.0, "room is left");
    }

    /// Dragging another column fixes Name where it was drawn; let go, both widths are kept, and
    /// cancelled, neither.
    #[test]
    fn dragging_a_column_fixes_a_name_that_took_the_rest() {
        let mut prefs = WindowPrefs::default();
        prefs.resize_to(Column::Progress, 190.0, Some(333.0));
        prefs.resize_to(Column::Progress, 200.0, None);
        assert_eq!((prefs.width(Column::Progress), prefs.width(Column::Name)), (Some(200.0), Some(333.0)));
        prefs.end_resize(true);
        assert_eq!((prefs.width(Column::Progress), prefs.width(Column::Name)), (None, None));
        prefs.resize_to(Column::Progress, 200.0, Some(333.0));
        prefs.end_resize(false);
        assert_eq!((prefs.width(Column::Progress), prefs.width(Column::Name)), (Some(200.0), Some(333.0)));
        prefs.resize_to(Column::Size, 90.0, Some(10.0));
        assert_eq!(prefs.width(Column::Name), Some(333.0), "a sized Name stays as it was");
    }

    #[test]
    fn any_column_hides_but_never_the_last() {
        let mut prefs = WindowPrefs::default();
        let all = Column::ALL.to_vec();
        prefs.toggle(Column::Name, &all);
        assert_eq!(prefs.hidden(), &BTreeSet::from([Column::Name]), "Name hides like any column");
        for c in &Column::ALL[1..7] {
            prefs.toggle(*c, &all);
        }
        prefs.toggle(Column::Eta, &[Column::Eta]);
        assert!(!prefs.hidden().contains(&Column::Eta), "the last one stays");
        let fitted = fit(960.0, prefs.order(), prefs.hidden(), true, Column::width);
        assert_eq!(fitted, vec![(Column::Eta, Some(72.0))]);
        prefs.toggle(Column::Name, &[Column::Eta]);
        assert!(!prefs.hidden().contains(&Column::Name), "shown again");
    }

    /// A column narrowing left alone on screen can't be hidden either, though others are wanted.
    #[test]
    fn the_last_column_showing_can_t_be_hidden() {
        let mut prefs = WindowPrefs::default();
        let shown = fit(150.0, prefs.order(), prefs.hidden(), true, Column::width).into_iter().map(|(c, _)| c).collect::<Vec<_>>();
        assert_eq!(shown, [Column::Name], "the first kept, however narrow");
        prefs.toggle(Column::Name, &shown);
        assert!(prefs.hidden().is_empty());
    }

    #[test]
    fn an_order_must_name_every_column_once() {
        let parse = |v: serde_json::Value| serde_json::from_value::<ColumnOrder>(v).is_ok();
        assert!(parse(serde_json::json!(["eta", "name", "progress", "status", "size", "down", "up", "peers"])));
        assert!(!parse(serde_json::json!(["name", "progress"])), "some missing");
        assert!(!parse(serde_json::json!(["name", "name", "progress", "status", "size", "down", "up", "peers"])), "one twice");
    }
}
