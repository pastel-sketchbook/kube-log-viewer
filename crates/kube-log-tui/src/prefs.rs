//! Lightweight user preferences persisted to `prefs.toml`.
//!
//! The file lives under the platform config directory:
//!   - macOS: `~/Library/Application Support/kube-log-viewer/prefs.toml`
//!   - Linux: `~/.config/kube-log-viewer/prefs.toml`
//!
//! All I/O errors are logged and silently ignored so that a corrupt or missing
//! prefs file never prevents the application from starting.

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::ui::theme::THEMES;

/// On-disk representation of user preferences.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Prefs {
    /// The `name` field of the last selected [`Theme`](crate::ui::theme::Theme).
    #[serde(default)]
    pub theme: Option<String>,
}

// ---------------------------------------------------------------------------
// File path
// ---------------------------------------------------------------------------

/// Returns the path to `prefs.toml`, or `None` if the platform config
/// directory cannot be determined.
fn prefs_path() -> Option<std::path::PathBuf> {
    Some(
        dirs::config_dir()?
            .join("kube-log-viewer")
            .join("prefs.toml"),
    )
}

// ---------------------------------------------------------------------------
// Load
// ---------------------------------------------------------------------------

/// Load preferences from disk.  Returns `Prefs::default()` on any failure.
pub fn load() -> Prefs {
    let Some(path) = prefs_path() else {
        debug!("config dir not available; using default prefs");
        return Prefs::default();
    };

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            debug!("prefs file not found at {}, using defaults", path.display());
            return Prefs::default();
        }
        Err(e) => {
            warn!("failed to read prefs file {}: {e}", path.display());
            return Prefs::default();
        }
    };

    match toml::from_str::<Prefs>(&content) {
        Ok(prefs) => {
            debug!("loaded prefs from {}", path.display());
            prefs
        }
        Err(e) => {
            warn!("failed to parse prefs file {}: {e}", path.display());
            Prefs::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Save
// ---------------------------------------------------------------------------

/// Persist preferences to disk.  Errors are logged but not propagated.
pub fn save(prefs: &Prefs) {
    let Some(path) = prefs_path() else {
        warn!("config dir not available; cannot save prefs");
        return;
    };

    // Ensure the parent directory exists.
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        warn!("failed to create config dir {}: {e}", parent.display());
        return;
    }

    let content = match toml::to_string_pretty(prefs) {
        Ok(c) => c,
        Err(e) => {
            warn!("failed to serialize prefs: {e}");
            return;
        }
    };

    if let Err(e) = std::fs::write(&path, content) {
        warn!("failed to write prefs file {}: {e}", path.display());
    } else {
        debug!("saved prefs to {}", path.display());
    }
}

// ---------------------------------------------------------------------------
// Theme helpers
// ---------------------------------------------------------------------------

/// Resolve a saved theme name to its index in [`THEMES`].  Returns `0`
/// (the default dark theme) when the name is `None` or unrecognised.
pub fn theme_index_from_prefs(prefs: &Prefs) -> usize {
    let Some(ref name) = prefs.theme else {
        return 0;
    };
    THEMES.iter().position(|t| t.name == name).unwrap_or(0)
}

/// Build a [`Prefs`] snapshot from the current theme index.
pub fn prefs_from_theme_index(index: usize) -> Prefs {
    let name = THEMES.get(index).map(|t| t.name.to_owned());
    Prefs { theme: name }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_index_default_when_none() {
        let prefs = Prefs { theme: None };
        assert_eq!(theme_index_from_prefs(&prefs), 0);
    }

    #[test]
    fn theme_index_default_when_unknown() {
        let prefs = Prefs {
            theme: Some("NonExistentTheme".into()),
        };
        assert_eq!(theme_index_from_prefs(&prefs), 0);
    }

    #[test]
    fn theme_index_resolves_known_name() {
        // Pick a theme that definitely exists and is not at index 0.
        let target = &THEMES[1];
        let prefs = Prefs {
            theme: Some(target.name.to_owned()),
        };
        assert_eq!(theme_index_from_prefs(&prefs), 1);
    }

    #[test]
    fn round_trip_prefs_from_theme_index() {
        for (i, theme) in THEMES.iter().enumerate() {
            let prefs = prefs_from_theme_index(i);
            assert_eq!(prefs.theme.as_deref(), Some(theme.name));
            assert_eq!(theme_index_from_prefs(&prefs), i);
        }
    }

    #[test]
    fn round_trip_toml_serialization() {
        let prefs = Prefs {
            theme: Some("Gruvbox Dark".into()),
        };
        let toml_str = toml::to_string_pretty(&prefs).expect("serialize");
        let parsed: Prefs = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(parsed.theme.as_deref(), Some("Gruvbox Dark"));
    }

    #[test]
    fn empty_toml_yields_defaults() {
        let parsed: Prefs = toml::from_str("").expect("deserialize");
        assert!(parsed.theme.is_none());
    }
}
