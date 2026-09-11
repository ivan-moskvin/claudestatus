//! The status line itself: session JSON on stdin, one line on stdout.
//!
//! We have no process of our own — the command lives for exactly one call.
//! Everything shown arrives here and now, the only exception being the update
//! check cache. Hence the rules: empty or unparsable input prints a dash rather
//! than an error, a window missing from `rate_limits` is simply not drawn, and
//! nothing waits for the network.

use std::io::Read;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};

use crate::i18n::tf;
use crate::snapshot;

const BAR_WIDTH: usize = 10;
const SEPARATOR: &str = " · ";
/// What the strip under the 7d bar is pushed right with. Not a space: Claude
/// Code wraps the line it is given, and the wrapping strips the leading spaces
/// off it — the strip would end up under the model name. An empty braille cell
/// is one column wide and counts as a character, so the indent survives.
const INDENT: &str = "\u{2800}";
const FIVE_HOUR_SECONDS: f64 = 5.0 * 60.0 * 60.0;
const SEVEN_DAY_SECONDS: f64 = 7.0 * 24.0 * 60.0 * 60.0;

/// Colors of the 256-color palette. Usage bars are painted by threshold, the
/// reset bar is always cyan — it shows time, not risk.
const GREEN: u8 = 35;
const ORANGE: u8 = 214;
const RED: u8 = 167;
const CYAN: u8 = 38;
const UPDATE: u8 = 178;
const EMPTY_BG: u8 = 237;
const EMPTY_FG: u8 = 250;
/// The strip that says how much of the weekly window has passed: grey, so that
/// it is never taken for one of the level colors of a bar.
const STRIP: u8 = 245;
const DARK_TEXT: u8 = 16;
const LIGHT_TEXT: u8 = 231;

/// The reasoning effort scale: the circle phases are the very symbols Claude
/// Code itself uses. The color is deliberately purple to pink so that it is
/// never confused with the green/orange/red of the usage bars.
const EFFORT: &[(&str, &str, u8)] = &[
    ("low", "○", 245),
    ("medium", "◔", 111),
    ("high", "◑", 141),
    ("xhigh", "◕", 171),
    ("max", "●", 199),
];

/// Where the context turns orange and red, in tokens, by what the model id
/// contains — Bedrock and Vertex wrap the id in prefixes and suffixes of their
/// own. Tokens rather than a share of the window: recall falls off after so many
/// tokens whatever window the model was sold with. The numbers follow the
/// long-context retrieval scores (MRCR, GraphWalks, Fiction.LiveBench); a model
/// with none published inherits the ones of its predecessor. The first match
/// wins, so a version goes above its family.
const CONTEXT: &[(&str, f64, f64)] = &[
    ("claude-fable-5", 250_000.0, 500_000.0),
    ("claude-mythos-5", 250_000.0, 500_000.0),
    ("claude-opus-5", 250_000.0, 500_000.0),
    // Recall of 4.7 at 256K fell to 59% from 92% on 4.6; 4.8 won it back.
    ("claude-opus-4-7", 120_000.0, 200_000.0),
    ("claude-opus-4", 250_000.0, 500_000.0),
    ("claude-sonnet-5", 200_000.0, 400_000.0),
    ("claude-sonnet-4-6", 200_000.0, 400_000.0),
    ("claude-sonnet-4", 100_000.0, 150_000.0),
    ("claude-haiku-4", 80_000.0, 130_000.0),
];
/// Near the end of the window Claude Code compacts the conversation whatever
/// the model could have held — and a model we do not know is painted by this
/// alone.
const CONTEXT_WINDOW_ORANGE: f64 = 60.0;
const CONTEXT_WINDOW_RED: f64 = 85.0;

/// Prints the line — the default mode, and the one settings.json points at.
pub fn run(update_mark: Option<String>) {
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        println!("—");
        return;
    }

    let Ok(session) = serde_json::from_str::<Value>(&input) else {
        println!("—");
        return;
    };

    let limits = session
        .get("rate_limits")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    // The limits arrive here and only during a session: there is nowhere else to
    // get them. A write error is swallowed — the status line matters more.
    let _ = snapshot::save(&limits);

    println!("{}", compose(&session, &limits, update_mark, now()));
}

/// Puts the line together. Takes the moment as an argument so that a test can
/// say when it is.
fn compose(
    session: &Value,
    limits: &Map<String, Value>,
    update_mark: Option<String>,
    now: f64,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    if let Some(name) = session
        .pointer("/model/display_name")
        .and_then(Value::as_str)
        && !name.is_empty()
    {
        // The level goes next to the model: how full the circle is reads even
        // without color, and it costs one character instead of a word.
        let level = session
            .pointer("/effort/level")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match EFFORT.iter().find(|(candidate, _, _)| *candidate == level) {
            Some((_, mark, color)) => parts.push(colorized(&format!("{name} {mark}"), *color)),
            None => parts.push(name.to_string()),
        }
    }

    // Right after the model: the context belongs to this session, the windows
    // that follow belong to the account.
    if let Some(used) = session
        .pointer("/context_window/used_percentage")
        .and_then(Value::as_f64)
    {
        let color = context_color(
            used,
            session
                .pointer("/context_window/context_window_size")
                .and_then(Value::as_f64),
            session
                .pointer("/model/id")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        );
        parts.push(format!(
            "ctx {}",
            labeled_bar(used, &percent_label(used), color)
        ));
    }

    let five = limits.get("five_hour");

    if let Some(used) = five.and_then(|window| percentage(window, "used_percentage")) {
        parts.push(format!(
            "5h {}",
            labeled_bar(used, &percent_label(used), usage_color(used))
        ));
    }

    if let Some(left) = five.and_then(|window| seconds_left(window, now)) {
        let elapsed = (FIVE_HOUR_SECONDS - left as f64) / FIVE_HOUR_SECONDS * 100.0;
        parts.push(format!(
            "{} {}",
            crate::i18n::t("reset"),
            labeled_bar(elapsed, &countdown(left), CYAN)
        ));
    }

    // The weekly window goes last: it moves slowly and gets in the way of what
    // matters right now.
    let week = limits.get("seven_day");
    let mut week_bar_at = None;

    if let Some(used) = week.and_then(|window| percentage(window, "used_percentage")) {
        let drawn = parts.join(SEPARATOR);
        let separator = if drawn.is_empty() {
            0
        } else {
            SEPARATOR.chars().count()
        };
        week_bar_at = Some(width(&drawn) + separator + "7d ".len());
        parts.push(format!(
            "7d {}",
            labeled_bar(used, &percent_label(used), usage_color(used))
        ));
    }

    // The update mark comes at the very end: it is not about the current session
    // and must not shift the limit numbers the eye is already used to.
    if let Some(tag) = update_mark {
        parts.push(colorized(&format!("↑ {tag}"), UPDATE));
    }

    if parts.is_empty() {
        return "—".to_string();
    }
    let line = parts.join(SEPARATOR);

    // How much of the week has passed goes on a second line, right under the 7d
    // bar: inside the bar there is only room for one value, and two of them side
    // by side is what the eye has to compare here.
    match (
        week_bar_at,
        week.and_then(|window| seconds_left(window, now)),
    ) {
        (Some(at), Some(left)) => {
            let elapsed = (SEVEN_DAY_SECONDS - left as f64) / SEVEN_DAY_SECONDS * 100.0;
            format!("{line}\n{}{}", INDENT.repeat(at), week_strip(elapsed))
        }
        _ => line,
    }
}

/// The strip under the 7d bar: the part of the week that has passed, in grey.
/// An overline rather than a block: it is thin, and it hangs a few pixels below
/// the top of its cell — close enough to the bar to be compared with it, far
/// enough not to touch it.
///
/// Only the passed part is drawn. A track under it would have to be told apart
/// by color alone, and a terminal that lifts the contrast of dim colors — the
/// one in the JetBrains IDEs does — paints it as bright as the strip itself,
/// leaving a week that always looks over. How wide the whole window is can be
/// read off the bar above.
fn week_strip(elapsed: f64) -> String {
    format!("\x1b[0;38;5;{STRIP}m{}\x1b[0m", "‾".repeat(cells(elapsed)))
}

/// The width of a drawn part on screen: what is left once the colors are gone.
fn width(drawn: &str) -> usize {
    let mut cells = 0;
    let mut rest = drawn;
    while let Some(start) = rest.find('\x1b') {
        cells += rest[..start].chars().count();
        match rest[start..].find('m') {
            Some(end) => rest = &rest[start + end + 1..],
            None => return cells,
        }
    }
    cells + rest.chars().count()
}

/// Draws a bar with the label centered inside it. The background spans the full
/// width — the filled part in the level color, the rest dark grey — so the edge
/// of the bar reads without a frame.
fn labeled_bar(percentage: f64, label: &str, color: u8) -> String {
    let text = centered(label);
    let filled = cells(percentage);

    let text_color = text_color(color);
    let (inside, outside): (String, String) = (
        text[..filled].iter().collect(),
        text[filled..].iter().collect(),
    );

    format!(
        "\x1b[1;48;5;{color};38;5;{text_color}m{inside}\x1b[0;48;5;{EMPTY_BG};38;5;{EMPTY_FG}m{outside}\x1b[0m"
    )
}

/// The label in the middle of a bar-wide row of spaces. A label of odd length
/// does not center exactly — the leftover space goes on the left: otherwise
/// "12%" and "39m" drift noticeably towards the left edge of the bar.
fn centered(label: &str) -> Vec<char> {
    let mut label: Vec<char> = label.chars().collect();
    label.truncate(BAR_WIDTH);

    let pad = BAR_WIDTH - label.len();
    let left = pad.div_ceil(2);
    " ".repeat(left)
        .chars()
        .chain(label)
        .chain(" ".repeat(pad - left).chars())
        .collect()
}

fn cells(percentage: f64) -> usize {
    (percentage / 100.0 * BAR_WIDTH as f64)
        .round_ties_even()
        .clamp(0.0, BAR_WIDTH as f64) as usize
}

fn text_color(color: u8) -> u8 {
    if color == RED { LIGHT_TEXT } else { DARK_TEXT }
}

fn usage_color(percentage: f64) -> u8 {
    match percentage {
        percentage if percentage >= 85.0 => RED,
        percentage if percentage >= 60.0 => ORANGE,
        _ => GREEN,
    }
}

/// The worse of two colors: by the tokens the model can keep track of, and by
/// the share of the window.
fn context_color(used: f64, window: Option<f64>, model: &str) -> u8 {
    let level = |value: f64, orange: f64, red: f64| match value {
        value if value >= red => 2,
        value if value >= orange => 1,
        _ => 0,
    };

    let by_share = level(used, CONTEXT_WINDOW_ORANGE, CONTEXT_WINDOW_RED);
    let by_tokens = match (
        window,
        CONTEXT.iter().find(|(name, _, _)| model.contains(name)),
    ) {
        (Some(window), Some((_, orange, red))) => level(used / 100.0 * window, *orange, *red),
        _ => 0,
    };

    [GREEN, ORANGE, RED][by_share.max(by_tokens)]
}

fn colorized(text: &str, color: u8) -> String {
    format!("\x1b[1;38;5;{color}m{text}\x1b[0m")
}

/// Does not right-align the number: leading spaces would end up inside the label
/// and push it right of the bar center.
fn percent_label(percentage: f64) -> String {
    format!("{}%", percentage.round() as i64)
}

fn percentage(window: &Value, field: &str) -> Option<f64> {
    window.get(field)?.as_f64()
}

/// A window that has already reset is not drawn at all — Claude Code will
/// redraw the line by itself.
fn seconds_left(window: &Value, now: f64) -> Option<i64> {
    let resets_at = normalize_epoch(window.get("resets_at")?.as_f64()?);
    let seconds = (resets_at - now) as i64;
    (seconds > 0).then_some(seconds)
}

/// Prints the hours even when they are zero: without them the label shrinks from
/// six characters to three as it crosses an hour, and the digits jump towards
/// the center of the bar.
fn countdown(seconds: i64) -> String {
    let minutes = seconds / 60;
    tf!("{0}h {1}m", minutes / 60, format!("{:02}", minutes % 60))
}

/// Accepts Unix time in seconds or in milliseconds.
fn normalize_epoch(value: f64) -> f64 {
    if value > 1e12 { value / 1000.0 } else { value }
}

/// The current time as a fraction, otherwise the remainder rounds up and the
/// countdown shows one minute too many.
fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The line with the escape sequences taken out — what the eye sees.
    fn plain(line: &str) -> String {
        let mut out = String::new();
        let mut rest = line;
        while let Some(start) = rest.find('\x1b') {
            out.push_str(&rest[..start]);
            match rest[start..].find('m') {
                Some(end) => rest = &rest[start + end + 1..],
                None => return out,
            }
        }
        out.push_str(rest);
        out
    }

    fn limits_of(value: Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap_or_default()
    }

    #[test]
    fn draws_nothing_but_a_dash_for_an_empty_session() {
        assert_eq!(compose(&json!({}), &Map::new(), None, 0.0), "—");
    }

    #[test]
    fn draws_the_windows_it_was_given() {
        let limits = limits_of(json!({
            "five_hour": {"used_percentage": 42.0},
            "seven_day": {"used_percentage": 8.0},
        }));
        let line = plain(&compose(&json!({}), &limits, None, 0.0));

        assert!(line.contains("5h"), "{line}");
        assert!(line.contains("42%"), "{line}");
        assert!(line.contains("7d"), "{line}");
        assert!(line.contains("8%"), "{line}");
        assert!(!line.contains("reset"), "{line}");
    }

    #[test]
    fn leaves_out_a_window_that_is_not_there() {
        let limits = limits_of(json!({"seven_day": {"used_percentage": 8.0}}));
        let line = plain(&compose(&json!({}), &limits, None, 0.0));

        assert!(!line.contains("5h"), "{line}");
        assert!(line.contains("7d"), "{line}");
    }

    #[test]
    fn counts_down_to_the_reset() {
        let limits =
            limits_of(json!({"five_hour": {"used_percentage": 10.0, "resets_at": 9000.0}}));
        // Two hours and forty-one minutes before the window resets.
        let line = plain(&compose(
            &json!({}),
            &limits,
            None,
            9000.0 - (2.0 * 3600.0 + 41.0 * 60.0),
        ));

        assert!(line.contains("2h 41m"), "{line}");
    }

    #[test]
    fn leaves_out_a_window_that_has_already_reset() {
        let limits = limits_of(json!({"five_hour": {"used_percentage": 10.0, "resets_at": 100.0}}));
        let line = plain(&compose(&json!({}), &limits, None, 200.0));

        assert!(!line.contains("0h 00m"), "{line}");
        assert!(line.contains("10%"), "{line}");
    }

    #[test]
    fn reads_a_reset_stamped_in_milliseconds() {
        // Milliseconds are told from seconds by their size, so the stamp has to
        // be one a real session would carry.
        let resets_at = 1_770_000_000.0_f64;
        let limits = limits_of(json!({"five_hour": {"resets_at": resets_at * 1000.0}}));
        let line = plain(&compose(&json!({}), &limits, None, resets_at - 3600.0));

        assert!(line.contains("1h 00m"), "{line}");
    }

    #[test]
    fn shows_the_model_and_its_effort() {
        let session = json!({"model": {"display_name": "Opus 5"}, "effort": {"level": "high"}});
        let line = plain(&compose(&session, &Map::new(), None, 0.0));

        assert_eq!(line, "Opus 5 ◑");
    }

    #[test]
    fn shows_a_model_whose_effort_is_not_a_level_we_know() {
        let session = json!({"model": {"display_name": "Opus 5"}, "effort": {"level": "turbo"}});
        assert_eq!(plain(&compose(&session, &Map::new(), None, 0.0)), "Opus 5");

        let session = json!({"model": {"display_name": "Opus 5"}});
        assert_eq!(plain(&compose(&session, &Map::new(), None, 0.0)), "Opus 5");
    }

    #[test]
    fn shows_how_full_the_context_is_right_after_the_model() {
        let session = json!({
            "model": {"id": "claude-opus-5", "display_name": "Opus 5"},
            "context_window": {"used_percentage": 12, "context_window_size": 1_000_000},
        });
        let limits = limits_of(json!({"five_hour": {"used_percentage": 42.0}}));
        let line = plain(&compose(&session, &limits, None, 0.0));

        let (model, context, five) = (
            line.find("Opus 5").unwrap(),
            line.find("ctx").unwrap(),
            line.find("5h").unwrap(),
        );
        assert!(model < context && context < five, "{line}");
        assert!(line.contains("12%"), "{line}");
    }

    #[test]
    fn leaves_out_the_context_before_claude_code_has_counted_it() {
        let session = json!({
            "model": {"id": "claude-opus-5", "display_name": "Opus 5"},
            "context_window": {"used_percentage": null, "context_window_size": 1_000_000},
        });
        assert_eq!(plain(&compose(&session, &Map::new(), None, 0.0)), "Opus 5");
    }

    #[test]
    fn paints_the_context_by_the_tokens_the_model_can_hold() {
        let million = Some(1_000_000.0);
        assert_eq!(context_color(20.0, million, "claude-opus-5"), GREEN);
        assert_eq!(context_color(30.0, million, "claude-opus-5"), ORANGE);
        assert_eq!(context_color(55.0, million, "claude-opus-5"), RED);

        // A version that holds less than its family is not painted by the family.
        assert_eq!(context_color(15.0, million, "claude-opus-4-8"), GREEN);
        assert_eq!(context_color(15.0, million, "claude-opus-4-7"), ORANGE);
        assert_eq!(
            context_color(25.0, million, "us.anthropic.claude-opus-4-7"),
            RED
        );

        let small = Some(200_000.0);
        assert_eq!(context_color(35.0, small, "claude-haiku-4-5"), GREEN);
        assert_eq!(context_color(45.0, small, "claude-haiku-4-5"), ORANGE);
        assert_eq!(context_color(70.0, small, "claude-haiku-4-5"), RED);
    }

    #[test]
    fn paints_the_context_by_the_window_where_the_tokens_say_nothing() {
        let small = Some(200_000.0);
        // The tokens of Opus 5 do not fit a small window: its end decides.
        assert_eq!(context_color(59.0, small, "claude-opus-5"), GREEN);
        assert_eq!(context_color(60.0, small, "claude-opus-5"), ORANGE);
        assert_eq!(context_color(85.0, small, "claude-opus-5"), RED);

        assert_eq!(context_color(59.0, Some(1_000_000.0), "some-model"), GREEN);
        assert_eq!(context_color(85.0, Some(1_000_000.0), "some-model"), RED);
        assert_eq!(context_color(70.0, None, "claude-haiku-4-5"), ORANGE);
    }

    #[test]
    fn puts_the_update_mark_last() {
        let limits = limits_of(json!({"five_hour": {"used_percentage": 42.0}}));
        let line = plain(&compose(&json!({}), &limits, Some("v1.4.0".into()), 0.0));

        assert!(line.ends_with("↑ v1.4.0"), "{line}");
    }

    #[test]
    fn paints_a_bar_by_how_full_it_is() {
        assert_eq!(usage_color(0.0), GREEN);
        assert_eq!(usage_color(59.9), GREEN);
        assert_eq!(usage_color(60.0), ORANGE);
        assert_eq!(usage_color(84.9), ORANGE);
        assert_eq!(usage_color(85.0), RED);
        assert_eq!(usage_color(100.0), RED);
    }

    /// How many of the ten cells the fill covers.
    fn filled_cells(bar: &str) -> usize {
        let empty = format!("\x1b[0;48;5;{EMPTY_BG};38;5;{EMPTY_FG}m");
        let (fill, _) = bar
            .split_once(&empty)
            .expect("a bar always has an empty part");
        plain(fill).chars().count()
    }

    #[test]
    fn fills_the_bar_in_proportion() {
        assert_eq!(filled_cells(&labeled_bar(50.0, "50%", GREEN)), 5);
        assert_eq!(filled_cells(&labeled_bar(0.0, "", GREEN)), 0);
        assert_eq!(filled_cells(&labeled_bar(100.0, "", GREEN)), 10);
        assert_eq!(filled_cells(&labeled_bar(42.0, "42%", GREEN)), 4);
    }

    /// The second line as the eye sees it, colors and all cut away.
    fn strip_line(line: &str) -> String {
        plain(line.split_once('\n').expect("a strip under the line").1)
    }

    #[test]
    fn draws_how_much_of_the_week_has_passed_under_the_bar() {
        let week = 7.0 * 24.0 * 3600.0;
        let limits = limits_of(json!({
            "five_hour": {"used_percentage": 3.0},
            "seven_day": {"used_percentage": 11.0, "resets_at": week},
        }));
        let line = compose(&json!({}), &limits, None, week * 0.6);
        let (first, strip) = (plain(&line), strip_line(&line));

        // The strip stands under the bar itself, not under the "7d" labelling it.
        let label_at = first[..first.find("7d").unwrap()].chars().count();
        assert_eq!(
            strip.chars().take_while(|cell| *cell == '\u{2800}').count(),
            label_at + "7d ".len(),
            "{line:?}"
        );
        // Six of the ten cells of the week have passed, and nothing beyond them
        // is drawn: a track would only be told apart by color.
        assert_eq!(strip.matches('\u{203e}').count(), 6);
        assert!(!strip.contains(&format!("38;5;{EMPTY_BG}")), "{line:?}");

        let barely = compose(&json!({}), &limits, None, week * 0.1);
        assert_eq!(strip_line(&barely).matches('\u{203e}').count(), 1);
    }

    #[test]
    fn draws_the_strip_only_when_the_week_says_when_it_resets() {
        let timeless = limits_of(json!({"seven_day": {"used_percentage": 11.0}}));
        let line = compose(&json!({}), &timeless, None, 0.0);
        assert!(!line.contains('\n'), "{line:?}");

        // A week that has already reset has no time left to draw either.
        let over = limits_of(json!({"seven_day": {"used_percentage": 11.0, "resets_at": 100.0}}));
        let line = compose(&json!({}), &over, None, 200.0);
        assert!(!line.contains('\n'), "{line:?}");
    }

    #[test]
    fn keeps_a_bar_ten_cells_wide_whatever_the_label() {
        for label in ["", "5%", "100%", "a very long label indeed"] {
            let bar = labeled_bar(40.0, label, GREEN);
            assert_eq!(plain(&bar).chars().count(), BAR_WIDTH, "label {label:?}");
        }
    }

    #[test]
    fn writes_on_red_in_a_color_that_can_be_read() {
        assert!(labeled_bar(90.0, "90%", RED).contains(&format!("38;5;{LIGHT_TEXT}m")));
        assert!(labeled_bar(10.0, "10%", GREEN).contains(&format!("38;5;{DARK_TEXT}m")));
    }

    #[test]
    fn spells_a_countdown_with_its_hours() {
        assert_eq!(plain(&countdown(2 * 3600 + 41 * 60)), "2h 41m");
        // Zero hours are spelled out too, or the label changes width mid-session.
        assert_eq!(plain(&countdown(59 * 60)), "0h 59m");
    }

    #[test]
    fn rounds_a_percentage_the_way_the_label_does() {
        assert_eq!(percent_label(0.4), "0%");
        assert_eq!(percent_label(42.6), "43%");
        assert_eq!(percent_label(100.0), "100%");
    }
}
