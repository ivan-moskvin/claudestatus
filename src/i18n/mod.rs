//! Every string the user is meant to read.
//!
//! English is the source language: [`t`] is called with the English text
//! itself, so a missing translation still prints a readable message instead of
//! a bare key, and the call site stays legible without looking the key up. A
//! language is added by one more catalog plus one line in [`catalog`] — nothing
//! branches on the language outside this module.

mod ru;

use std::sync::OnceLock;

/// A language tag, spelled the way `CLISTATUS_LANG` accepts it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Ru,
}

/// The languages the utility speaks, English first — it is the source language
/// and has no catalog of its own.
///
/// Nothing branches on this while the utility runs: it exists so that a test
/// can go over every language at once, which is how the panel labels are
/// checked for a glyph they might not have.
#[allow(dead_code)]
pub const LANGS: &[Lang] = &[Lang::En, Lang::Ru];

/// The language of this process. Resolved once: it cannot change while we run,
/// and the status line is called often enough not to redo the lookup.
pub fn current() -> Lang {
    static CURRENT: OnceLock<Lang> = OnceLock::new();
    *CURRENT.get_or_init(detect)
}

/// Translates the English source text, returning it unchanged when the catalog
/// has nothing for it.
pub fn t(text: &str) -> &str {
    translate(current(), text)
}

/// Translates and fills the numbered holes: `tf!("Removed {0}", path)`.
///
/// Rust wants a format string known at compile time, and a translated one never
/// is — hence the holes are filled here rather than by `format!`.
macro_rules! tf {
    ($text:literal $(, $arg:expr)* $(,)?) => {
        $crate::i18n::fill($crate::i18n::t($text), &[$(&$arg.to_string()),*])
    };
}
pub(crate) use tf;

/// Replaces every `{N}` with the argument of that number. An argument that is
/// not there leaves the hole as it was — a message with one hole too few is
/// still readable, and a panic would take the status line down with it.
pub fn fill(template: &str, args: &[&String]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;

    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];

        let Some(end) = rest.find('}') else { break };
        match rest[1..end].parse::<usize>() {
            Ok(index) if index < args.len() => out.push_str(args[index]),
            _ => out.push_str(&rest[..=end]),
        }
        rest = &rest[end + 1..];
    }

    out.push_str(rest);
    out
}

/// Translates into a language named explicitly, whatever the language of the
/// process is. The utility itself always speaks the one language it detected;
/// this is for looking a catalog over as a whole — the panel has to be able to
/// draw its labels in every language, not just in the current one.
pub fn translate(lang: Lang, text: &str) -> &str {
    catalog(lang)
        .and_then(|entries| {
            entries
                .iter()
                .find(|(source, _)| *source == text)
                .map(|(_, translated)| *translated)
        })
        .unwrap_or(text)
}

fn catalog(lang: Lang) -> Option<&'static [(&'static str, &'static str)]> {
    match lang {
        Lang::En => None,
        Lang::Ru => Some(ru::CATALOG),
    }
}

/// An explicit override wins over the system, so a Russian desktop can still be
/// checked in English while debugging.
fn detect() -> Lang {
    if let Some(lang) = std::env::var("CLISTATUS_LANG")
        .ok()
        .or_else(|| std::env::var("CLAUDESTATUS_LANG").ok())
        .and_then(|value| parse(&value))
    {
        return lang;
    }
    system_lang()
}

/// Reads the POSIX locale variables in their usual precedence.
#[cfg(unix)]
fn system_lang() -> Lang {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .find_map(|value| parse(&value))
        .unwrap_or(Lang::En)
}

#[cfg(windows)]
fn system_lang() -> Lang {
    // Windows has no locale environment to read: the language of the interface
    // is what the user set for the whole system.
    unsafe extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
    }
    // The low ten bits are the primary language; 0x19 is Russian.
    const LANG_RUSSIAN: u16 = 0x19;
    let id = unsafe { GetUserDefaultUILanguage() };
    if id & 0x3ff == LANG_RUSSIAN {
        Lang::Ru
    } else {
        Lang::En
    }
}

/// Takes the language out of a locale name ("ru_RU.UTF-8", "ru", "en_US").
/// Anything we have no catalog for is English.
fn parse(value: &str) -> Option<Lang> {
    let value = value.trim().to_lowercase();
    if value.is_empty() {
        return None;
    }
    if value.starts_with("ru") {
        return Some(Lang::Ru);
    }
    Some(Lang::En)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_language_out_of_a_locale_name() {
        assert_eq!(parse("ru_RU.UTF-8"), Some(Lang::Ru));
        assert_eq!(parse("ru"), Some(Lang::Ru));
        assert_eq!(parse("en_US.UTF-8"), Some(Lang::En));
        assert_eq!(parse("  RU  "), Some(Lang::Ru));
        assert_eq!(parse(""), None);
        // A language with no catalog is English, not an error.
        assert_eq!(parse("fr_FR"), Some(Lang::En));
    }

    #[test]
    fn fills_the_numbered_holes() {
        let one = "v1.2.3".to_string();
        let two = "v1.3.0".to_string();
        assert_eq!(fill("{0} then {1}", &[&one, &two]), "v1.2.3 then v1.3.0");
        assert_eq!(
            fill("{1} before {0}", &[&one, &two]),
            "v1.3.0 before v1.2.3"
        );
        assert_eq!(fill("{0} and {0}", &[&one]), "v1.2.3 and v1.2.3");
        assert_eq!(fill("nothing to fill", &[]), "nothing to fill");
    }

    #[test]
    fn leaves_a_hole_it_has_no_argument_for() {
        // A message short of an argument still has to print: the status line
        // must not go down over a typo in a catalog.
        assert_eq!(
            fill(
                "{0} and {1}",
                &["one".to_string()].iter().collect::<Vec<_>>()
            ),
            "one and {1}"
        );
        assert_eq!(fill("{not a number}", &[]), "{not a number}");
        assert_eq!(fill("unclosed {0", &[]), "unclosed {0");
    }

    #[test]
    fn leaves_the_source_text_alone_when_nothing_translates_it() {
        assert_eq!(translate(Lang::En, "reset"), "reset");
        assert_eq!(
            translate(Lang::Ru, "nothing translates this"),
            "nothing translates this"
        );
    }

    #[test]
    fn every_translation_keeps_its_format_arguments() {
        for (source, translated) in ru::CATALOG {
            let mut wanted: Vec<&str> = placeholders(source);
            let mut got: Vec<&str> = placeholders(translated);
            wanted.sort_unstable();
            got.sort_unstable();
            assert_eq!(
                wanted, got,
                "the translation of {source:?} loses an argument"
            );
        }
    }

    #[test]
    fn translates_nothing_twice() {
        let mut sources: Vec<&str> = ru::CATALOG.iter().map(|(source, _)| *source).collect();
        sources.sort_unstable();
        let count = sources.len();
        sources.dedup();
        assert_eq!(
            sources.len(),
            count,
            "the catalog translates the same text twice"
        );
    }

    /// The named holes a message has: `{}` alone is positional and may repeat,
    /// so it says nothing about the order being kept.
    fn placeholders(text: &str) -> Vec<&str> {
        let mut found = Vec::new();
        let mut rest = text;
        while let Some(start) = rest.find('{') {
            rest = &rest[start..];
            let Some(end) = rest.find('}') else { break };
            found.push(&rest[..=end]);
            rest = &rest[end + 1..];
        }
        found
    }
}
