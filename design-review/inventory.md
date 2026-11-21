# Phantom: every user action and where it's offered

Read from the code at master `8da3bda` (not from the UI). Sources: `gui/src/views/toolbar.rs`,
`views/row_menu.rs`, `views/file_menu.rs`, `views/details.rs`, `views/table.rs`, `views/status_bar.rs`,
`views/clash.rs`, `views/settings.rs`, `shortcuts.rs`, `main.rs` (`on_key`), `platform/menu_bar.rs`,
`platform/mod.rs`, `cli/src/main.rs`.

Stills of each place: `design-review/<area>/` on this branch.

## Torrent actions

| Verb | Toolbar | Row menu | Details pane | Menu bar | Shortcut | CLI | Notes |
|---|---|---|---|---|---|---|---|
| Add (magnet, .torrent URL, info hash) | field + **Add** | | | | Return in the field | `add SOURCE [--paused] [--destination]` | Also: drop a file/link on the window, a magnet clicked in a browser, a .torrent opened from Finder |
| Add file… | **Add file…** | | | | ⇧⌘A | `add FILE` | Not in the menu bar, so ⇧⌘A isn't discoverable there |
| Pause | **Pause** (selection) | Pause | | | none | `pause ID` | Duplicate: toolbar and menu. Disabled click says why in a note |
| Resume | **Resume** (selection) | Resume | | | none | `resume ID` | Duplicate: toolbar and menu |
| Check | | Check | | | none | `check ID` | Row menu only |
| Cancel check | | Cancel check | | | none | `cancel-check ID` | Row menu only |
| Cancel move | | Cancel move | | | none | `cancel-move ID…` | Row menu only |
| Open | | Open (one torrent) | | | ⌘O, ⌘↓, Return, double-click a row | none | In the file list: opens the selected file |
| Reveal in Finder | | Reveal in Finder (one) | | | ⇧⌘O | none | Also a button in the clash dialog and in the settings-reset notice's list |
| Move to… | | Move to… (one or several) | | | none | `move ID… FOLDER [--clash] [--name]` | Native folder picker, then the clash dialog if needed |
| Rename… | | Rename… (one) | | | none | `rename ID NAME` | Inline field in the Name cell; also in the clash dialog (Rename…) |
| Copy magnet link | | Copy magnet link (one) | | | none | `magnet ID` | Row menu only (earlier builds had a details-pane button; it's gone) |
| Remove (keep files) | | Remove torrent / Remove n torrents (keep files) | | | ⌫ (no confirmation) | `remove ID…` (`rm`) | No toolbar button |
| Delete (files to Trash) | | Delete torrent / Delete n torrents (files to the Trash) | | | ⌘⌫ (no confirmation) | `delete ID…` | No toolbar button |
| Undo | Undo button in the action notice (and its popover) | | | Edit ▸ Undo (title names what it takes back) | ⌘Z | `undo`, `settings undo` | Three places in the GUI; takes the newest change, setting or torrent action |
| Select all / none | | | | | Escape clears the selection (and closes the pane) | | Shift-click / shift-arrows extend; no Select All for rows |
| Sort | column headers (list, Files) | | | | | | Click a header |

## File actions (details pane's Files list)

| Verb | Toolbar | Row menu | Details pane | Menu bar | Shortcut | CLI | Notes |
|---|---|---|---|---|---|---|---|
| Set priority Normal/Off | | | click the Priority cell (cycles) and the file menu's Priority group | | none | `priority ID normal\|off FILE…` | The torrent row menu has an item "Set priority" in the code (`RowItem::Do(SetPriority)`) but runs nothing and `row_items` never offers it |
| Move to Trash (files) | | | file menu | | ⌘⌫ with the file list focused | none | Sets Off, then trashes; ⌘⌫ means Delete torrent when the list has the focus instead |
| Open (file) | | | file menu | | ⌘O, ⌘↓ with the file list focused | none | |
| Reveal in Finder (file) | | | file menu | | ⇧⌘O with the file list focused | none | |
| List files | | | the Files list itself | | | `files ID` | |

The details pane has **no buttons** at all in this build: only the Files list (sortable columns, a clickable Priority cell,
a right-click file menu). Everything that can be done to the torrent is in its row menu.

## Notices and status bar

| Verb | Where | Shortcut | Notes |
|---|---|---|---|
| Open the notice list | click the bar's line ("Removed ›", "· 1/3 ›") | | Only if the notice has a list or there are several; a lone notice without a list is dismissed by the click |
| Step between notices | ‹ › in the popover header | ← → with focus inside | Newest first |
| Dismiss one | × (popover) | | |
| Dismiss all | "Dismiss n messages" (popover, when several) | | |
| Close the list | click outside | Escape (first, before clearing the selection) | |

## Add field and dialogs

| Verb | Where | Shortcut |
|---|---|---|
| Add's error note | click Add while off, or Return in the field with a bad link | |
| Close the Add / Resume / Pause note | click elsewhere, Escape | Escape |
| Clash dialog: Use existing / Replace / Cancel / Reveal in Finder / Rename… (Move) | dialog buttons, × | Escape = Cancel / back from Rename |
| Inline Rename (Name cell): commit / cancel | Return / Escape | |

## Windows and app

| Verb | Toolbar | Menu bar | Shortcut | CLI | Notes |
|---|---|---|---|---|---|
| Settings | | Phantom ▸ Settings… | ⌘, | `settings`, `settings KEY VALUE`, `settings unset KEY` | ⌘, also works in Settings |
| Close window | | Window ▸ Close (Rune's standard menu; `main.rs` calls it File ▸ Close Window) | ⌘W | | Keeps downloading; in Settings it closes a preset sheet first |
| Minimize | | Window ▸ Minimize | ⌘M | | Rune's standard menu |
| Bring the main window back | | Window ▸ Phantom | | | Also the Dock icon |
| Quit | | Phantom ▸ Quit (the OS/winit default menu, not checked: Rune adds no Quit item; ⌘Q itself is handled in `on_key`) | ⌘Q | `stop` (stops the service even with clients) | |
| Edit: Undo, Redo, Cut, Copy, Paste, Select All | | Edit menu (Rune standard) | ⌘Z, ⇧⌘Z, ⌘X, ⌘C, ⌘V, ⌘A | | For fields; Redo has no meaning for torrent actions ("no redo") |
| Keep the service running | | | | `keepalive` | |
| List torrents | | | | `list` (`ls`), `watch` | The window is the list |

## Settings

| Verb | Pane | CLI |
|---|---|---|
| Default download folder: Choose… / Clear | General | `settings default-destination FOLDER`, `unset` |
| Keep awake (switch) | General | `keep-awake on\|off` |
| Stop seeding at ratio / after time (fields) | General | `seed-ratio`, `seed-time` |
| Listening port (field), Pick automatically | Network | `listening-port` |
| UPnP (switch) | Network | `upnp on\|off` |
| Download / upload limit (fields) | Network | `download-limit`, `upload-limit` |
| Look Presets: Edit… / Save as… (Active look); chips ◐ ☀ ☾ to apply, Edit…, Delete (saved) | Appearance | none |
| Preset editor sheet: Theme, Tone and other theme settings (Light, Dark, Split/Join), Done | Appearance | none |
| App icon: Follow theme, designed pairs, Custom pair…, one icon always | Appearance | none |

## The menu bar (what exists; native, not drawable headless)

- **Phantom** (app menu, macOS/winit default): Settings… ⌘,, Quit ⌘Q (plus the system's Hide, Services); only Settings… is Phantom's own.
- **Edit** (Rune standard): Undo (retitled to name the change) ⌘Z, Redo ⇧⌘Z, Cut ⌘X, Copy ⌘C, Paste ⌘V, Select All ⌘A.
- **Window** (Rune standard + one): Minimize ⌘M, Close ⌘W, **Phantom** (brings the main window back).
- There is no File, View or torrent menu: none of Add file…, Pause, Resume, Check, Move to…, Rename…, Remove, Delete, Open or Reveal is in the menu bar. Their shortcuts exist (see the table) but only the row menu shows them.

## Reading the table

**Duplicates (one action, several places)**
- Pause and Resume: toolbar and row menu (both act on the selection).
- Add file…: toolbar, ⇧⌘A (not in any menu).
- Open: row menu, three shortcuts (⌘O, ⌘↓, Return) and double-click.
- Undo: notice button, ⌘Z, Edit ▸ Undo.
- Reveal in Finder: row menu, ⇧⌘O, file menu, clash dialog, settings-reset notice.
- Priority: Priority cell click and the file menu.
- Rename: the row menu and the clash dialog's Rename… (a different thing: it names the destination).

**Offered in one place only**
- Check, Cancel check, Cancel move, Move to…, Rename…, Copy magnet link: the row menu only (no shortcut, no toolbar).
- Remove and Delete: row menu and keys; no toolbar button, so a mouse-only user needs right-click.
- Add (field + button + Return) is the only way to add a magnet from the window (besides macOS link handling and drops).
- Look Presets, the App icon, the editor sheet: Settings only; no CLI.
- Open and Reveal have no CLI command; Delete's Trash, Remove's keep-files choice have no confirmation (undo instead).

**Dead or hidden**
- `RowItem::Do(Action::SetPriority | TrashFiles)` is worded "Set priority" in `row_menu.rs` and handled as a no-op, but `row_items` never offers it.
- There is no way to see the shortcuts except the row/file menus' hints and the "Add file…" tooltip (⇧⌘A).
