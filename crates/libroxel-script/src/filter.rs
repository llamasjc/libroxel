//! Hiding bad words in what players type.
//!
//! [`filter`] replaces each word on the list with `#`s, whatever its case
//! and even when it's spelled with look-alike digits and symbols (`5h1t`)
//! or with letters repeated (`shiiit`). Words that only contain one of the
//! listed words (`class`, `scunthorpe`) are left alone, except for the
//! listed stems that are bad wherever they start a word.
//!
//! It's a short list, meant to keep games friendly for younger players, not
//! a complete one. `TextService:FilterStringAsync` uses it, and so does the
//! chat in the starter games.

/// Whole words that are hidden.
const WORDS: &[&str] = &[
    "arse", "arsehole", "ass", "asshole", "bastard", "bitch", "bitches", "bollocks", "bullshit", "crap",
    "cunt", "cunts", "damn", "dick", "dickhead", "dicks", "douche", "dumbass", "fag", "fags", "faggot",
    "jackass", "jerkoff", "nazi", "nigga", "nigger", "piss", "pissed", "porn", "prick", "pussy", "retard",
    "retarded", "shit", "shits", "shitty", "slut", "sluts", "twat", "wank", "wanker", "whore", "whores",
    "kys",
];

/// Stems hidden wherever a word starts with them (`fuck`, `fucking`, `fucker`…).
const STEMS: &[&str] = &["fuck", "motherfuck", "shithead", "bullshit", "dipshit", "horseshit"];

/// The letter a look-alike stands for.
fn plain(c: char) -> char {
    match c {
        '0' => 'o',
        '1' | '!' | '|' => 'i',
        '3' => 'e',
        '4' | '@' => 'a',
        '5' | '$' => 's',
        '7' => 't',
        '8' => 'b',
        '9' => 'g',
        c => c.to_ascii_lowercase(),
    }
}

/// A word as compared with the list: look-alikes read as letters, and
/// letters repeated read once.
fn squash(word: &str) -> String {
    let mut out = String::new();
    for c in word.chars().map(plain) {
        if out.ends_with(c) {
            continue;
        }
        out.push(c);
    }
    out
}

fn squashed_list(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| squash(w)).collect()
}

fn is_bad(word: &str) -> bool {
    let plain_word: String = word.chars().map(plain).collect();
    let squashed = squash(word);
    let words = squashed_list(WORDS);
    if WORDS.contains(&plain_word.as_str()) || words.contains(&squashed) {
        return true;
    }
    squashed_list(STEMS).iter().any(|stem| squashed.starts_with(stem.as_str()))
}

/// Whether `c` can be part of a word (look-alike symbols included).
fn in_word(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '@' | '$' | '!' | '|')
}

/// `text` with every bad word replaced by as many `#`s as it has letters.
pub fn filter(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if word.is_empty() {
            return;
        }
        // Symbols at the ends are punctuation, not letters ("shit!").
        let core = word.trim_matches(|c: char| !c.is_alphanumeric());
        if !core.is_empty() && is_bad(core) {
            let start = word.find(core).unwrap_or(0);
            out.push_str(&word[..start]);
            out.extend(std::iter::repeat_n('#', core.chars().count()));
            out.push_str(&word[start + core.len()..]);
        } else {
            out.push_str(word);
        }
        word.clear();
    };
    for c in text.chars() {
        if in_word(c) {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::filter;

    #[test]
    fn bad_words_are_hidden_however_they_are_spelled() {
        assert_eq!(filter("what the fuck"), "what the ####");
        assert_eq!(filter("FUCKING lag"), "####### lag");
        assert_eq!(filter("you are 5h1t!"), "you are ####!");
        assert_eq!(filter("shiiiit"), "#######");
        assert_eq!(filter("a$$hole"), "#######");
    }

    #[test]
    fn ordinary_words_are_left_alone() {
        for fine in
            ["class", "assassin", "Scunthorpe", "grape", "hello there", "pass the ball", "cocktail", "1337"]
        {
            assert_eq!(filter(fine), fine);
        }
        assert_eq!(filter("gg, nice obby :)"), "gg, nice obby :)");
    }
}
