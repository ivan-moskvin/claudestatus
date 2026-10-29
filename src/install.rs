//! Registering the status line in the settings of Claude Code, and clearing
//! everything behind us.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::i18n::tf;
use crate::{Outcome, paths, update};

/// Registers the binary that is running right now.
pub fn install() -> Outcome {
    let exe = update::self_path()?;
    let path = settings_path()?;

    let Some(report) = write_settings(&path, &exe)? else {
        return Ok(());
    };
    if let Some(previous) = report {
        println!(
            "{}",
            tf!("Replacing the previous status line: {0}", previous)
        );
        println!(
            "{}",
            tf!("The previous settings are in {0}.bak", path.display())
        );
    }

    println!("{}", tf!("Status line registered in {0}", path.display()));
    warn_if_not_in_path(&exe);
    Ok(())
}

/// Writes the command into `statusLine` of settings.json, backing the file up
/// first. Answers with the status line it replaced, if there was one worth
/// mentioning.
fn write_settings(path: &Path, exe: &Path) -> Result<Option<Option<String>>, String> {
    let mut settings = read_settings(path)?;

    if path.exists() {
        // Back up before writing and only when the file exists — otherwise we
        // would overwrite somebody's settings with no way back.
        let data =
            std::fs::read(path).map_err(|err| tf!("could not read settings.json: {0}", err))?;
        std::fs::write(with_suffix(path, ".bak"), data)
            .map_err(|err| tf!("could not back up settings.json: {0}", err))?;
    }

    // Quoted, in case the path to the binary has spaces in it.
    let command = quoted(exe);
    let replaced = settings
        .get("statusLine")
        .and_then(|line| line.get("command"))
        .and_then(Value::as_str)
        .filter(|previous| !previous.is_empty() && *previous != command)
        .map(str::to_string);

    settings.insert(
        "statusLine".into(),
        serde_json::json!({"type": "command", "command": command}),
    );

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|err| tf!("could not create {0}: {1}", dir.display(), err))?;
    }
    write_json(path, &settings)?;
    Ok(Some(replaced))
}

/// Clears everything behind us: the settings entry, the check cache and the
/// binary. The order matters — settings first, or Claude Code manages to call
/// an already deleted file.
pub fn uninstall() -> Outcome {
    let exe = update::self_path()?;
    let path = settings_path()?;

    match remove_from_settings(&path, &exe)? {
        Removal::Gone => println!(
            "{}",
            tf!(
                "Status line removed from {0} (the previous settings are in {0}.bak)",
                path.display()
            )
        ),
        Removal::NoSettings => println!(
            "{}",
            tf!(
                "There is no {0} — nothing to clean in the settings",
                path.display()
            )
        ),
        Removal::NoStatusLine => {
            println!("{}", tf!("There is no status line in {0}", path.display()))
        }
        Removal::SomebodyElses(command) => {
            println!(
                "{}",
                tf!(
                    "{0} holds a status line that is not ours — leaving it as is:\n  {1}",
                    path.display(),
                    command
                )
            );
        }
    }

    // The bridge is a separate process and outlives the deleted binary unless it
    // is stopped: the panel stays on the device with nobody left to update it.
    crate::panel::stop();

    // The whole application directory: usage snapshot, bridge settings, pid file.
    if let Ok(dir) = paths::dir()
        && std::fs::remove_dir_all(&dir).is_ok()
    {
        println!("{}", tf!("Removed {0}", dir.display()));
    }
    if let Ok(dir) = paths::cache_dir()
        && std::fs::remove_dir_all(&dir).is_ok()
    {
        println!("{}", tf!("Removed the cache {0}", dir.display()));
    }

    // A running binary can be deleted in place everywhere but on Windows.
    match std::fs::remove_file(&exe) {
        Ok(()) => println!("{}", tf!("Removed the binary {0}", exe.display())),
        Err(_) => println!(
            "{}",
            tf!(
                "The binary is still there — remove it by hand: {0}",
                exe.display()
            )
        ),
    }
    Ok(())
}

/// What became of the status line in the settings.
#[derive(Debug, PartialEq, Eq)]
enum Removal {
    Gone,
    NoSettings,
    NoStatusLine,
    /// Somebody else's line is left alone: replacing it was the user's
    /// decision, and we have nothing to put back.
    SomebodyElses(String),
}

fn remove_from_settings(path: &Path, exe: &Path) -> Result<Removal, String> {
    if !path.exists() {
        return Ok(Removal::NoSettings);
    }
    let mut settings = read_settings(path)?;

    let Some(command) = settings
        .get("statusLine")
        .and_then(|line| line.get("command"))
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return Ok(Removal::NoStatusLine);
    };
    if command != quoted(exe) {
        return Ok(Removal::SomebodyElses(command));
    }

    let data = std::fs::read(path).map_err(|err| tf!("could not read settings.json: {0}", err))?;
    std::fs::write(with_suffix(path, ".bak"), data)
        .map_err(|err| tf!("could not back up settings.json: {0}", err))?;

    settings.remove("statusLine");
    write_json(path, &settings)?;
    Ok(Removal::Gone)
}

fn read_settings(path: &Path) -> Result<Map<String, Value>, String> {
    let data = match std::fs::read(path) {
        Ok(data) => data,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(err) => return Err(tf!("could not read settings.json: {0}", err)),
    };
    if data.iter().all(u8::is_ascii_whitespace) {
        return Ok(Map::new());
    }
    serde_json::from_slice(&data)
        .map_err(|_| tf!("{0} does not parse — fix it by hand", path.display()))
}

fn write_json(path: &Path, settings: &Map<String, Value>) -> Result<(), String> {
    let mut data = serde_json::to_vec_pretty(settings).map_err(|err| err.to_string())?;
    data.push(b'\n');
    std::fs::write(path, data).map_err(|err| tf!("could not write {0}: {1}", path.display(), err))
}

fn quoted(exe: &Path) -> String {
    format!("{:?}", exe.display().to_string())
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

fn settings_path() -> Result<PathBuf, String> {
    let home = dirs::home_dir()
        .ok_or_else(|| tf!("could not determine our own path: {0}", "no home directory"))?;
    Ok(home.join(".claude").join("settings.json"))
}

/// Claude Code calls the binary by its absolute path and does fine without
/// PATH, but the user types `clistatus` by hand.
fn warn_if_not_in_path(exe: &Path) {
    let Some(dir) = exe.parent() else { return };
    let path = std::env::var_os("PATH").unwrap_or_default();

    if std::env::split_paths(&path).any(|entry| same_entry(&entry, dir)) {
        return;
    }
    println!(
        "{}",
        tf!(
            "\n{0} is not in PATH — the clistatus command will not be found.",
            dir.display()
        )
    );
    println!("{}", path_hint(dir));
}

/// Compares an entry of PATH against a directory.
#[cfg(unix)]
fn same_entry(entry: &Path, dir: &Path) -> bool {
    entry == dir
}

/// Windows spells the same directory in more than one case, and an entry may
/// carry a trailing separator.
#[cfg(windows)]
fn same_entry(entry: &Path, dir: &Path) -> bool {
    let tidy = |path: &Path| {
        path.to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .to_lowercase()
            .replace('/', "\\")
    };
    tidy(entry) == tidy(dir)
}

/// How to put the directory into PATH so that it stays there.
#[cfg(unix)]
fn path_hint(dir: &Path) -> String {
    tf!(
        "Line for ~/.zshrc:  export PATH=\"{0}:$PATH\"",
        dir.display()
    )
}

#[cfg(windows)]
fn path_hint(dir: &Path) -> String {
    tf!(
        "Command for PowerShell:  [Environment]::SetEnvironmentVariable(\"Path\", [Environment]::GetEnvironmentVariable(\"Path\", \"User\") + \";{0}\", \"User\")",
        dir.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("clistatus-settings-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn settings(&self) -> PathBuf {
            self.0.join("settings.json")
        }

        fn write(&self, content: &str) -> PathBuf {
            let path = self.settings();
            std::fs::write(&path, content).unwrap();
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn read(path: &Path) -> Value {
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    #[test]
    fn writes_the_status_line_into_settings_that_were_not_there() {
        let scratch = Scratch::new("fresh");
        let path = scratch.settings();
        let exe = Path::new("/usr/local/bin/clistatus");

        assert_eq!(write_settings(&path, exe).unwrap(), Some(None));

        let settings = read(&path);
        assert_eq!(settings["statusLine"]["type"], "command");
        assert_eq!(
            settings["statusLine"]["command"],
            "\"/usr/local/bin/clistatus\""
        );
    }

    #[test]
    fn keeps_every_setting_it_did_not_come_for() {
        let scratch = Scratch::new("keeps");
        let path = scratch.write(r#"{"theme":"dark","permissions":{"allow":["Bash"]}}"#);

        write_settings(&path, Path::new("/bin/clistatus")).unwrap();

        let settings = read(&path);
        assert_eq!(settings["theme"], "dark");
        assert_eq!(settings["permissions"]["allow"][0], "Bash");
        assert!(settings["statusLine"].is_object());
    }

    #[test]
    fn backs_the_settings_up_before_writing() {
        let scratch = Scratch::new("backup");
        let path = scratch.write(r#"{"theme":"dark"}"#);

        write_settings(&path, Path::new("/bin/clistatus")).unwrap();

        let backup = std::fs::read_to_string(with_suffix(&path, ".bak")).unwrap();
        assert_eq!(backup, r#"{"theme":"dark"}"#);
    }

    #[test]
    fn reports_the_status_line_it_overwrites() {
        let scratch = Scratch::new("replace");
        let path =
            scratch.write(r#"{"statusLine":{"type":"command","command":"starship prompt"}}"#);

        let replaced = write_settings(&path, Path::new("/bin/clistatus")).unwrap();

        assert_eq!(replaced, Some(Some("starship prompt".to_string())));
    }

    #[test]
    fn says_nothing_about_replacing_itself() {
        let scratch = Scratch::new("again");
        let exe = Path::new("/bin/clistatus");
        let path = scratch.settings();

        write_settings(&path, exe).unwrap();
        assert_eq!(write_settings(&path, exe).unwrap(), Some(None));
    }

    #[test]
    fn refuses_settings_it_cannot_parse() {
        let scratch = Scratch::new("broken");
        let path = scratch.write("{ this is not json");

        let err = write_settings(&path, Path::new("/bin/clistatus")).unwrap_err();

        assert!(err.contains("settings.json"), "{err}");
        // Nothing was written over what we could not understand.
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{ this is not json"
        );
    }

    #[test]
    fn treats_an_empty_file_as_no_settings_at_all() {
        let scratch = Scratch::new("empty");
        let path = scratch.write("   \n");

        write_settings(&path, Path::new("/bin/clistatus")).unwrap();

        assert!(read(&path)["statusLine"].is_object());
    }

    #[test]
    fn takes_our_own_status_line_out() {
        let scratch = Scratch::new("remove");
        let exe = Path::new("/bin/clistatus");
        let path = scratch.settings();
        write_settings(&path, exe).unwrap();

        assert_eq!(remove_from_settings(&path, exe).unwrap(), Removal::Gone);
        assert!(read(&path).get("statusLine").is_none());
    }

    #[test]
    fn leaves_a_status_line_that_is_not_ours() {
        let scratch = Scratch::new("foreign");
        let path =
            scratch.write(r#"{"statusLine":{"type":"command","command":"starship prompt"}}"#);

        let removal = remove_from_settings(&path, Path::new("/bin/clistatus")).unwrap();

        assert_eq!(removal, Removal::SomebodyElses("starship prompt".into()));
        assert_eq!(read(&path)["statusLine"]["command"], "starship prompt");
    }

    #[test]
    fn has_nothing_to_remove_from_settings_that_are_not_there() {
        let scratch = Scratch::new("nosettings");
        let removal =
            remove_from_settings(&scratch.settings(), Path::new("/bin/clistatus")).unwrap();
        assert_eq!(removal, Removal::NoSettings);
    }

    #[test]
    fn has_nothing_to_remove_when_no_status_line_is_set() {
        let scratch = Scratch::new("nostatusline");
        let path = scratch.write(r#"{"theme":"dark"}"#);

        assert_eq!(
            remove_from_settings(&path, Path::new("/bin/x")).unwrap(),
            Removal::NoStatusLine
        );
        assert_eq!(read(&path)["theme"], "dark");
    }

    #[test]
    fn quotes_a_path_with_spaces_in_it() {
        assert_eq!(
            quoted(Path::new("/Users/me/my tools/clistatus")),
            "\"/Users/me/my tools/clistatus\""
        );
    }
}
