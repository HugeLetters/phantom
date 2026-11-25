# Rune ddf1414 → 5986ab7: every scene, before | after

392 stills: 98 scenes (76 main, 22 Settings), light and dark, at 960x540 and 640x320 (Settings 600x460 and 480x320), 2×.
All differ in some pixels (two runs of the same build: none). *Left over* counts the device pixels that no shift of up to 1.5 pt explains; sorted by it.

What differs everywhere: Rune no longer rounds text widths up, so text, buttons and the field beside them move by up to 1.5 pt; icons and glyph edges are anti-aliased a little differently.

| Scene | Left over | Pixels off by > 24 | Judgement | Still |
|---|--:|--:|---|---|
| main · clash · dark · 640x320 | 2140 | 10944 | Harmless: buttons up to 1.5 pt narrower (text widths no longer rounded up, adds up over 4 buttons); the dialog's 1 pt edge is one colour where it crosses the selected row's outline | [before \| after](main__clash__dark__640x320.png) |
| main · clash · light · 640x320 | 2107 | 11875 | Harmless: buttons up to 1.5 pt narrower (text widths no longer rounded up, adds up over 4 buttons); the dialog's 1 pt edge is one colour where it crosses the selected row's outline | [before \| after](main__clash__light__640x320.png) |
| main · long-name · dark · 960x540 | 1242 | 14332 | User's eye: 1–2 more characters fit a shortened path, as text widths are no longer rounded up | [before \| after](main__long-name__dark__960x540.png) |
| main · long-name · light · 960x540 | 1227 | 14799 | User's eye: 1–2 more characters fit a shortened path, as text widths are no longer rounded up | [before \| after](main__long-name__light__960x540.png) |
| main · clashes · dark · 640x320 | 498 | 9295 | Harmless: buttons up to 1.5 pt narrower (text widths no longer rounded up, adds up over 4 buttons); the dialog's 1 pt edge is one colour where it crosses the selected row's outline | [before \| after](main__clashes__dark__640x320.png) |
| main · clashes · light · 640x320 | 483 | 10186 | Harmless: buttons up to 1.5 pt narrower (text widths no longer rounded up, adds up over 4 buttons); the dialog's 1 pt edge is one colour where it crosses the selected row's outline | [before \| after](main__clashes__light__640x320.png) |
| main · clash-another · light · 640x320 | 473 | 9613 | Harmless: buttons up to 1.5 pt narrower (text widths no longer rounded up, adds up over 4 buttons); the dialog's 1 pt edge is one colour where it crosses the selected row's outline | [before \| after](main__clash-another__light__640x320.png) |
| main · clash-another · dark · 640x320 | 433 | 8887 | Harmless: buttons up to 1.5 pt narrower (text widths no longer rounded up, adds up over 4 buttons); the dialog's 1 pt edge is one colour where it crosses the selected row's outline | [before \| after](main__clash-another__dark__640x320.png) |
| main · rename-refused · light · 640x320 | 378 | 8489 | Harmless: the note's 1 pt edge is one colour where it crosses the line under it | [before \| after](main__rename-refused__light__640x320.png) |
| main · clash-another · light · 960x540 | 334 | 10324 | Harmless: buttons up to 1.5 pt narrower (text widths no longer rounded up, adds up over 4 buttons); the dialog's 1 pt edge is one colour where it crosses the selected row's outline | [before \| after](main__clash-another__light__960x540.png) |
| main · clash-files · light · 640x320 | 324 | 10262 | Harmless: buttons up to 1.5 pt narrower (text widths no longer rounded up, adds up over 4 buttons); the dialog's 1 pt edge is one colour where it crosses the selected row's outline | [before \| after](main__clash-files__light__640x320.png) |
| main · clash-another · dark · 960x540 | 300 | 9624 | Harmless: buttons up to 1.5 pt narrower (text widths no longer rounded up, adds up over 4 buttons); the dialog's 1 pt edge is one colour where it crosses the selected row's outline | [before \| after](main__clash-another__dark__960x540.png) |
| main · clash-files · dark · 640x320 | 269 | 9353 | Harmless: buttons up to 1.5 pt narrower (text widths no longer rounded up, adds up over 4 buttons); the dialog's 1 pt edge is one colour where it crosses the selected row's outline | [before \| after](main__clash-files__dark__640x320.png) |
| main · clashes · dark · 960x540 | 262 | 9522 | Harmless: buttons up to 1.5 pt narrower (text widths no longer rounded up, adds up over 4 buttons); the dialog's 1 pt edge is one colour where it crosses the selected row's outline | [before \| after](main__clashes__dark__960x540.png) |
| main · clashes · light · 960x540 | 224 | 10392 | Harmless: buttons up to 1.5 pt narrower (text widths no longer rounded up, adds up over 4 buttons); the dialog's 1 pt edge is one colour where it crosses the selected row's outline | [before \| after](main__clashes__light__960x540.png) |
| main · clash · light · 960x540 | 212 | 11515 | Harmless: buttons up to 1.5 pt narrower (text widths no longer rounded up, adds up over 4 buttons); the dialog's 1 pt edge is one colour where it crosses the selected row's outline | [before \| after](main__clash__light__960x540.png) |
| main · clash-files · dark · 960x540 | 198 | 9189 | Harmless: buttons up to 1.5 pt narrower (text widths no longer rounded up, adds up over 4 buttons); the dialog's 1 pt edge is one colour where it crosses the selected row's outline | [before \| after](main__clash-files__dark__960x540.png) |
| main · clash-files · light · 960x540 | 174 | 10095 | Harmless: buttons up to 1.5 pt narrower (text widths no longer rounded up, adds up over 4 buttons); the dialog's 1 pt edge is one colour where it crosses the selected row's outline | [before \| after](main__clash-files__light__960x540.png) |
| main · clash · dark · 960x540 | 173 | 10617 | Harmless: buttons up to 1.5 pt narrower (text widths no longer rounded up, adds up over 4 buttons); the dialog's 1 pt edge is one colour where it crosses the selected row's outline | [before \| after](main__clash__dark__960x540.png) |
| main · renamed · dark · 640x320 | 143 | 9639 | User's eye: 1–2 more characters fit a shortened path, as text widths are no longer rounded up | [before \| after](main__renamed__dark__640x320.png) |
| main · renamed · light · 640x320 | 139 | 10374 | User's eye: 1–2 more characters fit a shortened path, as text widths are no longer rounded up | [before \| after](main__renamed__light__640x320.png) |
| main · rename-refused · dark · 960x540 | 88 | 7603 | Harmless: the note's 1 pt edge is one colour where it crosses the line under it | [before \| after](main__rename-refused__dark__960x540.png) |
| main · rename-short · light · 960x540 | 87 | 9244 | User's eye: Rune draws no caret while text is selected, as AppKit does | [before \| after](main__rename-short__light__960x540.png) |
| main · rename-short · light · 640x320 | 87 | 7750 | User's eye: Rune draws no caret while text is selected, as AppKit does | [before \| after](main__rename-short__light__640x320.png) |
| main · rename · light · 960x540 | 87 | 7281 | User's eye: Rune draws no caret while text is selected, as AppKit does | [before \| after](main__rename__light__960x540.png) |
| main · rename · light · 640x320 | 87 | 6983 | User's eye: Rune draws no caret while text is selected, as AppKit does | [before \| after](main__rename__light__640x320.png) |
| main · rename-short · dark · 960x540 | 87 | 8780 | User's eye: Rune draws no caret while text is selected, as AppKit does | [before \| after](main__rename-short__dark__960x540.png) |
| main · rename-short · dark · 640x320 | 87 | 7165 | User's eye: Rune draws no caret while text is selected, as AppKit does | [before \| after](main__rename-short__dark__640x320.png) |
| main · rename · dark · 960x540 | 87 | 6821 | User's eye: Rune draws no caret while text is selected, as AppKit does | [before \| after](main__rename__dark__960x540.png) |
| main · rename · dark · 640x320 | 87 | 6419 | User's eye: Rune draws no caret while text is selected, as AppKit does | [before \| after](main__rename__dark__640x320.png) |
| main · rename-refused · light · 960x540 | 72 | 8048 | Harmless: the note's 1 pt edge is one colour where it crosses the line under it | [before \| after](main__rename-refused__light__960x540.png) |
| main · rename-long · light · 960x540 | 66 | 8669 | User's eye: Rune draws no caret while text is selected, as AppKit does | [before \| after](main__rename-long__light__960x540.png) |
| main · rename-long · light · 640x320 | 66 | 8649 | User's eye: Rune draws no caret while text is selected, as AppKit does | [before \| after](main__rename-long__light__640x320.png) |
| main · rename-long · dark · 960x540 | 63 | 8172 | User's eye: Rune draws no caret while text is selected, as AppKit does | [before \| after](main__rename-long__dark__960x540.png) |
| main · rename-long · dark · 640x320 | 63 | 8067 | User's eye: Rune draws no caret while text is selected, as AppKit does | [before \| after](main__rename-long__dark__640x320.png) |
| main · rename-file · light · 640x320 | 44 | 8970 | User's eye: Rune draws no caret while text is selected, as AppKit does | [before \| after](main__rename-file__light__640x320.png) |
| main · rename-file · light · 960x540 | 44 | 8681 | User's eye: Rune draws no caret while text is selected, as AppKit does | [before \| after](main__rename-file__light__960x540.png) |
| main · move-palette · light · 640x320 | 43 | 8070 | Harmless: the panel's 1 pt edge is one colour where it crosses the rows behind | [before \| after](main__move-palette__light__640x320.png) |
| main · rename-file · dark · 640x320 | 41 | 8327 | User's eye: Rune draws no caret while text is selected, as AppKit does | [before \| after](main__rename-file__dark__640x320.png) |
| main · rename-file · dark · 960x540 | 41 | 8178 | User's eye: Rune draws no caret while text is selected, as AppKit does | [before \| after](main__rename-file__dark__960x540.png) |
| main · move-palette · dark · 640x320 | 35 | 6529 | Harmless: the panel's 1 pt edge is one colour where it crosses the rows behind | [before \| after](main__move-palette__dark__640x320.png) |
| main · move-palette · light · 960x540 | 21 | 4335 | Harmless: the panel's 1 pt edge is one colour where it crosses the rows behind | [before \| after](main__move-palette__light__960x540.png) |
| settings · settings-icon-row-mixed · light · 600x460 | 7 | 4333 | Harmless: anti-aliasing | [before \| after](settings__settings-icon-row-mixed__light__600x460.png) |
| settings · settings-icon-row-mixed · light · 480x320 | 7 | 4333 | Harmless: anti-aliasing | [before \| after](settings__settings-icon-row-mixed__light__480x320.png) |
| main · move-palette · dark · 960x540 | 6 | 3874 | Harmless: the panel's 1 pt edge is one colour where it crosses the rows behind | [before \| after](main__move-palette__dark__960x540.png) |
| settings · settings-icon-row · light · 600x460 | 5 | 4019 | Harmless: anti-aliasing | [before \| after](settings__settings-icon-row__light__600x460.png) |
| settings · settings-icon-row · light · 480x320 | 5 | 4019 | Harmless: anti-aliasing | [before \| after](settings__settings-icon-row__light__480x320.png) |
| settings · settings-icon-row · dark · 600x460 | 2 | 3565 | Harmless: anti-aliasing | [before \| after](settings__settings-icon-row__dark__600x460.png) |
| settings · settings-icon-row · dark · 480x320 | 2 | 3565 | Harmless: anti-aliasing | [before \| after](settings__settings-icon-row__dark__480x320.png) |
| main · add-failed-and-add-note · light · 640x320 | 0 | 8796 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · add-note · light · 640x320 | 0 | 6919 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · rename-refused · dark · 640x320 | 0 | 7645 | Harmless: the note's 1 pt edge is one colour where it crosses the line under it |  |
| settings · settings-icon-row-mixed · dark · 600x460 | 0 | 3911 | Harmless: anti-aliasing |  |
| settings · settings-icon-row-mixed · dark · 480x320 | 0 | 3911 | Harmless: anti-aliasing |  |
| settings · settings-replace · light · 600x460 | 0 | 10881 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-replace · light · 480x320 | 0 | 10881 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-replace · dark · 600x460 | 0 | 9357 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-replace · dark · 480x320 | 0 | 9357 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-presets · light · 600x460 | 0 | 8035 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-presets · light · 480x320 | 0 | 8035 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-save-as · light · 600x460 | 0 | 7815 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-save-as · light · 480x320 | 0 | 7815 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-presets · dark · 600x460 | 0 | 6711 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-presets · dark · 480x320 | 0 | 6711 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-save-as · dark · 600x460 | 0 | 6569 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-save-as · dark · 480x320 | 0 | 6569 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · add-failed-and-add-note · dark · 640x320 | 0 | 8175 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · add-failed-and-add-note · dark · 960x540 | 0 | 7096 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · add-note · dark · 640x320 | 0 | 6445 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · add-note · dark · 960x540 | 0 | 6335 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · notices · light · 960x540 | 0 | 9405 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · free-space · light · 960x540 | 0 | 8731 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · notices · light · 640x320 | 0 | 7360 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · free-space · light · 640x320 | 0 | 6686 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-icons-added · light · 600x460 | 0 | 1070 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-icons-follow · light · 600x460 | 0 | 1070 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-icons · light · 600x460 | 0 | 1070 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing | [before \| after](settings__settings-icons__light__600x460.png) |
| settings · settings-icons · dark · 600x460 | 0 | 1042 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-icons-follow · dark · 600x460 | 0 | 1042 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-icons-added · dark · 600x460 | 0 | 1042 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · add-failed-and-add-note · light · 960x540 | 0 | 7538 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · add-note · light · 960x540 | 0 | 6698 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · notices · dark · 960x540 | 0 | 8886 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing | [before \| after](main__notices__dark__960x540.png) |
| main · free-space · dark · 960x540 | 0 | 8219 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-sheet-themes · light · 600x460 | 0 | 8190 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-theme-menu · light · 600x460 | 0 | 7872 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-sheet-themes · dark · 600x460 | 0 | 7206 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-theme-menu · dark · 600x460 | 0 | 6888 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · notices · dark · 640x320 | 0 | 6868 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · free-space · dark · 640x320 | 0 | 6201 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-sheet-themes · dark · 480x320 | 0 | 5800 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-sheet-themes · light · 480x320 | 0 | 5577 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-theme-menu · dark · 480x320 | 0 | 5482 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-theme-menu · light · 480x320 | 0 | 5259 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · columns-menu · dark · 960x540 | 0 | 4625 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · columns-menu · dark · 640x320 | 0 | 4023 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · info-limit · light · 640x320 | 0 | 12613 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · few-rows · light · 640x320 | 0 | 12257 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · long-name · light · 640x320 | 0 | 11620 | User's eye: 1–2 more characters fit a shortened path, as text widths are no longer rounded up |  |
| main · fetching · light · 640x320 | 0 | 11159 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · single-file · light · 640x320 | 0 | 10875 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · demo · light · 640x320 | 0 | 10875 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · moving · light · 640x320 | 0 | 10775 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · moves-several · light · 640x320 | 0 | 10775 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · moved-some · light · 640x320 | 0 | 10775 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · move-failed · light · 640x320 | 0 | 10775 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · info-tab · light · 640x320 | 0 | 10463 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · put-back · light · 640x320 | 0 | 10419 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · missing · light · 640x320 | 0 | 10419 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · trashed-off · light · 640x320 | 0 | 10419 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · renames · light · 640x320 | 0 | 10419 | User's eye: Rune draws no caret while text is selected, as AppKit does |  |
| main · pane-dragged · light · 640x320 | 0 | 10419 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · details · light · 640x320 | 0 | 10419 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · pane-limit · light · 640x320 | 0 | 10322 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · pane-error · light · 960x540 | 0 | 10223 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · toggles · light · 960x540 | 0 | 9531 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · fetching · light · 960x540 | 0 | 9475 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · unchecked · light · 640x320 | 0 | 9323 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · checking · light · 640x320 | 0 | 9323 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · pane-limit · light · 960x540 | 0 | 9282 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · states · light · 960x540 | 0 | 9223 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing | [before \| after](main__states__light__960x540.png) |
| main · error · light · 640x320 | 0 | 9158 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · unavailable · light · 640x320 | 0 | 9025 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · destination-back · light · 640x320 | 0 | 9025 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · error · light · 960x540 | 0 | 8681 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · info-limit · light · 960x540 | 0 | 8481 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · notice · light · 960x540 | 0 | 8301 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · few-rows · light · 960x540 | 0 | 8299 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · part-not-saved · light · 960x540 | 0 | 8254 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · pane-error · light · 640x320 | 0 | 8249 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · renamed · light · 960x540 | 0 | 8070 | User's eye: 1–2 more characters fit a shortened path, as text widths are no longer rounded up |  |
| main · records-set-aside · light · 960x540 | 0 | 8000 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · mixed · light · 960x540 | 0 | 7913 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · cant-undo · light · 960x540 | 0 | 7853 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · many-errors · light · 960x540 | 0 | 7664 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · info-tab · light · 960x540 | 0 | 7648 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · checking · light · 960x540 | 0 | 7644 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · unchecked · light · 960x540 | 0 | 7636 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · several-entries · light · 960x540 | 0 | 7486 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · seeding-limit · light · 960x540 | 0 | 7474 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · open-failed · light · 960x540 | 0 | 7454 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · single-file · light · 960x540 | 0 | 7452 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · demo · light · 960x540 | 0 | 7452 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing | [before \| after](main__demo__light__960x540.png) |
| main · receipt · light · 960x540 | 0 | 7427 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · settings-reset · light · 960x540 | 0 | 7349 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · moving · light · 960x540 | 0 | 7325 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · moves-several · light · 960x540 | 0 | 7325 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · moved-some · light · 960x540 | 0 | 7325 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · move-failed · light · 960x540 | 0 | 7325 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · notice · light · 640x320 | 0 | 7269 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · part-not-saved · light · 640x320 | 0 | 7222 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · trashed-off · light · 960x540 | 0 | 7192 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · renames · light · 960x540 | 0 | 7192 | User's eye: Rune draws no caret while text is selected, as AppKit does |  |
| main · pane-dragged · light · 960x540 | 0 | 7192 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · details · light · 960x540 | 0 | 7192 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · toggles · light · 640x320 | 0 | 7097 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · removed-outdated · light · 960x540 | 0 | 7029 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · records-set-aside · light · 640x320 | 0 | 6968 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · mixed · light · 640x320 | 0 | 6881 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · removed-outdated · light · 640x320 | 0 | 6829 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · cant-undo · light · 640x320 | 0 | 6821 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · slow-details · light · 960x540 | 0 | 6806 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · slow-deletes · light · 960x540 | 0 | 6806 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · settings-script · light · 960x540 | 0 | 6806 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · removes · light · 960x540 | 0 | 6806 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · outside-adds · light · 960x540 | 0 | 6806 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · moves · light · 960x540 | 0 | 6806 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · moves-recent · light · 960x540 | 0 | 6806 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · columns-reordered · light · 960x540 | 0 | 6806 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · add-failed · light · 960x540 | 0 | 6806 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · add-failed-two · light · 960x540 | 0 | 6806 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · add-failed-some · light · 960x540 | 0 | 6806 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · many-errors · light · 640x320 | 0 | 6632 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · outdated · light · 640x320 | 0 | 6614 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · lost · light · 640x320 | 0 | 6601 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · replacing · light · 640x320 | 0 | 6474 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · spawning · light · 640x320 | 0 | 6474 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · connecting · light · 640x320 | 0 | 6474 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · rejecting · light · 640x320 | 0 | 6474 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · put-back · light · 960x540 | 0 | 6473 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · missing · light · 960x540 | 0 | 6473 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · several-entries · light · 640x320 | 0 | 6454 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · removed-two · light · 960x540 | 0 | 6447 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · open-failed · light · 640x320 | 0 | 6422 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · receipt · light · 640x320 | 0 | 6395 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · unavailable · light · 960x540 | 0 | 6362 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · destination-back · light · 960x540 | 0 | 6362 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · removed · light · 960x540 | 0 | 6343 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · settings-reset · light · 640x320 | 0 | 6317 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · seeding-limit · light · 640x320 | 0 | 6286 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · removed-two · light · 640x320 | 0 | 6247 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · removed · light · 640x320 | 0 | 6143 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · no-default · light · 640x320 | 0 | 5906 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · adds · light · 640x320 | 0 | 5906 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · slow-details · light · 640x320 | 0 | 5774 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · slow-deletes · light · 640x320 | 0 | 5774 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · settings-script · light · 640x320 | 0 | 5774 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · removes · light · 640x320 | 0 | 5774 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · outside-adds · light · 640x320 | 0 | 5774 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · moves · light · 640x320 | 0 | 5774 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · moves-recent · light · 640x320 | 0 | 5774 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · columns-reordered · light · 640x320 | 0 | 5774 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · add-failed · light · 640x320 | 0 | 5774 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · add-failed-two · light · 640x320 | 0 | 5774 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · add-failed-some · light · 640x320 | 0 | 5774 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · no-default · light · 960x540 | 0 | 5134 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · adds · light · 960x540 | 0 | 5134 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · columns-menu · light · 960x540 | 0 | 4993 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing | [before \| after](main__columns-menu__light__960x540.png) |
| main · outdated · light · 960x540 | 0 | 4830 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · lost · light · 960x540 | 0 | 4817 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · replacing · light · 960x540 | 0 | 4690 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · spawning · light · 960x540 | 0 | 4690 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · connecting · light · 960x540 | 0 | 4690 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · rejecting · light · 960x540 | 0 | 4690 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · states · light · 640x320 | 0 | 4641 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · columns-menu · light · 640x320 | 0 | 4593 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · info-limit · dark · 640x320 | 0 | 12064 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · few-rows · dark · 640x320 | 0 | 11687 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · long-name · dark · 640x320 | 0 | 11144 | User's eye: 1–2 more characters fit a shortened path, as text widths are no longer rounded up |  |
| main · fetching · dark · 640x320 | 0 | 10668 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · single-file · dark · 640x320 | 0 | 10312 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · demo · dark · 640x320 | 0 | 10312 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing | [before \| after](main__demo__dark__640x320.png) |
| main · moving · dark · 640x320 | 0 | 10287 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · moves-several · dark · 640x320 | 0 | 10287 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · moved-some · dark · 640x320 | 0 | 10287 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · move-failed · dark · 640x320 | 0 | 10287 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · pane-error · dark · 960x540 | 0 | 9883 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · info-tab · dark · 640x320 | 0 | 9789 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · trashed-off · dark · 640x320 | 0 | 9772 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · renames · dark · 640x320 | 0 | 9772 | User's eye: Rune draws no caret while text is selected, as AppKit does |  |
| main · put-back · dark · 640x320 | 0 | 9772 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · pane-dragged · dark · 640x320 | 0 | 9772 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · missing · dark · 640x320 | 0 | 9772 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · details · dark · 640x320 | 0 | 9772 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · pane-limit · dark · 640x320 | 0 | 9751 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · add-refused · light · 640x320 | 0 | 9680 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · toggles · dark · 960x540 | 0 | 9178 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · fetching · dark · 960x540 | 0 | 8973 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · pane-limit · dark · 960x540 | 0 | 8872 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · states · dark · 960x540 | 0 | 8840 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · unchecked · dark · 640x320 | 0 | 8727 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · checking · dark · 640x320 | 0 | 8727 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · error · dark · 640x320 | 0 | 8581 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · unavailable · dark · 640x320 | 0 | 8341 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · destination-back · dark · 640x320 | 0 | 8341 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · error · dark · 960x540 | 0 | 8281 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · info-limit · dark · 960x540 | 0 | 7929 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · notice · dark · 960x540 | 0 | 7877 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · few-rows · dark · 960x540 | 0 | 7789 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · part-not-saved · dark · 960x540 | 0 | 7740 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · pane-error · dark · 640x320 | 0 | 7656 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · mixed · dark · 960x540 | 0 | 7496 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · records-set-aside · dark · 960x540 | 0 | 7494 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · cant-undo · dark · 960x540 | 0 | 7474 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · renamed · dark · 960x540 | 0 | 7449 | User's eye: 1–2 more characters fit a shortened path, as text widths are no longer rounded up |  |
| main · many-errors · dark · 960x540 | 0 | 7294 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · checking · dark · 960x540 | 0 | 7179 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · unchecked · dark · 960x540 | 0 | 7171 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · info-tab · dark · 960x540 | 0 | 7084 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · several-entries · dark · 960x540 | 0 | 7082 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · add-refused · light · 960x540 | 0 | 7072 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · open-failed · dark · 960x540 | 0 | 7034 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · seeding-limit · dark · 960x540 | 0 | 7031 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · single-file · dark · 960x540 | 0 | 6995 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · demo · dark · 960x540 | 0 | 6995 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · receipt · dark · 960x540 | 0 | 6950 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · moving · dark · 960x540 | 0 | 6862 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · moves-several · dark · 960x540 | 0 | 6862 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · moved-some · dark · 960x540 | 0 | 6862 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · move-failed · dark · 960x540 | 0 | 6862 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · settings-reset · dark · 960x540 | 0 | 6835 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · notice · dark · 640x320 | 0 | 6806 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · trashed-off · dark · 960x540 | 0 | 6732 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · renames · dark · 960x540 | 0 | 6732 | User's eye: Rune draws no caret while text is selected, as AppKit does |  |
| main · pane-dragged · dark · 960x540 | 0 | 6732 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · details · dark · 960x540 | 0 | 6732 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · part-not-saved · dark · 640x320 | 0 | 6669 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · toggles · dark · 640x320 | 0 | 6508 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · mixed · dark · 640x320 | 0 | 6425 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · records-set-aside · dark · 640x320 | 0 | 6423 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · cant-undo · dark · 640x320 | 0 | 6403 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · slow-details · dark · 960x540 | 0 | 6337 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · slow-deletes · dark · 960x540 | 0 | 6337 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · settings-script · dark · 960x540 | 0 | 6337 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · removes · dark · 960x540 | 0 | 6337 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · outside-adds · dark · 960x540 | 0 | 6337 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · moves · dark · 960x540 | 0 | 6337 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · moves-recent · dark · 960x540 | 0 | 6337 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · columns-reordered · dark · 960x540 | 0 | 6337 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · add-failed · dark · 960x540 | 0 | 6337 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · add-failed-two · dark · 960x540 | 0 | 6337 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · add-failed-some · dark · 960x540 | 0 | 6337 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · removed-outdated · dark · 960x540 | 0 | 6330 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · many-errors · dark · 640x320 | 0 | 6223 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · removed-outdated · dark · 640x320 | 0 | 6066 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · put-back · dark · 960x540 | 0 | 6061 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · missing · dark · 960x540 | 0 | 6061 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · lost · dark · 640x320 | 0 | 6054 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · several-entries · dark · 640x320 | 0 | 6011 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · outdated · dark · 640x320 | 0 | 5995 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · unavailable · dark · 960x540 | 0 | 5981 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · destination-back · dark · 960x540 | 0 | 5981 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · open-failed · dark · 640x320 | 0 | 5963 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · replacing · dark · 640x320 | 0 | 5926 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · spawning · dark · 640x320 | 0 | 5926 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · rejecting · dark · 640x320 | 0 | 5926 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · connecting · dark · 640x320 | 0 | 5926 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · removed-two · dark · 960x540 | 0 | 5881 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · receipt · dark · 640x320 | 0 | 5879 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · seeding-limit · dark · 640x320 | 0 | 5793 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · removed · dark · 960x540 | 0 | 5767 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · settings-reset · dark · 640x320 | 0 | 5764 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · removed-two · dark · 640x320 | 0 | 5617 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · removed · dark · 640x320 | 0 | 5503 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · no-default · dark · 640x320 | 0 | 5423 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · adds · dark · 640x320 | 0 | 5423 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · slow-details · dark · 640x320 | 0 | 5266 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · slow-deletes · dark · 640x320 | 0 | 5266 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · settings-script · dark · 640x320 | 0 | 5266 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · removes · dark · 640x320 | 0 | 5266 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · outside-adds · dark · 640x320 | 0 | 5266 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · moves · dark · 640x320 | 0 | 5266 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · moves-recent · dark · 640x320 | 0 | 5266 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · columns-reordered · dark · 640x320 | 0 | 5266 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · add-failed · dark · 640x320 | 0 | 5266 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · add-failed-two · dark · 640x320 | 0 | 5266 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · add-failed-some · dark · 640x320 | 0 | 5266 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · no-default · dark · 960x540 | 0 | 4767 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · adds · dark · 960x540 | 0 | 4767 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · lost · dark · 960x540 | 0 | 4449 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-sheet-split · light · 600x460 | 0 | 4431 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · outdated · dark · 960x540 | 0 | 4390 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · replacing · dark · 960x540 | 0 | 4321 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · spawning · dark · 960x540 | 0 | 4321 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · rejecting · dark · 960x540 | 0 | 4321 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · connecting · dark · 960x540 | 0 | 4321 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · states · dark · 640x320 | 0 | 4091 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-sheet-split · dark · 600x460 | 0 | 3993 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-preset-sheet · light · 600x460 | 0 | 3638 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-preset-sheet · dark · 600x460 | 0 | 3638 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-sheet-joined · light · 600x460 | 0 | 3525 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-sheet-joined · dark · 600x460 | 0 | 3525 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-no-presets · dark · 600x460 | 0 | 3379 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-no-presets · dark · 480x320 | 0 | 3379 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-appearance · dark · 600x460 | 0 | 3379 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing | [before \| after](settings__settings-appearance__dark__600x460.png) |
| settings · settings-appearance · dark · 480x320 | 0 | 3379 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-sheet-split · light · 480x320 | 0 | 3041 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-icon-row-one · dark · 600x460 | 0 | 2943 | Harmless: anti-aliasing |  |
| settings · settings-icon-row-one · dark · 480x320 | 0 | 2943 | Harmless: anti-aliasing |  |
| settings · settings-sheet-split · dark · 480x320 | 0 | 2793 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-sheet-joined · light · 480x320 | 0 | 2438 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-sheet-joined · dark · 480x320 | 0 | 2438 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-preset-sheet · light · 480x320 | 0 | 2438 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-preset-sheet · dark · 480x320 | 0 | 2438 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-folder-gone · dark · 600x460 | 0 | 1739 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-no-default · dark · 600x460 | 0 | 1739 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-no-default · dark · 480x320 | 0 | 1739 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings · dark · 600x460 | 0 | 1739 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings · dark · 480x320 | 0 | 1739 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-folder-gone · dark · 480x320 | 0 | 1739 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-taken · dark · 600x460 | 0 | 1277 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-taken · dark · 480x320 | 0 | 1277 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · add-refused · dark · 640x320 | 0 | 9411 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| main · add-refused · dark · 960x540 | 0 | 6830 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-no-presets · light · 600x460 | 0 | 3814 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-no-presets · light · 480x320 | 0 | 3814 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-appearance · light · 600x460 | 0 | 3814 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-appearance · light · 480x320 | 0 | 3814 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-icon-row-one · light · 600x460 | 0 | 3326 | Harmless: anti-aliasing |  |
| settings · settings-icon-row-one · light · 480x320 | 0 | 3326 | Harmless: anti-aliasing |  |
| settings · settings-no-default · light · 600x460 | 0 | 1954 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-no-default · light · 480x320 | 0 | 1954 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings · light · 600x460 | 0 | 1954 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-folder-gone · light · 600x460 | 0 | 1954 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings · light · 480x320 | 0 | 1954 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-folder-gone · light · 480x320 | 0 | 1954 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-taken · light · 600x460 | 0 | 1303 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-taken · light · 480x320 | 0 | 1303 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-refused · dark · 600x460 | 0 | 1131 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-refused · dark · 480x320 | 0 | 1131 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-network · dark · 600x460 | 0 | 1131 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-network · dark · 480x320 | 0 | 1131 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-refused · light · 600x460 | 0 | 1129 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-network · light · 600x460 | 0 | 1129 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing | [before \| after](settings__settings-network__light__600x460.png) |
| settings · settings-refused · light · 480x320 | 0 | 1129 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-network · light · 480x320 | 0 | 1129 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-icons-follow · light · 480x320 | 0 | 505 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-icons-added · light · 480x320 | 0 | 505 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-icons · light · 480x320 | 0 | 505 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-icons · dark · 480x320 | 0 | 477 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-icons-follow · dark · 480x320 | 0 | 477 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
| settings · settings-icons-added · dark · 480x320 | 0 | 477 | Harmless: text and controls shifted up to 1.5 pt, anti-aliasing |  |
