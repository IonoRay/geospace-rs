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

use crate::{MsisError, model::MsisInput};

const PARAMETER_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/nrlmsis21/assets/msis21.parm"));
const PARAMETER_SHA256: &str = env!("IONORAY_NRLMSIS21_PARAMETER_SHA256");
const FORTRAN_PATH_LIMIT: usize = 128;

static PARAMETER_PATH: OnceLock<Result<PathBuf, String>> = OnceLock::new();
static BACKEND: Mutex<BackendState> = Mutex::new(BackendState { storm_time: None });

struct BackendState {
    storm_time: Option<bool>,
}

pub(crate) struct RawOutput {
    pub(crate) temperature: f64,
    pub(crate) densities: [f64; 10],
    pub(crate) exospheric_temperature: f64,
}

pub(crate) fn evaluate(
    input: &MsisInput,
    day_of_year: f64,
    ut_seconds: f64,
) -> Result<RawOutput, MsisError> {
    let wait_span = tracing::debug_span!(
        "model.lock_wait",
        model = "nrlmsis21",
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
    let mut state = lock.map_err(|_| MsisError::BackendPoisoned)?;
    let storm_time = input.drivers.geomagnetic_activity.is_storm_time();
    if state.storm_time != Some(storm_time) {
        initialize(storm_time)?;
        state.storm_time = Some(storm_time);
    }

    let mut output = RawOutput {
        temperature: 0.0,
        densities: [0.0; 10],
        exospheric_temperature: 0.0,
    };
    let position = input.query.position;
    let ap = input.drivers.geomagnetic_activity.values();
    let compute_span = tracing::debug_span!(
        "model.compute",
        model = "nrlmsis21",
        elapsed_us = tracing::field::Empty
    );
    let compute_started = Instant::now();
    // SAFETY: the generated ISO_C_BINDING shim fixes scalar widths and array
    // lengths. `state` serializes access to the non-reentrant Fortran globals.
    unsafe {
        ionoray_msis21_eval(
            day_of_year,
            ut_seconds,
            position.altitude().get::<kilometer>(),
            position.latitude().get::<degree>(),
            position.longitude().get::<degree>(),
            input.drivers.f107a,
            input.drivers.f107_previous_day,
            ap.as_ptr(),
            &raw mut output.temperature,
            output.densities.as_mut_ptr(),
            &raw mut output.exospheric_temperature,
        );
    }
    compute_span.record(
        "elapsed_us",
        u64::try_from(compute_started.elapsed().as_micros()).unwrap_or(u64::MAX),
    );
    Ok(output)
}

fn initialize(storm_time: bool) -> Result<(), MsisError> {
    let asset_reused = PARAMETER_PATH.get().is_some();
    let assets_span = tracing::debug_span!(
        "assets.prepare",
        model = "nrlmsis21",
        reused = asset_reused,
        elapsed_us = tracing::field::Empty,
        status = tracing::field::Empty
    );
    let assets_started = Instant::now();
    let path = assets_span.in_scope(parameter_path);
    assets_span.record(
        "elapsed_us",
        u64::try_from(assets_started.elapsed().as_micros()).unwrap_or(u64::MAX),
    );
    assets_span.record("status", if path.is_ok() { "succeeded" } else { "failed" });
    let path = path?;
    let path = path
        .to_str()
        .ok_or_else(|| MsisError::Asset("parameter path is not valid UTF-8".to_owned()))?;
    if path.len() > FORTRAN_PATH_LIMIT {
        return Err(MsisError::Asset(format!(
            "parameter path is {} bytes; upstream NRLMSIS accepts at most {FORTRAN_PATH_LIMIT}",
            path.len()
        )));
    }
    let path_length = i32::try_from(path.len())
        .map_err(|_| MsisError::Asset("parameter path exceeds the C ABI limit".to_owned()))?;
    let initialize_started = Instant::now();
    let span = info_span!(
        "model.initialize",
        model = "nrlmsis21",
        pid = std::process::id(),
        storm_time,
        parameter_sha256 = PARAMETER_SHA256,
        elapsed_us = tracing::field::Empty,
    );
    let _entered = span.enter();
    let mut status = -1_i32;
    // SAFETY: `path` remains alive for the call, its byte length is explicit,
    // and `status` points to writable storage expected by the generated shim.
    unsafe {
        ionoray_msis21_init(
            path.as_ptr().cast::<c_char>(),
            path_length,
            i32::from(storm_time),
            &raw mut status,
        );
    }
    span.record(
        "elapsed_us",
        u64::try_from(initialize_started.elapsed().as_micros()).unwrap_or(u64::MAX),
    );
    if status != 0 {
        return Err(MsisError::Initialization(status));
    }
    debug!(
        event = "msis.backend.initialized",
        elapsed_us = u64::try_from(initialize_started.elapsed().as_micros()).unwrap_or(u64::MAX),
        "initialized local NRLMSIS 2.1 backend"
    );
    Ok(())
}

fn parameter_path() -> Result<&'static Path, MsisError> {
    PARAMETER_PATH
        .get_or_init(materialize_parameter)
        .as_ref()
        .map(PathBuf::as_path)
        .map_err(|error| MsisError::Asset(error.clone()))
}

fn materialize_parameter() -> Result<PathBuf, String> {
    let digest_prefix = PARAMETER_SHA256
        .get(..16)
        .ok_or_else(|| "parameter SHA-256 is unexpectedly short".to_owned())?;
    let directory = std::env::temp_dir()
        .join("ionoray")
        .join("nrlmsis21")
        .join(digest_prefix);
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let target = directory.join("msis21.parm");
    if target.is_file() && digest(&target)? == PARAMETER_SHA256 {
        return Ok(target);
    }

    let temporary = directory.join(format!("msis21.parm.part-{}", std::process::id()));
    let mut file = File::create(&temporary).map_err(|error| error.to_string())?;
    file.write_all(PARAMETER_BYTES)
        .and_then(|()| file.sync_all())
        .map_err(|error| error.to_string())?;
    if digest(&temporary)? != PARAMETER_SHA256 {
        return Err("materialized parameter SHA-256 mismatch".to_owned());
    }
    if let Err(error) = fs::rename(&temporary, &target) {
        if !target.is_file() || digest(&target)? != PARAMETER_SHA256 {
            return Err(error.to_string());
        }
        fs::remove_file(temporary).map_err(|remove_error| remove_error.to_string())?;
    }
    Ok(target)
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
    fn ionoray_msis21_init(path: *const c_char, path_len: i32, storm_mode: i32, status: *mut i32);
    fn ionoray_msis21_eval(
        day: f64,
        utsec: f64,
        altitude: f64,
        latitude: f64,
        longitude: f64,
        f107a: f64,
        f107: f64,
        ap: *const f64,
        temperature: *mut f64,
        densities: *mut f64,
        exospheric_temperature: *mut f64,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn materialized_parameter_fits_upstream_fortran_path_limit() {
        let path = parameter_path().unwrap();
        assert!(path.as_os_str().len() <= FORTRAN_PATH_LIMIT);
        assert_eq!(digest(path).unwrap(), PARAMETER_SHA256);
    }
}
