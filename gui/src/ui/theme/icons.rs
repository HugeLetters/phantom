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

pub fn phantom(icon: Icon) -> Option<&'static Drawing> {
    Some(match icon {
        Icon::Copy => &COPY,
        Icon::Warning => &WARNING,
    })
}
