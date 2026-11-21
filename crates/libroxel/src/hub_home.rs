//! The Home and Friends tabs: friends along the top with who is online and
//! in which game, every game with how many play it, and for each game a
//! page with its live servers (players, ping, friends inside and a Join
//! button), the way a server browser lists them.

use bevy_egui::egui::{self, Color32, RichText};
use libroxel_engine::character::Look;
use libroxel_hub::{Friend, FriendAction, GameInfo, ServerInfo};
use libroxel_instance::serde_json::Value as Json;

use crate::browse::Platform;
use crate::menu::{Game, Pick, Tab, paint_blocks, picture_colour};
use crate::social::starter_id;
use crate::starter_games::STARTER_GAMES;
use crate::theme::{self, icon};

const SOFT: Color32 = theme::MUTED;

/// What the Home tab shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum View {
    #[default]
    Home,
    /// One game's page, by its key (see [`Entry::key`]).
    Game(String),
}

/// How the server list is sorted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Sort {
    #[default]
    Players,
    Ping,
}

/// What the Home and Friends tabs remember.
#[derive(Default)]
pub struct HomeState {
    pub view: View,
    pub sort: Sort,
    /// Hide servers with no room.
    pub hide_full: bool,
    /// The friend whose card is open on the Friends tab.
    confirm_unfriend: Option<String>,
    /// A `libroxel://` link from the website, acted on once the lists are in.
    pub launch: Option<crate::url_handler::PendingLaunch>,
    /// The game (its key) Play is finding a server for on the Hub.
    finding_for: Option<String>,
}

/// Where a game on the home screen comes from.
#[derive(Clone)]
enum Source {
    Starter(&'static str),
    Published(Box<GameInfo>),
    /// Only known from servers that run it.
    Live,
}

/// A game on the home screen, with the servers running it.
#[derive(Clone)]
struct Entry {
    /// Its id (`starter:<name>` or a published id), or `live:<title>`.
    key: String,
    title: String,
    by: String,
    about: String,
    symbol: &'static str,
    source: Source,
    servers: Vec<ServerInfo>,
}

impl Entry {
    fn playing(&self) -> u32 {
        self.servers.iter().map(|s| s.players).sum()
    }

    /// What to host when starting a new server, if this computer can.
    fn hostable(&self) -> bool {
        !matches!(self.source, Source::Live)
    }
}

/// Every game worth showing: the starter games, the published ones and any
/// other game someone is hosting, each with its live servers.
fn entries(platform: &Platform) -> Vec<Entry> {
    let mut list: Vec<Entry> = STARTER_GAMES
        .iter()
        .map(|g| Entry {
            key: starter_id(g.name),
            title: g.name.to_owned(),
            by: "Libroxel".into(),
            about: g.about.to_owned(),
            symbol: g.symbol,
            source: Source::Starter(g.name),
            servers: Vec::new(),
        })
        .collect();
    for game in platform.games() {
        list.push(Entry {
            key: game.game_id.clone(),
            title: game.title.clone(),
            by: game.author.clone(),
            about: game.description.clone(),
            symbol: icon::GLOBE_HEMISPHERE_WEST,
            source: Source::Published(Box::new(game.clone())),
            servers: Vec::new(),
        });
    }
    for server in platform.servers() {
        let found = list.iter_mut().find(|e| match &server.game_id {
            Some(id) => e.key == *id,
            // Hosts that don't say which game: match by title.
            None => e.title == server.game,
        });
        match found {
            Some(entry) => entry.servers.push(server.clone()),
            None => list.push(Entry {
                key: server.game_id.clone().unwrap_or_else(|| format!("live:{}", server.game)),
                title: server.game.clone(),
                by: "Someone hosting".into(),
                about: String::new(),
                symbol: icon::BROADCAST,
                source: Source::Live,
                servers: vec![server.clone()],
            }),
        }
    }
    // Busiest first; the rest keep their order.
    list.sort_by_key(|e| std::cmp::Reverse(e.playing()));
    list
}

fn rgb(c: libroxel_instance::Color3) -> Color32 {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgb(b(c.r), b(c.g), b(c.b))
}

fn seed(name: &str) -> usize {
    name.bytes().fold(7usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize))
}

/// A round portrait of someone's avatar: their head and shoulders in their
/// own colours.
pub fn paint_avatar(painter: &egui::Painter, centre: egui::Pos2, r: f32, look: &Json, name: &str) {
    let look = Look::from_json(look, Look::for_player(seed(name)));
    theme::radial(painter, centre, r, Color32::from_rgb(236, 248, 255), Color32::from_rgb(160, 208, 244));
    // Shoulders: the part of the circle below a line, in the shirt colour.
    let cut = centre.y + r * 0.42;
    let mut shoulders = Vec::new();
    let steps = 24;
    let half = ((r * r - (cut - centre.y).powi(2)).max(0.0)).sqrt();
    let start = (cut - centre.y).atan2(half);
    let end = std::f32::consts::PI - start;
    for i in 0..=steps {
        let a = start + (end - start) * i as f32 / steps as f32;
        shoulders.push(egui::pos2(centre.x + r * a.cos(), centre.y + r * a.sin()));
    }
    painter.add(egui::Shape::convex_polygon(shoulders, rgb(look.colors.shirt), egui::Stroke::NONE));
    // The head: a rounded block.
    let head =
        egui::Rect::from_center_size(centre + egui::vec2(0.0, -r * 0.08), egui::vec2(r * 0.86, r * 0.8));
    painter.rect_filled(head, r * 0.16, rgb(look.colors.skin));
    let eye = r * 0.075;
    for side in [-1.0, 1.0] {
        painter.circle_filled(
            head.center() + egui::vec2(side * r * 0.16, -r * 0.05),
            eye,
            Color32::from_rgb(30, 26, 22),
        );
    }
    painter.line_segment(
        [head.center() + egui::vec2(-r * 0.12, r * 0.17), head.center() + egui::vec2(r * 0.12, r * 0.17)],
        egui::Stroke::new((r * 0.05).max(1.0), Color32::from_rgb(30, 26, 22)),
    );
    // A hat's colour as a band on top, when they wear one.
    if look.hat != libroxel_engine::character::Hat::None || look.item.is_some() {
        let band = egui::Rect::from_min_max(head.left_top(), head.right_top() + egui::vec2(0.0, r * 0.18));
        painter.rect_filled(band.translate(egui::vec2(0.0, -r * 0.12)), r * 0.08, rgb(look.hat_color));
    }
    // Glass over the portrait: a gloss on its upper half and a bright rim.
    let gleam = egui::Rect::from_center_size(centre - egui::vec2(0.0, r * 0.5), egui::vec2(r * 1.5, r * 0.9));
    theme::gradient(
        painter,
        gleam,
        r * 0.45,
        &[
            (0.0, Color32::from_rgba_unmultiplied(255, 255, 255, 120)),
            (1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 0)),
        ],
    );
    painter.circle_stroke(centre, r, egui::Stroke::new((r * 0.06).clamp(1.0, 2.5), Color32::WHITE));
    painter.circle_stroke(centre, r + 0.5, egui::Stroke::new(1.0, theme::see_through(30, 80, 140, 110)));
}

/// Where a friend is, as a word and a colour.
fn whereabouts(friend: &Friend) -> (String, Color32) {
    match (&friend.game, friend.online) {
        (Some(game), true) => (game.clone(), theme::LINK),
        (None, true) => ("Online".into(), theme::GREEN),
        _ => (
            match friend.last_online_secs_ago {
                Some(secs) => format!("Offline · {}", ago(secs)),
                None => "Offline".into(),
            },
            SOFT,
        ),
    }
}

/// "5 min ago", "3 h ago", "2 days ago".
fn ago(secs: i64) -> String {
    match secs {
        ..60 => "just now".into(),
        60..3600 => format!("{} min ago", secs / 60),
        3600..86400 => format!("{} h ago", secs / 3600),
        _ => match secs / 86400 {
            1 => "1 day ago".into(),
            n => format!("{n} days ago"),
        },
    }
}

/// A big round number for player counts: 950, 1.2K, 34K.
fn count(n: u32) -> String {
    match n {
        ..1000 => n.to_string(),
        1000..10_000 => format!("{:.1}K", n as f32 / 1000.0),
        _ => format!("{}K", n / 1000),
    }
}

/// A friend in the row along the top of Home: portrait, status dot, name
/// and what they're doing.
fn friend_bubble(ui: &mut egui::Ui, friend: &Friend) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(96.0, 128.0), egui::Sense::click());
    let painter = ui.painter_at(rect);
    let centre = egui::pos2(rect.center().x, rect.min.y + 42.0);
    if response.hovered() {
        painter.circle_filled(centre, 43.0, theme::see_through(120, 200, 255, 110));
    }
    paint_avatar(&painter, centre, 38.0, &friend.look, &friend.name);
    let (status, colour) = whereabouts(friend);
    if friend.online {
        let dot = centre + egui::vec2(27.0, 27.0);
        painter.circle_filled(dot, 8.5, Color32::WHITE);
        theme::orb(&painter, dot, 6.5, colour);
    }
    let width = rect.width();
    let centred = |y: f32, text: &str, font: egui::FontId, colour: Color32| {
        let galley = painter.layout(text.to_owned(), font, colour, width);
        let galley = if galley.rows.len() > 1 {
            painter.layout_no_wrap(
                format!("{}…", text.chars().take(11).collect::<String>()),
                galley.job.sections[0].format.font_id.clone(),
                colour,
            )
        } else {
            galley
        };
        painter.galley(egui::pos2(rect.center().x - galley.size().x / 2.0, rect.min.y + y), galley, colour);
    };
    centred(88.0, &friend.name, theme::display(14.0), theme::TEXT);
    if friend.online {
        centred(107.0, &status, egui::FontId::proportional(11.5), colour);
    }
    let hover = match &friend.server {
        Some(server) => format!("{} is in {} on {}. Click to join.", friend.name, status, server.name),
        None if friend.game.is_some() => format!("{} is playing {status}.", friend.name),
        None => format!("{}: {status}", friend.name),
    };
    response.on_hover_text(hover).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The round "add friends" button at the start of the row.
fn add_bubble(ui: &mut egui::Ui) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(96.0, 128.0), egui::Sense::click());
    let painter = ui.painter_at(rect);
    let centre = egui::pos2(rect.center().x, rect.min.y + 42.0);
    let top = if response.hovered() { Color32::from_rgb(240, 250, 255) } else { Color32::WHITE };
    theme::radial(&painter, centre + egui::vec2(0.0, 10.0), 40.0, top, Color32::from_rgb(196, 226, 248));
    painter.circle_stroke(centre, 38.0, egui::Stroke::new(1.5, Color32::from_rgb(120, 170, 220)));
    painter.text(
        centre,
        egui::Align2::CENTER_CENTER,
        icon::USER_PLUS,
        egui::FontId::proportional(28.0),
        theme::ACCENT,
    );
    let galley = painter.layout_no_wrap("Add friends".into(), theme::display(14.0), theme::TEXT);
    painter.galley(
        egui::pos2(rect.center().x - galley.size().x / 2.0, rect.min.y + 88.0),
        galley,
        theme::TEXT,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A game's tile: its picture, name, and how many play it right now, with
/// friends' faces on it when they are in.
fn game_tile(ui: &mut egui::Ui, entry: &Entry, friends: &[&Friend]) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(188.0, 214.0), egui::Sense::click());
    let painter = ui.painter_at(rect.expand(20.0));
    let hovered = response.hovered();
    let face = rect.translate(egui::vec2(0.0, if hovered { -3.0 } else { 0.0 }));
    let colour = picture_colour(&entry.key);
    let picture = egui::Rect::from_min_size(face.min, egui::vec2(face.width(), 150.0));
    theme::soft_shadow(&painter, picture, 8.0, if hovered { 90 } else { 55 });
    if hovered {
        let glow = egui::Shadow {
            offset: [0, 0],
            blur: 14,
            spread: 1,
            color: theme::see_through(90, 180, 255, 160),
        };
        painter.add(glow.as_shape(picture, 8));
    }
    paint_blocks(&painter, picture, 8.0, &entry.key);
    let chip = egui::Rect::from_min_size(picture.min + egui::vec2(8.0, 8.0), egui::vec2(26.0, 26.0));
    theme::glass(&painter, chip, 6.0, colour);
    painter.text(
        chip.center(),
        egui::Align2::CENTER_CENTER,
        entry.symbol,
        egui::FontId::proportional(16.0),
        Color32::WHITE,
    );
    // Friends inside, as small faces in the corner.
    for (i, friend) in friends.iter().take(3).enumerate() {
        let at = egui::pos2(picture.max.x - 18.0 - i as f32 * 22.0, picture.min.y + 20.0);
        painter.circle_filled(at, 13.0, Color32::WHITE);
        paint_avatar(&painter, at, 11.5, &friend.look, &friend.name);
    }
    // Glass over the picture: a gloss over its top and a bright rim.
    let gloss = egui::Rect::from_min_max(picture.min, egui::pos2(picture.max.x, picture.min.y + 46.0));
    theme::gradient(
        &painter,
        gloss.shrink(1.0),
        7.0,
        &[
            (0.0, Color32::from_rgba_unmultiplied(255, 255, 255, 90)),
            (1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 0)),
        ],
    );
    painter.rect_stroke(
        picture,
        8,
        egui::Stroke::new(
            if hovered { 2.0 } else { 1.0 },
            if hovered { theme::ACCENT } else { Color32::from_rgb(130, 176, 222) },
        ),
        egui::StrokeKind::Inside,
    );
    let width = face.width();
    theme::text_line(
        &painter,
        egui::pos2(face.min.x, picture.max.y + 10.0),
        &entry.title,
        theme::display(16.0),
        theme::TEXT,
        width,
    );
    let playing = entry.playing();
    let (line, colour) = if !friends.is_empty() {
        let names: Vec<&str> = friends.iter().map(|f| f.name.as_str()).collect();
        (format!("{}  {} playing · {}", icon::USERS, count(playing), names.join(", ")), theme::LINK)
    } else if playing > 0 {
        (format!("{}  {} playing", icon::USERS, count(playing)), theme::GREEN)
    } else {
        (format!("{}  by {}", icon::USER, entry.by), SOFT)
    };
    theme::text_line(
        &painter,
        egui::pos2(face.min.x, picture.max.y + 34.0),
        &line,
        egui::FontId::proportional(12.5),
        colour,
        width,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Signal bars and milliseconds for a ping, like a server browser.
fn ping_badge(ui: &mut egui::Ui, ping: Option<Result<u32, String>>) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(78.0, 22.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let (bars, colour, text) = match &ping {
        None => (0, SOFT, "…".to_owned()),
        Some(Err(_)) => (0, theme::RED, "no answer".to_owned()),
        Some(Ok(ms)) => {
            let (bars, colour) = match ms {
                ..60 => (4, theme::GREEN),
                60..120 => (3, theme::GREEN),
                120..250 => (2, theme::AMBER),
                _ => (1, theme::RED),
            };
            (bars, colour, format!("{ms} ms"))
        }
    };
    for i in 0..4 {
        let h = 5.0 + i as f32 * 3.5;
        let bar = egui::Rect::from_min_size(
            egui::pos2(rect.min.x + i as f32 * 5.0, rect.max.y - 3.0 - h),
            egui::vec2(3.5, h),
        );
        painter.rect_filled(bar, 1.0, if i < bars { colour } else { theme::LINE });
    }
    painter.text(
        egui::pos2(rect.min.x + 25.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        egui::FontId::proportional(12.5),
        if bars > 0 { theme::TEXT } else { colour },
    );
    if let Some(Err(why)) = ping {
        response.on_hover_text(why);
    } else {
        response.on_hover_text("How long a message takes to get there and back, measured from this computer");
    }
}

/// How full a server is, as a bar with the numbers on it.
fn players_bar(ui: &mut egui::Ui, server: &ServerInfo) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(110.0, 22.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let track =
        egui::Rect::from_min_size(egui::pos2(rect.min.x, rect.max.y - 5.0), egui::vec2(rect.width(), 4.0));
    painter.rect_filled(track, 1.0, theme::LINE);
    let text = if server.max_players > 0 {
        let share = (server.players as f32 / server.max_players as f32).clamp(0.0, 1.0);
        let fill = if share >= 1.0 { theme::RED } else { theme::GREEN };
        painter.rect_filled(
            egui::Rect::from_min_size(track.min, egui::vec2(track.width() * share, 4.0)),
            1.0,
            fill,
        );
        format!("{} / {} players", server.players, server.max_players)
    } else {
        format!("{} {}", server.players, if server.players == 1 { "player" } else { "players" })
    };
    painter.text(
        rect.left_top(),
        egui::Align2::LEFT_TOP,
        text,
        egui::FontId::proportional(12.5),
        theme::TEXT,
    );
}

fn full(server: &ServerInfo) -> bool {
    server.max_players > 0 && server.players >= server.max_players
}

/// One server's row: picture, name, host and friends inside, players,
/// ping and Join. Returns whether Join was pressed.
fn server_row(ui: &mut egui::Ui, platform: &mut Platform, server: &ServerInfo, key: &str) -> bool {
    let friends: Vec<(String, Json)> = platform
        .social
        .friends_in(&server.server_id)
        .iter()
        .map(|f| (f.name.clone(), f.look.clone()))
        .collect();
    let ping = platform.social.ping(&server.code);
    let mut join = false;
    let frame = egui::Frame::new()
        .fill(if friends.is_empty() { theme::CARD } else { theme::CARD.lerp_to_gamma(theme::LINK, 0.14) })
        .stroke(egui::Stroke::new(
            1.0,
            if friends.is_empty() { theme::LINE } else { theme::LINK.gamma_multiply(0.5) },
        ))
        .corner_radius(theme::CORNER)
        .inner_margin(egui::Margin::symmetric(12, 10));
    frame.show(ui, |ui| {
        ui.set_min_width(ui.available_width() - 12.0);
        ui.horizontal(|ui| {
            // A small picture of the game, as server lists show a server's icon.
            let (icon_rect, _) = ui.allocate_exact_size(egui::vec2(52.0, 52.0), egui::Sense::hover());
            let painter = ui.painter_at(icon_rect);
            paint_blocks(&painter, icon_rect, theme::CORNER as f32, key);
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.set_width((ui.available_width() - 330.0).max(160.0));
                ui.label(RichText::new(&server.name).font(theme::display(16.0)).color(theme::TEXT));
                ui.horizontal(|ui| {
                    let kind_colour = match server.kind {
                        libroxel_hub::HostKind::Community => theme::GREEN,
                        libroxel_hub::HostKind::Player => theme::LINK,
                        libroxel_hub::HostKind::Official => theme::AMBER,
                    };
                    let kind = if server.hub_server { "Libroxel" } else { server.kind.label() };
                    ui.label(RichText::new(format!("{kind} server")).small().color(kind_colour))
                        .on_hover_text(if server.hub_server {
                            "Runs on Libroxel's own servers, so it's there whoever is playing"
                        } else {
                            "Someone runs this one themselves"
                        });
                    // Only an account the Hub checked; the server's name is whatever the host typed.
                    if server.admin_host {
                        ui.label(RichText::new("Admin").small().strong().color(theme::AMBER))
                            .on_hover_text("Run by Libroxel, checked by the Hub.");
                    } else if let Some(host) = &server.host {
                        let verified = if server.host_verified { " · verified" } else { "" };
                        ui.label(
                            RichText::new(format!("hosted by {host}{verified}")).small().color(theme::TEXT),
                        )
                        .on_hover_text("The Hub checked this account really hosts it.");
                    }
                    if !friends.is_empty() {
                        for (name, look) in friends.iter().take(4) {
                            let (r, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                            paint_avatar(ui.painter(), r.center(), 9.0, look, name);
                        }
                        let names: Vec<&str> = friends.iter().map(|(n, _)| n.as_str()).collect();
                        let verb = if names.len() == 1 { "is" } else { "are" };
                        ui.label(
                            RichText::new(format!("{} {verb} here", names.join(", ")))
                                .small()
                                .color(theme::LINK),
                        );
                    }
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let open = !full(server);
                let button = ui
                    .add_enabled_ui(open, |ui| {
                        theme::block_button(
                            ui,
                            theme::Kind::Go,
                            icon::SIGN_IN,
                            if open { "Join" } else { "Full" },
                            34.0,
                        )
                    })
                    .inner;
                join = button.clicked();
                ui.add_space(14.0);
                ping_badge(ui, ping);
                ui.add_space(10.0);
                players_bar(ui, server);
            });
        });
    });
    join
}

/// Play: into a friend's server if one has room, otherwise onto one of the
/// Hub's own servers (it finds one with room or starts one). Only on a Hub
/// that runs none does Play fall back to players' servers, or hosting here.
fn press_play(state: &mut HomeState, platform: &mut Platform, entry: &Entry, name: &str) -> Option<Pick> {
    let with_friends = entry
        .servers
        .iter()
        .filter(|s| !full(s) && !platform.social.friends_in(&s.server_id).is_empty())
        .max_by_key(|s| platform.social.friends_in(&s.server_id).len());
    if let Some(server) = with_friends {
        return Some(Pick::Join { code: server.code.clone(), name: name.to_owned() });
    }
    if !entry.hostable() {
        // Only known from players' servers: join one of those.
        return best_server(platform, &entry.servers)
            .map(|s| Pick::Join { code: s.code.clone(), name: name.to_owned() });
    }
    state.finding_for = Some(entry.key.clone());
    platform.find_server(&entry.key, &entry.title);
    None
}

/// What Play came to once the Hub answered: the server to join, or (on a
/// Hub with no servers of its own) the old way: the busiest server with
/// room, else a new one here.
fn found(state: &mut HomeState, platform: &mut Platform, list: &[Entry], name: &str) -> Option<Pick> {
    let answer = platform.take_found()?;
    let key = state.finding_for.take()?;
    if let Some(code) = answer {
        return Some(Pick::Join { code, name: name.to_owned() });
    }
    let entry = list.iter().find(|e| e.key == key)?;
    match best_server(platform, &entry.servers) {
        Some(server) => Some(Pick::Join { code: server.code.clone(), name: name.to_owned() }),
        None => host(entry, platform),
    }
}

/// The server to drop someone into when they press Play: one with friends
/// in it first, then the busiest with room.
fn best_server<'a>(platform: &Platform, servers: &'a [ServerInfo]) -> Option<&'a ServerInfo> {
    servers
        .iter()
        .filter(|s| !full(s))
        .max_by_key(|s| (platform.social.friends_in(&s.server_id).len(), s.players))
}

/// Acts on a `libroxel://` link from the website: joins a server, or opens
/// a game's page and plays or hosts it, once the Hub's lists are in (a
/// published game's page needs them). Links for games the Hub doesn't list
/// give up after a while and leave the player on Home.
fn follow_link(state: &mut HomeState, platform: &mut Platform, list: &[Entry], name: &str) -> Option<Pick> {
    use crate::url_handler::Launch;
    let pending = state.launch.take()?;
    let key = match &pending.launch {
        Launch::Join(code) => return Some(Pick::Join { code: code.clone(), name: name.to_owned() }),
        Launch::Play(key) | Launch::Host(key) => key.clone(),
    };
    let Some(entry) = list.iter().find(|e| e.key == key).filter(|_| platform.listed()) else {
        if pending.since.elapsed() < std::time::Duration::from_secs(15) {
            state.launch = Some(pending);
        }
        return None;
    };
    state.view = View::Game(key);
    match pending.launch {
        Launch::Play(_) => press_play(state, platform, entry, name),
        _ => host(entry, platform),
    }
}

/// Starts a new server of `entry` on this computer, with friends welcome.
fn host(entry: &Entry, platform: &mut Platform) -> Option<Pick> {
    match &entry.source {
        Source::Starter(name) => Some(Pick::Host(Game::Starter(name))),
        Source::Published(game) => {
            platform.get_game(game, true);
            None
        }
        Source::Live => None,
    }
}

/// A section title with a count and an optional link at the right.
fn section(ui: &mut egui::Ui, title: &str, link: Option<&str>) -> bool {
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).font(theme::display(20.0)).color(theme::TEXT));
        if let Some(link) = link {
            ui.add_space(8.0);
            clicked = ui
                .add(
                    egui::Label::new(
                        RichText::new(format!("{link} {}", icon::CARET_RIGHT)).color(theme::LINK),
                    )
                    .sense(egui::Sense::click()),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked();
        }
    });
    clicked
}

/// The Home tab. `tab` is set to switch tabs.
pub fn home_tab(
    state: &mut HomeState,
    ui: &mut egui::Ui,
    platform: &mut Platform,
    name: &str,
    tab: &mut Tab,
) -> Option<Pick> {
    platform.refresh_if_old();
    let mut pick = None;
    if let Some(done) = platform.take_download() {
        let game = Game::File(done.path);
        pick = Some(if done.host { Pick::Host(game) } else { Pick::Play(game) });
    }
    let list = entries(platform);
    if let Some(launched) = follow_link(state, platform, &list, name) {
        return Some(launched);
    }
    if let Some(joining) = found(state, platform, &list, name) {
        return Some(joining);
    }
    if let View::Game(key) = state.view.clone() {
        match list.iter().find(|e| e.key == key) {
            Some(entry) => return game_page(state, ui, platform, entry, name).or(pick),
            None => state.view = View::Home,
        }
    }
    // Hello, with your own face.
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(64.0, 64.0), egui::Sense::hover());
        let look = platform.account.as_ref().map_or(Json::Null, |a| a.look.clone());
        paint_avatar(ui.painter(), rect.center(), 30.0, &look, name);
        ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.add_space(8.0);
            ui.label(
                RichText::new(format!("Hello, {name}!")).font(theme::display_bold(28.0)).color(theme::TEXT),
            );
            let online = platform.social.friends.friends.iter().filter(|f| f.online).count();
            let note = match (platform.account.is_some(), online) {
                (false, _) => "Make an account on the Avatar tab to add friends.".to_owned(),
                (true, 0) => "None of your friends are online right now.".to_owned(),
                (true, 1) => "1 friend is online.".to_owned(),
                (true, n) => format!("{n} friends are online."),
            };
            ui.label(RichText::new(note).color(SOFT));
        });
    });
    ui.add_space(18.0);
    // Friends, the ones in games first.
    let friends = platform.social.friends.friends.clone();
    let waiting = platform.social.friends.incoming.len();
    let title = format!("Friends ({})", friends.len());
    if section(ui, &title, Some("See all")) {
        *tab = Tab::Friends;
    }
    if waiting > 0 {
        let label =
            format!("{} {waiting} friend request{} waiting", icon::BELL, if waiting == 1 { "" } else { "s" });
        if ui
            .add(egui::Label::new(RichText::new(label).color(theme::AMBER)).sense(egui::Sense::click()))
            .clicked()
        {
            *tab = Tab::Friends;
        }
    }
    ui.add_space(6.0);
    egui::ScrollArea::horizontal().id_salt("friends-row").max_height(136.0).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if add_bubble(ui).clicked() {
                *tab = Tab::Friends;
            }
            for friend in &friends {
                let bubble = friend_bubble(ui, friend);
                if bubble.clicked() {
                    if let Some(server) = &friend.server {
                        pick = Some(Pick::Join { code: server.code.clone(), name: name.to_owned() });
                    } else if let Some(id) = &friend.game_id {
                        state.view = View::Game(id.clone());
                    } else {
                        *tab = Tab::Friends;
                    }
                }
            }
        });
    });
    ui.add_space(18.0);
    // Games, busiest first.
    let in_games: Vec<&Friend> = friends.iter().filter(|f| f.game.is_some()).collect();
    section(ui, "Games", None);
    ui.label(
        RichText::new("Pick a game to see its servers, or press Play on its page to jump in.").color(SOFT),
    );
    ui.add_space(10.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(18.0, 18.0);
        for entry in &list {
            let here: Vec<&Friend> = in_games
                .iter()
                .copied()
                .filter(|f| {
                    f.game_id.as_deref() == Some(entry.key.as_str())
                        || (f.game_id.is_none() && f.game.as_deref() == Some(entry.title.as_str()))
                })
                .collect();
            if game_tile(ui, entry, &here).clicked() {
                state.view = View::Game(entry.key.clone());
            }
        }
    });
    if let Some(title) = platform.downloading() {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(format!("Getting {title}…"));
        });
    }
    pick
}

/// One game's page: its picture, Play, and every server running it.
fn game_page(
    state: &mut HomeState,
    ui: &mut egui::Ui,
    platform: &mut Platform,
    entry: &Entry,
    name: &str,
) -> Option<Pick> {
    let mut pick = None;
    if ui
        .add(
            egui::Label::new(RichText::new(format!("{} Home", icon::ARROW_LEFT)).color(theme::LINK))
                .sense(egui::Sense::click()),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
    {
        state.view = View::Home;
    }
    ui.add_space(10.0);
    let playing = entry.playing();
    ui.horizontal_top(|ui| {
        // The picture.
        let (rect, _) = ui.allocate_exact_size(egui::vec2(440.0, 248.0), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        paint_blocks(&painter, rect, theme::CORNER as f32, &entry.key);
        painter.rect_stroke(
            rect,
            theme::CORNER,
            egui::Stroke::new(1.0, theme::LINE),
            egui::StrokeKind::Inside,
        );
        ui.add_space(26.0);
        ui.vertical(|ui| {
            ui.label(RichText::new(&entry.title).font(theme::display_bold(34.0)).color(theme::TEXT));
            ui.label(RichText::new(format!("By {}", entry.by)).color(theme::LINK));
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                let stat = |ui: &mut egui::Ui, value: String, label: &str| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(value).font(theme::display(20.0)).color(theme::TEXT));
                        theme::label_caps(ui, label);
                    });
                    ui.add_space(26.0);
                };
                stat(ui, count(playing), "Playing");
                stat(ui, entry.servers.len().to_string(), "Servers");
                if let Source::Published(game) = &entry.source {
                    stat(ui, count(game.plays), "Visits");
                    stat(ui, format!("v{}", game.version), "Version");
                }
            });
            ui.add_space(16.0);
            let can_play = entry.hostable() || best_server(platform, &entry.servers).is_some();
            let busy = platform.downloading().is_some() || platform.finding_server().is_some();
            ui.horizontal(|ui| {
                let play = ui
                    .add_enabled_ui(can_play && !busy, |ui| {
                        theme::block_button(ui, theme::Kind::Go, icon::PLAY, "      Play      ", 52.0)
                    })
                    .inner;
                let hint = if entry.hostable() {
                    "Puts you on a Libroxel server of this game (with your friends, if they're playing)"
                } else {
                    "Joins the busiest server with room"
                };
                if play.on_hover_text(hint).clicked() {
                    pick = press_play(state, platform, entry, name);
                }
                ui.add_space(10.0);
                if entry.hostable()
                    && theme::block_button(ui, theme::Kind::Plain, icon::BROADCAST, "Host it myself", 52.0)
                        .on_hover_text(
                            "Run a server of this game on your own computer instead; it shows on this list",
                        )
                        .clicked()
                {
                    pick = host(entry, platform);
                }
            });
            if platform.finding_server().is_some() && state.finding_for.as_deref() == Some(entry.key.as_str())
            {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Finding you a server…");
                    if ui.small_button("Cancel").clicked() {
                        platform.stop_finding();
                        state.finding_for = None;
                    }
                });
            } else if let Some(why) = &platform.find_error {
                ui.colored_label(theme::RED, format!("{why} You can also host it yourself."));
            }
            if let Some(title) = platform.downloading() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!("Getting {title}…"));
                });
            }
        });
    });
    if !entry.about.is_empty() {
        ui.add_space(18.0);
        theme::label_caps(ui, "About");
        ui.label(RichText::new(&entry.about).color(theme::TEXT));
    }
    ui.add_space(22.0);
    // The servers.
    ui.horizontal(|ui| {
        ui.label(RichText::new("Servers").font(theme::display(22.0)).color(theme::TEXT));
        ui.add_space(16.0);
        ui.label(RichText::new("Sort by").color(SOFT));
        ui.selectable_value(&mut state.sort, Sort::Players, "Players");
        ui.selectable_value(&mut state.sort, Sort::Ping, "Ping");
        ui.add_space(8.0);
        ui.checkbox(&mut state.hide_full, "Hide full");
        ui.add_space(8.0);
        if theme::tool_button(ui, icon::ARROW_CLOCKWISE, "Refresh", false).clicked() {
            platform.refresh();
        }
    });
    ui.add_space(8.0);
    let mut servers: Vec<ServerInfo> =
        entry.servers.iter().filter(|s| !(state.hide_full && full(s))).cloned().collect();
    match state.sort {
        Sort::Players => servers.sort_by_key(|s| std::cmp::Reverse(s.players)),
        Sort::Ping => {
            let mut keyed: Vec<(u32, ServerInfo)> = servers
                .into_iter()
                .map(|s| {
                    let ms = match platform.social.ping(&s.code) {
                        Some(Ok(ms)) => ms,
                        _ => u32::MAX,
                    };
                    (ms, s)
                })
                .collect();
            keyed.sort_by_key(|(ms, _)| *ms);
            servers = keyed.into_iter().map(|(_, s)| s).collect();
        }
    }
    // Friends' servers on top, as their own group.
    let (with_friends, others): (Vec<ServerInfo>, Vec<ServerInfo>) =
        servers.into_iter().partition(|s| !platform.social.friends_in(&s.server_id).is_empty());
    let mut join = None;
    if !with_friends.is_empty() {
        theme::label_caps(ui, "Servers your friends are in");
        ui.add_space(4.0);
        for server in &with_friends {
            if server_row(ui, platform, server, &entry.key) {
                join = Some(server.code.clone());
            }
            ui.add_space(6.0);
        }
        ui.add_space(10.0);
        theme::label_caps(ui, "Other servers");
        ui.add_space(4.0);
    }
    if others.is_empty() && with_friends.is_empty() {
        theme::panel().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new("No servers are running this game right now.").color(theme::TEXT));
            let more = if entry.hostable() {
                "Press New server to start one; it shows up here for everyone."
            } else {
                "Servers show up here as soon as someone hosts it."
            };
            ui.label(RichText::new(more).color(SOFT));
        });
    }
    for server in &others {
        if server_row(ui, platform, server, &entry.key) {
            join = Some(server.code.clone());
        }
        ui.add_space(6.0);
    }
    if let Some(code) = join {
        pick = Some(Pick::Join { code, name: name.to_owned() });
    }
    pick
}

/// A row in the Friends tab: portrait, name, status and buttons.
fn person_row(
    ui: &mut egui::Ui,
    look: &Json,
    name: &str,
    status: (&str, Color32),
    buttons: impl FnOnce(&mut egui::Ui),
) {
    theme::panel().inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(44.0, 44.0), egui::Sense::hover());
            paint_avatar(ui.painter(), rect.center(), 21.0, look, name);
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.add_space(3.0);
                ui.label(RichText::new(name).font(theme::display(16.0)).color(theme::TEXT));
                ui.label(RichText::new(status.0).small().color(status.1));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), buttons);
        });
    });
    ui.add_space(4.0);
}

/// The Friends tab: find people, answer requests and see every friend.
pub fn friends_tab(
    state: &mut HomeState,
    ui: &mut egui::Ui,
    platform: &mut Platform,
    name: &str,
    tab: &mut Tab,
) -> Option<Pick> {
    platform.refresh_if_old();
    let mut pick = None;
    theme::heading(ui, "Friends");
    if platform.account.is_none() {
        ui.add_space(8.0);
        theme::panel().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new("Make an account first").font(theme::display(17.0)).color(theme::TEXT));
            ui.label(
                RichText::new("Friends find you by your name. It takes a second, on the Avatar tab.")
                    .color(SOFT),
            );
            ui.add_space(6.0);
            if theme::block_button(ui, theme::Kind::Primary, icon::T_SHIRT, "Make an account", 38.0).clicked()
            {
                *tab = Tab::Avatar;
            }
        });
        return None;
    }
    match &platform.social.change_status {
        Some(Ok(said)) => {
            theme::status(ui, theme::GREEN, said);
        }
        Some(Err(why)) => {
            theme::status(ui, theme::RED, why);
        }
        None => {}
    }
    if let Some(error) = &platform.social.friends_error {
        theme::status(ui, theme::RED, error);
    }
    ui.add_space(10.0);
    let friends = platform.social.friends.clone();
    let friends_loaded = platform.social.friends_loaded;
    let changing = platform.social.changing.clone();
    let mut change: Option<(String, String, FriendAction)> = None;
    ui.columns(2, |columns| {
        // Left: your friends.
        let ui = &mut columns[0];
        let online = friends.friends.iter().filter(|f| f.online).count();
        ui.label(
            RichText::new(format!("Your friends ({}) · {online} online", friends.friends.len()))
                .font(theme::display(18.0))
                .color(theme::TEXT),
        );
        ui.add_space(6.0);
        if friends.friends.is_empty() {
            ui.label(
                RichText::new(if friends_loaded {
                    "No friends yet. Find someone by name on the right and send a request."
                } else {
                    "Looking…"
                })
                .color(SOFT),
            );
        }
        for friend in &friends.friends {
            let (status, colour) = whereabouts(friend);
            let status = match &friend.server {
                Some(server) => format!("Playing {status} on {}", server.name),
                None if friend.game.is_some() => format!("Playing {status}"),
                None => status,
            };
            person_row(ui, &friend.look, &friend.name, (&status, colour), |ui| {
                let confirming = state.confirm_unfriend.as_deref() == Some(friend.player_id.as_str());
                if confirming {
                    if ui.button(RichText::new("Unfriend").color(theme::RED)).clicked() {
                        change = Some((friend.player_id.clone(), friend.name.clone(), FriendAction::Remove));
                        state.confirm_unfriend = None;
                    }
                    if ui.button("Keep").clicked() {
                        state.confirm_unfriend = None;
                    }
                } else if ui.small_button(icon::USER_MINUS).on_hover_text("Unfriend").clicked() {
                    state.confirm_unfriend = Some(friend.player_id.clone());
                }
                if let Some(server) = &friend.server {
                    let full = full(server);
                    if ui
                        .add_enabled_ui(!full, |ui| {
                            theme::block_button(ui, theme::Kind::Go, icon::SIGN_IN, "Join", 30.0)
                        })
                        .inner
                        .clicked()
                    {
                        pick = Some(Pick::Join { code: server.code.clone(), name: name.to_owned() });
                    }
                }
            });
        }
        // Right: requests and finding people.
        let ui = &mut columns[1];
        if !friends.incoming.is_empty() {
            ui.label(
                RichText::new(format!("Friend requests ({})", friends.incoming.len()))
                    .font(theme::display(18.0))
                    .color(theme::AMBER),
            );
            ui.add_space(6.0);
            for person in &friends.incoming {
                let busy = changing.contains(&person.player_id);
                person_row(ui, &Json::Null, &person.name, ("Wants to be your friend", SOFT), |ui| {
                    ui.add_enabled_ui(!busy, |ui| {
                        if ui.button("Decline").clicked() {
                            change =
                                Some((person.player_id.clone(), person.name.clone(), FriendAction::Remove));
                        }
                        if theme::block_button(ui, theme::Kind::Go, icon::CHECK, "Accept", 30.0).clicked() {
                            change = Some((person.player_id.clone(), person.name.clone(), FriendAction::Add));
                        }
                    });
                });
            }
            ui.add_space(14.0);
        }
        ui.label(RichText::new("Add friends").font(theme::display(18.0)).color(theme::TEXT));
        ui.add_space(4.0);
        let mut search = std::mem::take(&mut platform.social.people_search);
        let go = search_box(ui, &mut search, "Search by name");
        let me = platform.account.as_ref().map(|a| a.player_id.clone());
        if !platform.social.people_for.is_empty() && platform.social.people_for == search.trim() {
            people_results(ui, &platform.social, me.as_deref(), &mut change);
        }
        if !friends.outgoing.is_empty() {
            ui.add_space(14.0);
            theme::label_caps(ui, "Requests you sent");
            ui.add_space(4.0);
            for person in &friends.outgoing {
                person_row(ui, &Json::Null, &person.name, ("Waiting for them to accept", SOFT), |ui| {
                    if ui.button("Cancel").clicked() {
                        change = Some((person.player_id.clone(), person.name.clone(), FriendAction::Remove));
                    }
                });
            }
        }
        platform.social.people_search = search;
        if go {
            platform.find_people();
        }
    });
    if let Some((id, who, action)) = change {
        platform.change_friend(&id, &who, action);
    }
    pick
}

/// A search box with a Find button; whether to search now.
fn search_box(ui: &mut egui::Ui, search: &mut String, hint: &str) -> bool {
    let mut go = false;
    ui.horizontal(|ui| {
        let field = ui.add(
            egui::TextEdit::singleline(search)
                .hint_text(format!("{} {hint}", icon::MAGNIFYING_GLASS))
                .desired_width(ui.available_width() - 90.0)
                .margin(egui::vec2(8.0, 6.0)),
        );
        go = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if theme::tool_button(ui, icon::MAGNIFYING_GLASS, "Find", false).clicked() {
            go = true;
        }
    });
    go
}

/// The people a name search found, each with what you can do about them.
/// `me` is your own account, which shows up too (as you).
fn people_results(
    ui: &mut egui::Ui,
    social: &crate::social::Social,
    me: Option<&str>,
    change: &mut Option<(String, String, FriendAction)>,
) {
    if social.people.is_empty() {
        ui.label(RichText::new(format!("Nobody called “{}”.", social.people_for)).color(SOFT));
    }
    for person in &social.people {
        let busy = social.changing.contains(&person.player_id);
        let mine = me == Some(person.player_id.as_str());
        let (state_text, colour) = if mine {
            ("That's you", theme::GREEN)
        } else if social.is_friend(&person.player_id) {
            ("Friends", theme::GREEN)
        } else if social.asked(&person.player_id) {
            ("Request sent", theme::LINK)
        } else if social.asked_by(&person.player_id) {
            ("Asked to be your friend", theme::AMBER)
        } else {
            ("", SOFT)
        };
        let about = if state_text.is_empty() { person.about.as_str() } else { state_text };
        let name = format!("{}  #{}", person.name, person.number);
        person_row(ui, &Json::Null, &name, (about, colour), |ui| {
            if mine || me.is_none() || social.is_friend(&person.player_id) {
                return;
            }
            if social.asked(&person.player_id) {
                if ui.add_enabled(!busy, egui::Button::new("Cancel")).clicked() {
                    *change = Some((person.player_id.clone(), person.name.clone(), FriendAction::Remove));
                }
                return;
            }
            let label = if social.asked_by(&person.player_id) { "Accept" } else { "Add friend" };
            if ui
                .add_enabled_ui(!busy, |ui| {
                    theme::block_button(ui, theme::Kind::Primary, icon::USER_PLUS, label, 30.0)
                })
                .inner
                .clicked()
            {
                *change = Some((person.player_id.clone(), person.name.clone(), FriendAction::Add));
            }
        });
    }
}

/// The Search tab: find players and games on the Hub by name.
pub fn search_tab(state: &mut HomeState, ui: &mut egui::Ui, platform: &mut Platform, tab: &mut Tab) {
    platform.refresh_if_old();
    theme::heading(ui, "Search");
    ui.label(RichText::new("Find players and games.").color(SOFT));
    ui.add_space(8.0);
    let mut search = std::mem::take(&mut platform.social.people_search);
    if search_box(ui, &mut search, "A player's name or a game") {
        platform.social.people_search = search.clone();
        platform.find_people();
    }
    platform.social.people_search = search.clone();
    let looked_for = platform.social.people_for.clone();
    if looked_for.is_empty() || looked_for != search.trim() {
        return;
    }
    ui.add_space(12.0);
    let mut change = None;
    let me = platform.account.as_ref().map(|a| a.player_id.clone());
    ui.columns(2, |columns| {
        let ui = &mut columns[0];
        ui.label(RichText::new("Players").font(theme::display(18.0)).color(theme::TEXT));
        ui.add_space(6.0);
        people_results(ui, &platform.social, me.as_deref(), &mut change);

        let ui = &mut columns[1];
        ui.label(RichText::new("Games").font(theme::display(18.0)).color(theme::TEXT));
        ui.add_space(6.0);
        let needle = looked_for.to_lowercase();
        let found: Vec<Entry> = entries(platform)
            .into_iter()
            .filter(|e| {
                e.title.to_lowercase().contains(&needle)
                    || e.by.to_lowercase().contains(&needle)
                    || e.about.to_lowercase().contains(&needle)
            })
            .collect();
        if found.is_empty() {
            ui.label(RichText::new(format!("No games called “{looked_for}”.")).color(SOFT));
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(18.0, 18.0);
            for entry in &found {
                if game_tile(ui, entry, &[]).clicked() {
                    state.view = View::Game(entry.key.clone());
                    *tab = Tab::Home;
                }
            }
        });
    });
    if let Some((id, who, action)) = change {
        platform.change_friend(&id, &who, action);
    }
}
