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

### Changed
- Dropped `tokio` in favour of a synchronous model with a `std::thread`
  autosave loop (see `DEVIATIONS.md`).

## [1.0.1] — prior
- Legacy PyWebView front-end (superseded).

## [1.0.0] — prior
- Initial release.
