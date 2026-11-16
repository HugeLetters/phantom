//! The failures of a batch, as the view of its error notice: each the file or link and why.

use rune::{col, text};

use super::Ui;

/// One failure of a batch: what failed, and why.
pub struct Failure<'a> {
    pub what: &'a str,
    pub why: &'a str,
}

impl Ui {
    /// The failures of a batch, each the file or link and why, when there's a reason to give.
    pub fn failure_list(&self, failures: &[Failure<'_>]) -> rune::El {
        let k = &self.k;
        let rows = failures.iter().map(|failure| {
            let what = text(failure.what.to_owned()).font(k.ty.text_small).color(k.c.text_base).truncate_middle();
            let why = (!failure.why.is_empty()).then(|| text(failure.why.to_owned()).font(k.ty.text_small).color(k.c.text_muted).wrap());
            col().child(what).children(why)
        });
        // Indented under the notice's title.
        col().gap(6.0).pl(12.0).children(rows)
    }
}
