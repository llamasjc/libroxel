const SITE: &str = include_str!("../../../web/app.js");
const HUB: &str = include_str!("../src/lib.rs");

fn quoted_after<'a>(text: &'a str, marker: &str) -> Vec<&'a str> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(marker) {
        rest = &rest[at + marker.len()..];
        let Some(quote) = rest.chars().next() else { break };
        if !matches!(quote, '\'' | '`' | '"') {
            continue;
        }
        let body = &rest[1..];
        if let Some(end) = body.find(quote) {
            found.push(&body[..end]);
        }
    }
    found
}

fn segments(path: &str, open: &str, close: char) -> Vec<String> {
    let mut out = String::new();
    let mut rest = path;
    while let Some(at) = rest.find(open) {
        out.push_str(&rest[..at]);
        out.push('*');
        rest = rest[at..].split_once(close).map_or("", |(_, after)| after);
    }
    out.push_str(rest);
    out.split('/').map(String::from).collect()
}

#[test]
fn the_site_only_calls_routes_the_hub_has() {
    let routes: Vec<Vec<String>> =
        quoted_after(HUB, ".route(").into_iter().map(|r| segments(r, "{", '}')).collect();
    let calls: Vec<&str> =
        ["api(", "list("].iter().flat_map(|m| quoted_after(SITE, m)).filter(|p| p.starts_with('/')).collect();
    assert!(calls.len() > 10, "found only {calls:?}");
    for call in calls {
        let call_segments = segments(call, "${", '}');
        let known = routes.iter().any(|route| {
            route.len() == call_segments.len()
                && route.iter().zip(&call_segments).all(|(r, c)| r == c || r == "*" || c == "*")
        });
        assert!(known, "web/app.js calls {call}, which the Hub doesn't have");
    }
}
