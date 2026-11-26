//! The room Phantom's screens and blocks leave between and around things, and the fixed heights
//! they draw at: a small scale of steps for gaps and padding, and a size named for each fixed
//! height or inset that isn't on it. A theme sets them ([`Spacing::phantom`] is today's); screens
//! and blocks read these and never write a length of their own for room.
//!
//! Some sizes are also the model's, which lays out the pane before anything is drawn
//! (`model::layout`): they hold the same values, which a test pins.

/// Phantom's spacing: steps `s1` (the smallest) to `s7`, and named sizes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spacing {
    /// 2: between lines of one item.
    pub s1: f32,
    /// 4
    pub s2: f32,
    /// 6
    pub s3: f32,
    /// 8: the usual gap between controls.
    pub s4: f32,
    /// 10
    pub s5: f32,
    /// 12
    pub s6: f32,
    /// 16: a dialog's or a sheet's padding.
    pub s7: f32,

    /// The window's margin, either side of the list, the pane and the status bar.
    pub margin: f32,
    /// The titlebar band's height, which the toolbar fills.
    pub toolbar: f32,
    /// A table's row, and its header.
    pub row: f32,
    pub table_header: f32,
    /// The room either side of a table cell's content.
    pub cell_pad: f32,
    /// The status bar's notice line, a button.
    pub status_line: f32,
    /// The divider between the list and the pane.
    pub divider: f32,
    /// The pane's header: the name's line and the path's under it.
    pub pane_name_line: f32,
    pub pane_line: f32,
    /// The pane's tab bar, Files · Info.
    pub pane_tabs: f32,
    /// What shows in place of the files before the info arrives.
    pub no_files: f32,
    /// The Info tab: a line each, and the padding around them.
    pub info_line: f32,
    pub info_pad: f32,
    /// A tab bar's gap between tabs, and between a tab's title and its mark.
    pub tabs_gap: f32,
    pub tab_mark_gap: f32,
    /// The gap under a dialog's header and between its parts.
    pub dialog_gap: f32,
    /// A palette row, its section's title row, and the room over the title.
    pub palette_row: f32,
    pub palette_section: f32,
}

impl Spacing {
    /// Phantom's, as every screen has drawn so far.
    pub fn phantom() -> Self {
        Self {
            s1: 2.0,
            s2: 4.0,
            s3: 6.0,
            s4: 8.0,
            s5: 10.0,
            s6: 12.0,
            s7: 16.0,
            margin: 12.0,
            toolbar: 40.0,
            row: 28.0,
            table_header: 30.0,
            cell_pad: 10.0,
            status_line: 20.0,
            divider: 10.0,
            pane_name_line: 22.0,
            pane_line: 18.0,
            pane_tabs: 30.0,
            no_files: 64.0,
            info_line: 26.0,
            info_pad: 12.0,
            tabs_gap: 18.0,
            tab_mark_gap: 5.0,
            dialog_gap: 14.0,
            palette_row: 32.0,
            palette_section: 28.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::layout;

    /// Every token is the length its screen or block wrote before it was a token, and a size the
    /// model lays out with is the model's.
    #[test]
    fn tokens_equal_todays_lengths() {
        let t = Spacing::phantom();
        assert_eq!([t.s1, t.s2, t.s3, t.s4, t.s5, t.s6, t.s7], [2.0, 4.0, 6.0, 8.0, 10.0, 12.0, 16.0]);
        assert_eq!((t.margin, t.toolbar, t.cell_pad), (12.0, 40.0, 10.0));
        assert_eq!((t.row, t.table_header), (layout::ROW, layout::TABLE_HEADER));
        assert_eq!((t.row, t.table_header), (28.0, 30.0));
        assert_eq!((t.status_line, t.divider), (20.0, layout::DIVIDER));
        assert_eq!(t.s4 + t.s3, layout::HEADER_PAD, "the pane header's padding, over and under");
        assert_eq!((t.pane_name_line, t.pane_line, t.pane_tabs), (layout::NAME_LINE, layout::LINE, layout::TABS));
        assert_eq!((t.no_files, t.info_line, t.info_pad), (layout::NO_FILES, layout::INFO_LINE, layout::INFO_PAD));
        assert_eq!((t.tabs_gap, t.tab_mark_gap, t.dialog_gap), (18.0, 5.0, 14.0));
        assert_eq!((t.palette_row, t.palette_section), (32.0, 28.0));
        let kit = rune_kit::TableStyle::from_kit(&rune_kit::Kit::neutral(false));
        assert_eq!((t.row, t.table_header, t.cell_pad), (kit.row_height, kit.header_height, kit.cell_pad), "the kit's table");
    }
}
