use std::{
    io::{self, Write},
    path::Path,
};

use clap::{Args, Subcommand, ValueEnum};
use ionoray_omni::{OmniProduct, OmniRange, OmniStore};

use super::output;
use crate::GeospaceError;

#[derive(Debug, Subcommand)]
pub(super) enum OmniCommand {
    /// Check upstream each run, upgrade HRO to HRO2, and report selected inputs.
    Download {
        #[command(flatten)]
        range: RangeArgs,
        /// Unconditionally redownload monthly bodies and verify SHA-256.
        #[arg(long, action = clap::ArgAction::Set, num_args = 0..=1,
            require_equals = true, default_missing_value = "true", default_value = "false")]
        force: bool,
    },
    /// Read accepted local CDFs; JSONL on stdout, coverage/provenance on stderr.
    Read(RangeArgs),
}

#[derive(Debug, Args)]
pub(super) struct RangeArgs {
    /// Inclusive UTC epoch, e.g. 2024-05-10T00:00:00Z.
    #[arg(long)]
    start: String,
    /// Exclusive UTC epoch.
    #[arg(long)]
    end: String,
    /// Source cadence; currently only one-minute monthly CDFs are supported.
    #[arg(long = "cadence", value_enum, default_value = "1min")]
    _cadence: Cadence,
    /// Auto checks HRO2 first and retains accepted HRO2 if upstream is absent.
    #[arg(long, value_enum, default_value = "auto")]
    product: Product,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Product {
    Auto,
    Hro2,
    Hro,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Cadence {
    #[value(name = "1min")]
    OneMinute,
}

impl Product {
    const fn selected(self) -> Option<OmniProduct> {
        match self {
            Self::Auto => None,
            Self::Hro2 => Some(OmniProduct::Hro2),
            Self::Hro => Some(OmniProduct::Hro),
        }
    }
}

pub(super) async fn execute(
    home: Option<&Path>,
    command: OmniCommand,
) -> Result<(), GeospaceError> {
    let args = match &command {
        OmniCommand::Download { range, .. } | OmniCommand::Read(range) => range,
    };
    let range = OmniRange::parse(&args.start, &args.end)?;
    let product = args.product.selected();
    let store = OmniStore::open(home).await?;
    match command {
        OmniCommand::Download { force, .. } => {
            output::json(&store.download(range, product, force).await?)?;
        }
        OmniCommand::Read(_) => {
            let mut stdout = io::BufWriter::new(io::stdout().lock());
            let report = store
                .read(range, product, |record| {
                    serde_json::to_writer(&mut stdout, record).map_err(io::Error::other)?;
                    stdout.write_all(b"\n")?;
                    Ok(())
                })
                .await?;
            stdout.flush()?;
            let mut stderr = io::stderr().lock();
            serde_json::to_writer(&mut stderr, &report)?;
            stderr.write_all(b"\n")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::OmniCommand;
    use crate::cli::Cli;
    use crate::cli::Command;
    use clap::Parser;

    #[test]
    fn download_launch_arguments_support_force_and_only_real_cadences() {
        let base = [
            "geospace",
            "--home",
            "/tmp/omni-launch-test",
            "omni",
            "download",
            "--start",
            "2024-05-10T00:00:00Z",
            "--end",
            "2024-05-13T00:00:00Z",
            "--cadence",
            "1min",
            "--product",
            "auto",
        ];
        for (flag, expected) in [
            (None, false),
            (Some("--force"), true),
            (Some("--force=true"), true),
            (Some("--force=false"), false),
        ] {
            let args: Vec<_> = base.iter().copied().chain(flag).collect();
            let cli = Cli::try_parse_from(args).unwrap();
            let Command::Omni {
                command: OmniCommand::Download { force, .. },
            } = cli.command
            else {
                panic!("expected OMNI download");
            };
            assert_eq!(force, expected);
        }
        for unsupported in ["5min", "1hour"] {
            let mut args = base.to_vec();
            args[10] = unsupported;
            assert!(Cli::try_parse_from(args).is_err());
        }
    }
    #[test]
    fn accepts_default_and_explicit_products() {
        for product in ["auto", "hro2", "hro"] {
            assert!(
                Cli::try_parse_from([
                    "geospace",
                    "omni",
                    "read",
                    "--start",
                    "2024-05-10T00:00:00Z",
                    "--end",
                    "2024-05-11T00:00:00Z",
                    "--product",
                    product
                ])
                .is_ok()
            );
        }
    }
}
