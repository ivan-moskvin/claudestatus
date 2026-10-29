//! Checking for a new version and installing it.
//!
//! We update the same way we were installed: download the ready binary from a
//! release and replace ourselves with it. Neither a toolchain nor a clone of
//! the repository is needed on the machine.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::i18n::tf;
use crate::{Outcome, paths, version};

const REPOSITORY: &str = "ivan-moskvin/clistatus";

/// The first call checks right away — there is no cache yet — and after that no
/// more than once an hour.
const CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);
const NETWORK_TIMEOUT: Duration = Duration::from_secs(20);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Where releases are looked for. A struct rather than a constant so that
/// updating can be tested against a local server without publishing anything.
#[derive(Debug, Clone)]
pub struct Source {
    api: String,
    downloads: String,
}

impl Default for Source {
    fn default() -> Self {
        Self {
            api: format!("https://api.github.com/repos/{REPOSITORY}/releases/latest"),
            downloads: format!("https://github.com/{REPOSITORY}/releases/download/"),
        }
    }
}

/// Lives in the user cache: it is not a setting, losing it costs nothing.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Cache {
    #[serde(default)]
    checked_at: u64,
    #[serde(default)]
    latest: String,
}

/// Asks GitHub for the latest release and remembers its version. With `quiet` it
/// works silently — that is how the status line calls it.
pub fn check(quiet: bool) -> Outcome {
    let source = Source::default();
    touch_cache();

    let latest = latest_version(&source)?;
    write_cache(&Cache {
        checked_at: now(),
        latest: latest.clone(),
    })?;

    if quiet {
        return Ok(());
    }

    let installed = version();
    if is_newer(&installed, &latest) {
        println!(
            "{}",
            tf!(
                "{0} installed, {1} is out — update with: clistatus update",
                installed,
                latest
            )
        );
    } else if parse_version(&installed).is_none() {
        println!(
            "{}",
            tf!("Not a release build, the latest version is {0}", latest)
        );
    } else {
        println!("{}", tf!("The latest version is installed: {0}", installed));
    }
    Ok(())
}

/// Downloads the binary of the latest release and replaces us with it.
pub fn update() -> Outcome {
    let source = Source::default();
    let latest = latest_version(&source)?;
    let _ = write_cache(&Cache {
        checked_at: now(),
        latest: latest.clone(),
    });

    let installed = version();
    let a_release = parse_version(&installed).is_some();
    if a_release && !is_newer(&installed, &latest) {
        println!("{}", tf!("Already the latest version: {0}", installed));
        return Ok(());
    }

    let exe = self_path()?;
    if a_release {
        println!("{}", tf!("==> Updating {0} → {1}", installed, latest));
    } else {
        println!("{}", tf!("==> Installing {0}", latest));
    }

    replace_self(&source, &exe, &latest)?;

    println!(
        "{}",
        tf!(
            "Done: {0}. The status line picks it up by itself, no need to restart Claude Code.",
            latest
        )
    );
    Ok(())
}

/// The version to draw the update mark for, straight from the cache — the
/// status line never goes to the network.
pub fn available() -> Option<String> {
    let cache = read_cache()?;
    is_newer(&version(), &cache.latest).then_some(cache.latest)
}

/// Starts the check as a separate process — on the first call and then no more
/// than once an hour, that is both at the start of a session and while working.
/// Waiting for it is not allowed: the status line is drawn on every keystroke
/// and has to return at once.
pub fn auto_check() {
    if std::env::var_os("CLISTATUS_NO_AUTO_UPDATE").is_some()
        || std::env::var_os("CLAUDESTATUS_NO_AUTO_UPDATE").is_some()
    {
        return;
    }
    if let Some(cache) = read_cache()
        && now().saturating_sub(cache.checked_at) < CHECK_INTERVAL.as_secs()
    {
        return;
    }
    let Ok(exe) = self_path() else { return };

    // The timestamp is written before starting: otherwise several sessions would
    // each spawn their own check.
    touch_cache();

    // The output is detached from ours: Claude Code reads the stdout of the
    // status line and would wait for the background process to close the pipe.
    let _ = std::process::Command::new(exe)
        .args(["check", "--quiet"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Downloads the release binary next to the current one and swaps them by
/// renaming: writing over a running file is not allowed, while a rename
/// survives even our own running process.
fn replace_self(source: &Source, exe: &Path, tag: &str) -> Result<(), String> {
    let asset = asset_name();

    let sums = fetch(
        &format!("{}{tag}/checksums.txt", source.downloads),
        DOWNLOAD_TIMEOUT,
    )
    .map_err(|err| tf!("could not download the checksums: {0}", err))?;
    let sums = String::from_utf8_lossy(&sums);
    let want = checksum_for(&sums, &asset)
        .ok_or_else(|| tf!("release {0} has no binary for {1}", tag, platform()))?;

    let binary = fetch(
        &format!("{}{tag}/{asset}", source.downloads),
        DOWNLOAD_TIMEOUT,
    )
    .map_err(|err| tf!("could not download {0}: {1}", asset, err))?;

    let got: String = Sha256::digest(&binary)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if got != want {
        return Err(tf!(
            "the checksum of {0} does not match — the file is broken or tampered with",
            asset
        ));
    }

    let staged = with_suffix(exe, ".new");
    write_executable(&staged, &binary)
        .map_err(|err| tf!("could not write {0}: {1}", staged.display(), err))?;

    let previous = with_suffix(exe, ".old");
    let _ = std::fs::remove_file(&previous);
    if let Err(err) = std::fs::rename(exe, &previous) {
        let _ = std::fs::remove_file(&staged);
        return Err(tf!("could not move the previous binary away: {0}", err));
    }
    if let Err(err) = std::fs::rename(&staged, exe) {
        let _ = std::fs::rename(&previous, exe);
        return Err(tf!("could not put the new binary in place: {0}", err));
    }
    // Windows will not let a running file be deleted — it goes away on the next
    // update.
    let _ = std::fs::remove_file(&previous);
    Ok(())
}

fn write_executable(path: &Path, data: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

fn with_suffix(exe: &Path, suffix: &str) -> PathBuf {
    let mut name = exe.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// The file name in the release for the current platform.
///
/// The names are the ones the Go build produced, and install.sh still spells
/// them that way — the platform is named as Go named it, whatever this compiler
/// calls it.
fn asset_name() -> String {
    let name = format!("clistatus_{}", platform().replace('/', "_"));
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name
    }
}

fn platform() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    };
    format!("{os}/{arch}")
}

/// Looks for a line of the form `<sha256>  <file>` — the sha256sum format.
fn checksum_for(sums: &str, asset: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let sum = fields.next()?;
        let name = fields.next()?.trim_start_matches('*');
        (name == asset && fields.next().is_none()).then(|| sum.to_string())
    })
}

fn latest_version(source: &Source) -> Result<String, String> {
    let body = fetch(&source.api, NETWORK_TIMEOUT)
        .map_err(|err| tf!("could not find out the latest version: {0}", err))?;

    #[derive(Deserialize)]
    struct Release {
        #[serde(default)]
        tag_name: String,
    }
    let release: Release = serde_json::from_slice(&body)
        .map_err(|err| tf!("could not parse the GitHub response: {0}", err))?;

    if release.tag_name.is_empty() {
        return Err(tf!("{0} has no releases yet", REPOSITORY));
    }
    Ok(release.tag_name)
}

fn fetch(url: &str, timeout: Duration) -> Result<Vec<u8>, String> {
    let mut response = ureq::get(url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "clistatus")
        .config()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .build()
        .call()
        .map_err(|err| err.to_string())?;

    if response.status() != 200 {
        return Err(tf!("{0} answered {1}", url, response.status()));
    }
    response
        .body_mut()
        .with_config()
        // A release binary is a few megabytes; the default cap is smaller.
        .limit(64 * 1024 * 1024)
        .read_to_vec()
        .map_err(|err| err.to_string())
}

fn cache_path() -> Result<PathBuf, String> {
    Ok(paths::cache_dir()?.join("update.json"))
}

fn read_cache() -> Option<Cache> {
    let data = std::fs::read(cache_path().ok()?).ok()?;
    serde_json::from_slice(&data).ok()
}

fn write_cache(cache: &Cache) -> Result<(), String> {
    let path = cache_path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|err| tf!("could not create {0}: {1}", dir.display(), err))?;
    }
    let data = serde_json::to_vec(cache).map_err(|err| err.to_string())?;

    // Written through a temporary file: the status line may be reading the cache
    // right now.
    let staged = with_suffix(&path, ".tmp");
    std::fs::write(&staged, data)
        .map_err(|err| tf!("could not write {0}: {1}", staged.display(), err))?;
    std::fs::rename(staged, path).map_err(|err| err.to_string())
}

/// Records an attempt to check without touching the known version: a failed
/// request must neither put out the update mark nor make the check run every
/// second.
fn touch_cache() {
    let mut cache = read_cache().unwrap_or_default();
    cache.checked_at = now();
    let _ = write_cache(&cache);
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0)
}

/// The path to our own binary with symlinks resolved: the command is run
/// through a link from ~/.local/bin, but it has to update the real file.
pub fn self_path() -> Result<PathBuf, String> {
    let exe =
        std::env::current_exe().map_err(|err| tf!("could not determine our own path: {0}", err))?;
    std::fs::canonicalize(&exe).map_err(|err| tf!("could not determine our own path: {0}", err))
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Semver(u32, u32, u32);

/// Reads a tag of the form v1.2.3: a suffix after the dash (-rc1) does not take
/// part in comparing the number.
fn parse_version(value: &str) -> Option<Semver> {
    let value = value.trim().trim_start_matches('v');
    let value = value.split(['-', '+']).next()?;

    let mut fields = value.split('.');
    let mut number = || fields.next()?.parse::<u32>().ok();
    let parsed = Semver(number()?, number()?, number()?);

    fields.next().is_none().then_some(parsed)
}

/// Compares the installed version against a tag. A build made outside a release
/// counts as outdated: it has no number to compare with.
fn is_newer(installed: &str, tag: &str) -> bool {
    match (parse_version(installed), parse_version(tag)) {
        (Some(installed), Some(latest)) => installed < latest,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in for GitHub: it serves a release and remembers nothing else.
    /// No test here may reach the real network.
    struct Releases {
        server: std::sync::Arc<tiny_http::Server>,
        thread: Option<std::thread::JoinHandle<()>>,
        base: String,
    }

    impl Releases {
        fn serving(files: Vec<(String, Vec<u8>)>) -> Self {
            let server = std::sync::Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
            let base = format!("http://{}", server.server_addr().to_ip().unwrap());

            let thread = std::thread::spawn({
                let server = std::sync::Arc::clone(&server);
                move || {
                    for request in server.incoming_requests() {
                        let url = request.url().to_string();
                        let found = files.iter().find(|(name, _)| url.ends_with(name.as_str()));
                        let answer = match found {
                            Some((_, data)) => tiny_http::Response::from_data(data.clone()),
                            None => {
                                tiny_http::Response::from_data(Vec::new()).with_status_code(404)
                            }
                        };
                        let _ = request.respond(answer);
                    }
                }
            });

            Self {
                server,
                thread: Some(thread),
                base,
            }
        }

        fn source(&self) -> Source {
            Source {
                api: format!("{}/latest", self.base),
                downloads: format!("{}/download/", self.base),
            }
        }
    }

    impl Drop for Releases {
        fn drop(&mut self) {
            self.server.unblock();
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    fn sha256_of(data: &[u8]) -> String {
        Sha256::digest(data)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    /// A file standing in for the running binary, in a directory of its own.
    fn staged_binary(name: &str, content: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("clistatus-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("clistatus");
        std::fs::write(&exe, content).unwrap();
        exe
    }

    #[test]
    fn reads_the_tag_of_the_latest_release() {
        let releases = Releases::serving(vec![(
            "latest".into(),
            br#"{"tag_name":"v1.4.0"}"#.to_vec(),
        )]);
        assert_eq!(latest_version(&releases.source()).unwrap(), "v1.4.0");
    }

    #[test]
    fn says_so_when_there_are_no_releases_yet() {
        let releases = Releases::serving(vec![("latest".into(), b"{}".to_vec())]);
        let err = latest_version(&releases.source()).unwrap_err();
        assert!(err.contains(REPOSITORY), "{err}");
    }

    #[test]
    fn replaces_itself_with_the_binary_of_the_release() {
        let binary = b"the new binary".to_vec();
        let sums = format!("{}  {}\n", sha256_of(&binary), asset_name());
        let releases = Releases::serving(vec![
            ("checksums.txt".into(), sums.into_bytes()),
            (asset_name(), binary.clone()),
        ]);

        let exe = staged_binary("replace", b"the old binary");
        replace_self(&releases.source(), &exe, "v1.4.0").unwrap();

        assert_eq!(std::fs::read(&exe).unwrap(), binary);
        assert!(
            !exe.with_extension("new").exists(),
            "the staged file was left behind"
        );
    }

    #[test]
    fn refuses_a_binary_whose_checksum_does_not_match() {
        let sums = format!(
            "{}  {}\n",
            sha256_of(b"what we were promised"),
            asset_name()
        );
        let releases = Releases::serving(vec![
            ("checksums.txt".into(), sums.into_bytes()),
            (asset_name(), b"something else entirely".to_vec()),
        ]);

        let exe = staged_binary("tampered", b"the old binary");
        let err = replace_self(&releases.source(), &exe, "v1.4.0").unwrap_err();

        assert!(err.contains(&asset_name()), "{err}");
        // The binary we are running must survive a release we could not trust.
        assert_eq!(std::fs::read(&exe).unwrap(), b"the old binary");
    }

    #[test]
    fn refuses_a_release_that_has_no_binary_for_us() {
        let releases = Releases::serving(vec![(
            "checksums.txt".into(),
            b"aaa  clistatus_plan9_arm64\n".to_vec(),
        )]);

        let exe = staged_binary("missing", b"the old binary");
        let err = replace_self(&releases.source(), &exe, "v1.4.0").unwrap_err();

        assert!(err.contains("v1.4.0"), "{err}");
        assert_eq!(std::fs::read(&exe).unwrap(), b"the old binary");
    }

    #[test]
    fn reads_a_version_tag() {
        assert_eq!(parse_version("v1.2.3"), Some(Semver(1, 2, 3)));
        assert_eq!(parse_version("1.2.3"), Some(Semver(1, 2, 3)));
        assert_eq!(parse_version(" v0.0.1 "), Some(Semver(0, 0, 1)));
        // A suffix does not take part in the number.
        assert_eq!(parse_version("v1.2.3-rc1"), Some(Semver(1, 2, 3)));
    }

    #[test]
    fn refuses_what_is_not_a_version() {
        assert_eq!(parse_version("built from source"), None);
        assert_eq!(parse_version("v1.2"), None);
        assert_eq!(parse_version("v1.2.3.4"), None);
        assert_eq!(parse_version("v1.2.x"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn compares_versions_by_number_not_by_text() {
        assert!(is_newer("v1.2.3", "v1.2.4"));
        assert!(is_newer("v1.9.0", "v1.10.0"), "9 is not newer than 10");
        assert!(is_newer("v0.9.9", "v1.0.0"));
        assert!(!is_newer("v1.2.3", "v1.2.3"));
        assert!(!is_newer("v2.0.0", "v1.0.0"));
    }

    #[test]
    fn counts_a_build_without_a_version_as_outdated_but_never_marks_it() {
        // Nothing to compare with: the mark stays out rather than lights up.
        assert!(!is_newer("built from source", "v1.2.3"));
        assert!(!is_newer("v1.2.3", "not a tag"));
    }

    #[test]
    fn finds_the_checksum_of_our_asset() {
        let sums = "aaa  clistatus_linux_amd64\nbbb  clistatus_darwin_arm64\nccc *clistatus_windows_amd64.exe\n";
        assert_eq!(
            checksum_for(sums, "clistatus_darwin_arm64").as_deref(),
            Some("bbb")
        );
        // The sha256sum format marks a binary file with a star.
        assert_eq!(
            checksum_for(sums, "clistatus_windows_amd64.exe").as_deref(),
            Some("ccc")
        );
        assert_eq!(checksum_for(sums, "clistatus_plan9_arm64"), None);
    }

    #[test]
    fn does_not_mistake_one_asset_for_another() {
        let sums = "aaa  clistatus_linux_amd64\nbbb  clistatus_linux_amd64.exe\n";
        assert_eq!(
            checksum_for(sums, "clistatus_linux_amd64").as_deref(),
            Some("aaa")
        );
        assert_eq!(
            checksum_for(sums, "clistatus_linux_amd64.exe").as_deref(),
            Some("bbb")
        );
    }

    #[test]
    fn names_the_asset_the_way_the_release_does() {
        let name = asset_name();
        assert!(name.starts_with("clistatus_"), "{name}");
        // The names of the release are the ones the Go build produced.
        assert!(
            !name.contains("macos") && !name.contains("x86_64") && !name.contains("aarch64"),
            "{name}"
        );
        if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
            assert_eq!(name, "clistatus_darwin_arm64");
        }
    }
}
