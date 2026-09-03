# NotePad Pro

A line-oriented desktop notepad written entirely in **Rust** with a **Slint**
UI. Every pixel is owned by Rust + Slint — there is **no HTML, no CSS, no
JavaScript, no WebView/wry/PyWebView and no embedded browser engine**. The
release binary is a single self-contained file (~12 MB stripped).

* Version: `1.0.2-slint`
* Rust edition 2021, MSRV 1.78.0
* UI compiled at build time by `slint-build` (no runtime `.slint` parsing)

---

## Features

1. **Simple highlight** — toggle a highlight on the selected line
   (`Ctrl+Shift+H` or the toolbar).
2. **Multi-colour line highlighting** — six built-in colours
   (yellow, green, pink, blue, orange, purple) plus unlimited custom colours.
3. **Extract by colour** — pull highlighted lines out in *document order* or
   *group-by-colour order*, with a live preview, line/char counts, and
   copy / export / open-in-tab.
4. **List mode** — bullet `• ◦ ▪ ‣`, numbered, and checkbox lines with indent
   0–5, plus markdown shortcuts (`- `, `* `, `[] `, `1. `).
5. **Notes sidebar** — SQLite-backed (WAL, `STRICT` table) quick notes with
   pinning, search, sorting and highlight chips.
6. **Two themes** — light and dark, driven by a shared `AppTheme`
   token store.
7. **Liquid animations** — drifting background blobs, button ripple, toast
   bounce and theme cross-fade, all in pure Slint.
8. **Custom window controls** — Win11-metric (46×32) minimise/maximise/close,
   drag region, double-click to toggle maximise, dirty-tab close confirmation.

Plus: tabs with dirty indicators, 200-state undo/redo, find & replace
(case-sensitive, whole-word, next/prev, replace one/all), a full status bar,
encoding/BOM-aware file I/O, LF/CRLF/CR handling, atomic saves, native file
dialogs (`rfd`), system clipboard (`arboard`), and JSON settings/session.

---

## Building

```bash
cargo build --release        # production binary
cargo test                   # unit + integration + UI tests
```

The release profile uses `opt-level=3, lto=true, codegen-units=1, strip=true,
panic="abort"` for a small, fast binary.

## Running

```bash
notepadpro                        # restore previous session
notepadpro a.txt "my notes.txt"   # open files (paths with spaces work)
notepadpro --theme dark           # start in a specific theme
notepadpro --no-session           # ignore the saved session
notepadpro --reset-session        # clear the saved session and exit
```

Settings, the session and the notes database live in
`<data-dir>/NotePadPro/` (`settings.json`, `session.json`, `notes.db`).

## Layout

* `app/` — the binary crate (`main.rs`, `state.rs`, `callbacks/`, `dialogs/`).
* `core/` — a Slint-free library: editor, files, db, config, highlight.
* `ui/` — all `.slint` markup, components and themes.
* `tests/` — the 154-check integration suite (`slint-testing`).
* `wix/`, `debian/` — packaging. See `PACKAGING.md`.

See `DEVIATIONS.md` for deliberate changes from the original spec and
`CHANGELOG.md` for the per-version history.
