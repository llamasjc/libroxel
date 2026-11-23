//! Likes, favorites and comments on a game's page.

use std::sync::{Arc, Mutex};

use bevy_egui::egui::{self, RichText};
use libroxel_hub::{Comment, NewComment, Rate, Ratings, RemoveComment, Report, Vote};

use crate::browse::Platform;
use crate::theme::{self, icon};

type Slot<T> = Arc<Mutex<Option<Result<T, String>>>>;

#[derive(Default)]
pub struct GameRatings {
    key: Option<String>,
    who: Option<String>,
    ratings: Option<Ratings>,
    comments: Option<Vec<Comment>>,
    loaded: Slot<(Ratings, Vec<Comment>)>,
    rated: Slot<Ratings>,
    commented: Slot<Comment>,
    removed: Slot<i64>,
    reported: Slot<()>,
    text: String,
    busy: bool,
    status: Option<Result<String, String>>,
    clock: i64,
}

fn put<T: Send + 'static>(slot: &Slot<T>, work: impl FnOnce() -> Result<T, String> + Send + 'static) {
    let slot = slot.clone();
    std::thread::spawn(move || *slot.lock().unwrap() = Some(work()));
}

fn take<T>(slot: &Slot<T>) -> Option<Result<T, String>> {
    slot.lock().unwrap().take()
}

impl GameRatings {
    fn stamp(&mut self) -> i64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64);
        self.clock = now.max(self.clock + 1);
        self.clock
    }

    fn update(&mut self, platform: &Platform, key: &str) {
        let who = platform.account.as_ref().map(|a| a.player_id.clone());
        if self.key.as_deref() != Some(key) || self.who != who {
            *self = GameRatings { clock: self.clock, ..GameRatings::default() };
            self.key = Some(key.to_owned());
            self.who = who.clone();
            let (client, key) = (platform.client(), key.to_owned());
            put(&self.loaded, move || {
                let ratings = client.ratings(&key, who.as_deref()).unwrap_or_default();
                Ok((ratings, client.comments(&key)?))
            });
        }
        if let Some(Ok((ratings, comments))) = take(&self.loaded) {
            self.ratings = Some(ratings);
            self.comments = Some(comments);
        }
        match take(&self.rated) {
            Some(Ok(ratings)) => self.ratings = Some(ratings),
            Some(Err(e)) => self.status = Some(Err(e)),
            None => {}
        }
        match take(&self.commented) {
            Some(Ok(comment)) => {
                self.busy = false;
                self.text.clear();
                self.comments.get_or_insert_default().insert(0, comment);
            }
            Some(Err(e)) => {
                self.busy = false;
                self.status = Some(Err(e));
            }
            None => {}
        }
        match take(&self.removed) {
            Some(Ok(id)) => {
                if let Some(list) = &mut self.comments {
                    list.retain(|c| c.comment_id != id);
                }
            }
            Some(Err(e)) => self.status = Some(Err(e)),
            None => {}
        }
        match take(&self.reported) {
            Some(Ok(())) => self.status = Some(Ok("Reported. Thanks.".into())),
            Some(Err(e)) => self.status = Some(Err(e)),
            None => {}
        }
    }

    fn rate(&mut self, platform: &Platform, vote: Vote, favorite: bool) {
        let time = self.stamp();
        let (client, key) = (platform.client(), self.key.clone().unwrap_or_default());
        put(&self.rated, move || {
            let me = crate::identity()?;
            let player_id = me.player_id();
            let signature = me.sign(&libroxel_hub::rate_message(&player_id, &key, vote, favorite, time));
            client.rate(&key, &Rate { player_id, vote, favorite, time, signature })
        });
    }

    fn post(&mut self, platform: &Platform) {
        if self.text.trim().is_empty() || self.busy {
            return;
        }
        self.busy = true;
        self.status = None;
        let time = self.stamp();
        let (client, key, text) =
            (platform.client(), self.key.clone().unwrap_or_default(), self.text.clone());
        put(&self.commented, move || {
            let me = crate::identity()?;
            let player_id = me.player_id();
            let signature = me.sign(&libroxel_hub::comment_message(&player_id, &key, &text, time));
            client.comment(&key, &NewComment { player_id, text, time, signature })
        });
    }

    fn remove(&mut self, platform: &Platform, comment_id: i64) {
        let time = self.stamp();
        let client = platform.client();
        put(&self.removed, move || {
            let me = crate::identity()?;
            let player_id = me.player_id();
            let signature = me.sign(&libroxel_hub::remove_comment_message(&player_id, comment_id, time));
            client
                .remove_comment(comment_id, &RemoveComment { player_id, time, signature })
                .map(|_| comment_id)
        });
    }

    fn report(&mut self, platform: &Platform, comment_id: i64) {
        let client = platform.client();
        put(&self.reported, move || {
            let me = crate::identity()?;
            let reporter_id = me.player_id();
            let time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as i64);
            let (target, reason) = (comment_id.to_string(), "Not OK".to_owned());
            let message = libroxel_hub::report_message(&reporter_id, "comment", &target, &reason, time);
            client.report(&Report {
                signature: me.sign(&message),
                reporter_id,
                kind: "comment".into(),
                target,
                reason,
                time,
            })
        });
    }

    /// Like, dislike and favorite buttons.
    pub fn bar(&mut self, ui: &mut egui::Ui, platform: &Platform, key: &str) {
        self.update(platform, key);
        let r = self.ratings.clone().unwrap_or_default();
        let signed_in = platform.account.is_some();
        ui.horizontal(|ui| {
            ui.add_enabled_ui(signed_in, |ui| {
                let hint = if signed_in { "" } else { "Sign in to rate games" };
                let like =
                    theme::tool_button(ui, icon::THUMBS_UP, &r.likes.to_string(), r.vote == Vote::Like);
                if like.on_disabled_hover_text(hint).clicked() {
                    let vote = if r.vote == Vote::Like { Vote::None } else { Vote::Like };
                    self.rate(platform, vote, r.favorite);
                }
                let dislike = theme::tool_button(
                    ui,
                    icon::THUMBS_DOWN,
                    &r.dislikes.to_string(),
                    r.vote == Vote::Dislike,
                );
                if dislike.on_disabled_hover_text(hint).clicked() {
                    let vote = if r.vote == Vote::Dislike { Vote::None } else { Vote::Dislike };
                    self.rate(platform, vote, r.favorite);
                }
                let total = r.likes + r.dislikes;
                if total > 0 {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(120.0, 6.0), egui::Sense::hover());
                    let painter = ui.painter_at(rect);
                    painter.rect_filled(rect, 3.0, theme::RED);
                    let mut liked = rect;
                    liked.set_width(rect.width() * r.likes as f32 / total as f32);
                    painter.rect_filled(liked, 3.0, theme::GREEN);
                }
                let label = format!("{} {}", if r.favorite { "Favorited" } else { "Favorite" }, r.favorites);
                if theme::tool_button(ui, icon::STAR, &label, r.favorite)
                    .on_disabled_hover_text(hint)
                    .clicked()
                {
                    self.rate(platform, r.vote, !r.favorite);
                }
            });
        });
    }

    /// The comment box and the comments.
    pub fn comments(&mut self, ui: &mut egui::Ui, platform: &Platform, key: &str, author_id: Option<&str>) {
        self.update(platform, key);
        let me = platform.account.as_ref().map(|a| a.player_id.clone());
        let count = self.comments.as_ref().map_or(String::new(), |c| format!(" ({})", c.len()));
        ui.label(RichText::new(format!("Comments{count}")).font(theme::display(22.0)).color(theme::TEXT));
        ui.add_space(8.0);
        if me.is_some() {
            ui.horizontal(|ui| {
                let box_ = ui.add(
                    egui::TextEdit::singleline(&mut self.text)
                        .hint_text("Say something about this game")
                        .char_limit(libroxel_hub::MAX_COMMENT)
                        .desired_width(520.0),
                );
                let enter = box_.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.add_enabled(!self.busy, egui::Button::new("Post")).clicked() || enter {
                    self.post(platform);
                }
            });
        } else {
            ui.label(RichText::new("Sign in to comment.").color(theme::MUTED));
        }
        match &self.status {
            Some(Ok(text)) => {
                ui.colored_label(theme::GREEN, text);
            }
            Some(Err(text)) => {
                ui.colored_label(theme::RED, text);
            }
            None => {}
        }
        ui.add_space(6.0);
        let Some(list) = self.comments.clone() else {
            ui.label(RichText::new("Loading…").color(theme::MUTED));
            return;
        };
        if list.is_empty() {
            ui.label(RichText::new("No comments yet.").color(theme::MUTED));
        }
        for c in &list {
            ui.horizontal_top(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::hover());
                crate::hub_home::paint_avatar(
                    ui.painter(),
                    rect.center(),
                    18.0,
                    &libroxel_instance::serde_json::Value::Null,
                    &c.name,
                );
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&c.name).strong().color(theme::TEXT));
                        ui.label(RichText::new(crate::hub_home::ago(c.secs_ago)).color(theme::MUTED));
                        let mine = me.as_deref() == Some(c.player_id.as_str());
                        if (mine || (me.is_some() && me.as_deref() == author_id))
                            && ui.small_button(icon::TRASH).on_hover_text("Delete").clicked()
                        {
                            self.remove(platform, c.comment_id);
                        }
                        if me.is_some()
                            && !mine
                            && ui.small_button(icon::FLAG).on_hover_text("Report").clicked()
                        {
                            self.report(platform, c.comment_id);
                        }
                    });
                    ui.label(RichText::new(&c.text).color(theme::TEXT));
                });
            });
            ui.add_space(6.0);
        }
    }
}
