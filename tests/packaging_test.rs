//! Packaging artifact verification (16 checks). Pure filesystem reads.

use std::path::Path;

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").leak()
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(root().join(rel)).unwrap_or_default()
}

#[test]
fn wix_manifest_exists() {
    assert!(root().join("wix/main.wxs").exists());
}

#[test]
fn wix_ships_the_single_exe() {
    let wxs = read("wix/main.wxs");
    assert!(wxs.contains("notepadpro.exe"));
    assert!(wxs.contains("SourceFile"));
}

#[test]
fn wix_registers_txt_association_and_shortcut() {
    let wxs = read("wix/main.wxs");
    assert!(wxs.contains("TxtAssociation"));
    assert!(wxs.contains("ApplicationStartMenuShortcut"));
}

#[test]
fn wix_uses_a_stable_upgrade_code() {
    assert!(read("wix/main.wxs").contains("UpgradeCode"));
}

#[test]
fn debian_control_exists() {
    assert!(root().join("debian/control").exists());
}

#[test]
fn debian_control_declares_binary_and_build_deps() {
    let c = read("debian/control");
    assert!(c.contains("Package: notepad-pro"));
    assert!(c.contains("cargo"));
}

#[test]
fn debian_control_lists_renderer_runtime_deps() {
    let c = read("debian/control");
    assert!(c.contains("libxkbcommon0"));
}

#[test]
fn readme_documents_all_eight_features() {
    let r = read("README.md");
    for f in [
        "Multi-colour",
        "Extract by colour",
        "List mode",
        "Notes sidebar",
        "Seven themes",
        "Liquid animations",
        "Custom window controls",
    ] {
        assert!(r.contains(f), "README missing {f}");
    }
}

#[test]
fn readme_asserts_no_web_stack() {
    let r = read("README.md");
    assert!(r.contains("no WebView"));
}

#[test]
fn packaging_doc_exists_and_mentions_release() {
    let p = read("PACKAGING.md");
    assert!(p.contains("cargo build --release"));
}

#[test]
fn changelog_records_the_slint_reimplementation() {
    let c = read("CHANGELOG.md");
    assert!(c.contains("1.0.2-slint"));
}

#[test]
fn changelog_lists_all_ten_bug_fixes() {
    let c = read("CHANGELOG.md");
    for n in 1..=10 {
        assert!(c.contains(&format!("{n}.")), "changelog missing bug #{n}");
    }
}

#[test]
fn deviations_doc_discloses_tokio_drop() {
    let d = read("DEVIATIONS.md");
    assert!(d.contains("tokio"));
}

#[test]
fn release_profile_is_size_optimised() {
    let c = read("Cargo.toml");
    assert!(c.contains("[profile.release]"));
    assert!(c.contains("lto = true"));
    assert!(c.contains("codegen-units = 1"));
    assert!(c.contains("strip = true"));
    assert!(c.contains("panic = \"abort\""));
}

#[test]
fn logo_and_icon_assets_exist() {
    assert!(root().join("ui/assets/logo.png").exists());
    assert!(root().join("ui/assets/app.ico").exists());
}

#[test]
fn icon_is_a_valid_ico() {
    let bytes = std::fs::read(root().join("ui/assets/app.ico")).unwrap();
    assert_eq!(&bytes[0..2], &[0, 0], "ICO reserved field");
    assert_eq!(&bytes[2..4], &[1, 0], "ICO type = icon");
}
