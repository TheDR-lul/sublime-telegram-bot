//! Internationalization module.
//!
//! Locale files are embedded at compile time via `include_str!`.
//! API mirrors BC label-style access: t(), t_rand(), t_fmt(), t_rand_fmt().
//!
//! # Adding a new phrase
//! 1. Open `locale/ru.toml`.
//! 2. Find the relevant section (e.g. `[pidor]`).
//! 3. Append your string to the array.
//! 4. Deploy.
//!
//! # Adding a new language
//! 1. Copy `locale/ru.toml` → `locale/xx.toml`, translate strings.
//! 2. Add `include_str!` and `langs.insert("xx", ...)` in `Locale::new()`.
//! 3. Done.

use std::collections::HashMap;
use std::sync::Arc;

use rand::RngExt;
use toml::Value;

// ── Embedded locale files ──────────────────────────────────────────────────
const RU_TOML: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/locale/ru.toml"));

/// Global singleton locale, initialized once at startup.
/// Handlers use `LOCALE.t(lang, "key")` or `LOCALE.t_rand(lang, "key")`.
pub static LOCALE: std::sync::LazyLock<Locale> = std::sync::LazyLock::new(Locale::new);

// ── Public API ──────────────────────────────────────────────────────────────

/// Holds all parsed locales. Cheap to clone because it is Arc-backed.
#[derive(Clone)]
pub struct Locale {
    inner: Arc<LocaleInner>,
}

struct LocaleInner {
    /// lang code → parsed TOML root
    langs: HashMap<String, Value>,
    default_lang: String,
}

impl Locale {
    /// Parse embedded locale files. Call once at startup.
    pub fn new() -> Self {
        let mut langs = HashMap::new();

        let ru: Value = toml::from_str(RU_TOML).expect("locale/ru.toml must be valid TOML");
        langs.insert("ru".to_owned(), ru);

        Locale {
            inner: Arc::new(LocaleInner {
                langs,
                default_lang: "ru".to_owned(),
            }),
        }
    }

    // ── Low-level helpers ──────────────────────────────────────────────────

    /// Resolve a dotted key in the given language, fall back to default lang.
    fn resolve<'a>(&'a self, lang: &str, key: &str) -> Option<&'a Value> {
        let root = self
            .inner
            .langs
            .get(lang)
            .or_else(|| self.inner.langs.get(&self.inner.default_lang))?;
        navigate(root, key)
    }

    // ── Public methods ─────────────────────────────────────────────────────

    /// Return a single string for `key`. Panics if key not found or not a string.
    ///
    /// ```
    /// locale.t("ru", "pidor.static.registration_success")
    /// ```
    pub fn t(&self, lang: &str, key: &str) -> &str {
        self.resolve(lang, key)
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("i18n: key '{}' not found or not a string (lang={})", key, lang))
    }

    /// Like `t`, but returns `None` instead of panicking on missing key.
    pub fn t_opt(&self, lang: &str, key: &str) -> Option<&str> {
        self.resolve(lang, key).and_then(|v| v.as_str())
    }

    /// Return a random string from an array key.
    pub fn t_rand(&self, lang: &str, key: &str) -> &str {
        let arr = self
            .resolve(lang, key)
            .and_then(|v| v.as_array())
            .unwrap_or_else(|| panic!("i18n: key '{}' not found or not an array (lang={})", key, lang));
        let idx = rand::rng().random_range(0..arr.len());
        arr[idx]
            .as_str()
            .unwrap_or_else(|| panic!("i18n: array element '{}[{}]' is not a string", key, idx))
    }

    /// Return a formatted string with `{placeholder}` substitution.
    ///
    /// ```
    /// locale.t_fmt("ru", "pidor.stage4", &[("username", "Вася")])
    /// ```
    pub fn t_fmt(&self, lang: &str, key: &str, args: &[(&str, &str)]) -> String {
        let s = self.t(lang, key);
        apply_args(s, args)
    }

    /// Return a random string from an array with `{placeholder}` substitution.
    pub fn t_rand_fmt(&self, lang: &str, key: &str, args: &[(&str, &str)]) -> String {
        let s = self.t_rand(lang, key);
        apply_args(s, args)
    }
}

impl Default for Locale {
    fn default() -> Self {
        Self::new()
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Walk a TOML `Value` by a dotted key path.
fn navigate<'a>(root: &'a Value, key: &str) -> Option<&'a Value> {
    let mut cur = root;
    for part in key.split('.') {
        cur = cur.get(part)?;
    }
    Some(cur)
}

/// Replace all `{placeholder}` occurrences in `template`.
fn apply_args(template: &str, args: &[(&str, &str)]) -> String {
    let mut result = template.to_owned();
    for (k, v) in args {
        result = result.replace(&format!("{{{k}}}"), v);
    }
    result
}

// ── Unit tests ───────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn locale() -> Locale {
        Locale::new()
    }

    #[test]
    fn t_returns_string() {
        let loc = locale();
        let s = loc.t("ru", "pidor.static.registration_success");
        assert!(!s.is_empty());
    }

    #[test]
    fn t_rand_returns_non_empty() {
        let loc = locale();
        let s = loc.t_rand("ru", "pidor.stage1");
        assert!(!s.is_empty());
    }

    #[test]
    fn t_fmt_replaces_placeholder() {
        let loc = locale();
        let s = loc.t_fmt("ru", "pidor.static.current_result", &[("username", "Тест")]);
        assert!(s.contains("Тест"), "expected 'Тест' in '{s}'");
    }

    #[test]
    fn t_rand_fmt_replaces_placeholder() {
        let loc = locale();
        let s = loc.t_rand_fmt("ru", "pidor.stage4", &[("username", "Вася")]);
        assert!(s.contains("Вася"), "expected 'Вася' in '{s}'");
    }

    #[test]
    fn fallback_unknown_lang_uses_default() {
        let loc = locale();
        let s = loc.t("en", "pidor.static.registration_success");
        assert!(!s.is_empty());
    }

    #[test]
    fn t_opt_missing_key_returns_none() {
        let loc = locale();
        assert!(loc.t_opt("ru", "nonexistent.key.path").is_none());
    }

    #[test]
    fn navigate_nested_key() {
        let loc = locale();
        let v = loc.resolve("ru", "duel.static.accept_btn");
        assert!(v.is_some());
    }
}
