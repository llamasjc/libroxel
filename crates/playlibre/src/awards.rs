use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy_egui::egui::{self, Color32, FontId, RichText, Stroke, pos2, vec2};
use playlibre_engine::PlaySession;
use playlibre_hub::{Awarded, Badge, BadgeAward, HubClient};

use crate::browse::Platform;
use crate::theme::{self, icon};

type Slot<T> = Arc<Mutex<Option<Result<T, String>>>>;

const MINE_EVERY: Duration = Duration::from_secs(60);
const RETRY_AFTER: Duration = Duration::from_secs(15);
const TRIES: u32 = 4;

#[derive(Default)]
pub struct Awards {
    pub mine: Vec<Badge>,
    mine_for: Option<String>,
    mine_at: Option<Instant>,
    mine_busy: bool,
    mine_slot: Slot<Vec<Badge>>,
    game: Option<String>,
    game_slot: Slot<Vec<Badge>>,
    page: Option<String>,
    page_list: Vec<Badge>,
    page_slot: Slot<(String, Vec<Badge>)>,
    pending: Vec<(String, String, u32, Instant)>,
    sending: Option<(String, String, u32)>,
    sent: Slot<Awarded>,
    pub earned: Vec<String>,
    clock: i64,
}

fn put<T: Send + 'static>(slot: &Slot<T>, work: impl FnOnce() -> Result<T, String> + Send + 'static) {
    let slot = slot.clone();
    std::thread::spawn(move || *slot.lock().unwrap() = Some(work()));
}

fn take<T>(slot: &Slot<T>) -> Option<Result<T, String>> {
    slot.lock().unwrap().take()
}

impl Platform {
    pub fn awards_tick(&mut self, session: Option<&mut PlaySession>) {
        let me = self.account.as_ref().map(|a| a.player_id.clone());
        let game = self.playing_game_id(session.is_some());
        let hub = self.hub_address.clone();
        let a = &mut self.awards;
        if let Some(result) = take(&a.mine_slot) {
            a.mine_busy = false;
            if let Ok(list) = result {
                a.mine = list;
            }
        }
        if me != a.mine_for {
            a.mine.clear();
            a.mine_at = None;
            a.mine_for = me.clone();
        }
        if let Some(id) = me.clone()
            && !a.mine_busy
            && a.mine_at.is_none_or(|at| at.elapsed() > MINE_EVERY)
        {
            a.mine_busy = true;
            a.mine_at = Some(Instant::now());
            let client = HubClient::new(&hub);
            put(&a.mine_slot, move || client.badges(&id));
        }
        match session {
            Some(session) => {
                if let Some(id) = game.clone().filter(|_| game != a.game) {
                    a.game = game.clone();
                    let client = HubClient::new(&hub);
                    put(&a.game_slot, move || client.game_badges(&id));
                }
                if let Some(Ok(list)) = take(&a.game_slot) {
                    let owned = list
                        .iter()
                        .filter(|b| a.mine.iter().any(|m| m.badge_id == b.badge_id))
                        .map(|b| b.badge_id.clone())
                        .collect();
                    let info = list
                        .into_iter()
                        .map(|b| playlibre_script::BadgeInfo {
                            id: b.badge_id,
                            name: b.name,
                            description: b.description,
                        })
                        .collect();
                    session.set_badges(info, owned);
                }
                for id in session.take_earned_badges() {
                    let (Some(game), Some(_)) = (&game, &me) else { continue };
                    let have = a.mine.iter().any(|m| m.badge_id == id) || a.pending.iter().any(|p| p.1 == id);
                    if !have {
                        a.pending.push((game.clone(), id, 0, Instant::now()));
                    }
                }
            }
            None => a.game = None,
        }
        if let Some(result) = take(&a.sent) {
            let (game, id, tries) = a.sending.take().unwrap_or_default();
            match result {
                Ok(awarded) if awarded.new => {
                    a.earned.push(awarded.badge.name.clone());
                    let mut badge = awarded.badge;
                    badge.earned_secs_ago = Some(0);
                    a.mine.insert(0, badge);
                }
                Ok(_) => {}
                Err(_) if tries + 1 < TRIES => {
                    a.pending.push((game, id, tries + 1, Instant::now() + RETRY_AFTER));
                }
                Err(_) => {}
            }
        }
        if a.sending.is_none()
            && let Some(i) = a.pending.iter().position(|p| p.3 <= Instant::now())
        {
            let (game, id, tries, _) = a.pending.remove(i);
            let now = playlibre_hub::hub_now_ms();
            a.clock = now.max(a.clock + 1);
            let time = a.clock;
            a.sending = Some((game.clone(), id.clone(), tries));
            let client = HubClient::new(&hub);
            put(&a.sent, move || {
                let me = crate::identity()?;
                let player_id = me.player_id();
                let signature = me.sign(&playlibre_hub::award_message(&player_id, &game, &id, time));
                client.award_badge(&BadgeAward { player_id, game_id: game, badge_id: id, time, signature })
            });
        }
    }

    pub fn game_badges_ui(&mut self, ui: &mut egui::Ui, key: &str) {
        let client = (self.awards.page.as_deref() != Some(key)).then(|| self.client());
        let a = &mut self.awards;
        if let Some(client) = client {
            a.page = Some(key.to_owned());
            a.page_list.clear();
            let key = key.to_owned();
            put(&a.page_slot, move || client.game_badges(&key).map(|list| (key, list)));
        }
        if let Some(Ok((key, list))) = take(&a.page_slot)
            && a.page.as_deref() == Some(key.as_str())
        {
            a.page_list = list;
        }
        if a.page_list.is_empty() {
            return;
        }
        ui.add_space(18.0);
        theme::label_caps(ui, "Badges");
        let mine = &a.mine;
        shelf(ui, &a.page_list, |b| {
            let have = mine.iter().any(|m| m.badge_id == b.badge_id);
            let won = match b.earned_by {
                1 => "1 player has it".to_owned(),
                n => format!("{n} players have it"),
            };
            if have {
                format!("{}\n{won} · you have it", b.description)
            } else {
                format!("{}\n{won}", b.description)
            }
        });
    }

    pub fn my_badges_ui(&self, ui: &mut egui::Ui) {
        ui.add_space(10.0);
        ui.label(RichText::new("Badges").font(theme::display(17.0)));
        if self.awards.mine.is_empty() {
            ui.label(RichText::new("No badges yet.").color(theme::MUTED));
            return;
        }
        shelf(ui, &self.awards.mine, |b| match &b.game_title {
            Some(game) => format!("{}\n{game}", b.description),
            None => b.description.clone(),
        });
    }
}

pub fn glyph(name: &str) -> &'static str {
    match name {
        "star" => icon::STAR,
        "trophy" => icon::TROPHY,
        "medal" => icon::MEDAL,
        "crown" => icon::CROWN,
        "lightning" => icon::LIGHTNING,
        "fire" => icon::FIRE,
        "sword" => icon::SWORD,
        "shield" => icon::SHIELD,
        "diamond" => icon::DIAMOND,
        "flag" => icon::FLAG,
        "target" => icon::TARGET,
        "skull" => icon::SKULL,
        "ghost" => icon::GHOST,
        "rocket" => icon::ROCKET,
        "heart" => icon::HEART,
        "key" => icon::KEY,
        "clock" => icon::CLOCK,
        "smiley" => icon::SMILEY,
        "flower" => icon::FLOWER,
        "planet" => icon::PLANET,
        "coin" => icon::COIN,
        "footprints" => icon::FOOTPRINTS,
        "hammer" => icon::HAMMER,
        "butterfly" => icon::BUTTERFLY,
        "confetti" => icon::CONFETTI,
        "compass" => icon::COMPASS,
        "horse" => icon::HORSE,
        _ => icon::MEDAL,
    }
}

fn shade(c: Color32, by: f32) -> Color32 {
    let f = |v: u8| {
        let v = v as f32;
        (if by >= 0.0 { v + (255.0 - v) * by } else { v * (1.0 + by) }) as u8
    };
    Color32::from_rgb(f(c.r()), f(c.g()), f(c.b()))
}

pub fn emblem(ui: &mut egui::Ui, badge: &Badge, size: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let c = rect.center();
    let base = crate::groups::colour(&badge.color);
    let hex = |r: f32| -> Vec<egui::Pos2> {
        (0..6)
            .map(|i| {
                let a = std::f32::consts::FRAC_PI_3 * i as f32 - std::f32::consts::FRAC_PI_2;
                pos2(c.x + r * a.cos(), c.y + r * a.sin())
            })
            .collect()
    };
    let r = size * 0.48;
    painter.add(egui::Shape::convex_polygon(hex(r), shade(base, -0.35), Stroke::NONE));
    painter.add(egui::Shape::convex_polygon(hex(r * 0.86), base, Stroke::NONE));
    let top: Vec<egui::Pos2> = hex(r * 0.86).into_iter().map(|p| pos2(p.x, p.y.min(c.y))).collect();
    painter.add(egui::Shape::convex_polygon(top, shade(base, 0.18), Stroke::NONE));
    painter.add(egui::Shape::closed_line(hex(r * 0.72), Stroke::new(size * 0.03, shade(base, 0.45))));
    painter.text(
        c,
        egui::Align2::CENTER_CENTER,
        glyph(&badge.icon),
        FontId::proportional(size * 0.42),
        Color32::WHITE,
    );
    response
}

fn shelf(ui: &mut egui::Ui, badges: &[Badge], about: impl Fn(&Badge) -> String) {
    ui.horizontal_wrapped(|ui| {
        for badge in badges {
            ui.allocate_ui(vec2(84.0, 92.0), |ui| {
                ui.vertical_centered(|ui| {
                    let r = emblem(ui, badge, 52.0);
                    ui.label(RichText::new(&badge.name).small().color(theme::TEXT));
                    r.on_hover_text(format!("{}\n{}", badge.name, about(badge)));
                });
            });
        }
    });
}
