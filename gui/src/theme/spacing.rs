//! The room Phantom's screens and blocks leave between and around things, and the fixed heights
//! they draw at: a small scale of steps for gaps and padding, and a size named for each fixed
//! height or inset that isn't on it. A theme sets them ([`Spacing::phantom`] is today's); screens
//! and blocks read these and never write a length of their own for room.
//!
//! Some sizes are also the model's, which lays out the pane before anything is drawn
//! (`model::layout`): they hold the same values, in each density, which a test pins.

use crate::model::layout;

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

impl Spacing {
    /// Compact: the same text in less room. The steps shrink by about a quarter (2 stays: it
    /// separates lines of one item), the rows, bands and insets by what the lines of text allow.
    pub fn compact() -> Self {
        Self {
            s1: 2.0,
            s2: 3.0,
            s3: 4.0,
            s4: 6.0,
            s5: 8.0,
            s6: 9.0,
            s7: 12.0,
            margin: 8.0,
            toolbar: 32.0,
            row: 22.0,
            table_header: 24.0,
            cell_pad: 8.0,
            status_line: 20.0,
            divider: 8.0,
            pane_name_line: 20.0,
            pane_line: 18.0,
            pane_tabs: 24.0,
            no_files: 48.0,
            info_line: 22.0,
            info_pad: 8.0,
            tabs_gap: 14.0,
            tab_mark_gap: 4.0,
            dialog_gap: 10.0,
            palette_row: 28.0,
            palette_section: 24.0,
        }
    }

    /// The sizes the model lays the pane out with, as these tokens say.
    pub fn sizes(&self) -> layout::Sizes {
        layout::Sizes {
            row: self.row,
            table_header: self.table_header,
            divider: self.divider,
            header_pad: self.s4 + self.s3,
            name_line: self.pane_name_line,
            line: self.pane_line,
            tabs: self.pane_tabs,
            info_line: self.info_line,
            info_pad: self.info_pad,
            no_files: self.no_files,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The model's copies are the tokens', in both densities.
    #[test]
    fn the_models_sizes_are_the_tokens_in_both_densities() {
        assert_eq!(Spacing::phantom().sizes(), layout::Sizes::COMFORTABLE);
        assert_eq!(Spacing::compact().sizes(), layout::Sizes::COMPACT);
    }

    #[test]
    fn compact_is_tighter_in_every_step_and_the_listed_sizes() {
        let (c, p) = (Spacing::compact(), Spacing::phantom());
        assert_eq!((c.row, c.table_header, c.toolbar, c.margin), (22.0, 24.0, 32.0, 8.0));
        let (cs, ps) = ([c.s1, c.s2, c.s3, c.s4, c.s5, c.s6, c.s7], [p.s1, p.s2, p.s3, p.s4, p.s5, p.s6, p.s7]);
        assert!(cs.windows(2).all(|w| w[0] < w[1]), "the steps stay in order: {cs:?}");
        assert!(cs.iter().zip(ps).all(|(c, p)| *c <= p));
        assert_ne!(c, p);
    }

    /// Every token is the length its screen or block wrote before it was a token, and a size the
    /// model lays out with is the model's.
    #[test]
    fn tokens_equal_todays_lengths() {
        let t = Spacing::phantom();
        assert_eq!([t.s1, t.s2, t.s3, t.s4, t.s5, t.s6, t.s7], [2.0, 4.0, 6.0, 8.0, 10.0, 12.0, 16.0]);
        assert_eq!((t.margin, t.toolbar, t.cell_pad), (12.0, 40.0, 10.0));
        assert_eq!((t.row, t.table_header), (layout::Sizes::COMFORTABLE.row, layout::Sizes::COMFORTABLE.table_header));
        assert_eq!((t.row, t.table_header), (28.0, 30.0));
        assert_eq!((t.status_line, t.divider), (20.0, layout::Sizes::COMFORTABLE.divider));
        assert_eq!(t.s4 + t.s3, layout::Sizes::COMFORTABLE.header_pad, "the pane header's padding, over and under");
        assert_eq!(
            (t.pane_name_line, t.pane_line, t.pane_tabs),
            (layout::Sizes::COMFORTABLE.name_line, layout::Sizes::COMFORTABLE.line, layout::Sizes::COMFORTABLE.tabs)
        );
        assert_eq!(
            (t.no_files, t.info_line, t.info_pad),
            (layout::Sizes::COMFORTABLE.no_files, layout::Sizes::COMFORTABLE.info_line, layout::Sizes::COMFORTABLE.info_pad)
        );
        assert_eq!((t.tabs_gap, t.tab_mark_gap, t.dialog_gap), (18.0, 5.0, 14.0));
        assert_eq!((t.palette_row, t.palette_section), (32.0, 28.0));
        let kit = rune_kit::TableStyle::from_kit(&rune_kit::Kit::neutral(false));
        assert_eq!((t.row, t.table_header, t.cell_pad), (kit.row_height, kit.header_height, kit.cell_pad), "the kit's table");
    }
}
