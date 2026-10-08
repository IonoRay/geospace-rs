//! Build-time acquisition and local compilation of the official NRLMSIS 2.1 release.

use std::{
    env,
    error::Error,
    ffi::OsStr,
    fmt::Write as _,
    fs::{self, File},
    io::{self, Read},
    path::{Component, Path, PathBuf},
    process::Command,
};

use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};

const RELEASE_URL: &str =
    "https://map.nrl.navy.mil/map/pub/nrl/NRLMSIS/NRLMSIS2.1/nrlmsis2.1.tar.gz";
const RELEASE_SHA256: &str = "41e47b29f795d36a5cc252b2858aa2a384c4a7323ace3d48d3ea2f2b37a1a6a8";
const PARAMETER_SHA256: &str = "a322a749f368e73117dd20f3fdcf7389dabc5509f4c27073cc5580999381b508";
const SOURCE_ENV: &str = "IONORAY_NRLMSIS21_SOURCE_DIR";
const OFFLINE_ENV: &str = "IONORAY_NRLMSIS21_OFFLINE";
const GLOBAL_OFFLINE_ENV: &str = "IONORAY_OFFLINE";
const SOURCES: &[&str] = &[
    "msis_constants.F90",
    "msis_utils.F90",
    "msis_init.F90",
    "msis_gfn.F90",
    "msis_tfn.F90",
    "msis_dfn.F90",
    "msis_calc.F90",
    "msis_gtd8d.F90",
];
const REQUIRED: &[(&str, &str)] = &[
    (
        "msis_constants.F90",
        "319037a1dd098a99d8b336aba87019a485d51926f577c686a3e60026d8a90a2b",
    ),
    (
        "msis_utils.F90",
        "38edc875f463214fe3f1249a42fae31eacf7225ebf66f814e638c7178686f8cf",
    ),
    (
        "msis_init.F90",
        "d79e98d26d38923819dbf0ecaf2d43c7b79ddd2f969d4cc9191fee6515f359b7",
    ),
    (
        "msis_gfn.F90",
        "a284f405bd606bb8b9085907d85ccbd4e7fa77d40fa1665e710f374f64861039",
    ),
    (
        "msis_tfn.F90",
        "583aa67d867da3076502381f0b537cf4d194bb8837a786a5446d4bc57bd4c82b",
    ),
    (
        "msis_dfn.F90",
        "a77a2e2bd186d985d0ee40efc2ad2565f60ec98e2f3254c1a9334eee751142b2",
    ),
    (
        "msis_calc.F90",
        "be5ee7e90e9610f72b5db96aefade89601007ccd4a2c86b545e12102cfcbee55",
    ),
    (
        "msis_gtd8d.F90",
        "5c54e3d2ca0d7a7b85a0b66608e2496f18ba47368ff0a966bfd8e97429dbb219",
    ),
    ("msis21.parm", PARAMETER_SHA256),
    (
        "nrlmsis2.1_license..txt",
        "46352d9b1303b64d2e73d60e24d2939eccd1496c911b4e39b71fc7b3d5b8dd92",
    ),
];

fn main() {
    println!("cargo:rerun-if-env-changed=IONORAY_CACHE_ROOT");
    println!("cargo:rerun-if-env-changed={SOURCE_ENV}");
    println!("cargo:rerun-if-env-changed={OFFLINE_ENV}");
    println!("cargo:rerun-if-env-changed={GLOBAL_OFFLINE_ENV}");
    println!("cargo:rerun-if-env-changed=FC");
    println!("cargo:rerun-if-env-changed=AR");
    println!("cargo:rustc-env=IONORAY_NRLMSIS21_RELEASE_SHA256={RELEASE_SHA256}");
    println!("cargo:rustc-env=IONORAY_NRLMSIS21_PARAMETER_SHA256={PARAMETER_SHA256}");
    if env::var_os("CARGO_FEATURE_NRLMSIS21").is_none() {
        return;
    }
    if let Err(error) = build_backend() {
        panic!("cannot prepare the NRLMSIS 2.1 backend: {error}");
    }
}

fn build_backend() -> Result<(), Box<dyn Error>> {
    println!(
        "cargo:warning=NRLMSIS 2.1 is restricted to research, academic, and non-profit use; building this feature accepts the official NRL license"
    );
    let out = PathBuf::from(env::var_os("OUT_DIR").ok_or("Cargo did not set OUT_DIR")?);
    let root = out.join("nrlmsis21");
    fs::create_dir_all(&root)?;
    let (source, source_mode) = match env::var_os(SOURCE_ENV) {
        Some(path) => (PathBuf::from(path), "provided-directory"),
        None => acquire_release(&root)?,
    };
    validate_source(&source)?;
    for (file, _) in REQUIRED {
        println!("cargo:rerun-if-changed={}", source.join(file).display());
    }

    let assets = root.join("assets");
    fs::create_dir_all(&assets)?;
    copy_verified(
        &source.join("msis21.parm"),
        &assets.join("msis21.parm"),
        PARAMETER_SHA256,
    )?;
    fs::copy(
        source.join("nrlmsis2.1_license..txt"),
        assets.join("nrlmsis2.1_license.txt"),
    )?;
    fs::write(root.join("ionoray_msis21.f90"), SHIM)?;
    compile_fortran(&source, &root)?;
    println!("cargo:rustc-env=IONORAY_NRLMSIS21_SOURCE_MODE={source_mode}");
    Ok(())
}

fn acquire_release(root: &Path) -> Result<(PathBuf, &'static str), Box<dyn Error>> {
    // Check declared snapshots even when OUT_DIR already has a valid release.
    // A corrupted selected snapshot must never be silently ignored.
    let bundled = optional_archive()?;
    let source = root.join("source");
    if validate_source(&source).is_ok() {
        return Ok((source, "cargo-out-dir-cache"));
    }
    let archive = root.join("nrlmsis2.1.tar.gz");
    let source_mode = if archive.is_file() {
        verify_archive(&archive)?;
        "cargo-out-dir-cache"
    } else if let Some(path) = bundled {
        fs::copy(path, &archive)?;
        verify_archive(&archive)?;
        "bundled-cache"
    } else if offline() {
        return Err("offline build has no verified model source; provide SOURCE_DIR or IONORAY_CACHE_ROOT (see docs/releases.md)".into());
    } else {
        let temporary = root.join("nrlmsis2.1.tar.gz.part");
        download_archive(&temporary)?;
        verify_archive(&temporary)?;
        fs::rename(temporary, &archive)?;
        "automatic-download"
    };

    if source.exists() {
        fs::remove_dir_all(&source)?;
    }
    fs::create_dir_all(&source)?;
    unpack_flat_archive(&archive, &source)?;
    validate_source(&source)?;
    Ok((source, source_mode))
}

fn offline() -> bool {
    env::var_os(OFFLINE_ENV).is_some() || env::var_os(GLOBAL_OFFLINE_ENV).is_some()
}

fn optional_archive() -> Result<Option<PathBuf>, Box<dyn Error>> {
    let manifest =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").ok_or("missing manifest directory")?);
    let mut candidates = Vec::new();
    if let Some(root) = env::var_os("IONORAY_CACHE_ROOT") {
        candidates.push(PathBuf::from(root).join("crates/models/msis/cache/nrlmsis2.1.tar.gz"));
    }
    candidates.push(manifest.join("cache/nrlmsis2.1.tar.gz"));
    for path in candidates {
        println!("cargo:rerun-if-changed={}", path.display());
        println!(
            "cargo:rerun-if-changed={}",
            path.parent().ok_or("missing cache parent")?.display()
        );
        if path.try_exists()? {
            verify_archive(&path)?;
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn verify_archive(archive: &Path) -> Result<(), Box<dyn Error>> {
    if digest_file(archive)? != RELEASE_SHA256 {
        return Err("official NRLMSIS 2.1 archive SHA-256 mismatch".into());
    }
    Ok(())
}

fn download_archive(temporary: &Path) -> Result<(), Box<dyn Error>> {
    match ureq::get(RELEASE_URL).call() {
        Ok(response) => {
            let mut reader = response.into_body().into_reader();
            let mut output = File::create(temporary)?;
            io::copy(&mut reader, &mut output)?;
            output.sync_all()?;
        }
        Err(error) => {
            println!(
                "cargo:warning=Rust HTTP download failed ({error}); trying the flake-provided curl fallback"
            );
            let status = Command::new("curl")
                .args([
                    "--fail",
                    "--location",
                    "--silent",
                    "--show-error",
                    "--output",
                ])
                .arg(temporary)
                .arg(RELEASE_URL)
                .status()?;
            if !status.success() {
                return Err(format!("curl fallback failed with {status}").into());
            }
            File::open(temporary)?.sync_all()?;
        }
    }
    Ok(())
}

fn unpack_flat_archive(archive: &Path, target: &Path) -> Result<(), Box<dyn Error>> {
    let decoder = GzDecoder::new(File::open(archive)?);
    for entry in tar::Archive::new(decoder).entries()? {
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
            return Err("NRLMSIS archive contains a non-regular entry".into());
        }
        let path = entry.path()?;
        let mut components = path.components();
        let Some(Component::Normal(name)) = components.next() else {
            return Err("NRLMSIS archive contains an unsafe path".into());
        };
        if components.next().is_some() {
            return Err("NRLMSIS archive structure is not flat".into());
        }
        entry.unpack(target.join(name))?;
    }
    Ok(())
}

fn validate_source(source: &Path) -> Result<(), Box<dyn Error>> {
    for (file, expected) in REQUIRED {
        let path = source.join(file);
        if !path.is_file() {
            return Err(
                format!("missing required official source file: {}", path.display()).into(),
            );
        }
        if digest_file(&path)? != *expected {
            return Err(format!("official source SHA-256 mismatch: {}", path.display()).into());
        }
    }
    Ok(())
}

fn copy_verified(source: &Path, target: &Path, expected: &str) -> Result<(), Box<dyn Error>> {
    if target.is_file() && digest_file(target)? == expected {
        return Ok(());
    }
    fs::copy(source, target)?;
    if digest_file(target)? != expected {
        return Err(format!(
            "copied asset failed SHA-256 validation: {}",
            target.display()
        )
        .into());
    }
    Ok(())
}

fn digest_file(path: &Path) -> Result<String, Box<dyn Error>> {
    let mut input = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(hasher.finalize()))
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    let mut output = String::with_capacity(bytes.as_ref().len() * 2);
    for byte in bytes.as_ref() {
        write!(output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

fn compile_fortran(source: &Path, root: &Path) -> Result<(), Box<dyn Error>> {
    let build = root.join("build");
    fs::create_dir_all(&build)?;
    let compiler = env::var_os("FC").unwrap_or_else(|| OsStr::new("gfortran").to_owned());
    let mut command = Command::new(&compiler);
    command
        .current_dir(&build)
        .args(["-c", "-O2", "-cpp", "-DDBLE", "-fPIC", "-J"])
        .arg(&build)
        .arg("-I")
        .arg(&build);
    if env::var("CARGO_CFG_TARGET_VENDOR").as_deref() == Ok("apple") {
        command.arg("-fno-omit-frame-pointer");
    }
    for file in SOURCES {
        command.arg(source.join(file));
    }
    command.arg(root.join("ionoray_msis21.f90"));
    run(&mut command, "Fortran compilation")?;

    let archiver = env::var_os("AR").unwrap_or_else(|| OsStr::new("ar").to_owned());
    let mut archive = Command::new(archiver);
    archive
        .current_dir(&build)
        .args(["crus", "libionoray_nrlmsis21.a"]);
    for file in SOURCES {
        let object = Path::new(file).with_extension("o");
        archive.arg(object);
    }
    archive.arg("ionoray_msis21.o");
    run(&mut archive, "Fortran archive creation")?;

    println!("cargo:rustc-link-search=native={}", build.display());
    println!("cargo:rustc-link-lib=static=ionoray_nrlmsis21");
    add_runtime_search(&compiler, "libgfortran")?;
    println!("cargo:rustc-link-lib=dylib=gfortran");
    Ok(())
}

fn add_runtime_search(compiler: &OsStr, library: &str) -> Result<(), Box<dyn Error>> {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let filename = match target_os.as_str() {
        "macos" => format!("{library}.dylib"),
        "windows" => format!("{library}.dll.a"),
        _ => format!("{library}.so"),
    };
    let output = Command::new(compiler)
        .arg(format!("-print-file-name={filename}"))
        .output()?;
    if !output.status.success() {
        return Err("gfortran could not locate its runtime library".into());
    }
    let path = PathBuf::from(String::from_utf8(output.stdout)?.trim());
    if let Some(parent) = path.parent().filter(|_| path.is_absolute()) {
        println!("cargo:rustc-link-search=native={}", parent.display());
    }
    Ok(())
}

fn run(command: &mut Command, operation: &str) -> Result<(), Box<dyn Error>> {
    let status = command.status()?;
    if !status.success() {
        return Err(format!("{operation} failed with {status}").into());
    }
    Ok(())
}

const SHIM: &str = r#"
! IonoRay-generated ISO_C_BINDING adapter, created 2026-07-12.
! This adapter does not modify the official model formulation or parameters.
! This software incorporates the MSIS empirical atmospheric model software
! designed and provided by NRL. Use is governed by the Open Source Academic
! Research License Agreement contained in nrlmsis2.1_license.txt.
module ionoray_msis21_c_api
  use iso_c_binding
  implicit none
contains
  subroutine ionoray_msis21_init(path, path_len, storm_mode, status) &
      bind(C, name="ionoray_msis21_init")
    use msis_init, only: msisinit
    character(kind=c_char), intent(in) :: path(*)
    integer(c_int), value :: path_len, storm_mode
    integer(c_int), intent(out) :: status
    character(len=:), allocatable :: filepath
    real(c_float) :: switches(25)
    integer :: i
    allocate(character(len=path_len) :: filepath)
    do i = 1, path_len
      filepath(i:i) = path(i)
    end do
    switches = 1.0_c_float
    if (storm_mode /= 0) switches(9) = -1.0_c_float
    call msisinit(parmpath='', parmfile=filepath, switch_legacy=switches)
    status = 0_c_int
  end subroutine

  subroutine ionoray_msis21_eval(day, utsec, altitude, latitude, longitude, &
      f107a, f107, ap, temperature, densities, exospheric_temperature) &
      bind(C, name="ionoray_msis21_eval")
    use msis_calc, only: msiscalc
    real(c_double), value :: day, utsec, altitude, latitude, longitude
    real(c_double), value :: f107a, f107
    real(c_double), intent(in) :: ap(7)
    real(c_double), intent(out) :: temperature, densities(10)
    real(c_double), intent(out) :: exospheric_temperature
    call msiscalc(day, utsec, altitude, latitude, longitude, f107a, f107, &
      ap, temperature, densities, exospheric_temperature)
  end subroutine
end module
"#;
