# Packaging NotePad Pro

The release build is a single self-contained binary. All packaging is
therefore a thin wrapper around `cargo build --release`.

## Produce the binary

```bash
cargo build --release
# → target/release/notepadpro   (Linux)
# → target/release/notepadpro.exe (Windows)
```

Strip the binary (the release profile already does this with `strip=true`).
Expected size is roughly 12 MB.

## Windows (WiX)

The installer definition is `wix/main.wxs`. It ships the one `.exe`, registers
a Start-menu shortcut and a `.txt` association.

```powershell
candle.exe wix\main.wxs
light.exe -ext WixUIExtension wix\main.wixobj -o notepadpro-1.0.2.msi
```

No DLLs, runtimes or redistributables are required — the Slint software
renderer and SQLite are statically linked.

## Debian / Ubuntu

`debian/control` is provided. A minimal `debian/rules` invokes Cargo:

```make
#!/usr/bin/make -f
%:
	dh $@

override_dh_auto_build:
	cargo build --release --locked

override_dh_auto_install:
	install -Dm755 target/release/notepadpro \
	    debian/notepad-pro/usr/bin/notepadpro
```

Runtime deps are just `libxkbcommon0` and `libfontconfig1` for the Slint
software renderer; nothing else is needed.

## macOS (reference only)

Use `app/Cargo.toml`'s `[package.metadata.bundle]` with
`cargo bundle --release`. The bundle reuses `ui/assets/app.ico` converted to
`.icns` and `logo.png`.

## Verifying a build

* The binary has no runtime dependency on a WebView or browser engine.
* `ldd` (Linux) shows only the standard system libraries, no Qt/Electron.
* Deleting the install directory removes everything except the user data in
  `<data-dir>/NotePadPro/`.
