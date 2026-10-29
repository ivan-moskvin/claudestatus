//! Installation and execution of the pinned Codex status-line transport.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use flate2::read::GzDecoder;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::i18n::tf;
use crate::{Outcome, update};

mod rows;

const VERSION: &str = "0.165.0";
const BASE: &str = "https://github.com/adrijshikhar/cxstatusline/releases/download/codex-v0.165.0";
const MANIFEST_SHA: &str = "8defa35034b0feba33f1c90ab4834e176b51cc81f3a8277f1acf793949c902fa";
const MANIFEST_NAME: &str = ".clistatus-manifest.json";
const ARCHIVE_LIMIT: u64 = 512 * 1024 * 1024;
const FILE_LIMIT: u64 = 1024 * 1024 * 1024;

#[derive(Deserialize)]
struct Manifest {
    schema: u64,
    #[serde(rename = "codexVersion")]
    codex_version: String,
    artifacts: Vec<Artifact>,
}

#[derive(Deserialize)]
struct Artifact {
    platform: String,
    filename: String,
    sha256: String,
    size: u64,
    files: BTreeMap<String, FileDigest>,
}

#[derive(Deserialize)]
struct FileDigest {
    sha256: String,
    size: u64,
}

fn platform() -> Result<(&'static str, &'static str, u64), String> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok((
            "darwin-arm64",
            "d8a42b9a71dfbb40407cd5eceb048434ff2cfc0fb964a71fccd5c5673716a737",
            140496807,
        )),
        ("linux", "aarch64") => Ok((
            "linux-arm64",
            "fc2c46e6ff7f85f6b272c98f14888e5c1f2179094424c3a6477df97153f96aaf",
            164220266,
        )),
        (os, arch) => Err(tf!("Unsupported Codex platform: {0}/{1}", os, arch)),
    }
}

fn digest(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn parse_manifest(
    data: &[u8],
    platform: &str,
    expected_sha: &str,
    expected_size: u64,
) -> Result<Artifact, String> {
    if digest(data) != MANIFEST_SHA {
        return Err("Codex manifest checksum mismatch".into());
    }
    let manifest: Manifest = serde_json::from_slice(data).map_err(|e| e.to_string())?;
    if manifest.schema != 3 || manifest.codex_version != VERSION {
        return Err("Unsupported Codex manifest".into());
    }
    let mut found = manifest
        .artifacts
        .into_iter()
        .filter(|a| a.platform == platform);
    let artifact = found.next().ok_or("Codex platform missing from manifest")?;
    if found.next().is_some()
        || artifact.filename != format!("cxstatusline-codex-{VERSION}-{platform}.tar.gz")
        || artifact.sha256 != expected_sha
        || artifact.size != expected_size
        || artifact.size > ARCHIVE_LIMIT
        || artifact.files.is_empty()
        || artifact.files.len() > 64
        || !artifact.files.contains_key("bin/codex")
        || !artifact.files.contains_key("bin/codex-code-mode-host")
        || artifact.files.iter().any(|(p, d)| {
            !safe_path(Path::new(p))
                || p == MANIFEST_NAME
                || d.size > FILE_LIMIT
                || d.sha256.len() != 64
        })
        || artifact
            .files
            .values()
            .try_fold(0u64, |n, d| n.checked_add(d.size))
            .is_none_or(|n| n > FILE_LIMIT)
    {
        return Err("Invalid Codex artifact manifest".into());
    }
    Ok(artifact)
}

fn safe_path(path: &Path) -> bool {
    !path.as_os_str().is_empty() && path.components().all(|c| matches!(c, Component::Normal(_)))
}

fn fetch(url: &str, limit: u64) -> Result<Vec<u8>, String> {
    let mut response = ureq::get(url)
        .header("User-Agent", "clistatus")
        .config()
        .timeout_global(Some(Duration::from_secs(300)))
        .build()
        .call()
        .map_err(|e| e.to_string())?;
    response
        .body_mut()
        .with_config()
        .limit(limit)
        .read_to_vec()
        .map_err(|e| e.to_string())
}

fn hash_file(path: &Path) -> Result<(String, u64), String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut size = 0u64;
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        size += n as u64;
        if size > FILE_LIMIT {
            return Err("Codex file exceeds size limit".into());
        }
        hash.update(&buf[..n]);
    }
    Ok((hex(&hash.finalize()), size))
}

fn extract(data: &[u8], artifact: &Artifact, dir: &Path) -> Outcome {
    if data.len() as u64 != artifact.size || digest(data) != artifact.sha256 {
        return Err("Codex archive checksum mismatch".into());
    }
    let mut archive = tar::Archive::new(GzDecoder::new(data));
    let mut seen = BTreeSet::new();
    let mut total = 0u64;
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        if !entry.header().entry_type().is_file() {
            return Err("Invalid Codex archive entry".into());
        }
        let path = entry.path().map_err(|e| e.to_string())?.into_owned();
        if !safe_path(&path) {
            return Err("Unsafe Codex archive path".into());
        }
        let name = path.to_str().ok_or("Invalid Codex archive path")?;
        let expected = artifact
            .files
            .get(name)
            .ok_or("Unlisted Codex archive file")?;
        if !seen.insert(name.to_owned()) || seen.len() > 64 || entry.size() != expected.size {
            return Err("Invalid Codex archive file size or duplicate".into());
        }
        total = total
            .checked_add(entry.size())
            .ok_or("Codex archive too large")?;
        if total > FILE_LIMIT {
            return Err("Codex archive too large".into());
        }
        let dest = dir.join(&path);
        fs::create_dir_all(dest.parent().ok_or("Invalid path")?).map_err(|e| e.to_string())?;
        let mut out = File::create(&dest).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
        out.flush().map_err(|e| e.to_string())?;
        if hash_file(&dest)? != (expected.sha256.clone(), expected.size) {
            return Err("Codex file checksum mismatch".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = if entry.header().mode().map_err(|e| e.to_string())? & 0o111 != 0 {
                0o755
            } else {
                0o644
            };
            fs::set_permissions(&dest, fs::Permissions::from_mode(mode))
                .map_err(|e| e.to_string())?;
        }
    }
    if seen.len() != artifact.files.len() {
        return Err("Codex archive is incomplete".into());
    }
    Ok(())
}

fn verify_tree(dir: &Path, artifact: &Artifact) -> Outcome {
    fn walk(root: &Path, dir: &Path, seen: &mut BTreeSet<String>) -> Outcome {
        for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let ty = entry.file_type().map_err(|e| e.to_string())?;
            let path = entry.path();
            if ty.is_dir() {
                walk(root, &path, seen)?;
            } else if ty.is_file() {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|e| e.to_string())?
                    .to_str()
                    .ok_or("Invalid path")?
                    .to_owned();
                if relative != MANIFEST_NAME {
                    seen.insert(relative);
                }
            } else {
                return Err("Invalid file in Codex package".into());
            }
        }
        Ok(())
    }
    let mut seen = BTreeSet::new();
    walk(dir, dir, &mut seen)?;
    if seen.len() != artifact.files.len() {
        return Err("Codex package file list mismatch".into());
    }
    for (name, expected) in &artifact.files {
        if !seen.contains(name)
            || hash_file(&dir.join(name))? != (expected.sha256.clone(), expected.size)
        {
            return Err("Codex package checksum mismatch".into());
        }
    }
    Ok(())
}

fn verified_package(dir: &Path, platform: &str, sha: &str, size: u64) -> Outcome {
    let manifest = fs::read(dir.join(MANIFEST_NAME)).map_err(|e| e.to_string())?;
    let artifact = parse_manifest(&manifest, platform, sha, size)?;
    verify_tree(dir, &artifact)
}

fn package_path(platform: &str) -> Result<PathBuf, String> {
    let root = dirs::data_local_dir().ok_or("Could not locate application data directory")?;
    Ok(root
        .join("clistatus-codex")
        .join(format!("{VERSION}-{platform}")))
}

pub fn install() -> Outcome {
    let (platform, sha, size) = platform()?;
    let target = package_path(platform)?;
    if target.exists() {
        verified_package(&target, platform, sha, size)?;
        rows::prepare(&target, platform, sha, size)?;
        crate::codex_launcher::install(&update::self_path()?)?;
        println!(
            "{}",
            tf!("Codex footer connected. Start a new session with: codex")
        );
        return Ok(());
    }
    let manifest = fetch(&format!("{BASE}/manifest.json"), 1024 * 1024)?;
    let artifact = parse_manifest(&manifest, platform, sha, size)?;
    println!(
        "{}",
        tf!(
            "Downloading Codex footer transport ({0} MB)...",
            size / 1_000_000
        )
    );
    let archive = fetch(&format!("{BASE}/{}", artifact.filename), ARCHIVE_LIMIT)?;
    let parent = target.parent().ok_or("Invalid Codex package path")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let stage = (0..1000)
        .find_map(|n| {
            let path = parent.join(format!(".stage-{}-{n}", std::process::id()));
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&path) {
                Ok(()) => Some(path),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => None,
                Err(_) => None,
            }
        })
        .ok_or("Could not create Codex staging directory")?;
    let result: Outcome = (|| {
        extract(&archive, &artifact, &stage)?;
        fs::write(stage.join(MANIFEST_NAME), &manifest).map_err(|e| e.to_string())?;
        verified_package(&stage, platform, sha, size)?;
        fs::rename(&stage, &target).map_err(|e| e.to_string())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&stage);
    }
    result?;
    rows::prepare(&target, platform, sha, size)?;
    crate::codex_launcher::install(&update::self_path()?)?;
    println!(
        "{}",
        tf!("Codex footer connected. Start a new session with: codex")
    );
    Ok(())
}

#[cfg(unix)]
pub fn launch(args: &[String]) -> Outcome {
    use std::os::unix::process::CommandExt;
    let (platform, sha, size) = platform()?;
    let dir = package_path(platform)?;
    verified_package(&dir, platform, sha, size)
        .map_err(|e| tf!("Codex transport is unavailable: {0}", e))?;
    let dir = rows::prepare(&dir, platform, sha, size)?;
    let exe = update::self_path()?;
    let quoted = format!("'{}'", exe.to_string_lossy().replace('\'', "'\\''"));
    let err = std::process::Command::new(dir.join("bin/codex"))
        .args(args)
        .env("CXSTATUSLINE_COMMAND", format!("{quoted} render codex"))
        .exec();
    Err(tf!("Could not launch Codex: {0}", err))
}

#[cfg(not(unix))]
pub fn launch(_args: &[String]) -> Outcome {
    Err("Unsupported Codex platform".into())
}

pub fn uninstall() -> Outcome {
    crate::codex_launcher::uninstall()?;
    println!(
        "{}",
        tf!("Original Codex launcher restored. The transport remains cached.")
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::GzEncoder};

    fn fixture() -> (Vec<u8>, Artifact) {
        let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
        let mut files = BTreeMap::new();
        for (name, data) in [
            ("bin/codex", b"codex".as_slice()),
            ("bin/codex-code-mode-host", b"host".as_slice()),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, name, data).unwrap();
            files.insert(
                name.to_string(),
                FileDigest {
                    sha256: digest(data),
                    size: data.len() as u64,
                },
            );
        }
        let data = builder.into_inner().unwrap().finish().unwrap();
        let artifact = Artifact {
            platform: "test".into(),
            filename: "test".into(),
            sha256: digest(&data),
            size: data.len() as u64,
            files,
        };
        (data, artifact)
    }

    pub(super) fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "clistatus-codex-test-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn extracts_and_revalidates_all_files() {
        let dir = scratch("valid");
        let (archive, artifact) = fixture();
        extract(&archive, &artifact, &dir).unwrap();
        verify_tree(&dir, &artifact).unwrap();
        fs::write(dir.join("bin/codex"), b"changed").unwrap();
        assert!(verify_tree(&dir, &artifact).is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_bad_digests_malformed_archive_and_unsafe_paths() {
        let dir = scratch("invalid");
        let (archive, mut artifact) = fixture();
        artifact.sha256 = "0".repeat(64);
        assert!(extract(&archive, &artifact, &dir).is_err());
        let broken = b"not a gzip archive";
        artifact.sha256 = digest(broken);
        artifact.size = broken.len() as u64;
        assert!(extract(broken, &artifact, &dir).is_err());
        assert!(!safe_path(Path::new("../outside")));
        assert!(!safe_path(Path::new("/outside")));
        assert!(parse_manifest(b"{}", "darwin-arm64", "", 0).is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_archive_links() {
        let dir = scratch("link");
        let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
        let mut header = tar::Header::new_gnu();
        header.set_size(0);
        header.set_mode(0o755);
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_link_name("outside").unwrap();
        header.set_cksum();
        builder
            .append_data(&mut header, "bin/codex", &b""[..])
            .unwrap();
        let archive = builder.into_inner().unwrap().finish().unwrap();
        let (_, mut artifact) = fixture();
        artifact.sha256 = digest(&archive);
        artifact.size = archive.len() as u64;
        assert!(extract(&archive, &artifact, &dir).is_err());
        let _ = fs::remove_dir_all(dir);
    }
}
