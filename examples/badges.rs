//! Draws the README badges into .github/badges.
//!
//! Shields.io would draw them too, but every badge would then be a request from
//! the reader's browser to somebody else's server, and the numbers about the
//! tests would have to be published somewhere first. These are plain files in
//! the repository: they show the state of the commit they were made in, and
//! they keep working with no network at all.
//!
//! Run it after touching the tests or the release matrix:
//!
//! ```text
//! cargo run --example badges
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;

const BADGES: &str = ".github/badges";

/// The colors shields.io uses, so that the badges do not look like strangers
/// next to the ones people are used to. Rust keeps its own brand red.
const GREEN: &str = "#4c1";
const BLUE: &str = "#007ec6";
const RUST: &str = "#ce422b";
const RED: &str = "#e05d44";
const LABEL: &str = "#555";

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let root = repository_root()?;
    std::fs::create_dir_all(root.join(BADGES)).map_err(|err| err.to_string())?;

    println!("running the tests…");
    let tests = run_tests(&root)?;

    let manifest =
        std::fs::read_to_string(root.join("Cargo.toml")).map_err(|err| err.to_string())?;
    let badges = [
        ("tests.svg", tests_badge(&tests)),
        ("rust.svg", flat("Rust", &rust_version(&manifest)?, RUST)),
        (
            "platforms.svg",
            flat("platforms", &platforms(&root)?.join(" · "), BLUE),
        ),
        ("dependencies.svg", dependencies_badge(&manifest)),
    ];

    for (name, svg) in &badges {
        std::fs::write(root.join(BADGES).join(name), svg).map_err(|err| err.to_string())?;
    }

    println!("{} badges written to {BADGES}", badges.len());
    println!(
        "  tests: {} passed{}",
        tests.passed,
        match tests.failed {
            0 => String::new(),
            failed => format!(", {failed} failed"),
        }
    );
    Ok(())
}

struct Tests {
    passed: u32,
    failed: u32,
}

/// Counts what `cargo test` reports. A failing test is not a reason to stop:
/// the badge is there to say so.
fn run_tests(root: &Path) -> Result<Tests, String> {
    let output = Command::new(env!("CARGO"))
        .arg("test")
        .current_dir(root)
        .output()
        .map_err(|err| format!("cargo test: {err}"))?;

    let printed = String::from_utf8_lossy(&output.stdout);
    let mut tests = Tests {
        passed: 0,
        failed: 0,
    };

    // "test result: ok. 71 passed; 0 failed; 0 ignored; ..." — one line per
    // binary, and the doc tests add one more.
    for line in printed
        .lines()
        .filter(|line| line.starts_with("test result:"))
    {
        // Every count is a number followed by the word for it.
        let words: Vec<&str> = line.split_whitespace().collect();
        let count = |what: &str| -> u32 {
            words
                .windows(2)
                .find(|pair| pair[1].trim_end_matches(';') == what)
                .and_then(|pair| pair[0].parse().ok())
                .unwrap_or(0)
        };
        tests.passed += count("passed");
        tests.failed += count("failed");
    }

    if tests.passed == 0 && tests.failed == 0 {
        return Err("cargo test reported no tests at all".into());
    }
    Ok(tests)
}

fn tests_badge(tests: &Tests) -> String {
    if tests.failed > 0 {
        return flat(
            "tests",
            &format!("{} failed, {} passed", tests.failed, tests.passed),
            RED,
        );
    }
    flat("tests", &format!("{} passed", tests.passed), GREEN)
}

/// The oldest Rust the crate promises to build on — a promise worth showing,
/// and one CI would catch us breaking.
fn rust_version(manifest: &str) -> Result<String, String> {
    field(manifest, "rust-version").ok_or_else(|| "Cargo.toml names no rust-version".into())
}

/// What the manifest requires directly. Every one of them is a thing that can
/// break the build of a release, so the number is worth keeping in sight.
fn dependencies_badge(manifest: &str) -> String {
    let direct = manifest
        .lines()
        .skip_while(|line| line.trim() != "[dependencies]")
        .skip(1)
        .take_while(|line| !line.trim_start().starts_with('['))
        .filter(|line| line.contains('=') && !line.trim_start().starts_with('#'))
        .count();

    match direct {
        0 => flat("dependencies", "zero", GREEN),
        count => flat("dependencies", &count.to_string(), BLUE),
    }
}

/// Reads the release matrix out of the workflow: the badge promises exactly the
/// systems a release is actually built for, and cannot drift away from them.
fn platforms(root: &Path) -> Result<Vec<String>, String> {
    let workflow = std::fs::read_to_string(root.join(".github/workflows/release.yml"))
        .map_err(|err| err.to_string())?;

    let mut found: Vec<String> = Vec::new();
    for line in workflow.lines() {
        let Some(asset) = line.trim().strip_prefix("asset: clistatus_") else {
            continue;
        };
        let system = match asset.split('_').next() {
            Some("darwin") => "macOS",
            Some("linux") => "Linux",
            Some("windows") => "Windows",
            Some(other) => other,
            None => continue,
        };
        if !found.iter().any(|known| known == system) {
            found.push(system.to_string());
        }
    }

    if found.is_empty() {
        return Err("release.yml names no build targets".into());
    }
    Ok(found)
}

fn field(manifest: &str, name: &str) -> Option<String> {
    manifest.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == name).then(|| value.trim().trim_matches('"').to_string())
    })
}

fn repository_root() -> Result<PathBuf, String> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|err| format!("this is not a git repository: {err}"))?;
    if !output.status.success() {
        return Err("this is not a git repository".into());
    }
    Ok(PathBuf::from(
        String::from_utf8_lossy(&output.stdout).trim(),
    ))
}

/// The shields.io "flat" badge, drawn by hand: a grey label, a colored message,
/// one gradient over both. The width is guessed from the characters, because
/// there is no font here to measure with.
fn char_width(symbol: char) -> f64 {
    match symbol {
        'i' | 'l' | '.' | ':' | ',' | '|' | '\'' | '!' | ';' | '[' | ']' | '(' | ')' => 3.2,
        'f' | 't' | 'r' | 'j' | '/' | '\\' | ' ' => 4.2,
        'm' | 'w' | 'M' | 'W' | '@' | '%' | '·' => 10.5,
        'A'..='Z' => 8.2,
        _ => 6.8,
    }
}

fn text_width(text: &str) -> f64 {
    text.chars().map(char_width).sum()
}

fn flat(label: &str, message: &str, color: &str) -> String {
    const PADDING: i64 = 12;

    let label_width = (text_width(label) + PADDING as f64 + 0.5) as i64;
    let message_width = (text_width(message) + PADDING as f64 + 0.5) as i64;
    let width = label_width + message_width;

    // The text is drawn at ten times the size and scaled down, the way
    // shields.io does it: that is what keeps the letters crisp.
    let label_x = label_width * 10 / 2;
    let message_x = (label_width + message_width / 2) * 10;
    let label_length = ((label_width - PADDING) * 10).max(0);
    let message_length = ((message_width - PADDING) * 10).max(0);

    let title = format!("{label}: {message}");
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{width}" height="20" role="img" aria-label="{title}">
  <title>{title}</title>
  <linearGradient id="s" x2="0" y2="100%"><stop offset="0" stop-color="#bbb" stop-opacity=".1"/><stop offset="1" stop-opacity=".1"/></linearGradient>
  <clipPath id="r"><rect width="{width}" height="20" rx="3" fill="#fff"/></clipPath>
  <g clip-path="url(#r)">
    <rect width="{label_width}" height="20" fill="{LABEL}"/>
    <rect x="{label_width}" width="{message_width}" height="20" fill="{color}"/>
    <rect width="{width}" height="20" fill="url(#s)"/>
  </g>
  <g fill="#fff" text-anchor="middle" font-family="Verdana,Geneva,DejaVu Sans,sans-serif" text-rendering="geometricPrecision" font-size="110">
    <text aria-hidden="true" x="{label_x}" y="150" fill="#010101" fill-opacity=".3" transform="scale(.1)" textLength="{label_length}">{label}</text>
    <text x="{label_x}" y="140" transform="scale(.1)" textLength="{label_length}">{label}</text>
    <text aria-hidden="true" x="{message_x}" y="150" fill="#010101" fill-opacity=".3" transform="scale(.1)" textLength="{message_length}">{message}</text>
    <text x="{message_x}" y="140" transform="scale(.1)" textLength="{message_length}">{message}</text>
  </g>
</svg>
"##
    )
}
