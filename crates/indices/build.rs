//! Optional build-time index snapshots; no runtime repository scanning.
use sha2::{Digest, Sha256};
use std::{env, fmt::Write as _, fs, path::PathBuf};
#[allow(dead_code)]
#[path = "src/download/cache_manifest.rs"]
mod cache_manifest;

fn main() {
    println!("cargo:rerun-if-env-changed=IONORAY_CACHE_ROOT");
    println!("cargo:rerun-if-changed=build.rs");
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let mut generated = String::new();
    for (name, constant, expected) in [
        (
            "Kp_ap_Ap_SN_F107_since_1932.txt",
            "GFZ_BYTES",
            cache_manifest::GFZ_SHA256,
        ),
        ("ig_rz.dat", "IG_RZ_BYTES", cache_manifest::IRI_IG_RZ_SHA256),
        (
            "apf107.dat",
            "APF107_BYTES",
            cache_manifest::IRI_APF107_SHA256,
        ),
    ] {
        let mut candidates = Vec::new();
        if let Some(root) = env::var_os("IONORAY_CACHE_ROOT") {
            candidates.push(PathBuf::from(root).join("crates/indices/cache").join(name));
        }
        candidates.push(manifest.join("cache").join(name));
        let mut selected = None;
        for path in candidates {
            println!("cargo:rerun-if-changed={}", path.display());
            println!(
                "cargo:rerun-if-changed={}",
                path.parent().expect("cache parent").display()
            );
            if path.try_exists().expect("read cache file metadata") {
                let bytes = fs::read(&path).expect("read index snapshot");
                let actual = hex_digest(&bytes);
                assert_eq!(
                    actual,
                    expected,
                    "index snapshot SHA-256 mismatch: {}",
                    path.display()
                );
                let target = out.join(name);
                fs::write(&target, bytes).expect("write validated index snapshot");
                selected = Some(target);
                break;
            }
        }
        let value = selected.map_or_else(
            || "None".to_owned(),
            |path| {
                format!(
                    "Some(include_bytes!({:?}))",
                    path.to_str().expect("UTF-8 build path")
                )
            },
        );
        writeln!(generated, "const {constant}: Option<&[u8]> = {value};")
            .expect("writing to a String cannot fail");
    }
    fs::write(out.join("optional_cache.rs"), generated)
        .expect("write optional snapshot definitions");
}

fn hex_digest(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut text = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(text, "{byte:02x}").expect("writing to a String cannot fail");
    }
    text
}
