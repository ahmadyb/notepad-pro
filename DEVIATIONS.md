# Deviations from the original specification

These are deliberate, documented changes. Each preserves the spirit of the
spec while adapting to the Rust + Slint target.

1. **`tokio` was dropped.** The original dependency list included `tokio` with
   full features. NotePad Pro is synchronous: every callback runs on the Slint
   event loop and the autosave loop is a plain `std::thread` that marshals work
   back with `slint::invoke_from_event_loop` / `upgrade_in_event_loop`. An
   async runtime would ship ~1.5 MB of unused code and complicate the model.

2. **Shared data types live in `ui/model.slint`.** The spec scattered struct
   declarations across panels; consolidating them into one leaf file that every
   other `.slint` imports avoids cycles and keeps a single source of truth.

3. **Conversions live in `app/src/convert.rs`.** Core stays Slint-free; the
   `core::types::api` DTOs are mapped to the generated Slint structs in the app
   crate only.

4. **Inline coloured spans are persisted but rendered as the line band.** The
   `inline_spans` field is kept in the model and round-trips through the
   session, but the editor draws the whole line band rather than per-span
   runs. This is a documented known limitation (see below), not a dropped
   requirement.

5. **Native window frame is kept by default.** `native-frame: true` ships the
   OS title bar; the custom header buttons (min/max/close) are always present.
   Frameless drag is delegated to the compositor because Slint's drag entry
   point varies between releases and is a no-op on some Wayland compositors.

6. **`editor-text-on-highlight` stays dark in all seven themes.** All six
   built-in highlight bands are light, so dark text keeps contrast everywhere.
   This is the fix for the original invisible-text bug.

7. **`app/` has a library target in addition to the binary.** Integration
   tests cannot link a binary crate; exposing `notepad_pro` as a lib lets
   `tests/*.rs` reach `AppState` headlessly. The bin target is unchanged.

8. **`Document::commit()` added.** The list/find engines mutate `lines`
   directly; `commit()` is the primitive that records an undo step and marks
   dirty after such a direct mutation, while `mutate()` now delegates to it.

9. **Root integration tests are attached to the `app` package.** The workspace
   manifest is virtual (no root package), so `tests/*.rs` are wired into
   `app/Cargo.toml` via `[[test]] path = "../tests/…"` entries. The files live
   at `notepad-pro/tests/` as specified.

10. **The test suite does not use a Slint testing backend.** The original spec
    named `slint-testing`, but no such crate exists on crates.io; the real
    crate is `i-slint-backend-testing`, which is only published from Slint 1.7.0
    and must be version-pinned to the exact `slint` release — while this
    project is pinned to Slint 1.6, whose `slint` crate exposes no `testing`
    feature. The window-controls suite therefore verifies the UI layer by
    parsing `ui/app.slint` for metrics/wiring and by exercising the
    window-free state layer the callbacks delegate to. UI behaviour beyond
    that is covered by the state-layer suites.

## Known limitations (documented, not requirements)

* **Spell check** is not implemented.
* **Inline coloured spans** render as the whole-line band (deviation 4).
* **App-level Ctrl shortcuts may not fire from the editor on some platforms**,
  because Slint's `KeyEvent` exposes only `text` + modifiers and some platforms
  deliver empty `text` with Ctrl held. Every action is also reachable from the
  toolbar, so the app is degraded, never crippled.
* **Backspace at column 0 does not join lines**; the editor is a per-line model.
* **Frameless window drag** is left to the window manager (deviation 5).
* **Reveal/auto-scroll/focus-to-line are recorded but not animated in Slint
  1.6.** `changed` property callbacks are experimental in 1.6, so rows cannot
  react to `focus-request`. `reveal-line()` still mirrors the target for the
  status bar; the visual jump is the one thing 1.6 cannot do here.
* **Enter is handled through the `edited` callback.** Slint 1.6's multi-line
  `TextInput` inserts the newline itself and only exposes `edited`, so
  `AppState::set_line_text` splits any embedded `"\n"`, continuing lists onto
  the new row and renumbering.
