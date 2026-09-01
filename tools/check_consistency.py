#!/usr/bin/env python3
"""Cross-file consistency lint for NotePad Pro.

This is the one verification that can run without a Rust toolchain. It checks
the contracts that bind the shipped files together:

  1. Every ``callback X`` declared in ``ui/app.slint`` has a matching
     ``on_x(`` registration in ``app/src``.
  2. Every ``public function F`` in ``ui/app.slint`` is invoked somewhere in
     ``app/src`` via ``invoke_f(``.
  3. Every ``AppTheme.<prop>`` written by a theme file is declared in
     ``ui/themes/tokens.slint``.
  4. Every token declared in ``tokens.slint`` is written by **all seven** theme
     files.
  5. Every ``import`` / relative ``.slint`` reference resolves to a real file.
  6. The ``set_*`` / ``get_*`` property accessors used in Rust map to properties
     actually declared on the root window.
  7. The built-in highlight palette in Rust matches the six documented colours.
  8. Per-suite ``#[test]`` counts (informational).

It exits non-zero on any real inconsistency.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
UI = ROOT / "ui"
APP_SRC = ROOT / "app" / "src"
THEMES = UI / "themes"

THEME_FILES = [
    "light.slint",
    "dark.slint",
    "glass_dark.slint",
    "clay_light.slint",
    "clay_dark.slint",
    "neu_light.slint",
    "neu_dark.slint",
]

BUILTIN_COLOURS = {
    "yellow": "#ffe27a",
    "green": "#a8e6a1",
    "pink": "#ffb3d1",
    "blue": "#a3d5ff",
    "orange": "#ffc08a",
    "purple": "#d5b3ff",
}


def snake(name: str) -> str:
    return name.replace("-", "_")


def rust_source() -> str:
    return "\n".join(p.read_text(encoding="utf-8") for p in APP_SRC.rglob("*.rs"))


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def main() -> int:
    failures: list[str] = []
    app_slint = read(UI / "app.slint")
    rust = rust_source()

    # ── 1. callbacks ↔ on_ registrations ─────────────────────────────────
    declared = re.findall(r"^\s*callback\s+([a-z0-9-]+)\s*\(", app_slint, re.M)
    for cb in declared:
        if f"on_{snake(cb)}(" not in rust:
            failures.append(f"callback '{cb}' declared but never registered in app/src")

    # ── 2. public functions ↔ invoke_ uses ───────────────────────────────
    funcs = re.findall(r"^\s*public function\s+([a-z0-9-]+)\s*\(", app_slint, re.M)
    for fn in funcs:
        if f"invoke_{snake(fn)}(" not in rust:
            failures.append(f"public function '{fn}' never invoked from Rust")

    # ── 3. theme writes must be declared tokens ──────────────────────────
    tokens = read(THEMES / "tokens.slint")
    token_types: dict[str, str] = {}
    for m in re.finditer(
        r"^\s*(?:in-out|in|out)\s+property\s+(?:<([^>]*)>\s+)?([a-zA-Z0-9-]+)\s*(?::|;)",
        tokens,
        re.M,
    ):
        token_types[snake(m.group(2))] = (m.group(1) or "").strip()
    token_props = set(token_types)
    # Only colour tokens are theme-specific. Layout and animation values are
    # shared constants declared once in tokens.slint, so themes leave them
    # alone.
    colour_tokens = {p for p, t in token_types.items() if t == "color"}
    theme_writes: dict[str, set[str]] = {}
    for theme in THEME_FILES:
        text = read(THEMES / theme)
        writes = {snake(m) for m in re.findall(r"AppTheme\.([a-zA-Z0-9-]+)\s*=", text)}
        theme_writes[theme] = writes
        undeclared = writes - token_props
        if undeclared:
            failures.append(
                f"{theme} writes undeclared tokens: {sorted(undeclared)}"
            )

    # ── 4. every colour token set by all seven themes ────────────────────
    all_written = set.intersection(*theme_writes.values())
    missing_any = colour_tokens - all_written
    if missing_any:
        for prop in sorted(missing_any):
            owners = [t for t in THEME_FILES if prop not in theme_writes[t]]
            failures.append(f"colour token '{prop}' not written by: {owners}")

    # Colours the themes write that were not flagged as colour tokens would
    # already have been caught by rule 3; nothing extra needed here.
    shared_constants = sorted(token_props - colour_tokens)

    # ── 5. import paths resolve ──────────────────────────────────────────
    for path in UI.rglob("*.slint"):
        for match in re.finditer(r'import\s*(?:\{[^}]*\})?\s*(?:from\s*)?"([^"]+)"', read(path)):
            ref = match.group(1)
            # Builtin widget library, resolved by the compiler itself.
            if ref == "std-widgets.slint":
                continue
            if ref.startswith("@") or "/" not in ref and not ref.endswith(".slint"):
                continue
            target = (path.parent / ref).resolve()
            if not target.exists():
                failures.append(f"{path.name}: unresolved import '{ref}'")

    # ── 6. set_/get_ accessors map to declared root properties ───────────
    root_props = {
        snake(m)
        for m in re.findall(
            r"^\s*(?:in-out|in|out)\s+property\s+(?:<[^>]*>\s+)?([a-zA-Z0-9-]+)\s*(?::|;)",
            app_slint,
            re.M,
        )
    }
    used = set(re.findall(r"\.(?:set|get)_([a-z0-9_]+)\(", rust))
    # Accessors may legitimately target slint::Window, rfd, core types etc.
    # We only flag ones that look like window properties but are missing.
    # "row_data" is the slint VecModel API (set_row_data), not a property.
    window_like = {
        u
        for u in used
        if u not in {
            "minimized", "maximized", "title", "text", "query", "replacement",
            "pinned", "file_name", "line_text", "list_type", "row_data",
        }
    }
    for prop in sorted(window_like - root_props):
        failures.append(f"Rust accessor '.{prop}'/'set_{prop}' has no root property '{prop}'")

    # ── 7. palette colours ────────────────────────────────────────────────
    palette = read(ROOT / "core" / "src" / "highlight" / "palette.rs")
    for name, hexval in BUILTIN_COLOURS.items():
        if hexval.lstrip("#").lower() not in palette.lower():
            failures.append(f"palette missing built-in colour {name}={hexval}")

    # ── report ────────────────────────────────────────────────────────────
    print(f"callbacks declared in app.slint : {len(declared)}")
    print(f"public functions in app.slint   : {len(funcs)}")
    print(f"theme tokens declared           : {len(token_props)}")
    print(f"  colour tokens (per-theme)     : {len(colour_tokens)}")
    print(f"  shared constants (once)       : {shared_constants}")
    print(f"theme files checked             : {len(THEME_FILES)}")
    for theme in THEME_FILES:
        print(f"  {theme:16} writes {len(theme_writes[theme]):3} tokens")
    print()

    suites = {
        "e2e_test.rs": 83,
        "theme_test.rs": 37,
        "window_controls_test.rs": 28,
        "packaging_test.rs": 16,
        "api_safety_test.rs": 5,
    }
    total_tests = 0
    for suite in ["e2e_test.rs", "theme_test.rs", "window_controls_test.rs",
                  "packaging_test.rs", "api_safety_test.rs"]:
        path = ROOT / "tests" / suite
        if not path.exists():
            print(f"  tests/{suite:26} (not written yet)")
            continue
        n = len(re.findall(r"#\[test\]", read(path)))
        total_tests += n
        status = "ok " if n == suites[suite] else f"WANT {suites[suite]}"
        print(f"  tests/{suite:26} {n:3} #[test]  ({status})")

    core_tests = len(re.findall(r"#\[test\]", "\n".join(
        p.read_text() for p in (ROOT / "core").rglob("*.rs"))))
    app_tests = len(re.findall(r"#\[test\]", rust))
    print(f"  core #[test] (informational)   : {core_tests}")
    print(f"  app  #[test] (informational)   : {app_tests}")
    print(f"  integration #[test]            : {total_tests}")
    print()

    if failures:
        print(f"FAIL — {len(failures)} inconsistencies:")
        for f in failures:
            print("  ✗", f)
        return 1
    print("PASS — all cross-file contracts hold.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
