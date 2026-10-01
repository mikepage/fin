//! Locale mapping for the standard categories: display names and default rules, read
//! from `locales/<locale>.toml` (compiled in). The ids themselves live in
//! fin_shared::catalog.
//!
//! Names come in every language (`names`); the default rules belong to the Dutch market,
//! so they are only in nl-NL (`nl_nl`), whatever the app's language.

use std::collections::HashMap;
use std::sync::OnceLock;

use fin_shared::{catalog, Lang, RuleKind};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Locale {
    /// The locale tag, e.g. "nl-NL" (checked by the tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pub locale: String,
    groups: HashMap<String, String>,
    categories: HashMap<String, String>,
    #[serde(default)]
    pub rules: Vec<DefaultRule>,
}

/// Default text rules for one category, arriving with rules version `since`;
/// `direction` "in" or "out" makes them direction-aware.
#[derive(Debug, Deserialize)]
pub struct DefaultRule {
    pub since: u32,
    category: String,
    #[serde(default)]
    direction: Option<String>,
    pub patterns: Vec<String>,
}

impl Locale {
    /// Display name of a category id (`sys-groceries` → "Groceries"); the id itself
    /// when the locale lacks it (the catalog test makes sure nl-NL doesn't).
    pub fn category_name<'a>(&'a self, id: &'a str) -> &'a str {
        self.categories.get(catalog::key(id)).map_or(id, String::as_str)
    }

    /// Display name of a group key (`household` → "Household").
    pub fn group_name<'a>(&'a self, key: &'a str) -> &'a str {
        self.groups.get(key).map_or(key, String::as_str)
    }

    /// The group key a display name stands for (`Household` → `household`).
    pub fn group_key(&self, name: &str) -> Option<&str> {
        self.groups.iter().find(|(_, n)| n.eq_ignore_ascii_case(name)).map(|(k, _)| k.as_str())
    }

    /// The newest `since` among the default rules.
    pub fn rules_version(&self) -> u32 {
        self.rules.iter().map(|r| r.since).max().unwrap_or(0)
    }
}

impl DefaultRule {
    pub fn category_id(&self) -> String {
        format!("sys-{}", self.category)
    }

    pub fn kind(&self) -> RuleKind {
        match self.direction.as_deref() {
            Some("in") => RuleKind::TextIn,
            Some("out") => RuleKind::TextOut,
            _ => RuleKind::Text,
        }
    }
}

/// The Dutch mapping, parsed once: names, and the rules for every language.
pub fn nl_nl() -> &'static Locale {
    static NL: OnceLock<Locale> = OnceLock::new();
    NL.get_or_init(|| toml::from_str(include_str!("../locales/nl-NL.toml")).expect("locales/nl-NL.toml is valid"))
}

/// The English names, parsed once.
fn en() -> &'static Locale {
    static EN: OnceLock<Locale> = OnceLock::new();
    EN.get_or_init(|| toml::from_str(include_str!("../locales/en.toml")).expect("locales/en.toml is valid"))
}

/// The category and group names in a language.
pub fn names(lang: Lang) -> &'static Locale {
    match lang {
        Lang::Nl => nl_nl(),
        Lang::En => en(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_names_the_catalog() {
        let known = |id: &str| catalog::CATALOG.iter().any(|(c, _, _)| *c == id);
        for lang in Lang::ALL {
            let l = names(lang);
            assert_eq!(l.locale, lang.tag());
            for (id, group, _) in catalog::CATALOG {
                assert!(l.categories.contains_key(catalog::key(id)), "no {} name for {id}", l.locale);
                assert!(l.groups.contains_key(*group), "no {} name for group {group}", l.locale);
            }
            for key in l.categories.keys() {
                assert!(known(&format!("sys-{key}")), "{} names unknown category {key}", l.locale);
            }
            for key in l.groups.keys() {
                assert!(catalog::CATALOG.iter().any(|(_, g, _)| g == key), "{} names unknown group {key}", l.locale);
            }
            // Category names are unique, so pickers and reports are never ambiguous.
            let mut names: Vec<&String> = l.categories.values().collect();
            names.sort();
            let before = names.len();
            names.dedup();
            assert_eq!(names.len(), before, "two {} categories share a name", l.locale);
        }
        // A group name means the same group in every language, so the user's own
        // categories can follow their group when the language changes.
        for a in Lang::ALL {
            for b in Lang::ALL {
                for (key, name) in &names(a).groups {
                    assert!(names(b).group_key(name).is_none_or(|k| k == key), "group name {name} is ambiguous");
                }
            }
        }
        assert!(en().rules.is_empty(), "rules live in nl-NL only");
        assert_eq!(en().category_name(catalog::ids::GROCERIES), "Groceries");
    }

    #[test]
    fn nl_nl_covers_the_catalog() {
        let l = nl_nl();
        let known = |id: &str| catalog::CATALOG.iter().any(|(c, _, _)| *c == id);
        for r in &l.rules {
            assert!(known(&r.category_id()), "rule for unknown category {}", r.category);
            assert!(r.patterns.iter().all(|p| p == &p.to_lowercase() && p.trim() == p), "patterns are lower case, trimmed");
            assert!(matches!(r.direction.as_deref(), None | Some("in") | Some("out")), "direction is in or out");
        }
        // No default text may point at two categories in the same direction: that is the
        // conflict to avoid (in and out for one counterparty is the point of directions).
        // Each text is listed once, and one entry per category and direction keeps the
        // file easy to scan.
        let mut seen: HashMap<(String, RuleKind), &str> = HashMap::new();
        let mut entries: Vec<(&str, RuleKind)> = Vec::new();
        for r in &l.rules {
            assert!(!entries.contains(&(&r.category, r.kind())), "two rule entries for {}", r.category);
            entries.push((&r.category, r.kind()));
            for p in &r.patterns {
                let key = (fin_shared::match_form(p), r.kind());
                if let Some(other) = seen.insert(key, &r.category) {
                    panic!("default rule \"{p}\" is listed for {other} and {}", r.category);
                }
            }
        }
        // Defaults taken back for being ambiguous stay out.
        for gone in ["plus", "nationale-nederlanden"] {
            assert!(l.rules.iter().all(|r| !r.patterns.iter().any(|p| p == gone)), "\"{gone}\" is no default");
        }
        assert_eq!(l.category_name(catalog::ids::HEALTH_INSURANCE), "Zorgverzekering");
        assert_eq!(l.group_name(catalog::groups::HOUSEHOLD), "Huishouden");
    }
}
