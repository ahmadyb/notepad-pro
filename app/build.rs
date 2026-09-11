//! Compile the Slint UI into Rust at build time.
//!
//! The `.slint` files do not exist at runtime — `slint::include_modules!()`
//! pulls in the generated module instead.

fn main() {
    // Re-run when any markup file changes, not just app.slint.
    println!("cargo:rerun-if-changed=../ui");
    println!("cargo:rerun-if-changed=build.rs");

    slint_build::compile("../ui/app.slint").expect("Slint compilation failed");
}
