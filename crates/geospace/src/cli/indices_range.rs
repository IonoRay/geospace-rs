use clap::{Args, ValueEnum};
use ionoray_core::Epoch;
use ionoray_indices::{
    IndexDataset, IndexField, IndexStore, RangeRequest, RangeSyncStatus, SyncMode,
};

use crate::GeospaceError;

#[derive(Debug, Args)]
pub(super) struct RangeArgs {
    #[arg(long, value_enum)]
    dataset: Dataset,
    /// Inclusive UTC start, for example 2020-01-15T00:00:00Z.
    #[arg(long)]
    start: String,
    /// Exclusive UTC end.
    #[arg(long)]
    end: String,
    /// Required fields; omit to request every field in this dataset.
    #[arg(long, value_enum, value_delimiter = ',')]
    field: Vec<Field>,
    #[arg(long, value_enum, default_value_t = Mode::Refresh)]
    mode: Mode,
    /// Force a complete body comparison; invalid with offline mode.
    #[arg(long)]
    force: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Dataset {
    Dst,
    Ae,
    KpApF107,
    IriIgRz,
    #[value(name = "iri-apf107", alias = "iri-ap-f107")]
    IriApF107,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Mode {
    Offline,
    Ensure,
    Refresh,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Field {
    Kp,
    Ap3h,
    DailyAp,
    F107,
    F107a,
    Dst,
    Ae,
    Ig12,
    Rz12,
    IriF107,
    IriF107a81,
    IriF107a365,
}

impl RangeArgs {
    fn request(self) -> Result<RangeRequest, GeospaceError> {
        let parse = |text: &str| {
            text.parse::<Epoch>()
                .map_err(|error| GeospaceError::InvalidEpoch(error.to_string()))
        };
        let dataset = match self.dataset {
            Dataset::Dst => IndexDataset::Dst,
            Dataset::Ae => IndexDataset::Ae,
            Dataset::KpApF107 => IndexDataset::KpApF107,
            Dataset::IriIgRz => IndexDataset::IriIgRz,
            Dataset::IriApF107 => IndexDataset::IriApF107,
        };
        let fields = if self.field.is_empty() {
            IndexField::for_dataset(dataset).to_vec()
        } else {
            self.field.into_iter().map(IndexField::from).collect()
        };
        Ok(RangeRequest {
            dataset,
            fields,
            start: parse(&self.start)?,
            end: parse(&self.end)?,
            mode: match self.mode {
                Mode::Offline => SyncMode::Offline,
                Mode::Ensure => SyncMode::Ensure,
                Mode::Refresh => SyncMode::Refresh,
            },
            force: self.force,
        })
    }
}

pub(super) async fn execute(store: &IndexStore, args: RangeArgs) -> Result<(), GeospaceError> {
    let report = store.sync_range(args.request()?).await?;
    super::output::json(&report)?;
    if report.status != RangeSyncStatus::Complete {
        return Err(GeospaceError::PartialIndices(Box::new(report)));
    }
    Ok(())
}

impl From<Field> for IndexField {
    fn from(value: Field) -> Self {
        match value {
            Field::Kp => Self::Kp,
            Field::Ap3h => Self::Ap3h,
            Field::DailyAp => Self::DailyAp,
            Field::F107 => Self::F107,
            Field::F107a => Self::F107a,
            Field::Dst => Self::Dst,
            Field::Ae => Self::Ae,
            Field::Ig12 => Self::Ig12,
            Field::Rz12 => Self::Rz12,
            Field::IriF107 => Self::IriF107,
            Field::IriF107a81 => Self::IriF107a81,
            Field::IriF107a365 => Self::IriF107a365,
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Parser)]
    struct Command {
        #[command(flatten)]
        args: RangeArgs,
    }

    #[test]
    fn range_parsing_preserves_fields_and_boundaries() {
        let command = Command::try_parse_from([
            "test",
            "--dataset",
            "kp-ap-f107",
            "--start",
            "2020-12-31T23:00:00Z",
            "--end",
            "2021-01-01T01:00:00Z",
            "--field",
            "ap3h,f107",
            "--mode",
            "ensure",
        ])
        .unwrap();
        let request = command.args.request().unwrap();
        request.validate().unwrap();
        assert_eq!(request.fields, [IndexField::Ap3h, IndexField::F107]);
        assert_eq!(request.mode, SyncMode::Ensure);
        assert!(((request.end - request.start).to_seconds() - 7200.0).abs() < f64::EPSILON);
    }

    #[test]
    fn force_offline_is_rejected_before_sync() {
        let command = Command::try_parse_from([
            "test",
            "--dataset",
            "dst",
            "--start",
            "2020-01-01T00:00:00Z",
            "--end",
            "2020-01-02T00:00:00Z",
            "--mode",
            "offline",
            "--force",
        ])
        .unwrap();
        assert!(command.args.request().unwrap().validate().is_err());
    }

    #[test]
    fn dataset_field_mismatch_is_rejected() {
        let command = Command::try_parse_from([
            "test",
            "--dataset",
            "dst",
            "--start",
            "2020-01-01T00:00:00Z",
            "--end",
            "2020-01-02T00:00:00Z",
            "--field",
            "f107",
        ])
        .unwrap();
        assert!(command.args.request().unwrap().validate().is_err());
    }

    #[test]
    fn iri_scope_name_matches_documented_cli_dataset() {
        let command = Command::try_parse_from([
            "test",
            "--dataset",
            "iri-apf107",
            "--start",
            "2020-07-01T12:00:00Z",
            "--end",
            "2020-07-01T13:00:00Z",
            "--mode",
            "offline",
        ])
        .unwrap();
        assert_eq!(
            command.args.request().unwrap().dataset,
            IndexDataset::IriApF107
        );
    }
}
