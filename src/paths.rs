//! Where the utility keeps its files.
//!
//! Not in `~/.claude`: that directory belongs to Claude Code, and putting
//! foreign state there means littering under its feet. Every system offers its
//! own place for application settings, and that is the one we take.

use std::path::{Path, PathBuf};

use crate::i18n::tf;

const APP: &str = "clistatus";

/// The directory of our files, created if it is not there yet:
/// macOS `~/Library/Application Support/clistatus`, Windows
/// `%AppData%\clistatus`, Linux `~/.config/clistatus`.
pub fn dir() -> Result<PathBuf, String> {
    let base = dirs::config_dir().ok_or_else(|| {
        tf!(
            "could not determine our own path: {0}",
            "no config directory"
        )
    })?;
    let dir = base.join(APP);
    migrate_dir(&base.join("claudestatus"), &dir)?;
    std::fs::create_dir_all(&dir)
        .map_err(|err| tf!("could not create {0}: {1}", dir.display(), err))?;
    Ok(dir)
}

/// The path to one of our files.
pub fn file(name: &str) -> Result<PathBuf, String> {
    let dir = dir()?;
    Ok(adopt_legacy(&dir, dirs::home_dir().as_deref(), name))
}

/// The cache of the update check. It is not a setting: losing it costs nothing,
/// so it lives where the system puts throwaway state.
pub fn cache_dir() -> Result<PathBuf, String> {
    let base = dirs::cache_dir().ok_or_else(|| {
        tf!(
            "could not determine our own path: {0}",
            "no cache directory"
        )
    })?;
    let dir = base.join(APP);
    migrate_dir(&base.join("claudestatus"), &dir)?;
    Ok(dir)
}

fn migrate_dir(previous: &Path, current: &Path) -> Result<(), String> {
    if previous.is_dir() && !current.exists() {
        std::fs::rename(previous, current)
            .map_err(|err| tf!("could not migrate {0}: {1}", previous.display(), err))?;
    }
    Ok(())
}

/// Files of earlier versions are moved out of `~/.claude`: settings must not
/// disappear because of a relocation.
fn adopt_legacy(dir: &Path, home: Option<&Path>, name: &str) -> PathBuf {
    let path = dir.join(name);
    if path.exists() {
        return path;
    }
    if let Some(base) = dir.parent() {
        let previous = base.join("claudestatus").join(name);
        if previous.is_file() && std::fs::rename(&previous, &path).is_ok() {
            return path;
        }
    }
    if let Some(home) = home {
        let legacy = home.join(".claude").join(name);
        if legacy.exists() {
            let _ = std::fs::rename(&legacy, &path);
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory of this test alone, removed when the test is over.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("clistatus-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn join(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn takes_a_file_of_an_earlier_version_along() {
        let scratch = Scratch::new("legacy");
        let (home, dir) = (scratch.join("home"), scratch.join("app"));
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(home.join(".claude").join("divoom.json"), "{}").unwrap();

        let path = adopt_legacy(&dir, Some(&home), "divoom.json");

        assert_eq!(path, dir.join("divoom.json"));
        assert!(path.exists(), "the file was not moved out of ~/.claude");
        assert!(!home.join(".claude").join("divoom.json").exists());
    }

    #[test]
    fn leaves_the_file_we_already_have_alone() {
        let scratch = Scratch::new("ours");
        let (home, dir) = (scratch.join("home"), scratch.join("app"));
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("divoom.json"), "ours").unwrap();
        std::fs::write(home.join(".claude").join("divoom.json"), "older").unwrap();

        let path = adopt_legacy(&dir, Some(&home), "divoom.json");

        assert_eq!(std::fs::read_to_string(path).unwrap(), "ours");
    }

    #[test]
    fn names_a_file_that_is_nowhere_yet() {
        let scratch = Scratch::new("fresh");
        let dir = scratch.join("app");
        std::fs::create_dir_all(&dir).unwrap();

        let path = adopt_legacy(&dir, None, "divoom.json");

        assert_eq!(path, dir.join("divoom.json"));
        assert!(!path.exists());
    }
}
