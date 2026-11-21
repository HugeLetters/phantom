//! Phantom's icons: the only place SVG appears. Drawings are from Lucide (ISC); its notice is in `LICENSE-lucide.txt`, next to this file.

use rune::Icon as Drawing;

use crate::ui::Icon;

static COPY: Drawing = Drawing {
    name: "phantom-copy",
    view: 24.0,
    body: r#"<rect fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" width="14" height="14" x="8" y="8" rx="2" ry="2"/><path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/>"#,
};

static WARNING: Drawing = Drawing {
    name: "phantom-warning",
    view: 24.0,
    body: r#"<path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3"/><path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" d="M12 9v4"/><path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" d="M12 17h.01"/>"#,
};

static LIGHT: Drawing = Drawing {
    name: "phantom-sun",
    view: 24.0,
    body: r#"<circle cx="12" cy="12" r="4" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/><path d="M12 2v2" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/><path d="M12 20v2" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/><path d="m4.93 4.93 1.41 1.41" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/><path d="m17.66 17.66 1.41 1.41" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/><path d="M2 12h2" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/><path d="M20 12h2" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/><path d="m6.34 17.66-1.41 1.41" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/><path d="m19.07 4.93-1.41 1.41" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

static DARK: Drawing = Drawing {
    name: "phantom-moon",
    view: 24.0,
    body: r#"<path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z"/>"#,
};

static BOTH: Drawing = Drawing {
    name: "phantom-both",
    view: 24.0,
    body: r#"<circle cx="12" cy="12" r="9" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/><path d="M12 3a9 9 0 0 0 0 18Z" fill="currentColor" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

/// A Lucide icon of stroked paths, as Lucide draws them all: 2 wide, round caps and joins, no fill.
macro_rules! lucide {
    ($static:ident, $name:literal, [$($d:literal),+ $(,)?]) => {
        static $static: Drawing = Drawing {
            name: $name,
            view: 24.0,
            body: concat!($(
                r#"<path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" d=""#,
                $d,
                r#""/>"#
            ),+),
        };
    };
}

// The commands' (Lucide's names in brackets); circles and rectangles as paths.
// play
lucide!(RESUME, "phantom-resume", ["M6 3L20 12L6 21Z"]);
// pause
lucide!(
    PAUSE,
    "phantom-pause",
    [
        "M15 4h2a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1h-2a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1Z",
        "M7 4h2a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1Z",
    ]
);
// file-check
lucide!(
    VERIFY,
    "phantom-verify",
    ["M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z", "M14 2v4a2 2 0 0 0 2 2h4", "m9 15 2 2 4-4"]
);
// circle-x
lucide!(CANCEL_CHECK, "phantom-cancel-check", ["M2 12a10 10 0 1 0 20 0a10 10 0 1 0-20 0", "m15 9-6 6", "m9 9 6 6"]);
// folder-x
lucide!(
    CANCEL_MOVE,
    "phantom-cancel-move",
    [
        "M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z",
        "m9.5 10.5 5 5",
        "m14.5 10.5-5 5",
    ]
);
// external-link
lucide!(OPEN, "phantom-open", ["M15 3h6v6", "M10 14 21 3", "M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"]);
// folder-search
lucide!(
    REVEAL,
    "phantom-reveal",
    [
        "M10.7 20H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H20a2 2 0 0 1 2 2v4.1",
        "m21 21-1.9-1.9",
        "M14 17a3 3 0 1 0 6 0a3 3 0 1 0-6 0",
    ]
);
// folder-input
lucide!(
    MOVE_TO,
    "phantom-move-to",
    [
        "M2 9V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H20a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2v-1",
        "M2 13h10",
        "m9 16 3-3-3-3",
    ]
);
// pencil
lucide!(
    RENAME,
    "phantom-rename",
    [
        "M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z",
        "m15 5 4 4",
    ]
);
// list-minus
lucide!(REMOVE, "phantom-remove", ["M11 12H3", "M16 6H3", "M16 18H3", "M21 12h-6"]);
// trash-2
lucide!(
    TRASH,
    "phantom-trash",
    ["M3 6h18", "M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6", "M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2", "M10 11v6", "M14 11v6",]
);
// file-plus
lucide!(
    ADD_FILE,
    "phantom-add-file",
    ["M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z", "M14 2v4a2 2 0 0 0 2 2h4", "M9 15h6", "M12 18v-6"]
);
// undo-2
lucide!(UNDO, "phantom-undo", ["M9 14 4 9l5-5", "M4 9h10.5a5.5 5.5 0 0 1 5.5 5.5a5.5 5.5 0 0 1-5.5 5.5H11"]);
// search
lucide!(SEARCH, "phantom-search", ["M3 11a8 8 0 1 0 16 0a8 8 0 1 0-16 0", "m21 21-4.3-4.3"]);
static DONE: Drawing = Drawing {
    name: "phantom-done",
    view: 24.0,
    body: r#"<circle cx="12" cy="12" r="10" fill="none" stroke="currentColor" stroke-width="2"/><path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" d="m9 12 2 2 4-4"/>"#,
};

static ERROR: Drawing = Drawing {
    name: "phantom-error",
    view: 24.0,
    body: r#"<path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" d="M2.586 16.726A2 2 0 0 1 2 15.312V8.688a2 2 0 0 1 .586-1.414l4.688-4.688A2 2 0 0 1 8.688 2h6.624a2 2 0 0 1 1.414.586l4.688 4.688A2 2 0 0 1 22 8.688v6.624a2 2 0 0 1-.586 1.414l-4.688 4.688a2 2 0 0 1-1.414.586H8.688a2 2 0 0 1-1.414-.586z"/><path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" d="m15 9-6 6M9 9l6 6"/>"#,
};

static CLOSE: Drawing = Drawing {
    name: "phantom-close",
    view: 24.0,
    body: r#"<path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" d="M18 6 6 18M6 6l12 12"/>"#,
};

static PREVIOUS: Drawing = Drawing {
    name: "phantom-previous",
    view: 24.0,
    body: r#"<path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" d="m15 18-6-6 6-6"/>"#,
};

static NEXT: Drawing = Drawing {
    name: "phantom-next",
    view: 24.0,
    body: r#"<path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" d="m9 18 6-6-6-6"/>"#,
};

/// Phantom draws every icon.
pub fn icon(icon: Icon) -> &'static Drawing {
    match icon {
        Icon::Copy => &COPY,
        Icon::Warning => &WARNING,
        // Rune's own check, as its menus draw one.
        Icon::Check => &rune_kit::glyphs::CHECK_SMALL,
        Icon::Light => &LIGHT,
        Icon::Dark => &DARK,
        Icon::Both => &BOTH,
        Icon::Resume => &RESUME,
        Icon::Pause => &PAUSE,
        Icon::Verify => &VERIFY,
        Icon::CancelCheck => &CANCEL_CHECK,
        Icon::CancelMove => &CANCEL_MOVE,
        Icon::Open => &OPEN,
        Icon::Reveal => &REVEAL,
        Icon::MoveTo => &MOVE_TO,
        Icon::Rename => &RENAME,
        Icon::Remove => &REMOVE,
        Icon::Trash => &TRASH,
        Icon::AddFile => &ADD_FILE,
        Icon::Undo => &UNDO,
        Icon::Search => &SEARCH,
        Icon::Done => &DONE,
        Icon::Error => &ERROR,
        Icon::Close => &CLOSE,
        Icon::Previous => &PREVIOUS,
        Icon::Next => &NEXT,
    }
}
