//! Acquire the fixed IGRF-14 coefficients at build time only.
use sha2::{Digest, Sha256};
use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
};
const HASH: &str = "8f8d88403028fc4ee92c4f38d97b46e0a87e2cfc496045b43c9e26c1d6b0903c";
const URL: &str = "https://www.ngdc.noaa.gov/IAGA/vmod/coeffs/igrf14coeffs.txt";
fn main() {
    for name in [
        "IONORAY_CACHE_ROOT",
        "IONORAY_IGRF14_COEFFICIENT_FILE",
        "IONORAY_IGRF14_OFFLINE",
        "IONORAY_OFFLINE",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    println!("cargo:rerun-if-changed=build.rs");
    if let Err(error) = acquire() {
        panic!("cannot prepare IGRF-14 coefficients: {error}");
    }
}
fn verify(path: &Path) -> Result<(), Box<dyn Error>> {
    let bytes = fs::read(path)?;
    if hex_digest(&bytes) != HASH {
        return Err(format!("IGRF-14 coefficient SHA-256 mismatch: {}", path.display()).into());
    }
    Ok(())
}
fn acquire() -> Result<(), Box<dyn Error>> {
    let out = PathBuf::from(env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?);
    let target = out.join("igrf14coeffs.txt");
    let manifest =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").ok_or("missing manifest directory")?);
    let mut candidates = Vec::new();
    if let Some(path) = env::var_os("IONORAY_IGRF14_COEFFICIENT_FILE") {
        let path = PathBuf::from(path);
        println!("cargo:rerun-if-changed={}", path.display());
        verify(&path)?;
        fs::copy(path, target)?;
        return Ok(());
    }
    if let Some(root) = env::var_os("IONORAY_CACHE_ROOT") {
        candidates.push(PathBuf::from(root).join("crates/models/igrf/data/igrf14coeffs.txt"));
    }
    candidates.push(manifest.join("data/igrf14coeffs.txt"));
    for path in candidates {
        println!("cargo:rerun-if-changed={}", path.display());
        println!(
            "cargo:rerun-if-changed={}",
            path.parent().ok_or("missing coefficient parent")?.display()
        );
        if path.try_exists()? {
            verify(&path)?;
            fs::copy(path, target)?;
            return Ok(());
        }
    }
    if target.is_file() {
        verify(&target)?;
        return Ok(());
    }
    if env::var_os("IONORAY_OFFLINE").is_some() || env::var_os("IONORAY_IGRF14_OFFLINE").is_some() {
        return Err("offline build has no IGRF-14 coefficients; provide IONORAY_IGRF14_COEFFICIENT_FILE or IONORAY_CACHE_ROOT".into());
    }
    let temporary = out.join("igrf14coeffs.txt.part");
    let status = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--connect-timeout",
            "15",
            "--max-time",
            "60",
            "--output",
        ])
        .arg(&temporary)
        .arg(URL)
        .status()?;
    if !status.success() {
        return Err(format!("coefficient download failed: {status}").into());
    }
    verify(&temporary)?;
    fs::rename(temporary, target)?;
    Ok(())
}

fn hex_digest(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut text = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(text, "{byte:02x}").expect("writing to a String cannot fail");
    }
    text
}
