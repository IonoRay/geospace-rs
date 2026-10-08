use std::{io::IsTerminal, path::PathBuf, time::Duration};

use crate::resources::ProfileMode;

const CONSOLE_ENV: &str = "IONORAY_LOG_CONSOLE";
const FORMAT_ENV: &str = "IONORAY_LOG_FORMAT";
const COLOR_ENV: &str = "IONORAY_LOG_COLOR";
const FILE_ENV: &str = "IONORAY_LOG_FILE";
const PROFILE_ENV: &str = "IONORAY_PROFILE";
const PROFILE_INTERVAL_ENV: &str = "IONORAY_PROFILE_INTERVAL_MS";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LogFormat {
    Json,
    Pretty,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ConsoleConfig {
    pub(crate) format: LogFormat,
    pub(crate) ansi: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TracingConfig {
    console: Option<ConsoleConfig>,
    file: Option<PathBuf>,
    profile: Option<(ProfileMode, Duration)>,
}

impl TracingConfig {
    pub(crate) fn from_env() -> Result<Self, String> {
        Self::resolve(
            read_env(CONSOLE_ENV)?.as_deref(),
            read_env(FORMAT_ENV)?.as_deref(),
            read_env(COLOR_ENV)?.as_deref(),
            read_env(FILE_ENV)?.as_deref(),
            read_env(PROFILE_ENV)?.as_deref(),
            read_env(PROFILE_INTERVAL_ENV)?.as_deref(),
            std::io::stderr().is_terminal(),
        )
    }

    pub(crate) const fn console(&self) -> Option<ConsoleConfig> {
        self.console
    }

    pub(crate) fn file(&self) -> Option<&PathBuf> {
        self.file.as_ref()
    }

    pub(crate) const fn profile(&self) -> Option<(ProfileMode, Duration)> {
        self.profile
    }

    fn resolve(
        console: Option<&str>,
        format: Option<&str>,
        color: Option<&str>,
        file: Option<&str>,
        profile: Option<&str>,
        interval_ms: Option<&str>,
        is_terminal: bool,
    ) -> Result<Self, String> {
        let console_enabled = match normalized(console, "on").as_str() {
            "on" => true,
            "off" => false,
            value => {
                return Err(format!(
                    "invalid {CONSOLE_ENV} value `{value}`; expected on or off"
                ));
            }
        };
        let format = match normalized(format, "auto").as_str() {
            "auto" if is_terminal => LogFormat::Pretty,
            "auto" | "json" => LogFormat::Json,
            "pretty" => LogFormat::Pretty,
            value => {
                return Err(format!(
                    "invalid {FORMAT_ENV} value `{value}`; expected auto, pretty, or json"
                ));
            }
        };
        let ansi = match normalized(color, "auto").as_str() {
            "auto" => is_terminal,
            "always" => true,
            "never" => false,
            value => {
                return Err(format!(
                    "invalid {COLOR_ENV} value `{value}`; expected auto, always, or never"
                ));
            }
        };
        let file = match file.map(str::trim) {
            None | Some("off") => None,
            Some("") => return Err(format!("{FILE_ENV} must be `off` or a non-empty path")),
            Some(path) => Some(PathBuf::from(path)),
        };
        let profile = match normalized(profile, "off").as_str() {
            "off" => None,
            "basic" => Some(ProfileMode::Basic),
            "sampled" => Some(ProfileMode::Sampled),
            value => {
                return Err(format!(
                    "invalid {PROFILE_ENV} value `{value}`; expected off, basic, or sampled"
                ));
            }
        };
        let interval = match interval_ms {
            None => 250,
            Some(value) => value.trim().parse::<u64>().map_err(|_| {
                format!("invalid {PROFILE_INTERVAL_ENV} value `{value}`; expected an integer")
            })?,
        };
        if interval < 100 {
            return Err(format!(
                "invalid {PROFILE_INTERVAL_ENV} value `{interval}`; expected at least 100 ms"
            ));
        }
        Ok(Self {
            console: console_enabled.then_some(ConsoleConfig {
                format,
                ansi: format == LogFormat::Pretty && ansi,
            }),
            file,
            profile: profile.map(|profile| (profile, Duration::from_millis(interval))),
        })
    }
}

fn normalized(value: Option<&str>, default: &str) -> String {
    value.unwrap_or(default).trim().to_ascii_lowercase()
}

fn read_env(name: &str) -> Result<Option<String>, String> {
    match std::env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(format!("cannot read {name}: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{ConsoleConfig, LogFormat, TracingConfig};

    #[test]
    fn defaults_to_colored_console_only_for_a_terminal() {
        assert_eq!(
            TracingConfig::resolve(None, None, None, None, None, None, true),
            Ok(TracingConfig {
                console: Some(ConsoleConfig {
                    format: LogFormat::Pretty,
                    ansi: true,
                }),
                file: None,
                profile: None,
            })
        );
    }

    #[test]
    fn redirected_console_uses_uncolored_json() {
        assert_eq!(
            TracingConfig::resolve(None, None, None, None, None, None, false),
            Ok(TracingConfig {
                console: Some(ConsoleConfig {
                    format: LogFormat::Json,
                    ansi: false,
                }),
                file: None,
                profile: None,
            })
        );
    }

    #[test]
    fn console_and_file_are_independently_switchable() {
        assert_eq!(
            TracingConfig::resolve(
                Some("off"),
                None,
                None,
                Some("logs/run.ndjson"),
                None,
                None,
                true
            ),
            Ok(TracingConfig {
                console: None,
                file: Some(PathBuf::from("logs/run.ndjson")),
                profile: None,
            })
        );
        assert_eq!(
            TracingConfig::resolve(Some("on"), None, None, Some("off"), None, None, true),
            Ok(TracingConfig {
                console: Some(ConsoleConfig {
                    format: LogFormat::Pretty,
                    ansi: true,
                }),
                file: None,
                profile: None,
            })
        );
    }

    #[test]
    fn explicit_format_and_color_override_terminal_detection() {
        assert_eq!(
            TracingConfig::resolve(
                Some("on"),
                Some("pretty"),
                Some("always"),
                None,
                None,
                None,
                false,
            ),
            Ok(TracingConfig {
                console: Some(ConsoleConfig {
                    format: LogFormat::Pretty,
                    ansi: true,
                }),
                file: None,
                profile: None,
            })
        );
    }

    #[test]
    fn invalid_values_are_rejected() {
        assert!(TracingConfig::resolve(Some("maybe"), None, None, None, None, None, true).is_err());
        assert!(TracingConfig::resolve(None, Some("yaml"), None, None, None, None, true).is_err());
        assert!(
            TracingConfig::resolve(None, None, Some("sometimes"), None, None, None, true).is_err()
        );
        assert!(TracingConfig::resolve(None, None, None, Some(""), None, None, true).is_err());
        assert!(TracingConfig::resolve(None, None, None, None, Some("fast"), None, true).is_err());
        assert!(
            TracingConfig::resolve(None, None, None, None, Some("sampled"), Some("99"), true)
                .is_err()
        );
    }
}
