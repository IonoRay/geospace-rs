#![allow(unsafe_code)]

use std::{
    ffi::c_char,
    fmt::Write as _,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::Instant,
};

use ionoray_core::units::{degree, kilometer};
use sha2::{Digest, Sha256};
use tracing::{debug, info_span};

use crate::{IriError, IriInput};

static ASSET_PATH: OnceLock<Result<PathBuf, String>> = OnceLock::new();
static BACKEND: Mutex<bool> = Mutex::new(false);

struct Asset {
    name: &'static str,
    bytes: &'static [u8],
    sha256: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/iri2020/embedded_assets.rs"));

pub(crate) fn initialize() -> Result<(), IriError> {
    let wait_span = tracing::debug_span!(
        "model.lock_wait",
        model = "iri2020",
        operation = "initialize",
        elapsed_us = tracing::field::Empty,
        status = tracing::field::Empty
    );
    let wait_started = Instant::now();
    let lock = wait_span.in_scope(|| BACKEND.lock());
    wait_span.record(
        "elapsed_us",
        u64::try_from(wait_started.elapsed().as_micros()).unwrap_or(u64::MAX),
    );
    wait_span.record("status", if lock.is_ok() { "succeeded" } else { "failed" });
    let mut initialized = lock.map_err(|_| IriError::BackendPoisoned)?;
    if *initialized {
        return Ok(());
    }

    let asset_reused = ASSET_PATH.get().is_some();
    let assets_span = tracing::debug_span!(
        "assets.prepare",
        model = "iri2020",
        reused = asset_reused,
        elapsed_us = tracing::field::Empty,
        status = tracing::field::Empty
    );
    let assets_started = Instant::now();
    let root = assets_span.in_scope(asset_path);
    assets_span.record(
        "elapsed_us",
        u64::try_from(assets_started.elapsed().as_micros()).unwrap_or(u64::MAX),
    );
    assets_span.record("status", if root.is_ok() { "succeeded" } else { "failed" });
    let root = root?;
    let path = root
        .to_str()
        .ok_or_else(|| IriError::Asset("asset path is not valid UTF-8".to_owned()))?;
    let path_length = i32::try_from(path.len())
        .map_err(|_| IriError::Asset("asset path exceeds the C ABI limit".to_owned()))?;
    let initialize_started = Instant::now();
    let span = info_span!(
        "model.initialize",
        model = "iri2020",
        pid = std::process::id(),
        asset_set_sha256 = env!("IONORAY_IRI2020_ASSET_SET_SHA256"),
        elapsed_us = tracing::field::Empty,
    );
    let _entered = span.enter();
    let mut status = -1_i32;
    // SAFETY: `path` remains alive for the call, its byte length is explicit,
    // and `status` points to writable storage expected by the generated shim.
    unsafe {
        ionoray_iri2020_init(path.as_ptr().cast::<c_char>(), path_length, &raw mut status);
    }
    span.record(
        "elapsed_us",
        u64::try_from(initialize_started.elapsed().as_micros()).unwrap_or(u64::MAX),
    );
    if status != 0 {
        return Err(IriError::Initialization(status));
    }
    *initialized = true;
    debug!(
        event = "iri.backend.initialized",
        elapsed_us = u64::try_from(initialize_started.elapsed().as_micros()).unwrap_or(u64::MAX),
        "initialized local IRI-2020 backend"
    );
    Ok(())
}

pub(crate) fn evaluate(
    input: &IriInput,
    year: i32,
    mmdd: i32,
    ut_hours: f64,
) -> Result<[f64; 20], IriError> {
    initialize()?;
    let wait_span = tracing::debug_span!(
        "model.lock_wait",
        model = "iri2020",
        operation = "evaluate",
        elapsed_us = tracing::field::Empty,
        status = tracing::field::Empty
    );
    let wait_started = Instant::now();
    let lock = wait_span.in_scope(|| BACKEND.lock());
    wait_span.record(
        "elapsed_us",
        u64::try_from(wait_started.elapsed().as_micros()).unwrap_or(u64::MAX),
    );
    wait_span.record("status", if lock.is_ok() { "succeeded" } else { "failed" });
    let _guard = lock.map_err(|_| IriError::BackendPoisoned)?;
    let position = input.query.position;
    let drivers = input.drivers;
    let ut_hours = single_precision("ut_hours", ut_hours)?;
    let latitude_deg = single_precision("latitude_deg", position.latitude().get::<degree>())?;
    let longitude_deg = single_precision("longitude_deg", position.longitude().get::<degree>())?;
    let altitude_km = single_precision("altitude_km", position.altitude().get::<kilometer>())?;
    let rz12 = single_precision("sunspot_number_12_month", drivers.sunspot_number_12_month)?;
    let ig12 = single_precision(
        "ionospheric_index_12_month",
        drivers.ionospheric_index_12_month,
    )?;
    let f107_daily = single_precision("f107_daily", drivers.f107_daily)?;
    let f107_81_day = single_precision("f107_81_day", drivers.f107_81_day)?;
    let mut output = [0.0_f32; 20];
    let mut status = -1_i32;
    let compute_span = tracing::debug_span!(
        "model.compute",
        model = "iri2020",
        elapsed_us = tracing::field::Empty
    );
    let compute_started = Instant::now();
    // SAFETY: the generated ISO_C_BINDING shim fixes scalar widths and output
    // length. `_guard` serializes access to IRI common blocks and saved arrays.
    unsafe {
        ionoray_iri2020_eval(
            year,
            mmdd,
            ut_hours,
            latitude_deg,
            longitude_deg,
            altitude_km,
            rz12,
            ig12,
            f107_daily,
            f107_81_day,
            output.as_mut_ptr(),
            &raw mut status,
        );
    }
    compute_span.record(
        "elapsed_us",
        u64::try_from(compute_started.elapsed().as_micros()).unwrap_or(u64::MAX),
    );
    if status != 0 {
        return Err(IriError::Evaluation(status));
    }
    let converted = output.map(f64::from);
    if converted.iter().any(|value| !value.is_finite()) {
        return Err(IriError::NonFiniteOutput("output array"));
    }
    Ok(converted)
}

fn single_precision(name: &'static str, value: f64) -> Result<f32, IriError> {
    if !(f64::from(f32::MIN)..=f64::from(f32::MAX)).contains(&value) {
        return Err(IriError::InputOutOfRange { name, value });
    }
    // The official IRI ABI is REAL(4); range validation above makes this the
    // intentional and isolated precision-loss boundary.
    #[allow(clippy::cast_possible_truncation)]
    let converted = value as f32;
    Ok(converted)
}

fn asset_path() -> Result<&'static Path, IriError> {
    ASSET_PATH
        .get_or_init(materialize_assets)
        .as_ref()
        .map(PathBuf::as_path)
        .map_err(|error| IriError::Asset(error.clone()))
}

fn materialize_assets() -> Result<PathBuf, String> {
    let root = std::env::temp_dir()
        .join("ionoray")
        .join("iri2020")
        .join(env!("IONORAY_IRI2020_ASSET_SET_SHA256"));
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    for asset in ASSETS {
        materialize_asset(&root, asset)?;
    }
    Ok(root)
}

fn materialize_asset(root: &Path, asset: &Asset) -> Result<(), String> {
    let target = root.join(asset.name);
    if target.is_file() && digest(&target)? == asset.sha256 {
        return Ok(());
    }

    let temporary = root.join(format!("{}.part-{}", asset.name, std::process::id()));
    let mut file = File::create(&temporary).map_err(|error| error.to_string())?;
    file.write_all(asset.bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| error.to_string())?;
    if digest(&temporary)? != asset.sha256 {
        return Err(format!("materialized {} SHA-256 mismatch", asset.name));
    }
    if let Err(error) = fs::rename(&temporary, &target) {
        if !target.is_file() || digest(&target)? != asset.sha256 {
            return Err(error.to_string());
        }
        fs::remove_file(temporary).map_err(|remove_error| remove_error.to_string())?;
    }
    Ok(())
}

fn digest(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(output)
}

unsafe extern "C" {
    fn ionoray_iri2020_init(path: *const c_char, path_len: i32, status: *mut i32);
    fn ionoray_iri2020_eval(
        year: i32,
        mmdd: i32,
        ut_hours: f32,
        latitude: f32,
        longitude: f32,
        altitude_km: f32,
        rz12: f32,
        ig12: f32,
        f107_daily: f32,
        f107_81_day: f32,
        output: *mut f32,
        status: *mut i32,
    );
}
