//! Bad words in what players write, masked or turned away.

use rustrict::Type;

/// `text` with bad words masked, or `None` when it is too bad to keep.
pub(crate) fn clean(text: &str) -> Option<String> {
    let (masked, found) = rustrict::Censor::from_str(text).censor_and_analyze();
    if found.is(Type::OFFENSIVE & Type::SEVERE) {
        return None;
    }
    Some(masked)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_words_are_masked() {
        assert_eq!(clean("good game, see you").as_deref(), Some("good game, see you"));
        assert_ne!(clean("you are a shithead").as_deref(), Some("you are a shithead"));
        assert!(!clean("fuck you").unwrap_or_default().contains("uck"));
    }
}
