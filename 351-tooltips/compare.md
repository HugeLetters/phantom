# #351 tooltips: A (Rune's `.help` look) vs B (Phantom's look)

Prototype on branch `351-tooltips` (on `origin/381-rune-update`, Rune 5986ab7). Stills: headless, 2x, cropped around the place, the pointer resting on it 1.5 s (`render.py` in the scratchpad: `--script` move, `--at 1.5`).
- **A**: every tooltip is Rune's `.help` (`PHANTOM_TIP_A=1`: `explained` becomes `.help`, the two field notes become `.help_open(true)` on the field).
- **B**: every tooltip in Phantom's look (`PHANTOM_TIP_B=1`: `cx.help_look(ui.help_look())`, 14 lines, makes Rune draw all `.help` as `tooltip()` does; `explained`/`tooltip_under` are the real ones).

**Caveat on A:** on macOS the real app shows AppKit's own tooltip for `.help` (Phantom sets no `help_look`), which headless can't capture. The A stills are Rune's own drawn look (`HelpLook::system`), what Rune draws where AppKit isn't used. Real A on the Mac looks like any macOS tooltip (system fonts, system delay), not like these.

| Place | A | B |
|---|---|---|
| Truncated path in the clash dialog (hover the 2nd path; 960x600) (light) | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/clash-path-A-light.png" width="460"> | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/clash-path-B-light.png" width="460"> |
| (dark) | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/clash-path-A-dark.png" width="460"> | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/clash-path-B-dark.png" width="460"> |
| Long path (Making of/Interview…): wraps to 2 lines, covers the buttons (light) | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/clash-path-long-A-light.png" width="460"> | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/clash-path-long-B-light.png" width="460"> |
| (dark) | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/clash-path-long-A-dark.png" width="460"> | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/clash-path-long-B-dark.png" width="460"> |
| Clash dialog choice (Replace) (light) | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/clash-button-A-light.png" width="460"> | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/clash-button-B-light.png" width="460"> |
| (dark) | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/clash-button-A-dark.png" width="460"> | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/clash-button-B-dark.png" width="460"> |
| Disabled row button (Error), its reason (light) | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/disabled-toggle-A-light.png" width="460"> | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/disabled-toggle-B-light.png" width="460"> |
| (dark) | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/disabled-toggle-A-dark.png" width="460"> | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/disabled-toggle-B-dark.png" width="460"> |
| Status bar, "Service unavailable" (light) | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/status-bar-A-light.png" width="460"> | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/status-bar-B-light.png" width="460"> |
| (dark) | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/status-bar-A-dark.png" width="460"> | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/status-bar-B-dark.png" width="460"> |
| Details header, long name (one line: the whole name) (light) | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/details-name-A-light.png" width="460"> | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/details-name-B-light.png" width="460"> |
| (dark) | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/details-name-A-dark.png" width="460"> | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/details-name-B-dark.png" width="460"> |
| Details header, renamed torrent: two lines, name and original name (light) | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/details-renamed-A-light.png" width="460"> | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/details-renamed-B-light.png" width="460"> |
| (dark) | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/details-renamed-A-dark.png" width="460"> | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/details-renamed-B-dark.png" width="460"> |
| By the window's right edge (details "Drive … isn't connected") (light) | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/edge-reason-A-light.png" width="460"> | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/edge-reason-B-light.png" width="460"> |
| (dark) | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/edge-reason-A-dark.png" width="460"> | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/edge-reason-B-dark.png" width="460"> |
| Rename field note (open while the name is refused) (light) | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/rename-note-A-light.png" width="460"> | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/rename-note-B-light.png" width="460"> |
| (dark) | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/rename-note-A-dark.png" width="460"> | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/rename-note-B-dark.png" width="460"> |
| Settings control (App icon name, 640x500 window) (light) | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/settings-icon-A-light.png" width="460"> | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/settings-icon-B-light.png" width="460"> |
| (dark) | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/settings-icon-A-dark.png" width="460"> | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/settings-icon-B-dark.png" width="460"> |
| 640x320: clash dialog choice (light) | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/min-640x320-clash-A-light.png" width="460"> | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/min-640x320-clash-B-light.png" width="460"> |
| (dark) | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/min-640x320-clash-A-dark.png" width="460"> | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/min-640x320-clash-B-dark.png" width="460"> |
| 640x320: details header name (light) | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/min-640x320-name-A-light.png" width="460"> | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/min-640x320-name-B-light.png" width="460"> |
| (dark) | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/min-640x320-name-A-dark.png" width="460"> | <img src="https://raw.githubusercontent.com/HugeLetters/phantom/pr-artifacts/351-tooltips/min-640x320-name-B-dark.png" width="460"> |

What the stills show:
- Both wrap, neither cuts. A: max 300 pt wide, 12 pt text (small, muted-looking in light; a mid-grey card in dark). B: max 320 pt, 13 pt, 10x6 padding, shadow; in dark B's card is almost the page colour, separated by shadow alone (low contrast).
- The long path wraps to two lines in both and covers the dialog's own buttons while it shows.
- Edge: both stay inside the window (A 4 pt margin, B stays inside too); at 640x320 both fit.
- Position: A is centred under the element, 6 pt clear; B 4 pt clear. The two field notes in A are centred under the whole field/row (B: start-aligned under the field), and in A the rename note sits on top of the path line.

## Facts

| | A: `.help` | B: Phantom's (`explained`, `tooltip_*`) |
|---|---|---|
| Delay | Rune's `help_delay` 0.5 s when Rune draws; AppKit's own delay and look on a Mac unless `help_look` is set. Skip delay 0 (a second tooltip waits too) | 0.5 s (`EXPLAIN_AFTER`), none for the notes, which are shown by state |
| Disabled controls | Yes (the disabled row button's reason shows above) | Needs model state and a layer per site; whether a disabled control reports its rest to `on_hover_change` was not checked |
| Inside dialogs / popovers | Yes inside a modal (the clash paths show); one covered by a popup/modal over it is suppressed until the pointer leaves | Yes, it is a layer item over the dialog; takes no presses |
| Over text being selected / dragging | A press hides it; a press-and-drag across other elements starts none | Not checked; it follows hover state only |
| Keyboard focus | `HelpLook::on_focus(true)` shows it on Tab/arrows; off by default | None (hover only) |
| VoiceOver | The help text is the element's description (`a11y/node.rs`), skipped when it only repeats the name, or while the text isn't cut (`help_when_truncated`) | `.description(message)` on `explained` controls; notes (field) none |
| Truncated-only | `help_when_truncated` built in (paths, names, info values) | Not available; the site must measure |
| Programmatic / always open | `help_open(true)` (used for the notes in A) | Native: the note is just a layer item; start-aligned under the field |
| Themed | Not with AppKit's. With `Cx::help_look`: fill, ink, ring, radius, padding, font, max width, gap, delay, fade, shadows, arrow, keycaps, side; Rune then draws it on macOS too | Yes, it is `Kit` tokens (the same in both Appearances) |
| Rows / keys | `help_row`, `help_keys`, `help_above`, `help_side` | One text only |
| Code | ~20 `.help` sites stay as they are | See below |

## Code each choice removes

- **A** (all `.help`): removes `Ui::explained`, `tooltip_below`, `tooltip_under`, `EXPLAIN_AFTER` and the dialog's `Clash.explained` + `moving::explain` + the `help` list in `clash.rs` (13 lines of references in the sites, about 55 lines in all). `Ui::tooltip` stays for as long as `Ui::note` (the Add field's note, a press-opened popover with a scrim) does, so about 30 of those 55 lines can go unless the Add note moves too. The prototype A diff itself is +6 lines (the field notes), the removal not done.
- **B as `help_look`** (`.help` drawn in Phantom's look through `Cx::help_look`): removes nothing, adds 14 lines; both mechanisms stay but look the same (one look, two mechanisms). The tooltip surface is written twice (`tooltip()` and `help_look()`), so it should become one function; on macOS it replaces AppKit's tooltip everywhere.
- **B as replacing every `.help` by `explained`**: not prototyped: ~20 sites each need a per-site hover state in the model and a layer item (the clash dialog's 3 buttons take 13 lines, a `Clash.explained`, a handler). It can't replace `.help` on: truncated-only text (`help_when_truncated`: 7 sites), and on a disabled control (not checked whether it can report its rest).

## Where B can't replace `.help`
Truncated-only help (needs measuring: paths, names, the Info tab's values), possibly disabled controls (not checked), and the keys/rows/placement variants Rune has. Where A can't replace B: nothing was lost in these stills; the gap is the field notes' placement (centred under the whole field, not start-aligned, on top of the path line for Rename).
