# Commands are declared once and offered by what's on screen

Every command the GUI can run is declared once, in `gui/src/commands.rs`: its name, keys, icon and what it acts on (the selected torrents, the selected files, or the app). Behaviour lives in Phantom's own state, never in a theme's drawing. Pause acts on the selected torrents in `model`, whatever the theme draws; the worst a theme can do is draw a selection badly, it can't change what a command does.

Whether a command is offered right now is a separate question. The part of the screen it acts on registers it while that part is shown: the torrent list (`views/table.rs`) registers the torrent commands but Rename…, the details pane's header (`views/details.rs`) Rename…, whose field is its name, and its Files list the file commands, each frame as it's built (`commands::Mounted`). The row menu, the file menu, the command palette, the menu bar and the keys offer only what's registered, the selection allows and the service's actions list (the menu bar greys the rest, the others leave it out). A theme may show less ([ADR 0004](0004-themes-override-building-blocks.md)), for example a details pane with no Files list. Its commands then aren't offered anywhere, so a key never acts on something the user can't see. A theme that draws one of these screens itself makes the same one-line call. Registering belongs to the screens, not to the building blocks, so a theme that only redraws a block (an icon, the progress bar) keeps every command.

A test covers a theme whose details screen has no Files list and no header: the file menu has nothing and the palette lists only the list's torrent commands (no Rename…) and the app's. The rule's intent goes further: a theme that drops a command by accident should fail a test, and one that leaves it out on purpose should say so. Today only the Phantom theme's screens and that one case are tested, not every theme.

## Considered options

- **Every command always available**: simplest, but a theme that hides a view leaves its commands acting on what isn't shown, and menus offer items that do nothing.
- **Each theme implements its views' behaviour**: what a theme built on raw widgets would need. Nothing would check that a theme got the selection or the arrow keys right.

## Consequences

- **The command list** feeds the row menu, the file menu, the command palette, the row's Resume/Pause toggle, the empty list's text and the keys (`shortcuts.rs`), so their names, icons and keys can't drift apart. The menu bar (`menu_bar.rs`, shown by `platform/menu_bar.rs`) is built from it too, its titles in title case.
- **A command is registered by the part of the screen its field is in**: Rename… opens in the details pane's header, so the header registers it; the pane shows whenever exactly one torrent is selected, which is when Rename… applies, whatever columns the list shows (#87).
- **Icons** are looked up through the theme like any building block: a command names a `ui::Icon`, and a theme draws any of them in `BlockOverrides.icons`, falling back to its parent's and then Phantom's. A command may have none (the file menu's priorities: its check is the `Icon::Check` in the gutter).
- **The menu bar never runs a key twice**: AppKit matches an item's key before the window sees the press, so one of them runs it; the Torrent menu's keys (⌫, Space) are its items' only while it's open, so the window has them and a field keeps its own. A pick carries the torrents it was built for and does nothing once the selection is another.
- **Platforms:** the same list could feed an in-window menu on systems without a menu bar ([ADR 0005](0005-macos-only-for-now.md), #175).
- **Behaviour that rune-kit's widgets carry today**, such as ⌘A in its table, isn't a command yet; it moves into Phantom's own state when the command list takes it over.
