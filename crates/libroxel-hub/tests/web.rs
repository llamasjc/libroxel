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

#[test]
fn the_app_only_calls_routes_the_hub_has() {
    let routes: Vec<Vec<String>> =
        quoted_after(HUB, ".route(").into_iter().map(|r| segments(r, "{", '}')).collect();
    let calls: Vec<String> = HUB
        .split("format!(\"{}/")
        .skip(1)
        .filter_map(|rest| rest.split_once('"').map(|(path, _)| format!("/{path}")))
        .collect();
    assert!(calls.len() > 10, "found only {calls:?}");
    for call in calls {
        let call_segments = segments(&call, "{", '}');
        let known = routes.iter().any(|route| {
            route.len() == call_segments.len()
                && route.iter().zip(&call_segments).all(|(r, c)| r == c || r == "*" || c == "*")
        });
        assert!(known, "HubClient calls {call}, which the Hub doesn't have");
    }
}

const ROUTES: &str = include_str!("../../../web/routes.js");
const SITEMAP: &str = include_str!("../../../web/sitemap.xml");
const SITE_SETUP: &str = include_str!("../../../packaging/web/setup-site.sh");

fn pages() -> Vec<String> {
    let table = ROUTES.split("export const ROUTES = {").nth(1).and_then(|t| t.split("\n};").next()).unwrap();
    let aliases = ROUTES.split("export const ALIASES = {").nth(1).and_then(|t| t.split('}').next()).unwrap();
    let mut names: Vec<String> = table
        .lines()
        .chain(aliases.split(','))
        .filter_map(|line| line.split_once(':').map(|(name, _)| name.trim().trim_matches('\'').to_owned()))
        .filter(|name| !name.is_empty())
        .collect();
    names.push(String::new());
    names
}

fn first_segment(link: &str) -> &str {
    link.trim_start_matches('/').split(['/', '?', '#']).next().unwrap_or("")
}

fn app_links() -> Vec<(String, String)> {
    let mut links = Vec::new();
    for marker in ["href: ", "goTo("] {
        for link in quoted_after(SITE, marker).into_iter().filter(|l| l.starts_with('/')) {
            links.push(("web/app.js".to_owned(), link.to_owned()));
        }
    }
    for loc in SITEMAP.split("<loc>").skip(1) {
        let url = loc.split("</loc>").next().unwrap();
        links.push(("web/sitemap.xml".to_owned(), url.trim_start_matches("https://playlibre.net").to_owned()));
    }
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../libroxel/src");
    let mut dirs = vec![src];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") || path.ends_with("url_handler.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            for rest in text.split("\"https://playlibre.net").skip(1) {
                let link = rest.split('"').next().unwrap();
                links.push((
                    path.display().to_string(),
                    if link.is_empty() { "/".into() } else { link.into() },
                ));
            }
        }
    }
    links
}

#[test]
fn every_link_to_the_site_is_a_page() {
    let pages = pages();
    let links = app_links();
    assert!(links.len() > 20, "found only {links:?}");
    for (from, link) in links {
        let page = first_segment(&link);
        assert!(
            page.starts_with("${") || pages.iter().any(|p| p == page),
            "{from} links to {link}, which isn't a page"
        );
    }
}

#[test]
fn every_page_has_a_view() {
    let views = SITE.split("const views = {").nth(1).and_then(|v| v.split('}').next()).unwrap();
    let shown: Vec<&str> =
        views.split(',').map(|v| v.split(':').next().unwrap().trim().trim_matches('\'')).collect();
    let table = ROUTES.split("export const ROUTES = {").nth(1).and_then(|t| t.split("\n};").next()).unwrap();
    for line in table.lines().filter(|l| l.contains(':')) {
        let name = line.split(':').next().unwrap().trim().trim_matches('\'');
        assert!(shown.contains(&name), "the page {name} has no view in web/app.js");
    }
}

#[test]
fn the_server_sends_every_page_to_the_site() {
    assert!(SITE_SETUP.contains("try_files {path} /index.html"));
}

#[test]
fn the_site_knows_when_the_hub_is_older() {
    assert!(SITE.contains(&format!("'{}'", libroxel_hub::NO_ROUTE)));
    let hub = libroxel_hub::start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let mut r = ureq::get(format!("{}/no/such/route", hub.url()))
        .config()
        .http_status_as_error(false)
        .build()
        .call()
        .unwrap();
    assert_eq!(r.status(), 404);
    assert_eq!(r.body_mut().read_to_string().unwrap(), libroxel_hub::NO_ROUTE);
}
