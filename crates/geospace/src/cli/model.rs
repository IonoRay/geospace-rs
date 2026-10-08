use std::{
    fs::File,
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

use clap::{Args, Subcommand};
use ionoray_core::{Epoch, GeodeticPosition, QueryPoint};
use serde_json::Value;

use super::{
    ExecuteStatus, model_evaluate,
    model_types::{Drivers, Mode, Model, Policy, Record, Request},
};
use crate::GeospaceError;

pub(super) type Failure = (&'static str, String);

/// One JSON object or a sequential JSONL stream.
#[derive(Debug, Subcommand)]
pub(super) enum ModelCommand {
    /// Evaluate exactly one JSON object (including pretty-printed JSON).
    Run(InputArgs),
    /// Evaluate every physical JSONL line, including invalid/blank lines.
    Batch(InputArgs),
}

#[derive(Debug, Args)]
pub(super) struct InputArgs {
    /// Input file, or `-` for standard input.
    #[arg(long)]
    input: PathBuf,
}

// One lazy open per process call, including one cached failed open. Explicit
// inputs never call get(), so an unusable home does not prevent direct models.
pub(super) struct Session<'a> {
    #[cfg(all(
        feature = "indices",
        any(feature = "iri", feature = "hwm", feature = "msis")
    ))]
    home: Option<&'a Path>,
    #[cfg(all(
        feature = "indices",
        any(feature = "iri", feature = "hwm", feature = "msis")
    ))]
    opened: Option<Result<crate::Geospace, Failure>>,
    // Keep the same concrete type in CLI builds without a preparation backend.
    marker: std::marker::PhantomData<&'a Path>,
}

impl<'a> Session<'a> {
    fn new(home: Option<&'a Path>) -> Self {
        let _ = home;
        Self {
            #[cfg(all(
                feature = "indices",
                any(feature = "iri", feature = "hwm", feature = "msis")
            ))]
            home,
            #[cfg(all(
                feature = "indices",
                any(feature = "iri", feature = "hwm", feature = "msis")
            ))]
            opened: None,
            marker: std::marker::PhantomData,
        }
    }

    #[cfg(all(
        feature = "indices",
        any(feature = "iri", feature = "hwm", feature = "msis")
    ))]
    pub(super) async fn get(&mut self) -> Result<&crate::Geospace, Failure> {
        if self.opened.is_none() {
            self.opened = Some(
                crate::Geospace::open(self.home)
                    .await
                    .map_err(|e| failure(&e)),
            );
        }
        self.opened.as_ref().unwrap().as_ref().map_err(Clone::clone)
    }
}

#[cfg(feature = "indices")]
impl From<Policy> for crate::DataPolicy {
    fn from(policy: Policy) -> Self {
        match policy {
            Policy::Ensure => Self::Ensure,
            Policy::Offline => Self::Offline,
            Policy::Refresh => Self::Refresh,
        }
    }
}

pub(super) async fn execute(
    home: Option<&Path>,
    command: ModelCommand,
) -> Result<ExecuteStatus, GeospaceError> {
    let (input, batch) = match command {
        ModelCommand::Run(args) => (args.input, false),
        ModelCommand::Batch(args) => (args.input, true),
    };
    if input.as_os_str() == "-" {
        process(home, io::stdin().lock(), io::stdout().lock(), batch).await
    } else {
        process(
            home,
            BufReader::new(File::open(input)?),
            io::stdout().lock(),
            batch,
        )
        .await
    }
}

/// Processes a single JSON object or physical JSONL lines through model APIs.
///
/// # Errors
/// Returns [`GeospaceError`] on stream I/O failure. Invalid records emit a
/// structured error and contribute to [`ExecuteStatus::RecordFailures`].
pub async fn process<R: BufRead, W: Write>(
    home: Option<&Path>,
    mut reader: R,
    mut writer: W,
    batch: bool,
) -> Result<ExecuteStatus, GeospaceError> {
    let mut session = Session::new(home);
    let mut failures = false;
    let mut bytes = Vec::new();
    let mut line = 0;
    loop {
        bytes.clear();
        let count = if batch {
            reader.read_until(b'\n', &mut bytes)?
        } else {
            reader.read_to_end(&mut bytes)?
        };
        if batch && count == 0 {
            break;
        }
        line += 1;
        let record = match std::str::from_utf8(&bytes) {
            Ok(text) => evaluate_line(&mut session, line, text).await,
            Err(error) => Record::failed(line, None, None, "invalid_json", error.to_string()),
        };
        failures |= record.status == "failed";
        serde_json::to_writer(&mut writer, &record)?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        if !batch {
            break;
        }
    }
    Ok(if failures {
        ExecuteStatus::RecordFailures
    } else {
        ExecuteStatus::Succeeded
    })
}

async fn evaluate_line(session: &mut Session<'_>, line: usize, text: &str) -> Record {
    // The Value is only for structural validation and safe ID/model echo.
    // Deserialize the original text too, so duplicate known keys are rejected.
    let value: Value = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(error) => return Record::failed(line, None, None, "invalid_json", error.to_string()),
    };
    let id = value.get("id").and_then(Value::as_str).map(str::to_owned);
    let model = value
        .get("model")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let request: Request = match serde_json::from_str(text) {
        Ok(request) => request,
        Err(error) => return Record::failed(line, id, model, "invalid_request", error.to_string()),
    };
    match validate(&request, &value) {
        Err(message) => Record::failed(line, id, model, "invalid_request", message),
        Ok((query, policy)) => {
            let defaults = Drivers::default();
            let drivers = request.drivers.as_ref().unwrap_or(&defaults);
            let result = match request.model {
                Model::Igrf14 => model_evaluate::evaluate_igrf(query),
                Model::Iri2020 => {
                    model_evaluate::evaluate_iri(session, &request, query, policy, drivers).await
                }
                Model::Hwm14 => {
                    model_evaluate::evaluate_hwm(session, &request, query, policy, drivers).await
                }
                Model::Nrlmsis21 => {
                    model_evaluate::evaluate_msis(session, &request, query, policy, drivers).await
                }
            };
            match result {
                Ok(value) => Record::succeeded(line, &request, value),
                Err((code, message)) => Record::failed(line, id, model, code, message),
            }
        }
    }
}

fn validate(request: &Request, raw: &Value) -> Result<(QueryPoint, Policy), String> {
    if request.mode == Mode::Direct && raw.get("data_policy").is_some() {
        return Err("direct mode forbids data_policy".into());
    }
    let allowed: &[&str] = match request.model {
        Model::Igrf14 => &[],
        Model::Iri2020 => &["rz12", "ig12", "f107_daily", "f107_81_day"],
        Model::Hwm14 => &["activity", "current_ap"],
        Model::Nrlmsis21 => &["f107a", "f107_previous_day", "ap_daily", "ap_history"],
    };
    if let Some(fields) = raw.get("drivers").and_then(Value::as_object) {
        for key in fields.keys() {
            if !allowed.contains(&key.as_str()) {
                return Err(format!("driver {key} is not accepted by this model"));
            }
        }
    }
    if let Some(drivers) = &request.drivers {
        drivers.validate_finite()?;
    }
    let at = request.at.trim();
    if !(at.ends_with('Z') || at.ends_with("+00:00") || at.ends_with(" UTC")) {
        return Err("at must include explicit UTC (Z, +00:00, or UTC)".into());
    }
    let epoch = at.parse::<Epoch>().map_err(|e| e.to_string())?;
    let position = GeodeticPosition::from_degrees_kilometers(
        request.latitude_deg,
        request.longitude_deg,
        request.altitude_km,
    )
    .map_err(|e| e.to_string())?;
    Ok((
        QueryPoint { epoch, position },
        request.data_policy.unwrap_or(Policy::Ensure),
    ))
}

#[cfg(any(feature = "iri", feature = "msis"))]
pub(super) fn direct_only(drivers: Option<&Drivers>, fields: &[Option<f64>]) -> Result<(), String> {
    if drivers.is_none() || fields.iter().any(Option::is_none) {
        Err("direct mode requires every model driver".into())
    } else {
        Ok(())
    }
}

#[cfg(any(feature = "igrf", feature = "iri", feature = "hwm", feature = "msis"))]
pub(super) fn json(value: &impl serde::Serialize) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| e.to_string())
}

#[cfg(any(feature = "igrf", feature = "iri", feature = "hwm", feature = "msis"))]
pub(super) use super::model_error::failure;

#[cfg(all(test, feature = "cli-standard"))]
#[path = "model_tests.rs"]
mod tests;
