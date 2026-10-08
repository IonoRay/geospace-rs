//! Avoid retaining unused Nix-provided dylibs in macOS Python distributions.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-dead_strip_dylibs");
    }
}
