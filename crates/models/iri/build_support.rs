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

use sha2::{Digest, Sha256};

use crate::build_manifest::{
    ARCHIVE_ROOT, ASSETS, GLOBAL_OFFLINE_ENV, OFFLINE_ENV, RELEASE_SHA256, RELEASE_URL, SOURCE_ENV,
    SOURCES, required,
};

pub(crate) fn build_backend() -> Result<(), Box<dyn Error>> {
    let out = PathBuf::from(env::var_os("OUT_DIR").ok_or("Cargo did not set OUT_DIR")?);
    let root = out.join("iri2020");
    fs::create_dir_all(&root)?;
    let (source, source_mode) = match env::var_os(SOURCE_ENV) {
        Some(path) => (normalize_source(PathBuf::from(path)), "provided-directory"),
        None => acquire_release(&root)?,
    };
    validate_source(&source)?;
    for (file, _) in required() {
        println!("cargo:rerun-if-changed={}", source.join(file).display());
    }

    materialize_build_assets(&source, &root)?;
    generate_sources(&source, &root)?;
    crate::build_fortran::compile(&root)?;
    println!("cargo:rustc-env=IONORAY_IRI2020_SOURCE_MODE={source_mode}");
    Ok(())
}

fn normalize_source(path: PathBuf) -> PathBuf {
    let nested = path.join(ARCHIVE_ROOT);
    if nested.is_dir() { nested } else { path }
}

fn acquire_release(root: &Path) -> Result<(PathBuf, &'static str), Box<dyn Error>> {
    // Check declared snapshots even when OUT_DIR already has a valid release.
    // A corrupted selected snapshot must never be silently ignored.
    let bundled = optional_archive()?;
    let source = root.join("source");
    if validate_source(&source).is_ok() {
        return Ok((source, "cargo-out-dir-cache"));
    }
    let archive = root.join("iri2020.tar");
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
        let temporary = root.join("iri2020.tar.part");
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
        candidates.push(PathBuf::from(root).join("crates/models/iri/cache/iri2020.tar"));
    }
    candidates.push(manifest.join("cache/iri2020.tar"));
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
        return Err("official IRI-2020 archive SHA-256 mismatch".into());
    }
    Ok(())
}

fn download_archive(temporary: &Path) -> Result<(), Box<dyn Error>> {
    match ureq::get(RELEASE_URL)
        .header("User-Agent", "ionoray-iri build.rs")
        .call()
    {
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
                    "--user-agent",
                    "ionoray-iri build.rs",
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
    for entry in tar::Archive::new(File::open(archive)?).entries()? {
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path()?;
        let components: Vec<_> = path.components().collect();
        let [Component::Normal(root), Component::Normal(name)] = components.as_slice() else {
            continue;
        };
        if *root != OsStr::new(ARCHIVE_ROOT) {
            return Err("IRI-2020 archive contains an unexpected root directory".into());
        }
        let Some(name) = name.to_str() else {
            return Err("IRI-2020 archive contains a non-UTF-8 filename".into());
        };
        if required().any(|(required, _)| required == name) {
            entry.unpack(target.join(name))?;
        }
    }
    Ok(())
}

fn validate_source(source: &Path) -> Result<(), Box<dyn Error>> {
    for (file, expected) in required() {
        let path = source.join(file);
        if !path.is_file() {
            return Err(format!("missing required official file: {}", path.display()).into());
        }
        if digest_file(&path)? != expected {
            return Err(format!("official file SHA-256 mismatch: {}", path.display()).into());
        }
    }
    Ok(())
}

fn materialize_build_assets(source: &Path, root: &Path) -> Result<(), Box<dyn Error>> {
    let assets = root.join("assets");
    fs::create_dir_all(&assets)?;
    for (name, expected) in ASSETS {
        copy_verified(&source.join(name), &assets.join(name), expected)?;
    }

    let mut generated = String::from("const ASSETS: &[Asset] = &[\n");
    for (name, digest) in ASSETS {
        writeln!(
            generated,
            "    Asset {{ name: \"{name}\", bytes: include_bytes!(concat!(env!(\"OUT_DIR\"), \"/iri2020/assets/{name}\")), sha256: \"{digest}\" }},"
        )?;
    }
    generated.push_str("];\n");
    fs::write(root.join("embedded_assets.rs"), generated)?;
    Ok(())
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

fn generate_sources(source: &Path, root: &Path) -> Result<(), Box<dyn Error>> {
    fs::write(
        root.join("ionoray_iri2020_assets.f90"),
        crate::build_fortran::ASSET_MODULE,
    )?;
    fs::write(
        root.join("ionoray_iri2020.f90"),
        crate::build_fortran::C_SHIM,
    )?;
    for (name, _) in SOURCES {
        let upstream = String::from_utf8_lossy(&fs::read(source.join(name))?).into_owned();
        let derived = match *name {
            "irisub.for" => patch_irisub(upstream)?,
            "irifun.for" => patch_irifun(upstream)?,
            "igrf.for" => patch_igrf(upstream)?,
            _ => upstream,
        };
        fs::write(root.join(name), derived)?;
    }
    Ok(())
}

fn patch_irisub(mut source: String) -> Result<String, Box<dyn Error>> {
    replace_once(
        &mut source,
        "     &    HEIBEG,HEIEND,HEISTP,OUTF,OARR)\n",
        "     &    HEIBEG,HEIEND,HEISTP,OUTF,OARR)\n      use ionoray_iri2020_assets, only: iri_asset_path\n",
    )?;
    source = source.replace("FILE=FILNAM", "FILE=iri_asset_path(FILNAM)");
    replace_once(
        &mut source,
        "        call APF_ONLY(iyear,month,iday,F107_daily,F107PD,F107_81,\n     &      F107_365,IAP_daily,isdate)\n        if(.not.f107in.or..not.f107_81in) then\n",
        "        IAP_daily=-11\n        isdate=0\n        if(.not.f107in.or..not.f107_81in) then\n          call APF_ONLY(iyear,month,iday,F107_daily,F107PD,F107_81,\n     &      F107_365,IAP_daily,isdate)\n",
    )?;
    // This four-driver climatology has no Ap history. Follow the upstream
    // missing-Ap branch explicitly, without APFMSIS reading an unset ISDATE.
    // SWMI(9)=0 disables magnetic response; this is NOT an Ap=0 scenario.
    replace_once(
        &mut source,
        "      CALL APFMSIS(ISDATE,HOURUT,IAPO)\n      if(iapo(2).lt.0.0) then\n           SWMI(9)=0.\n           IAPO(1)=0.\n      else\n           SWMI(9)=-1.0\n      endif",
        "C ionoray four-driver climatology: no internal magnetic response.\n      IAPO=0.\n      SWMI(9)=0.",
    )?;
    Ok(source)
}

fn patch_irifun(mut source: String) -> Result<String, Box<dyn Error>> {
    replace_once(
        &mut source,
        "      subroutine read_data_SD(month,coeff_month)\n",
        "      subroutine read_data_SD(month,coeff_month)\n      use ionoray_iri2020_assets, only: iri_asset_path\n",
    )?;
    replace_once(
        &mut source,
        "File=filedata",
        "File=iri_asset_path(filedata)",
    )?;
    Ok(source)
}

fn patch_igrf(mut source: String) -> Result<String, Box<dyn Error>> {
    replace_once(
        &mut source,
        "        SUBROUTINE GETSHC (IU, FSPEC, NMAX, ERAD, GH, IER)",
        "        SUBROUTINE GETSHC (IU, FSPEC, NMAX, ERAD, GH, IER)\n      use ionoray_iri2020_assets, only: iri_asset_path",
    )?;
    replace_once(&mut source, "FILE=FOUT", "FILE=iri_asset_path(FOUT)")?;
    Ok(source)
}

fn replace_once(source: &mut String, from: &str, to: &str) -> Result<(), Box<dyn Error>> {
    let count = source.matches(from).count();
    if count != 1 {
        return Err(format!("expected one upstream patch marker, found {count}: {from:?}").into());
    }
    *source = source.replacen(from, to, 1);
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
