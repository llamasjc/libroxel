const SECRET_WORDS: [&str; 8] =
    ["password", "passwd", "token", "secret", "authorization", "bearer", "api_key", "apikey"];

pub fn scrub(text: &str) -> String {
    text.lines().map(|line| scrub_line(&user_paths(line))).collect::<Vec<_>>().join("\n")
}

fn user_paths(line: &str) -> String {
    let mut out = line.to_owned();
    let unix: &[char] = &['/', ' ', '"', '\'', ')', ':'];
    let windows: &[char] = &['\\', '"', '\''];
    for (root, ends) in [("/home/", unix), ("/Users/", unix), ("\\Users\\", windows)] {
        let mut from = 0;
        while let Some(at) = out[from..].find(root).map(|i| i + from) {
            let name = at + root.len();
            let end = out[name..].find(ends).map_or(out.len(), |i| i + name);
            if end > name {
                out.replace_range(name..end, "~");
            }
            from = (name + 1).min(out.len());
        }
    }
    out
}

fn scrub_line(line: &str) -> String {
    let lower = line.to_ascii_lowercase();
    for word in SECRET_WORDS {
        if let Some(at) = lower.find(word) {
            let after = at + word.len();
            if let Some(sep) = line[after..].find([':', '=']) {
                let cut = after + sep + 1;
                return format!("{} [hidden]", scrub_words(&line[..cut]));
            }
        }
    }
    scrub_words(line)
}

fn scrub_words(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut word = String::new();
    for c in line.chars() {
        if c.is_whitespace() || matches!(c, '"' | '\'' | '(' | ')' | '<' | '>' | '[' | ']' | ',' | ';' | '`')
        {
            out.push_str(&hide(&word));
            word.clear();
            out.push(c);
        } else {
            word.push(c);
        }
    }
    out.push_str(&hide(&word));
    out
}

fn hide(word: &str) -> String {
    let core = word.trim_end_matches(['.', ':', '!', '?']);
    let tail = &word[core.len()..];
    if is_email(core) {
        return format!("[email]{tail}");
    }
    if is_secret(core) {
        return format!("[hidden]{tail}");
    }
    word.to_owned()
}

fn is_email(word: &str) -> bool {
    let Some((name, domain)) = word.rsplit_once('@') else { return false };
    !name.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}

fn is_secret(word: &str) -> bool {
    let hex = word.len() >= 32 && word.chars().all(|c| c.is_ascii_hexdigit());
    let token = word.len() >= 24
        && word.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '+' | '/' | '='))
        && word.chars().any(|c| c.is_ascii_digit())
        && word.chars().any(|c| c.is_ascii_uppercase())
        && word.chars().any(|c| c.is_ascii_lowercase());
    hex || token
}

#[cfg(test)]
mod tests {
    use super::scrub;

    #[test]
    fn hides_user_folders() {
        assert_eq!(scrub("at /home/pat/x.rs and /Users/kim/y"), "at /home/~/x.rs and /Users/~/y");
        assert_eq!(scrub("C:\\Users\\Bob Smith\\AppData"), "C:\\Users\\~\\AppData");
        assert_eq!(scrub("ends /home/"), "ends /home/");
    }
}
