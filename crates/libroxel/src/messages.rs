//! The app's side of messages: the conversations list, the one open, and sending.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy_egui::egui::{self, RichText};
use libroxel_hub::{Conversation, Message, NewMessage, Report};
use libroxel_instance::serde_json::Value as Json;

use crate::browse::Platform;
use crate::theme::{self, icon};

type Slot<T> = Arc<Mutex<Option<Result<T, String>>>>;
type Loaded = (Vec<Conversation>, Option<(String, Vec<Message>)>);

const OPEN_EVERY: Duration = Duration::from_secs(4);
const CLOSED_EVERY: Duration = Duration::from_secs(20);

#[derive(Default)]
pub struct Chats {
    pub list: Vec<Conversation>,
    /// The friend whose messages are open: their id and name.
    pub with: Option<(String, String)>,
    pub talk: Vec<Message>,
    pub draft: String,
    pub error: Option<String>,
    pub status: Option<String>,
    reporting: Option<String>,
    sending: bool,
    busy: bool,
    at: Option<Instant>,
    loaded: Slot<Loaded>,
    sent: Slot<()>,
    reported: Slot<()>,
    clock: i64,
}

impl Chats {
    pub fn unread(&self) -> u32 {
        self.list.iter().map(|c| c.unread).sum()
    }

    fn stamp(&mut self) -> i64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64);
        self.clock = now.max(self.clock + 1);
        self.clock
    }
}

impl Platform {
    /// Keeps messages fresh; call once a frame. `open` is whether the Messages tab shows.
    pub fn chats_tick(&mut self, open: bool) {
        let chats = &mut self.chats;
        if let Some(result) = chats.loaded.lock().unwrap().take() {
            chats.busy = false;
            match result {
                Ok((list, talk)) => {
                    chats.list = list;
                    if let Some((with, talk)) = talk
                        && chats.with.as_ref().is_some_and(|(id, _)| *id == with)
                    {
                        chats.talk = talk;
                    }
                    chats.error = None;
                }
                Err(e) => chats.error = Some(e),
            }
        }
        if let Some(result) = chats.sent.lock().unwrap().take() {
            chats.sending = false;
            match result {
                Ok(()) => chats.draft.clear(),
                Err(e) => chats.error = Some(e),
            }
            chats.at = None;
        }
        if let Some(result) = chats.reported.lock().unwrap().take() {
            chats.status = Some(match result {
                Ok(()) => "Reported.".to_owned(),
                Err(e) => e,
            });
        }
        let every = if open { OPEN_EVERY } else { CLOSED_EVERY };
        if self.account.is_some() && !self.chats.busy && self.chats.at.is_none_or(|at| at.elapsed() > every) {
            self.refresh_chats();
        }
    }

    pub fn refresh_chats(&mut self) {
        let times = (self.chats.stamp(), self.chats.stamp());
        self.chats.busy = true;
        self.chats.at = Some(Instant::now());
        let with = self.chats.with.as_ref().map(|(id, _)| id.clone());
        let client = self.client();
        let out = self.chats.loaded.clone();
        std::thread::spawn(move || {
            let result = crate::identity().and_then(|me| {
                let id = me.player_id();
                let signed = |time| me.sign(&libroxel_hub::read_messages_message(&id, time));
                let list = client.conversations(&id, times.0, &signed(times.0))?;
                let talk = match with {
                    Some(with) => {
                        Some((with.clone(), client.conversation(&id, &with, times.1, &signed(times.1))?))
                    }
                    None => None,
                };
                Ok((list, talk))
            });
            *out.lock().unwrap() = Some(result);
        });
    }

    pub fn open_chat(&mut self, id: &str, name: &str) {
        if self.chats.with.as_ref().is_some_and(|(with, _)| with == id) {
            return;
        }
        self.chats.with = Some((id.to_owned(), name.to_owned()));
        self.chats.talk.clear();
        self.chats.draft.clear();
        self.chats.error = None;
        self.chats.status = None;
        self.chats.reporting = None;
        self.chats.at = None;
    }

    fn send_chat(&mut self) {
        let text = self.chats.draft.trim().to_owned();
        let Some((to, _)) = self.chats.with.clone() else { return };
        if text.is_empty() || self.chats.sending {
            return;
        }
        let time = self.chats.stamp();
        self.chats.sending = true;
        self.chats.error = None;
        let client = self.client();
        let out = self.chats.sent.clone();
        std::thread::spawn(move || {
            let result = crate::identity().and_then(|me| {
                let from_id = me.player_id();
                let signature = me.sign(&libroxel_hub::message_message(&from_id, &to, &text, time));
                client.send_message(&NewMessage { from_id, to_id: to, text, time, signature }).map(|_| ())
            });
            *out.lock().unwrap() = Some(result);
        });
    }

    fn report_chat(&mut self, message_id: String) {
        let time = self.chats.stamp();
        let client = self.client();
        let out = self.chats.reported.clone();
        std::thread::spawn(move || {
            let result = crate::identity().and_then(|me| {
                let reporter_id = me.player_id();
                let reason = "message";
                let signed =
                    libroxel_hub::report_message(&reporter_id, "message", &message_id, reason, "", time);
                client.report(&Report {
                    signature: me.sign(&signed),
                    reporter_id,
                    kind: "message".into(),
                    target: message_id,
                    reason: reason.into(),
                    evidence: String::new(),
                    time,
                })
            });
            *out.lock().unwrap() = Some(result);
        });
    }
}

fn ago(secs: i64) -> String {
    match secs {
        ..60 => "just now".into(),
        60..3600 => format!("{} min ago", secs / 60),
        3600..86400 => format!("{} h ago", secs / 3600),
        _ => format!("{} days ago", secs / 86400),
    }
}

/// The Messages tab: friends down the left, the open conversation on the right.
pub fn messages_tab(ui: &mut egui::Ui, platform: &mut Platform) {
    theme::heading(ui, "Messages");
    let Some(me) = platform.account.as_ref().map(|a| a.player_id.clone()) else {
        ui.label(RichText::new("Make an account on the Avatar tab to message friends.").color(theme::MUTED));
        return;
    };
    if let Some(error) = &platform.chats.error {
        crate::report::status(ui, error);
    }
    if let Some(said) = &platform.chats.status {
        theme::status(ui, theme::GREEN, said);
    }
    ui.add_space(8.0);
    let list = platform.chats.list.clone();
    let friends = platform.social.friends.friends.clone();
    let with = platform.chats.with.clone();
    let mut open = None;
    let mut send = false;
    let mut report = None;
    let now =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    ui.columns(2, |columns| {
        let ui = &mut columns[0];
        let row = |ui: &mut egui::Ui, id: &str, name: &str, look: &Json, line: &str, unread: u32| {
            let on = with.as_ref().is_some_and(|(w, _)| w == id);
            let response = theme::panel()
                .inner_margin(egui::Margin::symmetric(12, 8))
                .stroke(if on { egui::Stroke::new(1.5, theme::ACCENT) } else { egui::Stroke::NONE })
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
                        crate::hub_home::paint_avatar(ui.painter(), rect.center(), 19.0, look, name);
                        ui.add_space(6.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new(name).font(theme::display(16.0)).color(theme::TEXT));
                            ui.add(
                                egui::Label::new(RichText::new(line).small().color(theme::MUTED)).truncate(),
                            );
                        });
                        if unread > 0 {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new(unread.to_string()).strong().color(theme::RED));
                            });
                        }
                    });
                })
                .response
                .interact(egui::Sense::click());
            ui.add_space(4.0);
            response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
        };
        for c in &list {
            let line =
                if c.last.from_id == me { format!("You: {}", c.last.text) } else { c.last.text.clone() };
            if row(ui, &c.with.player_id, &c.with.name, &c.look, &line, c.unread) {
                open = Some((c.with.player_id.clone(), c.with.name.clone()));
            }
        }
        for f in friends.iter().filter(|f| !list.iter().any(|c| c.with.player_id == f.player_id)) {
            let line = if f.online { "Online" } else { "Offline" };
            if row(ui, &f.player_id, &f.name, &f.look, line, 0) {
                open = Some((f.player_id.clone(), f.name.clone()));
            }
        }
        if list.is_empty() && friends.is_empty() {
            ui.label(RichText::new("Add friends to message them.").color(theme::MUTED));
        }

        let ui = &mut columns[1];
        let Some((_, name)) = &with else {
            ui.label(RichText::new("Pick a friend.").color(theme::MUTED));
            return;
        };
        ui.label(RichText::new(name).font(theme::display(18.0)).color(theme::TEXT));
        ui.add_space(4.0);
        theme::panel().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            egui::ScrollArea::vertical()
                .id_salt("chat-log")
                .max_height(360.0)
                .min_scrolled_height(360.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    if platform.chats.talk.is_empty() {
                        ui.label(RichText::new("No messages yet.").color(theme::MUTED));
                    }
                    for m in &platform.chats.talk {
                        let mine = m.from_id == me;
                        let layout = if mine {
                            egui::Layout::top_down(egui::Align::Max)
                        } else {
                            egui::Layout::top_down(egui::Align::Min)
                        };
                        ui.with_layout(layout, |ui| {
                            let fill = if mine { theme::ACCENT } else { theme::WELL };
                            let colour = if mine { egui::Color32::WHITE } else { theme::TEXT };
                            egui::Frame::new()
                                .fill(fill)
                                .corner_radius(10.0)
                                .inner_margin(egui::Margin::symmetric(10, 6))
                                .show(ui, |ui| {
                                    ui.set_max_width(ui.available_width() * 0.75);
                                    ui.add(egui::Label::new(RichText::new(&m.text).color(colour)).wrap());
                                });
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(ago(now - m.sent)).small().color(theme::MUTED));
                                if !mine {
                                    if platform.chats.reporting.as_deref() == Some(m.message_id.as_str()) {
                                        if ui.small_button("Report").clicked() {
                                            report = Some(m.message_id.clone());
                                        }
                                    } else if ui.small_button(icon::FLAG).on_hover_text("Report").clicked() {
                                        platform.chats.reporting = Some(m.message_id.clone());
                                    }
                                }
                            });
                        });
                        ui.add_space(4.0);
                    }
                });
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let field = ui.add(
                egui::TextEdit::singleline(&mut platform.chats.draft)
                    .char_limit(libroxel_hub::MAX_MESSAGE)
                    .hint_text("Message")
                    .desired_width(ui.available_width() - 90.0)
                    .margin(egui::vec2(8.0, 6.0)),
            );
            if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                send = true;
                field.request_focus();
            }
            let sending = platform.chats.sending;
            if ui
                .add_enabled_ui(!sending, |ui| {
                    theme::block_button(ui, theme::Kind::Go, icon::PAPER_PLANE_RIGHT, "Send", 30.0)
                })
                .inner
                .clicked()
            {
                send = true;
            }
        });
    });
    if let Some((id, name)) = open {
        platform.open_chat(&id, &name);
    }
    if send {
        platform.send_chat();
    }
    if let Some(id) = report {
        platform.chats.reporting = None;
        platform.report_chat(id);
    }
}
