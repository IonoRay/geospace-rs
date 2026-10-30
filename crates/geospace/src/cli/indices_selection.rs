use clap::ValueEnum;
use ionoray_indices::IndexDataset;

/// One native annual dataset; all preserves the existing five-group run.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(super) enum AnnualDataset {
    All,
    KpApF107,
    Dst,
    Ae,
    IriIgRz,
    IriApf107,
    Hp30,
    Hp60,
    Pcn,
    Omni,
}

impl AnnualDataset {
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::KpApF107 => "kp-ap-f107",
            Self::Dst => "dst",
            Self::Ae => "ae",
            Self::IriIgRz => "iri-ig-rz",
            Self::IriApf107 => "iri-apf107",
            Self::Hp30 => "hp30",
            Self::Hp60 => "hp60",
            Self::Pcn => "pcn",
            Self::Omni => "omni",
        }
    }

    pub(super) const fn core(self) -> Option<IndexDataset> {
        match self {
            Self::KpApF107 => Some(IndexDataset::KpApF107),
            Self::Dst => Some(IndexDataset::Dst),
            Self::Ae => Some(IndexDataset::Ae),
            Self::IriIgRz => Some(IndexDataset::IriIgRz),
            Self::IriApf107 => Some(IndexDataset::IriApF107),
            _ => None,
        }
    }

    pub(super) fn includes(self, product: &str) -> bool {
        self == Self::All || self.name() == product
    }

    pub(super) const fn total_groups(self) -> usize {
        if matches!(self, Self::All) { 5 } else { 1 }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Cli, Command, indices::IndicesCommand};
    use super::*;
    use clap::Parser;

    #[test]
    fn omitted_dataset_defaults_to_all_and_year_is_required() {
        let cli = Cli::try_parse_from(["geospace", "indices", "sync", "--year", "2025"]).unwrap();
        let Command::Indices {
            command: IndicesCommand::Sync { dataset, year, .. },
        } = cli.command
        else {
            panic!("wrong command");
        };
        assert_eq!(dataset, AnnualDataset::All);
        assert_eq!(year, 2025);
        assert!(Cli::try_parse_from(["geospace", "indices", "sync", "--dataset", "dst"]).is_err());
        assert!(
            Cli::try_parse_from([
                "geospace",
                "indices",
                "sync",
                "--year",
                "2025",
                "--dataset",
                "unknown"
            ])
            .is_err()
        );
    }
}
