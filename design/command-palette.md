# Command palette: settings and every screen's controls (#434)

Design only, for review. Today ⌘P (#33) lists the commands for the selection: the selected
torrents', the selected files', then the app's. This adds Settings and its controls, so that
"Tone" or "Keep awake" is a few keys away from the main window. ⌘P stays the shortcut: it's
the palette's already, and ⌘, still opens Settings.

## An entry

Each row of the palette is an **entry**:

```rust
struct Entry {
    name: String,            // "Corners", "Pause", "Appearance"
    synonyms: &'static [&'static str], // "radius", "rounded": matched, never shown
    place: Option<String>,   // where it lives, muted after the name: "Appearance ▸ Edit… ▸ Look"
    kind: Kind,
}
enum Kind {
    Action(Command),         // today's commands: Pause, Move to…, Add file…, Undo …
    Setting(SettingRef),     // a service setting or a theme setting; its value shows on the right
    Place(Place),            // Settings, a tab (General, Network, Appearance), the editor sheet (Edit…)
}
```

## Finding

- Typing matches the name and the synonyms, fuzzily, as today (the letters in order; word starts
  and runs score most). A synonym match ranks a little under a name match.
- Today's groups stay (the torrent's name, "2 files", "Phantom"), plus **Settings**. While typing,
  the group with the best match comes first, so "tone" puts Settings on top; with nothing typed,
  the order is today's.
- With nothing typed, **Recent** comes first: the last 3 settings or places picked. Actions
  don't go into Recent: the selection already lists them.
- Settings match more strictly than commands: each letter starts a word or follows the one before
  it. Otherwise "tone" also finds "Stop seeding after" (see the still).

## What picking does

| Kind | Return | ⌘Return |
|---|---|---|
| Action | runs it (today) | the same |
| Place | opens Settings there; Edit… opens the editor sheet on the Active look | the same |
| Setting, on/off (Keep awake, UPnP, a theme toggle) | flips it now; the status bar says "Keep awake: On", with Undo | Show in Settings |
| Setting, Choice (Tone, Corners, Motion, Theme) | lists its choices in the palette, the current one ticked and highlighted; Return sets it | Show in Settings |
| Setting, typed value (port, limits, folder, a color) | Show in Settings | the same |

**Show in Settings** opens the Settings window at the setting's tab (and the editor sheet for a
theme setting), with its control focused, as an undo of that setting does today
(`show_setting`, `focus_setting`). A field takes the focus with its text selected.

A change from the palette is the same **Settings change** as the one in Settings: one undo step,
the same status bar wording, and Settings shows it live if it's open. If the editor sheet is open
on a Saved Look Preset, a theme setting from the palette still changes the Active look; the
sheet's preview is unchanged.

Show in Settings for a setting outside an open sheet closes that sheet the way an undo does today:
an Active look's sheet keeps its changes (Done), a saved preset's goes back (Cancel).

## Keyboard

- ↑ ↓ move (wrapping round), Return picks, ⌘Return shows the setting in Settings.
- Escape goes back one level: from a setting's choices to the list, with the typed text back;
  from the list, closes.
- ⇧Return runs an action, an on/off flip or a Choice's option like Return, but the palette stays
  open with its field empty and the list back at the top level (Recent first), so the next command
  can be typed at once. Its notice (with Undo) still shows, and ⌘Z takes each run back, newest
  first. A pick that opens something (a place, Rename…, a setting's choices, Show in Settings)
  leaves the palette as Return does.
- Tab does nothing new: the focus stays in the field, as today.
- In the choices, typing filters them ("ro" → Round).

## VoiceOver

- The field is labelled "Commands and settings". Each row reads its name, value and place:
  "Corners, Small, Appearance, Edit…, Look". A Choice setting adds "has choices", and a Show in
  Settings row its key.
- Entering a setting's choices says "Corners, 3 choices", and the current one is a selected
  option. Group headings stay headings, as today.
- The status bar's notice after a change is announced, as notices are today.

## Where entries come from

Nothing is a hand-kept list in the palette. Each source declares its entries once, and both the
palette and its screen read the same declaration:

```
gui/src/commands.rs        # Actions: today's command list; each declares where it applies
gui/src/theme/setting.rs   # Theme settings: Setting { key, label, kind, section } + synonyms
gui/src/model/settings.rs  # Service settings: one table { key, label, tab, control } replacing
                           #   the Switch / Row / tab_of matches; the Settings view draws from it
gui/src/model/palette.rs   # gathers them (the registry); Places from Tab::ALL and the sheets
```

A theme that declares a new setting shows up in the palette with no change elsewhere: its label is
the name, its section the place ("Appearance ▸ Edit… ▸ Every color"), and its kind decides what
picking does. The prototype reads the Phantom theme's declarations this way.

## In the Settings window (#434, question 4)

⌘P in Settings opens the same palette, from the same registry. A window gives the palette only a
`Host` (its context, its commands, how acting is done there); the entries, their filtering,
ranking, keys and picking are one body of code.

```rust
struct Context { window: Which /* Main | Settings */, tab: Tab, sheet: Sheet /* Closed | Look */ }
struct Applies { windows: Main | Settings | Both, when: Always | Closed | Look }   // on every entry
fn entries(host) -> Vec<Entry>   // registry(host) filtered by entry.applies.holds(host.context())
```

| Entry | Main window | Settings, no sheet | Settings, Look sheet open |
|---|---|---|---|
| The selection's commands, Add file…, Undo | ✓ | | |
| Settings… | ✓ | | |
| The tabs, App icon…, Look Presets, Edit… | ✓ | ✓ | |
| Service settings | ✓ | ✓ | |
| The Look's settings | ✓ | ✓ | ✓ (the sheet's own rows) |
| Done, Cancel, Restore Defaults, Split, Join (`OnSheet`) | | | ✓ |

Return acts in every window. ⌘Return is Show in Settings: from the main window it opens Settings at
the control; in Settings it focuses the control where it is (opening the Look sheet for a Look
setting), since Return, acting, is all else it could mean. With nothing typed: Recent, then this
window's context (the sheet's commands, or the tab shown, under its name), then everything else.
While the Look sheet is open its edits are the sheet's: previewed, kept by Done, put back by
Cancel. A flip in Settings has no notice (there's no status bar there): the control shows it, and
⌘Z takes it back.

## Out of scope

- Typing a value in the palette (port, speed limits, a color's hex): Show in Settings instead.
- A Saved Look Preset's Apply, Save as… and Delete, and the App icon's choices.
- Changing the theme's own declarations, other than adding synonyms.

## Open questions

1. **A theme setting whose halves differ (Tone is Light in ☀ and Dark in ☾ by default): what
   does the palette change?** *Recommendation:* one entry per half while the row is split
   ("Tone ☀ Light", "Tone ☾ Dark") and one entry for both while it's joined, as the editor sheet
   shows it. The prototype sets both halves, which turns a split Tone into one value.
2. **Should an on/off setting flip at once on Return, or list On and Off first?**
   *Recommendation:* flip at once, with a notice that has Undo. It takes one key, and ⌘Z takes it
   back.
3. **Should Recent survive quitting?** *Recommendation:* no, keep it in memory only. Keeping it
   would add a value to `gui.json` for little gain.
4. **Should ⌘P work in the Settings window too?** Yes, with the same list less the selection's
   commands: it helps reach a color deep in Every color. Built in a follow-up (see above).

## Prototype (branch `434-palette-design`, not for merge)

The stills and GIFs come from a throwaway change marked `PROTOTYPE (#434)` in
`gui/src/model/palette.rs`, `gui/src/views/palette.rs` and `gui/src/ui/palette.rs`. These parts
are faked:

- Recent is a fixed list (Corners, Keep awake, Tone).
- Keep awake never reaches the service: it's a flag in the palette, with a notice that has no
  Undo. UPnP always reads On.
- A theme change from the palette sets both halves and isn't an undo step.
- The service settings' names are typed in the palette, not read from a shared table.
- Show in Settings makes the control first in Tab order (the existing undo path), and the GIF
  presses Tab to show the focus ring. The real version would focus the control directly.
- The ranking change, the stricter settings matching and synonyms aren't built, and there are no
  tests.
