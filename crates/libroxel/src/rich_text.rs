//! Roblox's rich text: the tags a text object reads when its RichText is on
//! (`<b>`, `<i>`, `<u>`, `<s>`, `<font color size transparency>`, `<br/>`,
//! `<uc>`, `<sc>`, `<mark>`, `<stroke>`, comments) and the escapes `&lt;`
//! `&gt;` `&amp;` `&quot;` `&apos;`, turned into an egui layout.

use bevy_egui::egui;
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId, Stroke};

/// The look a run of text has, from the tags around it.
#[derive(Clone, Copy)]
struct Style {
    colour: Color32,
    size: f32,
    italics: bool,
    underline: bool,
    strike: bool,
    upper: bool,
    small_caps: bool,
    bold: bool,
    mark: Option<Color32>,
}

/// Text laid out with its tags read: `text` at `size` in `colour`, wrapped
/// at `width`; `scale` is how many screen points a GUI pixel is (for sizes
/// the tags give).
pub fn job(text: &str, size: f32, colour: Color32, width: f32, scale: f32) -> LayoutJob {
    let mut job = LayoutJob {
        wrap: egui::text::TextWrapping { max_width: width, ..Default::default() },
        ..Default::default()
    };
    let base = Style {
        colour,
        size,
        italics: false,
        underline: false,
        strike: false,
        upper: false,
        small_caps: false,
        bold: false,
        mark: None,
    };
    let mut stack: Vec<(String, Style)> = Vec::new();
    let mut style = base;
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("<!--") {
            rest = after.find("-->").map_or("", |end| &after[end + 3..]);
            continue;
        }
        if rest.starts_with('<')
            && let Some(end) = rest.find('>')
            && let Some(tag) = parse_tag(&rest[1..end])
        {
            rest = &rest[end + 1..];
            match tag {
                Tag::Close(name) => {
                    if let Some(at) = stack.iter().rposition(|(n, _)| *n == name) {
                        style = stack[at].1;
                        stack.truncate(at);
                    }
                }
                Tag::Break => push(&mut job, "\n", style),
                Tag::Open(name, attrs) => {
                    stack.push((name.clone(), style));
                    apply(&mut style, &name, &attrs, base, scale);
                    if attrs.iter().any(|(k, _)| k == "/") {
                        style = stack.pop().map_or(style, |(_, s)| s);
                    }
                }
            }
            continue;
        }
        // Plain text up to the next tag.
        let next = rest[1..].find('<').map_or(rest.len(), |i| i + 1);
        push(&mut job, &unescape(&rest[..next]), style);
        rest = &rest[next..];
    }
    job
}

/// `text` without its tags (for measuring and for plain uses).
#[cfg(test)]
fn plain(text: &str) -> String {
    job(text, 14.0, Color32::WHITE, f32::INFINITY, 1.0).text
}

enum Tag {
    Open(String, Vec<(String, String)>),
    Close(String),
    Break,
}

fn parse_tag(inner: &str) -> Option<Tag> {
    let inner = inner.trim();
    if let Some(name) = inner.strip_prefix('/') {
        let name = name.trim().to_ascii_lowercase();
        return known(&name).then_some(Tag::Close(name));
    }
    let self_closing = inner.ends_with('/');
    let inner = inner.trim_end_matches('/').trim();
    let (name, mut attr_text) = inner.split_once(char::is_whitespace).unwrap_or((inner, ""));
    let name = name.to_ascii_lowercase();
    if name == "br" {
        return Some(Tag::Break);
    }
    if !known(&name) {
        return None;
    }
    let mut attrs = Vec::new();
    loop {
        attr_text = attr_text.trim_start();
        let Some(eq) = attr_text.find('=') else { break };
        let key = attr_text[..eq].trim().to_ascii_lowercase();
        let value_text = attr_text[eq + 1..].trim_start();
        let (value, left) = match value_text.chars().next() {
            Some(q @ ('"' | '\'')) => match value_text[1..].find(q) {
                Some(end) => (&value_text[1..1 + end], &value_text[end + 2..]),
                None => (&value_text[1..], ""),
            },
            _ => {
                let end = value_text.find(char::is_whitespace).unwrap_or(value_text.len());
                (&value_text[..end], &value_text[end..])
            }
        };
        attrs.push((key, value.to_owned()));
        attr_text = left;
    }
    if self_closing {
        attrs.push(("/".to_owned(), String::new()));
    }
    Some(Tag::Open(name, attrs))
}

fn known(name: &str) -> bool {
    matches!(
        name,
        "b" | "i" | "u" | "s" | "font" | "stroke" | "uc" | "uppercase" | "sc" | "smallcaps" | "mark"
    )
}

fn apply(style: &mut Style, name: &str, attrs: &[(String, String)], base: Style, scale: f32) {
    match name {
        "b" => style.bold = true,
        "i" => style.italics = true,
        "u" => style.underline = true,
        "s" => style.strike = true,
        "uc" | "uppercase" => style.upper = true,
        "sc" | "smallcaps" => style.small_caps = true,
        "mark" => {
            let alpha = attrs
                .iter()
                .find(|(k, _)| k == "transparency")
                .and_then(|(_, v)| v.trim().parse::<f32>().ok())
                .map_or(255, |t| ((1.0 - t.clamp(0.0, 1.0)) * 255.0) as u8);
            let c = attrs.iter().find(|(k, _)| k == "color").and_then(|(_, v)| colour(v));
            style.mark = c.map(|c| Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha));
        }
        "font" => {
            for (key, value) in attrs {
                match key.as_str() {
                    "color" => {
                        if let Some(c) = colour(value) {
                            style.colour =
                                Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), style.colour.a());
                        }
                    }
                    "size" => {
                        if let Ok(n) = value.trim().parse::<f32>() {
                            // Sizes are in the same units as TextSize, scaled like it.
                            style.size = (n * scale).max(1.0);
                        }
                    }
                    "transparency" => {
                        if let Ok(t) = value.trim().parse::<f32>() {
                            let a = (f32::from(base.colour.a()) * (1.0 - t.clamp(0.0, 1.0))) as u8;
                            let c = style.colour;
                            style.colour = Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a);
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

/// A colour written `#rrggbb`, `#rgb` or `rgb(r, g, b)`.
fn colour(text: &str) -> Option<Color32> {
    let text = text.trim();
    if let Some(hex) = text.strip_prefix('#') {
        let n = |s: &str| u8::from_str_radix(s, 16).ok();
        return match hex.len() {
            6 => Some(Color32::from_rgb(n(&hex[0..2])?, n(&hex[2..4])?, n(&hex[4..6])?)),
            3 => {
                let d = |i: usize| n(&hex[i..=i]).map(|v| v * 17);
                Some(Color32::from_rgb(d(0)?, d(1)?, d(2)?))
            }
            _ => None,
        };
    }
    let inner = text.strip_prefix("rgb(")?.strip_suffix(')')?;
    let parts: Vec<u8> = inner
        .split(',')
        .filter_map(|p| p.trim().parse::<f32>().ok())
        .map(|v| v.clamp(0.0, 255.0) as u8)
        .collect();
    (parts.len() == 3).then(|| Color32::from_rgb(parts[0], parts[1], parts[2]))
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn push(job: &mut LayoutJob, text: &str, style: Style) {
    if text.is_empty() {
        return;
    }
    let mut format = TextFormat {
        font_id: FontId::proportional(style.size),
        color: style.colour,
        italics: style.italics,
        ..Default::default()
    };
    if style.underline {
        format.underline = Stroke::new((style.size / 14.0).max(1.0), style.colour);
    }
    if style.strike {
        format.strikethrough = Stroke::new((style.size / 14.0).max(1.0), style.colour);
    }
    if let Some(mark) = style.mark {
        format.background = mark;
    }
    if style.bold {
        // No bold face among the fonts: a slightly bigger, solid run reads as bold.
        format.font_id = FontId::proportional(style.size * 1.04);
    }
    if style.small_caps {
        // Lower-case letters as smaller capitals.
        let mut run = String::new();
        let mut lower = None;
        for ch in text.chars() {
            let is_lower = ch.is_lowercase();
            if lower.is_some_and(|l| l != is_lower) {
                small_caps_run(job, &run, lower == Some(true), &format, style.size);
                run.clear();
            }
            lower = Some(is_lower);
            run.push(ch);
        }
        small_caps_run(job, &run, lower == Some(true), &format, style.size);
        return;
    }
    let text = if style.upper { text.to_uppercase() } else { text.to_owned() };
    job.append(&text, 0.0, format);
}

fn small_caps_run(job: &mut LayoutJob, run: &str, lower: bool, format: &TextFormat, size: f32) {
    if run.is_empty() {
        return;
    }
    let mut format = format.clone();
    if lower {
        format.font_id = FontId::proportional(size * 0.8);
    }
    job.append(&run.to_uppercase(), 0.0, format);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_go_and_their_looks_stay() {
        let j = job("<s>Done</s> next &lt;3", 14.0, Color32::WHITE, f32::INFINITY, 1.0);
        assert_eq!(j.text, "Done next <3");
        assert!(j.sections[0].format.strikethrough.width > 0.0);
        assert_eq!(j.sections[1].format.strikethrough.width, 0.0);
        let j = job(
            "a<br/>b <font color=\"#ff0000\" size=\"20\">red</font><uc>x</uc>",
            14.0,
            Color32::WHITE,
            f32::INFINITY,
            1.0,
        );
        assert_eq!(j.text, "a\nb redX");
        let red = j.sections.iter().find(|s| s.format.color == Color32::from_rgb(255, 0, 0)).unwrap();
        assert_eq!(red.format.font_id.size, 20.0);
        assert_eq!(plain("1 < 2 <!-- note -->ok <unknown>"), "1 < 2 ok <unknown>");
    }
}
