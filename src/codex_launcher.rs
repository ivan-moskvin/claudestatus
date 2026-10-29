//! Reversible launcher for interactive Codex sessions.

use std::fs;
#[cfg(unix)]
use std::fs::OpenOptions;
use std::io::Read;
#[cfg(unix)]
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::Outcome;

const MARKER: &str = "#!/bin/sh\n# clistatus codex launcher v1\n";

struct Paths {
    launcher: PathBuf,
    backup: PathBuf,
    renderer: PathBuf,
}

fn paths() -> Result<Paths, String> {
    let home = dirs::home_dir().ok_or("Home directory not found")?;
    let data = dirs::data_local_dir().ok_or("Local data directory not found")?;
    let bin = home.join(".local/bin");
    Ok(Paths {
        launcher: bin.join("codex"),
        backup: bin.join("codex.before-clistatus"),
        renderer: data.join("clistatus-codex/renderer"),
    })
}

fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn owned(path: &Path) -> bool {
    if !fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_file()) {
        return false;
    }
    let mut header = vec![0; MARKER.len()];
    fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .is_ok()
        && header == MARKER.as_bytes()
}

fn quote(path: &Path) -> Result<String, String> {
    let value = path.to_str().ok_or("Launcher path is not UTF-8")?;
    Ok(format!("'{}'", value.replace('\'', "'\\''")))
}

#[cfg(unix)]
fn atomic_write(path: &Path, bytes: &[u8]) -> Outcome {
    use std::os::unix::fs::PermissionsExt;
    let parent = path.parent().ok_or("Invalid launcher path")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let tmp = parent.join(format!(
        ".clistatus-{}-{}.tmp",
        std::process::id(),
        path.file_name()
            .ok_or("Invalid filename")?
            .to_string_lossy()
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|e| e.to_string())?;
        file.write_all(bytes).map_err(|e| e.to_string())?;
        file.set_permissions(fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        fs::rename(&tmp, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}

#[cfg(not(unix))]
fn atomic_write(_path: &Path, _bytes: &[u8]) -> Outcome {
    Err("Codex launcher requires Unix".into())
}

fn install_at(source: &Path, paths: &Paths) -> Outcome {
    let already_owned = owned(&paths.launcher);
    if already_owned {
        if !exists(&paths.backup) {
            return Err("Codex launcher backup is missing".into());
        }
    } else {
        if exists(&paths.backup) {
            return Err("Codex launcher backup already exists; resolve it manually".into());
        }
        let meta = fs::symlink_metadata(&paths.launcher)
            .map_err(|_| "Official Codex launcher not found".to_string())?;
        if !meta.file_type().is_symlink() {
            return Err("Refusing to replace a non-symlink Codex launcher".into());
        }
        let target = fs::read_link(&paths.launcher).map_err(|e| e.to_string())?;
        if !target
            .to_string_lossy()
            .contains(".codex/packages/standalone/")
            || target.file_name().is_none_or(|n| n != "codex")
        {
            return Err("Refusing to replace an unrecognized Codex launcher".into());
        }
    }
    let script = format!(
        "{MARKER}case \"${{1-}}\" in\n  exec|e|app-server|update|login|logout|mcp|plugin|completion|features|doctor|debug|remote-control|exec-server) exec {} \"$@\" ;;\nesac\nexec {} codex \"$@\"\n",
        quote(&paths.backup)?,
        quote(&paths.renderer)?
    );
    if source != paths.renderer {
        let binary = fs::read(source).map_err(|e| e.to_string())?;
        atomic_write(&paths.renderer, &binary)?;
    }
    if !already_owned {
        fs::rename(&paths.launcher, &paths.backup).map_err(|e| e.to_string())?;
    }
    if let Err(error) = atomic_write(&paths.launcher, script.as_bytes()) {
        if !already_owned {
            let _ = fs::rename(&paths.backup, &paths.launcher);
        }
        return Err(error);
    }
    Ok(())
}

fn uninstall_at(paths: &Paths) -> Outcome {
    if !exists(&paths.launcher) && !exists(&paths.backup) {
        return Ok(());
    }
    if !owned(&paths.launcher) {
        if exists(&paths.backup) {
            return Err("Codex launcher changed; backup left untouched".into());
        }
        return Ok(());
    }
    if !exists(&paths.backup) {
        return Err("Codex launcher backup is missing".into());
    }
    fs::rename(&paths.backup, &paths.launcher).map_err(|e| e.to_string())
}

pub fn install(renderer_source: &Path) -> Outcome {
    let paths = paths()?;
    install_at(renderer_source, &paths)?;
    crate::codex_agent_hooks::install(&paths.renderer)
}

pub fn uninstall() -> Outcome {
    let paths = paths()?;
    uninstall_at(&paths)?;
    crate::codex_agent_hooks::uninstall(&paths.renderer)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);
    fn fixture() -> (PathBuf, Paths, PathBuf) {
        let base = std::env::temp_dir().join(format!(
            "clistatus launcher's test {} {}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let official = base.join(".codex/packages/standalone/current/bin/codex");
        fs::create_dir_all(official.parent().unwrap()).unwrap();
        fs::write(
            &official,
            "#!/bin/sh\nprintf 'official\\n'; printf '<%s>\\n' \"$@\"\n",
        )
        .unwrap();
        fs::set_permissions(&official, fs::Permissions::from_mode(0o755)).unwrap();
        let paths = Paths {
            launcher: base.join("bin/codex"),
            backup: base.join("bin/codex.before-clistatus"),
            renderer: base.join("data/renderer"),
        };
        fs::create_dir_all(paths.launcher.parent().unwrap()).unwrap();
        symlink(&official, &paths.launcher).unwrap();
        let source = base.join("source");
        fs::write(
            &source,
            "#!/bin/sh\nprintf 'renderer\\n'; printf '<%s>\\n' \"$@\"\n",
        )
        .unwrap();
        (base, paths, source)
    }

    #[test]
    fn install_routes_and_restores_original_symlink() {
        let (base, paths, source) = fixture();
        let original = fs::read_link(&paths.launcher).unwrap();
        install_at(&source, &paths).unwrap();
        install_at(&source, &paths).unwrap();
        let out = Command::new(&paths.launcher)
            .args(["resume", "a b", "x'y"])
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8(out.stdout).unwrap(),
            "renderer\n<codex>\n<resume>\n<a b>\n<x'y>\n"
        );
        let out = Command::new(&paths.launcher)
            .args(["update", "a b"])
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8(out.stdout).unwrap(),
            "official\n<update>\n<a b>\n"
        );
        uninstall_at(&paths).unwrap();
        assert_eq!(fs::read_link(&paths.launcher).unwrap(), original);
        uninstall_at(&paths).unwrap();
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn foreign_launcher_is_never_replaced() {
        let (base, paths, source) = fixture();
        fs::remove_file(&paths.launcher).unwrap();
        fs::write(&paths.launcher, "foreign").unwrap();
        assert!(install_at(&source, &paths).is_err());
        assert_eq!(fs::read(&paths.launcher).unwrap(), b"foreign");
        fs::remove_dir_all(base).unwrap();
    }
}
