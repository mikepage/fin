//! Translations. Source strings are English and are their own key; `nl.rs` holds the
//! Dutch catalog, compiled in (no files to load, no extra crates).
//!
//! - `t!("Save")` translates one string; `t!("{} of {}", a, b)` fills `{}` in order.
//! - `tn!(n, "{} transaction", "{} transactions", n)` picks the form by count.
//! - `error(msg)` translates a message from the backend, which speaks English:
//!   exact messages, and templates with `{}` for the parts that vary.
//!
//! The language comes from the data file (`Dataset::language`); `App` sets it before
//! rendering and renders again when it changes. The tests check that every string the
//! app uses, and every error the backend returns, has a Dutch translation.

mod nl;

use std::cell::Cell;
use std::collections::HashMap;

use fin_shared::{format_cents_in, parse_amount, Lang};

thread_local! {
    static LANG: Cell<Lang> = const { Cell::new(Lang::Nl) };
    static NL_UI: HashMap<&'static str, &'static str> = nl::UI.iter().copied().collect();
}

/// Translates a string (a `t!` literal). English, and anything missing, stays as is.
macro_rules! t {
    ($en:literal) => {
        $crate::i18n::tr($en)
    };
    ($en:literal, $($arg:expr),+ $(,)?) => {
        $crate::i18n::fill($crate::i18n::tr($en), &[$(::std::string::ToString::to_string(&$arg)),+])
    };
}

/// A string with a count: the first form for 1, the second otherwise (both languages
/// work like that). The count is not filled in by itself: pass it as an argument.
macro_rules! tn {
    ($n:expr, $one:literal, $other:literal $(, $arg:expr)* $(,)?) => {
        $crate::i18n::fill(
            $crate::i18n::tr(if $n == 1 { $one } else { $other }),
            &[$(::std::string::ToString::to_string(&$arg)),*],
        )
    };
}

pub fn lang() -> Lang {
    LANG.with(Cell::get)
}

pub fn set_lang(lang: Lang) {
    LANG.with(|l| l.set(lang));
}

pub fn tr(en: &'static str) -> &'static str {
    match lang() {
        Lang::En => en,
        Lang::Nl => NL_UI.with(|m| m.get(en).copied()).unwrap_or(en),
    }
}

/// Replaces each `{}` in `template` with the next argument.
pub fn fill(template: &str, args: &[String]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut args = args.iter();
    let mut rest = template;
    while let Some(i) = rest.find("{}") {
        out.push_str(&rest[..i]);
        out.push_str(args.next().map_or("", String::as_str));
        rest = &rest[i + 2..];
    }
    out.push_str(rest);
    out
}

/// A backend message in the app's language.
pub fn error(msg: &str) -> String {
    match lang() {
        Lang::En => msg.to_string(),
        Lang::Nl => translate(nl::ERRORS, msg, 2),
    }
}

fn translate(catalog: &[(&str, &str)], msg: &str, depth: u32) -> String {
    if let Some((_, to)) = catalog.iter().find(|(en, _)| *en == msg) {
        return to.to_string();
    }
    // The most specific template first ("{}: {}" last).
    let mut templates: Vec<&(&str, &str)> = catalog.iter().filter(|(en, _)| en.contains("{}")).collect();
    templates.sort_by_key(|(en, _)| std::cmp::Reverse(en.len() - 2 * en.matches("{}").count()));
    for (en, to) in templates {
        if let Some(args) = match_template(en, msg) {
            let before: Vec<&str> = en.split("{}").collect();
            let args: Vec<String> =
                args.into_iter().enumerate().map(|(i, a)| localize_arg(catalog, a, before[i].ends_with("€ "), depth)).collect();
            return fill(to, &args);
        }
    }
    msg.to_string()
}

/// An amount (after a `€ `, English notation) in the app's notation; a nested message
/// translated.
fn localize_arg(catalog: &[(&str, &str)], arg: &str, amount: bool, depth: u32) -> String {
    if let Some(c) = amount.then(|| parse_amount(arg)).flatten() {
        return cents(c);
    }
    if depth > 0 {
        translate(catalog, arg, depth - 1)
    } else {
        arg.to_string()
    }
}

/// The parts of `msg` that stand in for the `{}` of `template`, if it fits.
fn match_template<'a>(template: &str, msg: &'a str) -> Option<Vec<&'a str>> {
    let parts: Vec<&str> = template.split("{}").collect();
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !msg.starts_with(first) || !msg.ends_with(last) || msg.len() < first.len() + last.len() {
        return None;
    }
    let body = &msg[first.len()..msg.len() - last.len()];
    let mut args = Vec::new();
    let mut rest = body;
    for lit in &parts[1..parts.len() - 1] {
        let i = rest.find(lit)?;
        args.push(&rest[..i]);
        rest = &rest[i + lit.len()..];
    }
    args.push(rest);
    Some(args)
}

/// Cents in the app's notation: `1.234,56` or `1,234.56`.
pub fn cents(c: i64) -> String {
    format_cents_in(c, lang())
}

/// `€ 1.234,56` or `€ 1,234.56`.
pub fn euro(c: i64) -> String {
    format!("€ {}", cents(c))
}

/// Without the cents when they are whole: `1.250`, `12,50`.
pub fn whole(c: i64) -> String {
    let s = cents(c);
    match s.strip_suffix(",00").or_else(|| s.strip_suffix(".00")) {
        Some(w) => w.to_string(),
        None => s,
    }
}

const MONTHS_NL: [&str; 12] = [
    "januari", "februari", "maart", "april", "mei", "juni", "juli", "augustus", "september", "oktober", "november",
    "december",
];
const MONTHS_EN: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November",
    "December",
];
const SHORT_NL: [&str; 12] = ["jan", "feb", "mrt", "apr", "mei", "jun", "jul", "aug", "sep", "okt", "nov", "dec"];
const SHORT_EN: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// Month name by index 0–11, as written mid-sentence ("januari", "January").
pub fn month_name(i: usize) -> &'static str {
    match lang() {
        Lang::Nl => MONTHS_NL[i.min(11)],
        Lang::En => MONTHS_EN[i.min(11)],
    }
}

/// Short month name by index 0–11 ("mrt", "Mar").
pub fn month_short(i: usize) -> &'static str {
    match lang() {
        Lang::Nl => SHORT_NL[i.min(11)],
        Lang::En => SHORT_EN[i.min(11)],
    }
}

/// A date as `30-9-2026` (nl) or `30 Sep 2026` (en).
pub fn date(year: i32, month: u32, day: u32) -> String {
    match lang() {
        Lang::Nl => format!("{day}-{month}-{year}"),
        Lang::En => format!("{day} {} {year}", month_short(month.clamp(1, 12) as usize - 1)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// String literals in Rust source, with where they are: (literal, the code before it).
    /// Skips comments, char literals and lifetimes; understands escapes and raw strings.
    fn literals(src: &str) -> Vec<(String, usize)> {
        let b = src.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                b'/' if b.get(i + 1) == Some(&b'/') => {
                    while i < b.len() && b[i] != b'\n' {
                        i += 1;
                    }
                }
                b'\'' => {
                    // 'x', '\n', '\u{201c}' or a lifetime.
                    if b.get(i + 1) == Some(&b'\\') {
                        i += 2;
                        while i < b.len() && b[i] != b'\'' {
                            i += 1;
                        }
                        i += 1;
                    } else if let Some(end) = src[i + 1..].chars().next().map(|c| i + 1 + c.len_utf8()) {
                        i = if b.get(end) == Some(&b'\'') { end + 1 } else { i + 1 };
                    } else {
                        i += 1;
                    }
                }
                b'r' if b.get(i + 1) == Some(&b'#') || (b.get(i + 1) == Some(&b'"') && (i == 0 || !b[i - 1].is_ascii_alphanumeric())) => {
                    let hashes = b[i + 1..].iter().take_while(|c| **c == b'#').count();
                    let start = i + 1 + hashes + 1;
                    let close = format!("\"{}", "#".repeat(hashes));
                    let end = src[start..].find(&close).map_or(b.len(), |e| start + e);
                    out.push((src[start..end].to_string(), i));
                    i = end + close.len();
                }
                b'"' => {
                    let mut s = String::new();
                    let start = i;
                    i += 1;
                    while i < b.len() && b[i] != b'"' {
                        if b[i] == b'\\' {
                            i += 1;
                            match b[i] {
                                b'n' => s.push('\n'),
                                b't' => s.push('\t'),
                                b'u' => {
                                    let close = src[i..].find('}').unwrap() + i;
                                    let hex = &src[i + 2..close];
                                    s.push(char::from_u32(u32::from_str_radix(hex, 16).unwrap()).unwrap());
                                    i = close;
                                }
                                b'\n' => {
                                    // Line continuation: skip the newline and leading spaces.
                                    while i + 1 < b.len() && b[i + 1].is_ascii_whitespace() {
                                        i += 1;
                                    }
                                }
                                c => s.push(c as char),
                            }
                            i += 1;
                        } else {
                            let c = src[i..].chars().next().unwrap();
                            s.push(c);
                            i += c.len_utf8();
                        }
                    }
                    out.push((s, start));
                    i += 1;
                }
                _ => i += 1,
            }
        }
        out
    }

    /// The code outside `#[cfg(test)] mod tests`.
    fn without_tests(src: &str) -> &str {
        src.find("#[cfg(test)]\nmod ").map_or(src, |i| &src[..i])
    }

    /// Every literal passed to `t!` (the first) or `tn!` (the first two).
    fn ui_strings(src: &str) -> Vec<String> {
        let lits = literals(src);
        let mut out = Vec::new();
        for (name, count) in [("t!(", 1), ("tn!(", 2)] {
            let mut from = 0;
            while let Some(p) = src[from..].find(name).map(|p| p + from) {
                from = p + name.len();
                if p > 0 && (src.as_bytes()[p - 1].is_ascii_alphanumeric() || src.as_bytes()[p - 1] == b'_') {
                    continue;
                }
                out.extend(lits.iter().filter(|(_, at)| *at >= from).take(count).map(|(s, _)| s.clone()));
            }
        }
        out
    }

    /// What reads as a message in backend code: starts with a capital (or a
    /// placeholder), has a space, and isn't a panic message. Named placeholders
    /// (`{e}`, `{:.0}`) become `{}`.
    fn backend_messages(src: &str) -> Vec<String> {
        let code = without_tests(src);
        literals(code)
            .into_iter()
            .filter(|(s, at)| {
                let before = code[..*at].trim_end();
                (s.starts_with(|c: char| c.is_ascii_uppercase()) || s.starts_with('{'))
                    && s.contains(' ')
                    && !before.ends_with("expect(")
                    && !before.ends_with("panic!(")
            })
            .map(|(s, _)| {
                let mut out = String::new();
                let mut rest = s.as_str();
                while let Some(i) = rest.find('{') {
                    let close = rest[i..].find('}').map_or(rest.len(), |c| i + c + 1);
                    out.push_str(&rest[..i]);
                    out.push_str("{}");
                    rest = &rest[close..];
                }
                out.push_str(rest);
                out
            })
            .collect()
    }

    const UI_SOURCES: [(&str, &str); 4] = [
        ("src/app.rs", include_str!("app.rs")),
        ("src/app/contracts.rs", include_str!("app/contracts.rs")),
        ("src/charts.rs", include_str!("charts.rs")),
        ("src/combo.rs", include_str!("combo.rs")),
    ];
    const BACKEND_SOURCES: [(&str, &str); 5] = [
        ("store.rs", include_str!("../src-tauri/src/store.rs")),
        ("lib.rs", include_str!("../src-tauri/src/lib.rs")),
        ("camt.rs", include_str!("../src-tauri/src/camt.rs")),
        ("backup.rs", include_str!("../src-tauri/src/backup.rs")),
        ("locale.rs", include_str!("../src-tauri/src/locale.rs")),
    ];

    fn check_catalog(name: &str, catalog: &[(&str, &str)]) {
        let mut keys: Vec<&str> = catalog.iter().map(|(en, _)| *en).collect();
        keys.sort();
        for w in keys.windows(2) {
            assert_ne!(w[0], w[1], "{name}: \"{}\" is in the catalog twice", w[0]);
        }
        for (en, nl) in catalog {
            assert!(!nl.trim().is_empty(), "{name}: no Dutch for \"{en}\"");
            assert_eq!(en.matches("{}").count(), nl.matches("{}").count(), "{name}: placeholders of \"{en}\" / \"{nl}\"");
        }
    }

    #[test]
    fn every_ui_string_has_a_dutch_translation() {
        check_catalog("UI", nl::UI);
        let mut used = Vec::new();
        for (file, src) in UI_SOURCES {
            for s in ui_strings(src) {
                assert!(nl::UI.iter().any(|(en, _)| *en == s), "{file}: no Dutch translation for \"{s}\"");
                used.push(s);
            }
        }
        for (en, _) in nl::UI {
            assert!(used.iter().any(|u| u == en), "nl.rs: \"{en}\" is not used any more");
        }
    }

    #[test]
    fn every_backend_message_has_a_dutch_translation() {
        check_catalog("ERRORS", nl::ERRORS);
        let mut used = Vec::new();
        for (file, src) in BACKEND_SOURCES {
            for s in backend_messages(src) {
                assert!(nl::ERRORS.iter().any(|(en, _)| *en == s), "{file}: no Dutch translation for \"{s}\"");
                used.push(s);
            }
        }
        for (en, _) in nl::ERRORS {
            assert!(used.iter().any(|u| u == en), "nl.rs: error \"{en}\" is not used any more");
        }
    }

    #[test]
    fn the_scanner_finds_strings() {
        let src = r#"let a = t!("One"); let b = tn!(n, "{} x", "{} xs", n); c = 'x'; d = '"'; e: &'a str; t!("Say \u{201c}hi\u{201d}")"#;
        assert_eq!(ui_strings(src), vec!["One", "Say \u{201c}hi\u{201d}", "{} x", "{} xs"]);
        let back = "fn f() { Err(format!(\"Bad {e} and {:.0}\")); x.expect(\"Never happens here\"); }\n#[cfg(test)]\nmod tests { \"Not this one\" }";
        assert_eq!(backend_messages(back), vec!["Bad {} and {}"]);
    }

    #[test]
    fn translates_with_counts_and_amounts() {
        set_lang(Lang::Nl);
        assert_eq!(t!("Overview"), "Overzicht");
        assert_eq!(tn!(1, "Delete {} transaction?", "Delete {} transactions?", 1), "1 transactie verwijderen?");
        assert_eq!(tn!(3, "Delete {} transaction?", "Delete {} transactions?", 3), "3 transacties verwijderen?");
        assert_eq!(euro(-123456), "€ -1.234,56");
        assert_eq!(whole(125000), "1.250");
        assert_eq!(error("Account not found"), "Rekening niet gevonden");
        assert_eq!(error("Category Boodschappen is disabled"), "Categorie Boodschappen is uitgeschakeld");
        assert_eq!(
            error("Does not fit in the 2026 budget: the year result would be € -1,234.56. Lower another budget first."),
            "Past niet in het budget van 2026: het jaarresultaat zou € -1.234,56 worden. Verlaag eerst een ander budget."
        );
        assert_eq!(error("sept.xml: Ntry without Amt"), "sept.xml: Ntry zonder Amt", "nested");
        assert_eq!(error("Something new"), "Something new", "unknown stays");
        assert_eq!(date(2026, 9, 30), "30-9-2026");
        set_lang(Lang::En);
        assert_eq!(t!("Overview"), "Overview");
        assert_eq!(tn!(2, "Delete {} transaction?", "Delete {} transactions?", 2), "Delete 2 transactions?");
        assert_eq!(euro(-123456), "€ -1,234.56");
        assert_eq!(whole(125000), "1,250");
        assert_eq!(error("Account not found"), "Account not found");
        assert_eq!(date(2026, 9, 30), "30 Sep 2026");
        assert_eq!(month_name(0), "January");
        set_lang(Lang::Nl);
    }
}
