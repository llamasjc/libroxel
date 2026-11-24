//! The app's text in other languages.
//!
//! Text is looked up by its English wording: `t("Play")` is "Jugar" in
//! Spanish. Each language's wordings are in `assets/lang/<code>.json`, an
//! object from English to the translation; anything not in it yet shows in
//! English. Text with something filled in uses `{name}` in both, through
//! [`tf`].
//!
//! The language follows the computer's until the player picks one on the
//! home screen; the choice is kept in `language.txt` beside the settings.

use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    English,
    Chinese,
    Spanish,
    Portuguese,
    French,
}

impl Language {
    pub const ALL: [Language; 5] =
        [Language::English, Language::Chinese, Language::Spanish, Language::Portuguese, Language::French];

    /// Its code, as computers and the language files name it.
    pub fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::Chinese => "zh-CN",
            Language::Spanish => "es",
            Language::Portuguese => "pt-BR",
            Language::French => "fr",
        }
    }

    /// Its name, in itself, for the language menu.
    pub fn name(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Chinese => "简体中文",
            Language::Spanish => "Español",
            Language::Portuguese => "Português (Brasil)",
            Language::French => "Français",
        }
    }

    /// The language a locale such as `es_MX.UTF-8` or `zh-Hans-CN` is in,
    /// if the app has it.
    pub fn from_locale(locale: &str) -> Option<Language> {
        let lower = locale.to_ascii_lowercase();
        let main = lower.split(['_', '-', '.', '@']).next().unwrap_or("");
        match main {
            "en" => Some(Language::English),
            "zh" => Some(Language::Chinese),
            "es" => Some(Language::Spanish),
            "pt" => Some(Language::Portuguese),
            "fr" => Some(Language::French),
            _ => None,
        }
    }

    fn index(self) -> u8 {
        Language::ALL.iter().position(|l| *l == self).unwrap_or(0) as u8
    }

    fn table(self) -> &'static HashMap<&'static str, &'static str> {
        static EMPTY: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(HashMap::new);
        static CHINESE: LazyLock<HashMap<&'static str, &'static str>> =
            LazyLock::new(|| parse(include_str!("../assets/lang/zh-CN.json")));
        static SPANISH: LazyLock<HashMap<&'static str, &'static str>> =
            LazyLock::new(|| parse(include_str!("../assets/lang/es.json")));
        static PORTUGUESE: LazyLock<HashMap<&'static str, &'static str>> =
            LazyLock::new(|| parse(include_str!("../assets/lang/pt-BR.json")));
        static FRENCH: LazyLock<HashMap<&'static str, &'static str>> =
            LazyLock::new(|| parse(include_str!("../assets/lang/fr.json")));
        match self {
            Language::English => &EMPTY,
            Language::Chinese => &CHINESE,
            Language::Spanish => &SPANISH,
            Language::Portuguese => &PORTUGUESE,
            Language::French => &FRENCH,
        }
    }
}

/// A language file's wordings. They're kept for as long as the app runs.
fn parse(text: &'static str) -> HashMap<&'static str, &'static str> {
    let map: HashMap<String, String> = serde_json::from_str(text).expect("a language file is a JSON object");
    map.into_iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| (&*Box::leak(k.into_boxed_str()), &*Box::leak(v.into_boxed_str())))
        .collect()
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn current() -> Language {
    Language::ALL[usize::from(CURRENT.load(Ordering::Relaxed)) % Language::ALL.len()]
}

/// Switches the app's text to `language` (and remembers it, if `keep`).
pub fn set(language: Language, keep: bool) {
    CURRENT.store(language.index(), Ordering::Relaxed);
    if keep {
        let path = crate::server::data_dir().join("language.txt");
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, language.code());
    }
}

/// Picks the language on start: the one chosen before, or else the computer's.
pub fn start() {
    let chosen = std::fs::read_to_string(crate::server::data_dir().join("language.txt"))
        .ok()
        .and_then(|code| Language::from_locale(code.trim()));
    let system = || sys_locale::get_locales().find_map(|l| Language::from_locale(&l));
    set(chosen.or_else(system).unwrap_or(Language::English), false);
}

/// `english` in the app's language (or as it is, if it isn't translated yet).
pub fn t(english: &str) -> &str {
    t_in(current(), english)
}

/// `english` in `language`.
pub fn t_in(language: Language, english: &str) -> &str {
    language.table().get(english).copied().unwrap_or(english)
}

/// [`t`], then each `{name}` replaced by its value.
pub fn tf(english: &str, values: &[(&str, &dyn std::fmt::Display)]) -> String {
    tf_in(current(), english, values)
}

/// [`tf`] in `language`.
pub fn tf_in(language: Language, english: &str, values: &[(&str, &dyn std::fmt::Display)]) -> String {
    let mut text = t_in(language, english).to_owned();
    for (name, value) in values {
        text = text.replace(&format!("{{{name}}}"), &value.to_string());
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locales_name_their_language() {
        assert_eq!(Language::from_locale("es_MX.UTF-8"), Some(Language::Spanish));
        assert_eq!(Language::from_locale("pt-BR"), Some(Language::Portuguese));
        assert_eq!(Language::from_locale("zh-Hans-CN"), Some(Language::Chinese));
        assert_eq!(Language::from_locale("fr"), Some(Language::French));
        assert_eq!(Language::from_locale("en-US"), Some(Language::English));
        assert_eq!(Language::from_locale("de-DE"), None);
        assert_eq!(Language::from_locale(""), None);
    }

    /// Every language file reads, and each translation keeps the `{names}`
    /// its English has, so filled-in text never loses a value.
    #[test]
    fn translations_keep_their_blanks() {
        let blanks = |text: &str| {
            let mut found: Vec<String> = text
                .split('{')
                .skip(1)
                .filter_map(|rest| rest.split_once('}').map(|(name, _)| name.to_owned()))
                .collect();
            found.sort();
            found
        };
        for language in Language::ALL {
            for (english, translated) in language.table() {
                assert_eq!(
                    blanks(english),
                    blanks(translated),
                    "{}: {english:?} -> {translated:?}",
                    language.code()
                );
            }
        }
    }

    /// Every language translates the same wordings, so none falls behind.
    #[test]
    fn every_language_has_every_wording() {
        let english: Vec<&str> = {
            let mut keys: Vec<&str> = Language::Spanish.table().keys().copied().collect();
            keys.sort();
            keys
        };
        assert!(english.len() > 50, "the home screen is translated");
        for language in [Language::Chinese, Language::Portuguese, Language::French] {
            let mut keys: Vec<&str> = language.table().keys().copied().collect();
            keys.sort();
            let missing: Vec<&&str> = english.iter().filter(|k| !keys.contains(k)).collect();
            let extra: Vec<&&str> = keys.iter().filter(|k| !english.contains(k)).collect();
            assert!(
                missing.is_empty() && extra.is_empty(),
                "{}: missing {missing:?}, extra {extra:?}",
                language.code()
            );
        }
    }

    /// Every wording the code asks for is in the language files: a typo
    /// in either would leave text in English.
    #[test]
    fn the_code_only_asks_for_wordings_the_files_have() {
        let sources = [include_str!("menu.rs"), include_str!("hub_home.rs")];
        let table = Language::Spanish.table();
        let mut missing = Vec::new();
        for source in sources {
            for call in ["t(", "tf("] {
                for (at, _) in source.match_indices(call) {
                    // Only calls of t and tf themselves, not get( and the like.
                    let before = source[..at].chars().next_back();
                    if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                        continue;
                    }
                    // The wording may start on the next line, after rustfmt.
                    let piece = source[at + call.len()..].trim_start();
                    let Some(piece) = piece.strip_prefix('"') else { continue };
                    let Some(end) = piece.find('"') else { continue };
                    let english = piece[..end].replace("\\'", "'");
                    if !table.contains_key(english.as_str()) {
                        missing.push(english);
                    }
                }
            }
        }
        missing.sort();
        missing.dedup();
        assert!(missing.is_empty(), "not in the language files: {missing:#?}");
    }

    #[test]
    fn blanks_are_filled_in() {
        let bea: [(&str, &dyn std::fmt::Display); 1] = [("name", &"Bea")];
        assert_eq!(tf_in(Language::English, "Hello, {name}!", &bea), "Hello, Bea!");
        assert_eq!(tf_in(Language::Spanish, "Hello, {name}!", &bea), "¡Hola, Bea!");
        assert_eq!(t_in(Language::French, "A wording no file has"), "A wording no file has");
    }
}
