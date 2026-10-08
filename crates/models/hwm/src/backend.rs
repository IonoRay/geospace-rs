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

use crate::{HwmError, model::HwmInput};

const ASSETS: &[Asset] = &[
    Asset {
        name: "hwm123114.bin",
        bytes: include_bytes!(concat!(env!("OUT_DIR"), "/hwm14/assets/hwm123114.bin")),
        sha256: "6e445f8337c7efc815b7ff7f9f967d8a9a16b469d93df9c46e54930553beb906",
    },
    Asset {
        name: "dwm07b104i.dat",
        bytes: include_bytes!(concat!(env!("OUT_DIR"), "/hwm14/assets/dwm07b104i.dat")),
        sha256: "f2b8eff002d55b0f6d49d202c73b7a9cb6685f2c6bb57f1291a2adc7aa07cf4f",
    },
    Asset {
        name: "gd2qd.dat",
        bytes: include_bytes!(concat!(env!("OUT_DIR"), "/hwm14/assets/gd2qd.dat")),
        sha256: "6bb1f2384e30b409240ee92c32726ed2a3d73eb550c05e0969d1178032319804",
    },
];

static ASSET_PATH: OnceLock<Result<PathBuf, String>> = OnceLock::new();
static BACKEND: Mutex<bool> = Mutex::new(false);

struct Asset {
    name: &'static str,
    bytes: &'static [u8],
    sha256: &'static str,
}

pub(crate) fn initialize() -> Result<(), HwmError> {
    let wait_span = tracing::debug_span!(
        "model.lock_wait",
        model = "hwm14",
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
    let mut initialized = lock.map_err(|_| HwmError::BackendPoisoned)?;
    if *initialized {
        return Ok(());
    }

    let asset_reused = ASSET_PATH.get().is_some();
    let assets_span = tracing::debug_span!(
        "assets.prepare",
        model = "hwm14",
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
        .ok_or_else(|| HwmError::Asset("asset path is not valid UTF-8".to_owned()))?;
    let path_length = i32::try_from(path.len())
        .map_err(|_| HwmError::Asset("asset path exceeds the C ABI limit".to_owned()))?;
    let initialize_started = Instant::now();
    let span = info_span!(
        "model.initialize",
        model = "hwm14",
        pid = std::process::id(),
        asset_set_sha256 = env!("IONORAY_HWM14_ASSET_SET_SHA256"),
        elapsed_us = tracing::field::Empty,
    );
    let _entered = span.enter();
    let mut status = -1_i32;
    // SAFETY: `path` remains alive for the call, its byte length is explicit,
    // and `status` points to writable storage expected by the generated shim.
    unsafe {
        ionoray_hwm14_init(path.as_ptr().cast::<c_char>(), path_length, &raw mut status);
    }
    span.record(
        "elapsed_us",
        u64::try_from(initialize_started.elapsed().as_micros()).unwrap_or(u64::MAX),
    );
    if status != 0 {
        return Err(HwmError::Initialization(status));
    }
    *initialized = true;
    debug!(
        event = "hwm.backend.initialized",
        elapsed_us = u64::try_from(initialize_started.elapsed().as_micros()).unwrap_or(u64::MAX),
        "initialized local HWM14 backend"
    );
    Ok(())
}

pub(crate) fn evaluate(input: &HwmInput, iyd: i32, ut_seconds: f64) -> Result<[f32; 2], HwmError> {
    initialize()?;
    let wait_span = tracing::debug_span!(
        "model.lock_wait",
        model = "hwm14",
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
    let _guard = lock.map_err(|_| HwmError::BackendPoisoned)?;
    let position = input.query.position;
    let ut_seconds = single_precision("ut_seconds", ut_seconds)?;
    let altitude_km = single_precision("altitude_km", position.altitude().get::<kilometer>())?;
    let latitude_deg = single_precision("latitude_deg", position.latitude().get::<degree>())?;
    let longitude_deg = single_precision("longitude_deg", position.longitude().get::<degree>())?;
    let ap = single_precision("current_ap", input.geomagnetic_activity.ap())?;
    let mut wind = [0.0_f32; 2];
    let compute_span = tracing::debug_span!(
        "model.compute",
        model = "hwm14",
        elapsed_us = tracing::field::Empty
    );
    let compute_started = Instant::now();
    // SAFETY: the generated ISO_C_BINDING shim fixes scalar widths and output
    // length. `_guard` serializes access to the non-reentrant Fortran globals.
    unsafe {
        ionoray_hwm14_eval(
            iyd,
            ut_seconds,
            altitude_km,
            latitude_deg,
            longitude_deg,
            ap,
            wind.as_mut_ptr(),
        );
    }
    compute_span.record(
        "elapsed_us",
        u64::try_from(compute_started.elapsed().as_micros()).unwrap_or(u64::MAX),
    );
    Ok(wind)
}

fn single_precision(name: &'static str, value: f64) -> Result<f32, HwmError> {
    if !(f64::from(f32::MIN)..=f64::from(f32::MAX)).contains(&value) {
        return Err(HwmError::InputOutOfRange { name, value });
    }
    // The official HWM14 ABI is REAL(4); range validation above makes this the
    // intentional and isolated precision-loss boundary.
    #[allow(clippy::cast_possible_truncation)]
    let converted = value as f32;
    Ok(converted)
}

fn asset_path() -> Result<&'static Path, HwmError> {
    ASSET_PATH
        .get_or_init(materialize_assets)
        .as_ref()
        .map(PathBuf::as_path)
        .map_err(|error| HwmError::Asset(error.clone()))
}

fn materialize_assets() -> Result<PathBuf, String> {
    let root = std::env::temp_dir()
        .join("ionoray")
        .join("hwm14")
        .join(env!("IONORAY_HWM14_ASSET_SET_SHA256"));
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
    fn ionoray_hwm14_init(path: *const c_char, path_len: i32, status: *mut i32);
    fn ionoray_hwm14_eval(
        iyd: i32,
        ut_seconds: f32,
        altitude_km: f32,
        latitude_deg: f32,
        longitude_deg: f32,
        ap: f32,
        wind: *mut f32,
    );
}
