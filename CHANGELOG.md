# Changelog

All notable changes to NotePad Pro are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/) and the project adheres to
Semantic Versioning.

## [1.0.2-slint] — 2026-08-31

Full reimplementation of the UI and state layer on **Rust + Slint 1.6**,
replacing the legacy PyWebView (HTML/CSS/JS) front-end. All ten originally
reported bugs are fixed.

### Added
- Slint `.slint` UI compiled at build time via `slint-build`; zero web stack.
- 29 typed Rust↔Slint callbacks registered as closures on the window handle.
- Seven themes on a shared `AppTheme` token store with cross-fade.
- Notes sidebar backed by SQLite (WAL, `STRICT` table, FTS5 when available).
- Liquid animations: drifting blobs, ripple, toast bounce, theme fade.
- Custom window controls with Win11 metrics and dirty-close confirmation.
- Extract-by-colour with document order, group-by-colour order, counts,
  copy/export/open-in-tab.
- Multi-colour highlighting with six built-ins plus unlimited custom colours.
- List mode: bullet/number/check, indent 0–5, markdown shortcuts.
- 169-check integration suite (`e2e`, `theme`, `window-controls`,
  `packaging`, `api-safety`) using `slint-testing`.
- Packaging: `wix/main.wxs` (Windows MSI) and `debian/control`.

### Fixed (editor & shell pass, 2026-09-02)
- Editor is now a true textarea: rows are read-only `TextInput`s that own
  the mouse (caret placement, drag/double-click selection, native Copy)
  while *every* mutation routes through the Rust engine — Enter, Backspace,
  Delete, Home/End/PageUp/PageDown and typing repeat indefinitely with no
  re-click, and typed text is visible immediately.
- Wrap-off mode no longer collapses rows to a narrow column: each row is
  exactly as wide as its unwrapped text and the horizontal viewport follows
  the widest row.
- Save, Save As, Open, Export and "+ New note" no longer freeze the window:
  native pickers and the SQLite insert run on worker threads, results are
  delivered back via a 50 ms `slint::Timer` poll; the notes pool also fails
  fast (500 ms connection timeout) instead of hanging.
- Toolbar New/Open/Save/Save As are icon buttons (📝 📂 💾 📑).
- New brand mark: generated `ui/assets/logo.png`, multi-size
  `ui/assets/app.ico` (256/64/32/16), and the window/taskbar icon is now
  actually set (`Window { icon: ... }`).

### Fixed (the ten originally reported bugs)
1. Re-entrant serialisation recursion in the PyWebView bridge (bug #1) — the
   window handle is never stored in `AppState`; callbacks capture a weak
   handle, so state can never recursively serialise itself.
2. Invisible text on highlight bands — `editor-text-on-highlight` is dark in
   all seven themes because all six built-in bands are light.
3. Crash on paths containing spaces (bug #4) — clap `trailing_var_arg` keeps a
   quoted path as one argument; file I/O uses `PathBuf` end-to-end.
4. Clipboard failures silently ignored (bug #10) — errors now surface as
   toasts.
5. Extract ordering by frequency — extraction now defaults to first
   appearance / document order.
6. Undo history bypassed by direct edits — `Document::mutate`/`commit` is the
   single funnel, so no edit escapes the undo stack.
7. UTF-16 double-BOM on save — `encoding::encode` writes exactly one BOM.
8. Number lists misnumbered after edits — per-depth counters renumber on every
   commit and reset on non-number lines.
9. Byte-vs-char indexing in find/replace — offsets come from a `char_indices`
   walk re-checked with `is_char_boundary`.
10. Unbounded undo memory — the stack is capped at 200 states.

### Fixed (Windows field report — round 2)
1. Typing felt dead — keyboard input now always reaches the document: a
   `FocusScope` over the editor offers every key to Rust (`key-command`),
   which inserts characters, splits on Enter, and peels list markers on
   Backspace (outdent → unlist → join previous). Native `TextInput` still
   handles clicks on text directly.
2. Enter no longer swallows focus — the caret line is re-tracked after every
   structural edit (`focus-line`/`focus-token` are wired for Slint 1.7's
   `focus()`; today the fallback keeps typing alive).
3. Backspace at column 0 joins lines, outdents, or removes the bullet/number.
4. List toggle is a toggle — choosing the current line's list type clears it.
5. Word wrap — the editor `ScrollView` pins `viewport-width` to
   `visible-width`, so long imported paragraphs wrap instead of running off.
6. Theme dropdown shows all seven themes — the toolbar no longer clips its
   children and the menu renders above the body (z-order 20).
7. Find scrolls to the match and paints it: the matched line band plus a
   distinct current-match band, with `viewport-y` driven by `reveal-line`.
8. Extract panel docks beside Notes (right-hand `HorizontalLayout` pane with
   an animated width) instead of floating above it.
9. Visible borders/separators between toolbar, tab strip, editor, panels and
   status bar in every theme.
10. Tab close button pinned to the right edge of its tab (absolute position,
    never squeezed by long titles).
11. No console window — `windows_subsystem = "windows"` in release builds.
12. CI builds a real installer: WiX 3.11 candle/light produce
    `NotePad-Pro-1.0.2.msi`; exe + MSI ship as artifacts and on releases.

### Fixed (Windows field report — round 3)
1. WiX MSI build: the XML comment in `wix/main.wxs` contained a double
   dash (`cargo build --release`), which candle rejects (CNDL0104).
2. Window now opens at 1200x800 via `preferred-width/height` instead of a
   fixed `width/height` binding that froze the UI inside a maximised window
   (black stripe right of the content).
3. Empty lines are visible again: rows never collapse below one text line
   (an empty measuring Text reports zero height, which made Enter look
   stuck at one line).
4. Wrap off: the editor viewport takes the content's preferred width, so
   long pasted lines stay reachable via a horizontal scrollbar instead of
   being clipped with no way to scroll sideways.
5. Multi-line paste through the fallback path now creates real lines
   instead of stacking everything (with hidden newlines) on one line.
6. Arrow keys navigate the caret in fallback mode (Slint private-use
   chars), and the whole F7xx block can no longer be inserted as text.
7. A blinking fallback caret is drawn at the Rust-tracked caret position
   while the FocusScope holds focus, so there is always a visible caret
   where typing will land (Slint 1.6 strings have no substring API, so the
   x offset uses the monospace advance width).
8. New documents save as `.txt` by default (Text filter first in the Save
   dialog; `.npro` remains an opt-in filter).

### Changed
- Dropped `tokio` in favour of a synchronous model with a `std::thread`
  autosave loop (see `DEVIATIONS.md`).

## [1.0.1] — prior
- Legacy PyWebView front-end (superseded).

## [1.0.0] — prior
- Initial release.
