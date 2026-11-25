use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy_egui::egui::{self, Key, RichText};
use libroxel_engine::PlaySession;
use libroxel_engine::chat::MAX_CHAT;
use libroxel_hub::{Chat, ChatRule, HubClient, MAX_EVIDENCE, strict_filter};
use libroxel_instance::InstanceId;

use crate::i18n::t;
use crate::theme::{self, icon};

const KEPT: usize = 100;
const FADE: Duration = Duration::from_secs(20);
const ASK_EVERY: Duration = Duration::from_secs(120);
const SHOWN_OPEN: usize = 12;
const SHOWN_CLOSED: usize = 6;

struct Line {
    from: InstanceId,
    name: String,
    text: String,
    at: Instant,
}

#[derive(Default)]
pub struct TextChat {
    lines: VecDeque<Line>,
    me: Option<InstanceId>,
    draft: String,
    open: bool,
    focus: bool,
    muted: HashSet<String>,
    rule: Arc<Mutex<Option<(ChatRule, Instant)>>>,
    asked: Option<Instant>,
}

impl TextChat {
    fn rule(&self) -> Option<(ChatRule, Instant)> {
        *self.rule.lock().unwrap()
    }

    fn level(&self) -> Option<Chat> {
        self.rule().map(|(r, _)| r.chat)
    }

    fn muted_by_hub(&self) -> bool {
        self.rule().is_some_and(|(r, at)| r.muted_secs.is_some_and(|s| at.elapsed().as_secs() < s as u64))
    }

    fn push(&mut self, from: InstanceId, name: String, text: String) {
        self.lines.push_back(Line { from, name, text, at: Instant::now() });
        while self.lines.len() > KEPT {
            self.lines.pop_front();
        }
    }

    pub fn tick(&mut self, session: Option<&mut PlaySession>, client: impl FnOnce() -> HubClient) {
        let Some(session) = session.filter(|s| s.online()) else {
            if self.asked.is_some() {
                let muted = std::mem::take(&mut self.muted);
                *self = Self { muted, ..Self::default() };
            }
            return;
        };
        if self.asked.is_none_or(|at| at.elapsed() > ASK_EVERY) {
            self.asked = Some(Instant::now());
            let (out, client) = (self.rule.clone(), client());
            std::thread::spawn(move || {
                let rule = crate::identity()
                    .and_then(|me| client.chat_rule(&me.player_id()))
                    .unwrap_or(ChatRule { chat: Chat::Off, muted_secs: None });
                *out.lock().unwrap() = Some((rule, Instant::now()));
            });
        }
        self.me = session.local_player();
        let heard = session.take_chat();
        if heard.is_empty() {
            return;
        }
        let tree = session.runtime().tree().clone();
        let tree = tree.borrow();
        for (from, text) in heard {
            let name = tree.name(from).unwrap_or("?").to_owned();
            self.push(from, name, text);
        }
    }

    pub fn evidence(&self, game: &str, reported: &str) -> String {
        let head = format!("In {game}");
        let mut lines: Vec<String> = Vec::new();
        let mut used = head.chars().count();
        for l in self.lines.iter().rev() {
            let mark = if l.name == reported { "> " } else { "" };
            let line = format!("{mark}{}: {}", l.name, l.text);
            used += line.chars().count() + 1;
            if used > MAX_EVIDENCE {
                break;
            }
            lines.push(line);
        }
        lines.push(head);
        lines.reverse();
        lines.join("\n")
    }

    pub fn ui(&mut self, ui: &egui::Ui, session: Option<&mut PlaySession>) -> Option<String> {
        let session = session.filter(|s| s.online())?;
        let level = self.level().filter(|l| *l != Chat::Off)?;
        let muted_by_hub = self.muted_by_hub();
        if !self.open && ui.input(|i| i.key_pressed(Key::Slash)) && !ui.ctx().egui_wants_keyboard_input() {
            self.open = true;
            self.focus = true;
        }
        let shown = |text: &str| if level == Chat::Strict { strict_filter(text) } else { text.to_owned() };
        let now = Instant::now();
        let mut report = None;
        let mut toggle = None;
        let rect = ui.max_rect();
        egui::Area::new("text-chat".into())
            .fixed_pos(rect.left_bottom() + egui::vec2(10.0, -60.0))
            .pivot(egui::Align2::LEFT_BOTTOM)
            .show(ui.ctx(), |ui| {
                ui.set_max_width(340.0);
                let recent: Vec<&Line> = self
                    .lines
                    .iter()
                    .rev()
                    .filter(|l| !self.muted.contains(&l.name))
                    .filter(|l| self.open || now.duration_since(l.at) < FADE)
                    .take(if self.open { SHOWN_OPEN } else { SHOWN_CLOSED })
                    .collect();
                if !recent.is_empty() {
                    egui::Frame::popup(ui.style()).fill(theme::see_through(20, 24, 30, 170)).show(ui, |ui| {
                        for l in recent.into_iter().rev() {
                            ui.horizontal_wrapped(|ui| {
                                let name = ui.label(
                                    RichText::new(format!("{}:", l.name)).strong().color(theme::AMBER),
                                );
                                if Some(l.from) != self.me {
                                    name.context_menu(|ui| {
                                        if ui
                                            .button(format!("{} {}", icon::SPEAKER_SLASH, t("Mute")))
                                            .clicked()
                                        {
                                            toggle = Some(l.name.clone());
                                            ui.close();
                                        }
                                        if ui.button(format!("{} {}", icon::FLAG, t("Report"))).clicked() {
                                            report = Some(l.name.clone());
                                            ui.close();
                                        }
                                    });
                                }
                                ui.label(RichText::new(shown(&l.text)).color(egui::Color32::WHITE));
                            });
                        }
                    });
                }
                if !self.muted.is_empty() && self.open {
                    ui.horizontal_wrapped(|ui| {
                        for name in &self.muted {
                            if ui
                                .small_button(format!("{} {name}", icon::SPEAKER_SLASH))
                                .on_hover_text(t("Unmute"))
                                .clicked()
                            {
                                toggle = Some(name.clone());
                            }
                        }
                    });
                }
                ui.horizontal(|ui| {
                    if theme::block_button(ui, theme::Kind::Plain, icon::CHAT_CIRCLE, "", 30.0).clicked() {
                        self.open = !self.open;
                        self.focus = self.open;
                    }
                    if !self.open {
                        return;
                    }
                    if self.draft == "/" {
                        self.draft.clear();
                    }
                    let hint = if muted_by_hub { t("You are muted") } else { t("Chat") };
                    let field = egui::TextEdit::singleline(&mut self.draft)
                        .char_limit(MAX_CHAT)
                        .hint_text(hint)
                        .desired_width(280.0);
                    let response = ui.add_enabled(!muted_by_hub, field);
                    if std::mem::take(&mut self.focus) {
                        response.request_focus();
                    }
                    if response.lost_focus() {
                        if ui.input(|i| i.key_pressed(Key::Enter)) {
                            let typed = std::mem::take(&mut self.draft);
                            let typed = if level == Chat::Strict { strict_filter(&typed) } else { typed };
                            if let (Some(said), Some(me)) = (session.say(&typed), self.me) {
                                let name =
                                    session.runtime().tree().borrow().name(me).unwrap_or("?").to_owned();
                                self.lines.push_back(Line { from: me, name, text: said, at: now });
                            }
                        }
                        self.open = false;
                    }
                });
            });
        if let Some(name) = toggle
            && !self.muted.remove(&name)
        {
            self.muted.insert(name);
        }
        if let Some(l) = self.lines.back() {
            let left = FADE.saturating_sub(now.duration_since(l.at));
            if !left.is_zero() {
                ui.ctx().request_repaint_after(left);
            }
        }
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_keeps_the_latest_lines_that_fit() {
        let mut chat = TextChat::default();
        let id = InstanceId::from_bits(1);
        for i in 0..200 {
            chat.push(
                id,
                if i % 2 == 0 { "Ana" } else { "Bo" }.into(),
                format!("line {i} {}", "x".repeat(20)),
            );
        }
        let evidence = chat.evidence("Obby", "Bo");
        assert!(evidence.chars().count() <= MAX_EVIDENCE);
        assert!(evidence.starts_with("In Obby\n"));
        assert!(evidence.ends_with(&format!("> Bo: line 199 {}", "x".repeat(20))));
        assert!(evidence.contains("\nAna: line 198"));
        assert!(!evidence.contains("line 0 "));
    }
}
