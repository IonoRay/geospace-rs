use std::{
    env,
    ffi::OsStr,
    path::{Path, PathBuf},
};

use crate::StoreError;

/// Resolved root directory for all `IonoRay` runtime data and configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreHome(PathBuf);

fn expand_tilde(path: PathBuf, user_home: Option<&Path>) -> Result<PathBuf, StoreError> {
    let Ok(rest) = path.strip_prefix("~") else {
        return Ok(path);
    };

    let home = user_home.ok_or(StoreError::HomeUnavailable)?;

    Ok(home.join(rest))
}

fn resolve(
    explicit: Option<&Path>,
    configured: Option<&OsStr>,
    user_home: Option<&Path>,
) -> Result<StoreHome, StoreError> {
    if let Some(path) = explicit {
        return StoreHome::new_with_home(path.to_path_buf(), user_home);
    }
    if let Some(path) = configured {
        return StoreHome::new_with_home(PathBuf::from(path), user_home);
    }

    let user_home = user_home.ok_or(StoreError::HomeUnavailable)?;
    let legacy_home = user_home.join(".ionoray");
    let default_home = legacy_home.join("geospace-rs");
    if legacy_home.join("indices").exists() || legacy_home.join("objects").exists() {
        return Err(StoreError::LegacyHomeNeedsSelection {
            legacy_home,
            default_home,
        });
    }

    StoreHome::new_with_home(default_home, Some(user_home))
}

impl StoreHome {
    /// Resolves an explicit path, then `IONORAY_HOME`, then `~/.ionoray/geospace-rs`.
    ///
    /// If an old `~/.ionoray/indices` or `~/.ionoray/objects` exists, an
    /// implicit default is refused so data is never silently split between roots.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::HomeUnavailable`] if no home can be resolved,
    /// [`StoreError::RelativeHome`] when the selected path is relative, or
    /// [`StoreError::LegacyHomeNeedsSelection`] for an ambiguous legacy root.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "preserve the public API accepting owned or borrowed paths"
    )]
    pub fn discover(explicit: Option<impl AsRef<Path>>) -> Result<Self, StoreError> {
        let explicit = explicit.as_ref().map(AsRef::as_ref);
        let configured = env::var_os("IONORAY_HOME");
        let user_home = dirs::home_dir();
        resolve(explicit, configured.as_deref(), user_home.as_deref())
    }

    /// Validates a resolved store root.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::RelativeHome`] for a relative path.
    pub fn new(path: PathBuf) -> Result<Self, StoreError> {
        let user_home = if path.strip_prefix("~").is_ok() {
            Some(dirs::home_dir().ok_or(StoreError::HomeUnavailable)?)
        } else {
            None
        };
        Self::new_with_home(path, user_home.as_deref())
    }

    fn new_with_home(path: PathBuf, user_home: Option<&Path>) -> Result<Self, StoreError> {
        let path = expand_tilde(path, user_home)?;
        if !path.is_absolute() {
            return Err(StoreError::RelativeHome(path));
        }
        Ok(Self(path))
    }

    /// Returns the root path.
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn resolve_in(
        temporary: &TempDir,
        explicit: Option<&Path>,
        configured: Option<&OsStr>,
    ) -> Result<StoreHome, StoreError> {
        resolve(explicit, configured, Some(temporary.path()))
    }

    #[test]
    fn explicit_path_has_priority() {
        let temporary = TempDir::new().unwrap();
        std::fs::create_dir_all(temporary.path().join(".ionoray/indices")).unwrap();
        let configured = OsStr::new("/tmp/ionoray-environment");
        let home = resolve_in(
            &temporary,
            Some(Path::new("/tmp/ionoray-explicit")),
            Some(configured),
        )
        .unwrap();
        assert_eq!(home.as_path(), Path::new("/tmp/ionoray-explicit"));
    }

    #[test]
    fn configured_path_has_priority_over_default_and_legacy_layout() {
        let temporary = TempDir::new().unwrap();
        std::fs::create_dir_all(temporary.path().join(".ionoray/indices")).unwrap();
        let home = resolve_in(
            &temporary,
            None,
            Some(OsStr::new("/tmp/ionoray-environment")),
        )
        .unwrap();
        assert_eq!(home.as_path(), Path::new("/tmp/ionoray-environment"));
    }

    #[test]
    fn default_uses_new_root_without_a_legacy_layout_or_directory_creation() {
        let temporary = TempDir::new().unwrap();
        let expected = temporary.path().join(".ionoray/geospace-rs");
        let home = resolve_in(&temporary, None, None).unwrap();
        assert_eq!(home.as_path(), expected);
        assert!(!expected.exists());
    }

    #[test]
    fn legacy_indices_require_an_explicit_selection() {
        let temporary = TempDir::new().unwrap();
        let legacy = temporary.path().join(".ionoray");
        std::fs::create_dir_all(legacy.join("indices")).unwrap();
        let error = resolve_in(&temporary, None, None).unwrap_err();
        assert!(matches!(
            error,
            StoreError::LegacyHomeNeedsSelection {
                legacy_home,
                default_home,
            } if legacy_home == legacy && default_home == legacy.join("geospace-rs")
        ));
    }

    #[test]
    fn legacy_objects_require_selection_even_when_the_new_root_exists() {
        let temporary = TempDir::new().unwrap();
        let legacy = temporary.path().join(".ionoray");
        std::fs::create_dir_all(legacy.join("objects")).unwrap();
        std::fs::create_dir_all(legacy.join("geospace-rs")).unwrap();
        assert!(matches!(
            resolve_in(&temporary, None, None),
            Err(StoreError::LegacyHomeNeedsSelection { .. })
        ));
    }

    #[test]
    fn legacy_layout_marker_is_ambiguous_even_when_it_is_not_a_directory() {
        let temporary = TempDir::new().unwrap();
        let legacy = temporary.path().join(".ionoray");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::File::create(legacy.join("objects")).unwrap();
        assert!(matches!(
            resolve_in(&temporary, None, None),
            Err(StoreError::LegacyHomeNeedsSelection { .. })
        ));
    }

    #[test]
    fn tilde_paths_expand_against_the_selected_user_home() {
        let temporary = TempDir::new().unwrap();
        let home = resolve_in(&temporary, Some(Path::new("~/chosen-home")), None).unwrap();
        assert_eq!(home.as_path(), temporary.path().join("chosen-home"));
    }

    #[test]
    fn relative_environment_path_is_rejected() {
        let temporary = TempDir::new().unwrap();
        assert!(matches!(
            resolve_in(&temporary, None, Some(OsStr::new("relative"))),
            Err(StoreError::RelativeHome(_))
        ));
    }

    #[test]
    fn absolute_explicit_and_configured_paths_do_not_require_a_user_home() {
        let explicit = resolve(Some(Path::new("/tmp/ionoray-explicit")), None, None).unwrap();
        assert_eq!(explicit.as_path(), Path::new("/tmp/ionoray-explicit"));

        let configured = resolve(None, Some(OsStr::new("/tmp/ionoray-environment")), None).unwrap();
        assert_eq!(configured.as_path(), Path::new("/tmp/ionoray-environment"));
    }

    #[test]
    fn rejects_relative_path() {
        let temporary = TempDir::new().unwrap();
        assert!(matches!(
            resolve_in(&temporary, Some(Path::new("relative")), None),
            Err(StoreError::RelativeHome(_))
        ));
    }
}
