//! Reading the snapshot the status line writes. It is the only way the numbers
//! reach the panel: the limits arrive during a session and nowhere else.

use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use divoomkit::Rgb;
use serde::Deserialize;

use crate::i18n::t;
use crate::paths;
use crate::snapshot::NAME;

/// A snapshot older than this describes the past: Claude Code only refreshes it
/// during a session.
const STALE_AFTER: Duration = Duration::from_secs(90);

/// The lengths of the windows — how much of one has passed is counted against
/// these, because `resets_at` says when a window ends and never when it began.
pub const FIVE_HOUR_SECONDS: f64 = 5.0 * 60.0 * 60.0;
pub const SEVEN_DAY_SECONDS: f64 = 7.0 * 24.0 * 60.0 * 60.0;

#[derive(Debug, Default, Clone)]
pub struct Snapshot {
    windows: BTreeMap<String, Window>,
    pub stale: bool,
    /// The age of the snapshot; shown once the data stops being refreshed.
    pub age: Duration,
    pub trouble: Option<String>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Window {
    pub used: f64,
    pub seconds_left: i64,
    pub expired: bool,
    pub present: bool,
}

#[derive(Deserialize)]
struct Raw {
    #[serde(default)]
    rate_limits: BTreeMap<String, RawWindow>,
    #[serde(default)]
    updated_at: String,
}

#[derive(Deserialize)]
struct RawWindow {
    #[serde(default)]
    used_percentage: Option<f64>,
    #[serde(default)]
    resets_at: Option<f64>,
}

pub fn read() -> Snapshot {
    let path = match paths::file(NAME) {
        Ok(path) => path,
        Err(err) => {
            return Snapshot {
                trouble: Some(err),
                ..Default::default()
            };
        }
    };
    let Ok(data) = std::fs::read(&path) else {
        return Snapshot {
            trouble: Some(t("no snapshot found").into()),
            ..Default::default()
        };
    };
    parse(&data, now())
}

/// Takes the moment as an argument so that a test can say when it is.
fn parse(data: &[u8], now: f64) -> Snapshot {
    let Ok(raw) = serde_json::from_slice::<Raw>(data) else {
        return Snapshot {
            trouble: Some(t("the snapshot is damaged").into()),
            ..Default::default()
        };
    };

    let mut windows = BTreeMap::new();
    for (id, entry) in raw.rate_limits {
        let Some(used) = entry.used_percentage else {
            continue;
        };
        let mut window = Window {
            used,
            present: true,
            ..Default::default()
        };

        if let Some(resets_at) = entry.resets_at.map(normalize_epoch) {
            if resets_at <= now {
                // The percentages describe a window that is over; in the new one
                // usage starts from zero — the old number must not be shown.
                window.expired = true;
                window.used = 0.0;
            } else {
                window.seconds_left = (resets_at - now) as i64;
            }
        }
        windows.insert(id, window);
    }

    if windows.is_empty() {
        return Snapshot {
            trouble: Some(t("the snapshot holds no limits").into()),
            ..Default::default()
        };
    }

    let mut snapshot = Snapshot {
        windows,
        ..Default::default()
    };
    if let Some(written) = parse_rfc3339(&raw.updated_at) {
        snapshot.age = Duration::from_secs_f64((now - written).max(0.0));
        snapshot.stale = snapshot.age >= STALE_AFTER;
    }
    snapshot
}

impl Snapshot {
    pub fn window(&self, id: &str) -> Window {
        self.windows.get(id).copied().unwrap_or_default()
    }

    /// The percentages only, without the countdown: their growth is worth
    /// showing at once, a shifted minute can wait.
    pub fn usage_key(&self) -> String {
        if let Some(trouble) = &self.trouble {
            return format!("err:{trouble}");
        }
        ["five_hour", "seven_day", "seven_day_opus"]
            .iter()
            .map(|id| {
                let window = self.window(id);
                format!("{id}={:.0}/{};", window.used, window.expired)
            })
            .collect()
    }
}

impl Window {
    pub fn fraction(&self) -> f32 {
        (self.used / 100.0) as f32
    }

    pub fn elapsed_fraction(&self, window_seconds: f64) -> f32 {
        if self.expired || self.seconds_left <= 0 {
            return 1.0;
        }
        (((window_seconds - self.seconds_left as f64) / window_seconds) as f32).clamp(0.0, 1.0)
    }

    /// Whether we know where in its window the moment is. A window Claude Code
    /// reported without a `resets_at` has a percentage and no clock.
    pub fn timed(&self) -> bool {
        self.present && (self.expired || self.seconds_left > 0)
    }

    pub fn percent_label(&self) -> String {
        if !self.present {
            return "-".into();
        }
        format!("{}%", self.used.round() as i64)
    }

    /// The only place with the color thresholds: green up to 60%, orange up to
    /// 85%, red above — exactly like the status line. Cyan belongs to the reset
    /// bar alone, because that one shows time and not risk. A window that has
    /// reset is dimmed.
    pub fn tint(&self) -> Rgb {
        match self.used {
            _ if !self.present || self.expired => Rgb::GREY,
            used if used >= 85.0 => Rgb::RED,
            used if used >= 60.0 => Rgb::ORANGE,
            _ => Rgb::GREEN,
        }
    }

    /// The week is painted by pace against the part of it that has passed, by
    /// the same rule as in the status line.
    pub fn week_tint(&self) -> Rgb {
        if !self.present || self.expired {
            return Rgb::GREY;
        }
        let elapsed = self
            .timed()
            .then(|| f64::from(self.elapsed_fraction(SEVEN_DAY_SECONDS)) * 100.0);
        [Rgb::GREEN, Rgb::ORANGE, Rgb::RED][crate::statusline::week_level(self.used, elapsed)]
    }
}

/// Accepts Unix time in seconds or in milliseconds.
fn normalize_epoch(value: f64) -> f64 {
    if value > 1e12 { value / 1000.0 } else { value }
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs_f64())
        .unwrap_or(0.0)
}

/// Reads back what the status line wrote: `2026-08-04T09:15:00Z` and nothing
/// else. A stamp we cannot read leaves the age unknown, which only costs the
/// staleness mark.
fn parse_rfc3339(stamp: &str) -> Option<f64> {
    let stamp = stamp.strip_suffix('Z')?;
    let (date, time) = stamp.split_once('T')?;

    let mut date = date.split('-');
    let year: i64 = date.next()?.parse().ok()?;
    let month: i64 = date.next()?.parse().ok()?;
    let day: i64 = date.next()?.parse().ok()?;

    let mut time = time.split(':');
    let hour: f64 = time.next()?.parse().ok()?;
    let minute: f64 = time.next()?.parse().ok()?;
    let second: f64 = time.next()?.parse().ok()?;

    Some(
        days_from_civil(year, month, day) as f64 * 86_400.0
            + hour * 3600.0
            + minute * 60.0
            + second,
    )
}

/// A calendar date to days since the epoch, by Howard Hinnant's days_from_civil.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let shifted = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * shifted + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// A snapshot with numbers in it, for the tests of whoever draws them.
#[cfg(test)]
pub fn for_test(used: f64, five_left: i64, week_left: i64) -> Snapshot {
    let window = |seconds_left| Window {
        used,
        seconds_left,
        present: true,
        expired: false,
    };
    Snapshot {
        windows: [
            ("five_hour".to_string(), window(five_left)),
            ("seven_day".to_string(), window(week_left)),
        ]
        .into(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: f64 = 1_770_000_000.0;

    fn snapshot_of(json: &str) -> Snapshot {
        parse(json.as_bytes(), NOW)
    }

    #[test]
    fn reads_the_windows_the_status_line_wrote() {
        let snapshot = snapshot_of(
            r#"{"rate_limits":{"five_hour":{"used_percentage":42.0,"resets_at":1770009000.0},
                "seven_day":{"used_percentage":8.0}},"updated_at":"2026-02-02T02:40:00Z"}"#,
        );

        assert!(snapshot.trouble.is_none());
        let five = snapshot.window("five_hour");
        assert_eq!(five.used, 42.0);
        assert_eq!(five.seconds_left, 9000);
        assert!(!five.expired);
        assert_eq!(snapshot.window("seven_day").used, 8.0);
    }

    #[test]
    fn zeroes_a_window_that_has_already_reset() {
        // Usage in the new window starts from zero: the old number would be a lie.
        let snapshot = snapshot_of(
            r#"{"rate_limits":{"five_hour":{"used_percentage":97.0,"resets_at":1769999000.0}}}"#,
        );

        let five = snapshot.window("five_hour");
        assert!(five.expired);
        assert_eq!(five.used, 0.0);
        assert_eq!(five.tint(), Rgb::GREY);
    }

    #[test]
    fn says_what_is_wrong_rather_than_showing_nothing() {
        assert!(snapshot_of("not json at all").trouble.is_some());
        assert!(snapshot_of(r#"{"rate_limits":{}}"#).trouble.is_some());
        // A window without a percentage is no window at all.
        assert!(
            snapshot_of(r#"{"rate_limits":{"five_hour":{"resets_at":1770009000.0}}}"#)
                .trouble
                .is_some()
        );
    }

    #[test]
    fn knows_when_the_numbers_stopped_being_refreshed() {
        let fresh = snapshot_of(
            r#"{"rate_limits":{"five_hour":{"used_percentage":1.0}},"updated_at":"2026-02-02T02:39:30Z"}"#,
        );
        assert!(!fresh.stale, "age {:?}", fresh.age);

        let old = snapshot_of(
            r#"{"rate_limits":{"five_hour":{"used_percentage":1.0}},"updated_at":"2026-02-02T01:40:00Z"}"#,
        );
        assert!(old.stale);
        assert_eq!(old.age.as_secs(), 3600);
    }

    #[test]
    fn turns_a_date_into_the_moment_it_stands_for() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0.0));
        assert_eq!(parse_rfc3339("2026-02-02T02:40:00Z"), Some(NOW));
        assert_eq!(parse_rfc3339("nonsense"), None);
        assert_eq!(parse_rfc3339("2026-02-02 02:40:00"), None);
    }

    #[test]
    fn paints_a_window_by_how_full_it_is() {
        let window = |used| Window {
            used,
            present: true,
            ..Default::default()
        };
        assert_eq!(window(0.0).tint(), Rgb::GREEN);
        assert_eq!(window(59.9).tint(), Rgb::GREEN);
        assert_eq!(window(60.0).tint(), Rgb::ORANGE);
        assert_eq!(window(85.0).tint(), Rgb::RED);
        assert_eq!(
            Window::default().tint(),
            Rgb::GREY,
            "a window we have no numbers for"
        );
    }

    #[test]
    fn paints_the_week_by_its_pace() {
        let day = 24 * 3600;
        let week = |used, seconds_left| Window {
            used,
            seconds_left,
            present: true,
            expired: false,
        };
        assert_eq!(week(40.0, 2 * day).week_tint(), Rgb::GREEN);
        assert_eq!(week(40.0, 4 * day).week_tint(), Rgb::ORANGE);
        assert_eq!(week(40.0, 6 * day).week_tint(), Rgb::RED);
        assert_eq!(week(70.0, 0).week_tint(), Rgb::ORANGE, "without a clock");
        assert_eq!(Window::default().week_tint(), Rgb::GREY);
    }

    #[test]
    fn measures_how_much_of_the_window_has_passed() {
        let half = Window {
            seconds_left: 9000,
            present: true,
            ..Default::default()
        };
        assert!((half.elapsed_fraction(FIVE_HOUR_SECONDS) - 0.5).abs() < 0.001);

        let week = Window {
            seconds_left: 3 * 24 * 3600 + 12 * 3600,
            present: true,
            ..Default::default()
        };
        assert!((week.elapsed_fraction(SEVEN_DAY_SECONDS) - 0.5).abs() < 0.001);

        let over = Window {
            expired: true,
            present: true,
            ..Default::default()
        };
        assert_eq!(over.elapsed_fraction(SEVEN_DAY_SECONDS), 1.0);
    }

    #[test]
    fn knows_whether_the_window_has_a_clock() {
        assert!(
            !Window {
                used: 8.0,
                present: true,
                ..Default::default()
            }
            .timed(),
            "a window reported without a resets_at"
        );
        assert!(
            Window {
                seconds_left: 60,
                present: true,
                ..Default::default()
            }
            .timed()
        );
        assert!(!Window::default().timed());
    }

    #[test]
    fn changes_its_key_only_when_the_percentages_change() {
        let with_countdown = snapshot_of(
            r#"{"rate_limits":{"five_hour":{"used_percentage":42.0,"resets_at":1770009000.0}}}"#,
        );
        let later = parse(
            r#"{"rate_limits":{"five_hour":{"used_percentage":42.0,"resets_at":1770009000.0}}}"#
                .as_bytes(),
            NOW + 600.0,
        );
        assert_eq!(
            with_countdown.usage_key(),
            later.usage_key(),
            "a shifted minute is not news"
        );

        let grown = snapshot_of(
            r#"{"rate_limits":{"five_hour":{"used_percentage":43.0,"resets_at":1770009000.0}}}"#,
        );
        assert_ne!(with_countdown.usage_key(), grown.usage_key());
    }
}
