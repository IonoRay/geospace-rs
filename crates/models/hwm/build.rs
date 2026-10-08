//! Build-time acquisition and local compilation of the official HWM14 release.

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
    "https://map.nrl.navy.mil/map/pub/nrl/HWM/HWM14/HWM14_ess224-sup-0002-supinfo.tgz";
const RELEASE_SHA256: &str = "4de451beeadef7b3ec3aa5b91129ea98866b9e7156cecf4be1343c33a6f57978";
const ASSET_SET_SHA256: &str = "8121fa349137301f99a869e9521d8e732e933893d599d1ab75f2ce198d3048bb";
const SOURCE_ENV: &str = "IONORAY_HWM14_SOURCE_DIR";
const OFFLINE_ENV: &str = "IONORAY_HWM14_OFFLINE";
const GLOBAL_OFFLINE_ENV: &str = "IONORAY_OFFLINE";
const SOURCE_FILE: &str = "hwm14.f90";
const ASSETS: &[&str] = &["hwm123114.bin", "dwm07b104i.dat", "gd2qd.dat"];
const REQUIRED: &[(&str, &str)] = &[
    (
        "Check/gfortran.txt",
        "2b1d4f4f103be3531393c48bf32d034548babfb6c080549884cad9fd3a2c8652",
    ),
    (
        "hwm14.f90",
        "6bb4e031917b44c93201f0289ada8bf6f6d81eb3f292c89151c6937e2e7bf76e",
    ),
    (
        "hwm123114.bin",
        "6e445f8337c7efc815b7ff7f9f967d8a9a16b469d93df9c46e54930553beb906",
    ),
    (
        "dwm07b104i.dat",
        "f2b8eff002d55b0f6d49d202c73b7a9cb6685f2c6bb57f1291a2adc7aa07cf4f",
    ),
    (
        "gd2qd.dat",
        "6bb1f2384e30b409240ee92c32726ed2a3d73eb550c05e0969d1178032319804",
    ),
    (
        "README.txt",
        "14b6e5e48d346ff034e2f6bfd7f26dc550945c9b90ad9661c7710e721ac89b15",
    ),
];

fn main() {
    println!("cargo:rerun-if-env-changed=IONORAY_CACHE_ROOT");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed={SOURCE_ENV}");
    println!("cargo:rerun-if-env-changed={OFFLINE_ENV}");
    println!("cargo:rerun-if-env-changed={GLOBAL_OFFLINE_ENV}");
    println!("cargo:rerun-if-env-changed=FC");
    println!("cargo:rerun-if-env-changed=AR");
    println!("cargo:rustc-env=IONORAY_HWM14_RELEASE_SHA256={RELEASE_SHA256}");
    println!("cargo:rustc-env=IONORAY_HWM14_ASSET_SET_SHA256={ASSET_SET_SHA256}");
    if env::var_os("CARGO_FEATURE_HWM14").is_none() {
        return;
    }
    if let Err(error) = build_backend() {
        panic!("cannot prepare the HWM14 backend: {error}");
    }
}

fn build_backend() -> Result<(), Box<dyn Error>> {
    let out = PathBuf::from(env::var_os("OUT_DIR").ok_or("Cargo did not set OUT_DIR")?);
    let root = out.join("hwm14");
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
    for name in ASSETS {
        let expected = expected_digest(name).ok_or("asset is missing from the manifest")?;
        copy_verified(&source.join(name), &assets.join(name), expected)?;
    }
    let reference = fs::read(source.join("Check/gfortran.txt"))?;
    let reference = reference
        .get(..3083)
        .ok_or("official HWM reference is too short")?;
    if hex(Sha256::digest(reference))
        != "06f393ce2bd5782e0d54409149eef048b0cb21b5f25b3cb3c17971fe816ddb99"
    {
        return Err("official HWM reference excerpt SHA-256 mismatch".into());
    }
    fs::write(root.join("reference-profiles.txt"), reference)?;
    generate_sources(&source, &root)?;
    compile_fortran(&root)?;
    println!("cargo:rustc-env=IONORAY_HWM14_SOURCE_MODE={source_mode}");
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
    let archive = root.join("hwm14.tgz");
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
        let temporary = root.join("hwm14.tgz.part");
        download_archive(&temporary)?;
        verify_archive(&temporary)?;
        fs::rename(temporary, &archive)?;
        "automatic-download"
    };

    if source.exists() {
        fs::remove_dir_all(&source)?;
    }
    fs::create_dir_all(&source)?;
    unpack_release(&archive, &source)?;
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
        candidates.push(PathBuf::from(root).join("crates/models/hwm/cache/hwm14.tgz"));
    }
    candidates.push(manifest.join("cache/hwm14.tgz"));
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
        return Err("official HWM14 archive SHA-256 mismatch".into());
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

fn unpack_release(archive: &Path, target: &Path) -> Result<(), Box<dyn Error>> {
    let decoder = GzDecoder::new(File::open(archive)?);
    for entry in tar::Archive::new(decoder).entries()? {
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path()?;
        let components: Vec<_> = path.components().collect();
        let (root, name) = match components.as_slice() {
            [Component::Normal(root), Component::Normal(name)] => (root, name),
            [
                Component::Normal(root),
                Component::Normal(check),
                Component::Normal(name),
            ] if *check == OsStr::new("Check") && *name == OsStr::new("gfortran.txt") => {
                (root, name)
            }
            _ => continue,
        };
        if *root != OsStr::new("HWM14") {
            return Err("HWM14 archive contains an unexpected root directory".into());
        }
        let Some(name) = name.to_str() else {
            return Err("HWM14 archive contains a non-UTF-8 filename".into());
        };
        if name == "gfortran.txt" {
            fs::create_dir_all(target.join("Check"))?;
            entry.unpack(target.join("Check/gfortran.txt"))?;
        } else if REQUIRED.iter().any(|(required, _)| *required == name) {
            entry.unpack(target.join(name))?;
        }
    }
    Ok(())
}

fn validate_source(source: &Path) -> Result<(), Box<dyn Error>> {
    for (file, expected) in REQUIRED {
        let path = source.join(file);
        if !path.is_file() {
            return Err(format!("missing required official file: {}", path.display()).into());
        }
        if digest_file(&path)? != *expected {
            return Err(format!("official file SHA-256 mismatch: {}", path.display()).into());
        }
    }
    Ok(())
}

fn expected_digest(name: &str) -> Option<&'static str> {
    REQUIRED
        .iter()
        .find_map(|(file, digest)| (*file == name).then_some(*digest))
}

fn copy_verified(source: &Path, target: &Path, expected: &str) -> Result<(), Box<dyn Error>> {
    if target.is_file() && digest_file(target)? == expected {
        return Ok(());
    }
    fs::copy(source, target)?;
    if digest_file(target)? != expected {
        return Err(format!("copied asset failed validation: {}", target.display()).into());
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

fn generate_sources(source: &Path, root: &Path) -> Result<(), Box<dyn Error>> {
    let upstream = fs::read_to_string(source.join(SOURCE_FILE))?;
    let start_marker = "subroutine findandopen(datafile,unitid)";
    let end_marker = "end subroutine findandopen";
    let start = upstream
        .find(start_marker)
        .ok_or("official HWM14 source is missing findandopen")?;
    let end = upstream[start..]
        .find(end_marker)
        .map(|offset| start + offset + end_marker.len())
        .ok_or("official HWM14 source has an incomplete findandopen")?;
    let derived = format!(
        "{}{}{}",
        &upstream[..start],
        FIND_AND_OPEN,
        &upstream[end..]
    );
    fs::write(root.join("hwm14.f90"), derived)?;
    fs::write(root.join("ionoray_hwm14_assets.f90"), ASSET_MODULE)?;
    fs::write(root.join("ionoray_hwm14.f90"), C_SHIM)?;
    Ok(())
}

fn compile_fortran(root: &Path) -> Result<(), Box<dyn Error>> {
    let build = root.join("build");
    fs::create_dir_all(&build)?;
    let compiler = env::var_os("FC").unwrap_or_else(|| OsStr::new("gfortran").to_owned());
    let mut command = Command::new(&compiler);
    command
        .current_dir(&build)
        .args(["-c", "-O2", "-fPIC", "-J"])
        .arg(&build)
        .arg("-I")
        .arg(&build)
        .arg(root.join("ionoray_hwm14_assets.f90"))
        .arg(root.join("hwm14.f90"))
        .arg(root.join("ionoray_hwm14.f90"));
    if env::var("CARGO_CFG_TARGET_VENDOR").as_deref() == Ok("apple") {
        command.arg("-fno-omit-frame-pointer");
    }
    run(&mut command, "Fortran compilation")?;

    let archiver = env::var_os("AR").unwrap_or_else(|| OsStr::new("ar").to_owned());
    let mut archive = Command::new(archiver);
    archive
        .current_dir(&build)
        .args(["crus", "libionoray_hwm14.a"])
        .args(["ionoray_hwm14_assets.o", "hwm14.o", "ionoray_hwm14.o"]);
    run(&mut archive, "Fortran archive creation")?;
    println!("cargo:rustc-link-search=native={}", build.display());
    println!("cargo:rustc-link-lib=static=ionoray_hwm14");
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

const ASSET_MODULE: &str = r#"
module ionoray_hwm14_assets
  use iso_c_binding
  implicit none
  character(len=:), allocatable :: asset_root
contains
  subroutine ionoray_hwm14_set_asset_root(path, path_len, status) &
      bind(C, name="ionoray_hwm14_set_asset_root")
    character(kind=c_char), intent(in) :: path(*)
    integer(c_int), value :: path_len
    integer(c_int), intent(out) :: status
    integer :: i
    if (path_len <= 0) then
      status = 1_c_int
      return
    end if
    if (allocated(asset_root)) deallocate(asset_root)
    allocate(character(len=path_len) :: asset_root)
    do i = 1, path_len
      asset_root(i:i) = path(i)
    end do
    status = 0_c_int
  end subroutine
end module
"#;

const FIND_AND_OPEN: &str = r#"subroutine findandopen(datafile,unitid)
    use ionoray_hwm14_assets, only: asset_root
    implicit none
    character(128), intent(in) :: datafile
    integer, intent(in) :: unitid
    character(len=:), allocatable :: filepath
    logical :: havefile
    integer :: io_status

    if (.not. allocated(asset_root)) error stop "HWM14 asset root is not initialized"
    filepath = asset_root // '/' // trim(datafile)
    inquire(file=filepath, exist=havefile)
    if (.not. havefile) error stop "HWM14 asset is missing"
    if (index(datafile, 'bin') == 0) then
      open(unit=unitid, file=filepath, status='old', form='unformatted', &
        iostat=io_status)
    else
      open(unit=unitid, file=filepath, status='old', access='stream', &
        iostat=io_status)
    end if
    if (io_status /= 0) error stop "HWM14 asset could not be opened"
end subroutine findandopen"#;

const C_SHIM: &str = r#"
module ionoray_hwm14_c_api
  use iso_c_binding
  implicit none
contains
  subroutine ionoray_hwm14_init(path, path_len, status) &
      bind(C, name="ionoray_hwm14_init")
    character(kind=c_char), intent(in) :: path(*)
    integer(c_int), value :: path_len
    integer(c_int), intent(out) :: status
    interface
      subroutine set_root(path, path_len, status) &
          bind(C, name="ionoray_hwm14_set_asset_root")
        use iso_c_binding
        character(kind=c_char), intent(in) :: path(*)
        integer(c_int), value :: path_len
        integer(c_int), intent(out) :: status
      end subroutine
    end interface
    call set_root(path, path_len, status)
    if (status /= 0_c_int) return
    call inithwm()
  end subroutine

  subroutine ionoray_hwm14_eval(iyd, sec, alt, glat, glon, ap_value, wind) &
      bind(C, name="ionoray_hwm14_eval")
    integer(c_int), value :: iyd
    real(c_float), value :: sec, alt, glat, glon, ap_value
    real(c_float), intent(out) :: wind(2)
    real(c_float) :: ap(2)
    ap = [0.0_c_float, ap_value]
    call hwm14(iyd, sec, alt, glat, glon, 0.0_c_float, 0.0_c_float, &
      0.0_c_float, ap, wind)
  end subroutine
end module
"#;
