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
    let squashed_counts = squashed.len() >= 3 || plain_word.len() > squashed.len();
    if WORDS.contains(&plain_word.as_str()) || squashed_counts && words.contains(&squashed) {
        return true;
    }
    squashed_list(STEMS).iter().any(|stem| squashed.starts_with(stem.as_str()))
}

/// Whether `c` can be part of a word (look-alike symbols included).
fn in_word(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '@' | '$' | '!' | '|')
}

/// `text` with every bad word replaced by as many `#`s as it has letters,
/// including words split up with spaces or dots (`f u c k`, `sh.it`).
pub fn filter(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut hidden = vec![false; chars.len()];
    let mut hide = |start: usize, end: usize| {
        let core = trim_core(&chars, start, end);
        for h in &mut hidden[core.0..core.1] {
            *h = true;
        }
    };
    for (start, end) in runs(&chars, in_word) {
        if is_bad_run(&chars, start, end) {
            hide(start, end);
        }
    }
    let pieces = runs(&chars, |c| !splits(c));
    let mut i = 0;
    while i < pieces.len() {
        let mut joined = String::new();
        let mut found = None;
        for j in i..pieces.len().min(i + MAX_PIECES) {
            if j > i {
                let gap = &chars[pieces[j - 1].1..pieces[j].0];
                if gap.len() > 3 || gap.contains(&'\n') {
                    break;
                }
            }
            let (a, b) = trim_core(&chars, pieces[j].0, pieces[j].1);
            if a == b || !chars[a..b].iter().all(|c| in_word(*c)) {
                break;
            }
            joined.extend(&chars[a..b]);
            if j > i && squash(&joined).chars().count() >= 3 && is_bad(&joined) {
                found = Some(j);
                break;
            }
            if b - a > 3 {
                break;
            }
        }
        if let Some(j) = found {
            for p in &pieces[i..=j] {
                hide(p.0, p.1);
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    chars.iter().zip(hidden).map(|(c, h)| if h { '#' } else { *c }).collect()
}

const MAX_PIECES: usize = 10;

fn splits(c: char) -> bool {
    c.is_whitespace() || matches!(c, '.' | '-' | '_' | '*' | ',' | '+' | '~')
}

fn runs(chars: &[char], keep: impl Fn(char) -> bool) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in chars.iter().enumerate() {
        match (keep(*c), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, chars.len()));
    }
    out
}

/// Symbols at the ends are punctuation, not letters ("shit!").
fn trim_core(chars: &[char], mut start: usize, mut end: usize) -> (usize, usize) {
    while start < end && !chars[start].is_alphanumeric() {
        start += 1;
    }
    while end > start && !chars[end - 1].is_alphanumeric() {
        end -= 1;
    }
    (start, end)
}

fn is_bad_run(chars: &[char], start: usize, end: usize) -> bool {
    let (a, b) = trim_core(chars, start, end);
    a < b && is_bad(&chars[a..b].iter().collect::<String>())
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
        assert_eq!(filter("asss"), "####");
    }

    #[test]
    fn split_up_bad_words_are_hidden() {
        assert_eq!(filter("fu ck"), "## ##");
        assert_eq!(filter("f u c k you"), "# # # # you");
        assert_eq!(filter("what the f.u.c.k"), "what the #.#.#.#");
        assert_eq!(filter("sh it happens"), "## ## happens");
        assert_eq!(filter("fu cking lag"), "## ##### lag");
        assert_eq!(filter("a ss hole"), "# ## ####");
        assert_eq!(filter("b-i-t-c-h!"), "#-#-#-#-#!");
    }

    #[test]
    fn ordinary_words_are_left_alone() {
        for fine in [
            "class",
            "assassin",
            "Scunthorpe",
            "grape",
            "hello there",
            "pass the ball",
            "as",
            "cocktail",
            "1337",
        ] {
            assert_eq!(filter(fine), fine);
        }
        assert_eq!(filter("gg, nice obby :)"), "gg, nice obby :)");
        for fine in
            ["it's hit", "a s d f", "as soon as", "is hit", "go to the shop", "who reads it", "I am at a b c"]
        {
            assert_eq!(filter(fine), fine);
        }
    }
}
