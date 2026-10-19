//! Drawing the panel. What the device does with the picture is the business of
//! divoomkit; what goes on the picture is ours.

use std::time::Duration;

use divoomkit::{BarStyle, Canvas, Frame, Rgb, Size};

use crate::i18n::{t, tf};
use crate::panel::usage::{FIVE_HOUR_SECONDS, SEVEN_DAY_SECONDS, Snapshot, Window};

/// The color of the Claude sparkle in the header.
const CLAUDE: Rgb = Rgb(0xd9, 0x77, 0x57);

/// The header with the sparkle on top, three bars with their labels below it.
const LABEL_X: i32 = 6;
const LABEL_WIDE: i32 = 22;
const BAR_X: i32 = 32;
const BAR_WIDTH: u32 = 128 - BAR_X as u32 - LABEL_X as u32;
const BAR_HEIGHT: u32 = 20;
const ROW_GAP: i32 = 7;
/// The week row is split in two: the usage above, the week running out below,
/// together taking the height of an ordinary row.
const WEEK_BAR_HEIGHT: u32 = 14;
const WEEK_GAP: i32 = 2;
const WEEK_ELAPSED_HEIGHT: u32 = BAR_HEIGHT - WEEK_BAR_HEIGHT - WEEK_GAP as u32;
const HEADER_Y: i32 = 8;
const SPARK_SIZE: i32 = 17;
const FIRST_ROW_Y: i32 = 38;
const RESET_ICON: i32 = 15;

const GLYPH_HEIGHT: i32 = 7;

pub fn draw(state: &Snapshot) -> Result<Frame, String> {
    let mut canvas = Canvas::new(Size::Px128);

    sparkle(&mut canvas, LABEL_X, HEADER_Y, SPARK_SIZE, CLAUDE);
    canvas.text(
        LABEL_X + SPARK_SIZE + 7,
        HEADER_Y + (SPARK_SIZE - GLYPH_HEIGHT * 2) / 2,
        2,
        Rgb::WHITE,
        "CLAUDE",
    );

    // The numbers only grow during an active Claude Code session: if the
    // snapshot has not been updated for a while, the percentages below describe
    // the past, and that has to be visible. Smaller than the header, or the mark
    // stands right against it and reads as part of the word — "CLAUDE2H".
    if state.stale {
        let label = age_label(state.age);
        let width = divoomkit::font::text_width(&label, 1) as i32;
        canvas.text(
            128 - LABEL_X - width,
            HEADER_Y + (SPARK_SIZE - GLYPH_HEIGHT) / 2,
            1,
            Rgb::GREY,
            &label,
        );
    }

    if state.trouble.is_some() {
        canvas.text_centered(52, 3, Rgb::GREY, t("NO"));
        canvas.text_centered(86, 2, Rgb::GREY, t("DATA"));
        return canvas
            .finish()
            .map_err(|err| tf!("could not draw the panel: {0}", err));
    }

    let (five, week) = (state.window("five_hour"), state.window("seven_day"));

    let mut y = FIRST_ROW_Y;
    canvas.text(
        LABEL_X,
        y + (BAR_HEIGHT as i32 - GLYPH_HEIGHT * 2) / 2,
        2,
        Rgb::GREY,
        "5H",
    );
    bar(
        &mut canvas,
        y,
        BAR_HEIGHT,
        five.fraction(),
        five.tint(FIVE_HOUR_SECONDS),
        &five.percent_label(),
    );

    y += BAR_HEIGHT as i32 + ROW_GAP;
    reset_icon(
        &mut canvas,
        LABEL_X + (LABEL_WIDE - RESET_ICON) / 2,
        y + (BAR_HEIGHT as i32 - RESET_ICON) / 2,
        RESET_ICON,
        Rgb::GREY,
    );
    bar(
        &mut canvas,
        y,
        BAR_HEIGHT,
        five.elapsed_fraction(FIVE_HOUR_SECONDS),
        reset_tint(five),
        &reset_label(five),
    );

    y += BAR_HEIGHT as i32 + ROW_GAP;
    canvas.text(
        LABEL_X,
        y + (BAR_HEIGHT as i32 - GLYPH_HEIGHT * 2) / 2,
        2,
        Rgb::GREY,
        "7D",
    );
    // The grey strip below the week is the week itself running out. Usage ahead
    // of it is spent faster than the window gives it back; behind it there is
    // room to spare. Without a resets_at there is no such strip to draw, and the
    // usage takes the whole row.
    if week.timed() {
        bar(
            &mut canvas,
            y,
            WEEK_BAR_HEIGHT,
            week.fraction(),
            week.tint(SEVEN_DAY_SECONDS),
            &week.percent_label(),
        );
        canvas.bar(
            (
                BAR_X,
                y + (WEEK_BAR_HEIGHT as i32) + WEEK_GAP,
                BAR_WIDTH,
                WEEK_ELAPSED_HEIGHT,
            ),
            week.elapsed_fraction(SEVEN_DAY_SECONDS),
            BarStyle {
                fill: Rgb::GREY,
                ..Default::default()
            },
        );
    } else {
        bar(
            &mut canvas,
            y,
            BAR_HEIGHT,
            week.fraction(),
            week.tint(SEVEN_DAY_SECONDS),
            &week.percent_label(),
        );
    }

    canvas
        .finish()
        .map_err(|err| tf!("could not draw the panel: {0}", err))
}

fn bar(canvas: &mut Canvas, y: i32, height: u32, fraction: f32, fill: Rgb, label: &str) {
    canvas.bar(
        (BAR_X, y, BAR_WIDTH, height),
        fraction,
        BarStyle {
            fill,
            label,
            // Red is dark enough for black digits to drown in it, and the status
            // line writes on it in white too.
            label_on_fill: if fill == Rgb::RED {
                Rgb::WHITE
            } else {
                Rgb::BLACK
            },
            ..Default::default()
        },
    );
}

fn reset_label(five: Window) -> String {
    if five.expired {
        return t("RESET").into();
    }
    countdown_label(five.seconds_left)
}

fn reset_tint(five: Window) -> Rgb {
    if five.expired { Rgb::GREY } else { Rgb::CYAN }
}

fn age_label(age: Duration) -> String {
    let hours = age.as_secs() / 3600;
    if hours > 0 {
        return format!("{hours}H");
    }
    format!("{}M", age.as_secs() / 60)
}

/// "2:41" until the window resets. The status line spells the time with letters,
/// but in a 5×7 grid the Russian letter for an hour is indistinguishable from a
/// four, and the label reads as "24 41M". A colon can be mistaken for nothing
/// and needs no translation.
fn countdown_label(seconds: i64) -> String {
    if seconds <= 0 {
        return "-".into();
    }
    let minutes = seconds / 60;
    format!("{}:{:02}", minutes / 60, minutes % 60)
}

/// A petal is the wider the closer it is to the center: the axial rays have more
/// room, the diagonal ones are half as long — that keeps the sparkle from
/// turning into an even star.
fn sparkle(canvas: &mut Canvas, x: i32, y: i32, size: i32, color: Rgb) {
    let radius = size / 2;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let (ax, ay) = ((dx.abs()) as f64, (dy.abs()) as f64);
            let distance = ((dx * dx + dy * dy) as f64).sqrt();
            if distance > radius as f64 {
                continue;
            }
            let axial = ax <= (radius as f64 - ay) / 2.6 || ay <= (radius as f64 - ax) / 2.6;
            let diagonal = (ax - ay).abs() <= (radius as f64 - ax.max(ay)) / 2.6
                && distance <= radius as f64 * 0.82;

            if axial || diagonal {
                canvas.pixel(x + radius + dx, y + radius + dy, color);
            }
        }
    }
}

fn reset_icon(canvas: &mut Canvas, x: i32, y: i32, size: i32, color: Rgb) {
    let radius = size as f64 / 2.0;
    let center = radius - 0.5;

    for dy in 0..size {
        for dx in 0..size {
            let (ox, oy) = (dx as f64 - center, dy as f64 - center);
            let distance = ox.hypot(oy);
            if distance > radius || distance < radius - 2.5 {
                continue;
            }
            // The ring is broken at the upper right — that is where the arrow
            // head starts.
            let angle = ox.atan2(-oy);
            if angle > 0.2 && angle < 1.5 {
                continue;
            }
            canvas.pixel(x + dx, y + dy, color);
        }
    }

    // The arrow head at the break: three rows narrowing to the right.
    let (tip_x, tip_y) = (x + size / 2, y);
    for row in 0..3 {
        canvas.rect((tip_x + row, tip_y + row, (3 - row) as u32, 1), color);
        canvas.rect((tip_x + row, tip_y - row, (3 - row) as u32, 1), color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{LANGS, translate};

    #[test]
    fn spells_the_countdown_in_digits() {
        assert_eq!(countdown_label(2 * 3600 + 41 * 60), "2:41");
        assert_eq!(countdown_label(59 * 60), "0:59");
        assert_eq!(countdown_label(0), "-");
    }

    #[test]
    fn tells_how_old_the_numbers_are() {
        assert_eq!(age_label(Duration::from_secs(3 * 3600)), "3H");
        assert_eq!(age_label(Duration::from_secs(20 * 60)), "20M");
        assert_eq!(age_label(Duration::from_secs(90)), "1M");
    }

    #[test]
    fn every_label_of_the_panel_has_a_glyph_in_every_language() {
        // An unknown character is drawn as "?" — a panel of question marks is
        // worse than an untranslated word.
        for lang in LANGS {
            for label in ["RESET", "NO", "DATA"] {
                let drawn = translate(*lang, label);
                assert!(
                    divoomkit::font::covers(drawn),
                    "{drawn:?} has no glyph in {lang:?}"
                );
            }
        }
    }

    #[test]
    fn every_label_of_the_panel_fits_its_bar() {
        for lang in LANGS {
            let drawn = translate(*lang, "RESET");
            assert!(
                divoomkit::font::text_width(drawn, 2) <= BAR_WIDTH,
                "{drawn:?} does not fit the bar"
            );
        }
    }

    #[test]
    fn draws_a_panel_of_the_size_the_device_takes() {
        let frame = draw(&Snapshot::default()).unwrap();
        assert_eq!(&frame.bytes()[..6], b"GIF89a");
        // 128 pixels, little-endian, right after the header.
        assert_eq!(&frame.bytes()[6..10], &[128, 0, 128, 0]);
    }

    #[test]
    fn draws_something_else_when_there_are_no_numbers() {
        let empty = draw(&Snapshot::default()).unwrap();
        let with_numbers = draw(&crate::panel::usage::for_test(42.0, 9000, 400_000)).unwrap();
        assert_ne!(empty.hash(), with_numbers.hash());
    }

    #[test]
    fn shows_how_much_of_the_week_has_passed() {
        let day = 24 * 3600;
        let early = draw(&crate::panel::usage::for_test(42.0, 9000, 6 * day)).unwrap();
        let late = draw(&crate::panel::usage::for_test(42.0, 9000, day)).unwrap();
        assert_ne!(
            early.hash(),
            late.hash(),
            "the same usage at a different point of the week"
        );
    }

    #[test]
    fn the_week_row_stays_within_its_row() {
        assert_eq!(
            WEEK_BAR_HEIGHT + WEEK_GAP as u32 + WEEK_ELAPSED_HEIGHT,
            BAR_HEIGHT
        );
        // The percentage is written at scale 2 and must still fit the shortened bar.
        assert!(WEEK_BAR_HEIGHT >= (GLYPH_HEIGHT * 2) as u32);
    }
}
