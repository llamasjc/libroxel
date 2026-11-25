use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use bevy_egui::egui::{self, RichText};
use playlibre_hub::{Change, GameInfo, MOST_PLAYERS, ManageGame, Version, creator_message, manage_message};

use crate::browse::Platform;
use crate::i18n::{t, tf};
use crate::menu::{Game, paint_blocks};
use crate::theme::{self, icon};

type Slot<T> = Arc<Mutex<Option<Result<T, String>>>>;

#[derive(Clone, PartialEq)]
struct Edit {
    title: String,
    description: String,
    private: bool,
    max_players: u32,
}

#[derive(Clone, Copy, PartialEq)]
enum Pane {
    Settings,
    Versions,
}

#[derive(Default)]
pub struct MyGames {
    list: Option<Vec<GameInfo>>,
    list_slot: Slot<Vec<GameInfo>>,
    loading: bool,
    open: Option<(String, Pane)>,
    edit: Option<Edit>,
    versions: Option<Vec<Version>>,
    versions_slot: Slot<Vec<Version>>,
    versions_loading: bool,
    done: Slot<String>,
    busy: bool,
    sure: bool,
    status: Option<Result<String, String>>,
    files: HashMap<String, PathBuf>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

impl MyGames {
    fn take(&mut self) {
        if let Some(result) = self.list_slot.lock().unwrap().take() {
            self.loading = false;
            match result {
                Ok(list) => self.list = Some(list),
                Err(e) => self.status = Some(Err(e)),
            }
        }
        if let Some(result) = self.versions_slot.lock().unwrap().take() {
            self.versions_loading = false;
            match result {
                Ok(list) => self.versions = Some(list),
                Err(e) => self.status = Some(Err(e)),
            }
        }
        if let Some(result) = self.done.lock().unwrap().take() {
            self.busy = false;
            self.sure = false;
            if result.is_ok() {
                self.list = None;
                self.versions = None;
                self.edit = None;
            }
            self.status = Some(result);
        }
    }

    fn load(&mut self, platform: &Platform) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.files = crate::browse::published_files(platform.client().url());
        let client = platform.client();
        let out = self.list_slot.clone();
        std::thread::spawn(move || {
            let result = crate::identity().and_then(|me| {
                let id = me.player_id();
                let time = now_ms();
                client.my_games(&id, time, &me.sign(&creator_message(&id, time)))
            });
            *out.lock().unwrap() = Some(result);
        });
    }

    fn load_versions(&mut self, platform: &Platform, game_id: String) {
        if self.versions_loading {
            return;
        }
        self.versions_loading = true;
        let client = platform.client();
        let out = self.versions_slot.clone();
        std::thread::spawn(move || {
            let result = crate::identity().and_then(|me| {
                let id = me.player_id();
                let time = now_ms();
                client.versions(&game_id, &id, time, &me.sign(&creator_message(&id, time)))
            });
            *out.lock().unwrap() = Some(result);
        });
    }

    fn change(&mut self, platform: &Platform, game_id: String, change: Change, said: String) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.status = None;
        let client = platform.client();
        let out = self.done.clone();
        std::thread::spawn(move || {
            let result = crate::identity().and_then(|me| {
                let player_id = me.player_id();
                let time = now_ms();
                let signature = me.sign(&manage_message(&player_id, &game_id, time, &change));
                client.manage(&game_id, &ManageGame { player_id, time, signature, change })
            });
            *out.lock().unwrap() = Some(result.map(|()| said));
        });
    }
}

pub fn my_games(state: &mut MyGames, ui: &mut egui::Ui, platform: &Platform) -> Option<Game> {
    platform.account.as_ref()?;
    state.take();
    if state.list.is_none() && !state.loading {
        state.load(platform);
    }
    if state.loading || state.busy {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(150));
    }
    let mut pick = None;
    ui.add_space(20.0);
    theme::heading(ui, t("Your games"));
    if let Some(status) = &state.status {
        match status {
            Ok(said) => theme::status(ui, theme::GREEN, said),
            Err(why) => theme::status(ui, theme::RED, why),
        };
    }
    let Some(list) = state.list.clone() else {
        ui.label(RichText::new(t("Loading…")).color(theme::MUTED));
        return None;
    };
    if list.is_empty() {
        ui.label(RichText::new(t("No published games yet.")).color(theme::MUTED));
        return None;
    }
    for game in &list {
        if let Some(chosen) = row(state, ui, platform, game) {
            pick = Some(chosen);
        }
    }
    pick
}

fn row(state: &mut MyGames, ui: &mut egui::Ui, platform: &Platform, game: &GameInfo) -> Option<Game> {
    let mut pick = None;
    let open = state.open.as_ref().filter(|(id, _)| *id == game.game_id).map(|(_, pane)| *pane);
    theme::panel().show(ui, |ui| {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(56.0, 56.0), egui::Sense::hover());
            paint_blocks(ui.painter(), rect, &game.game_id);
            ui.vertical(|ui| {
                ui.label(RichText::new(&game.title).font(theme::display(17.0)).color(theme::HEADING));
                let mut bits = vec![
                    t(if game.private { "Private" } else { "Public" }).to_owned(),
                    tf("Version {n}", &[("n", &game.version)]),
                    crate::browse::ago(game.updated_secs_ago),
                ];
                if game.playing > 0 {
                    bits.push(tf("{n} playing", &[("n", &game.playing)]));
                }
                ui.label(RichText::new(bits.join(" · ")).color(theme::MUTED));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let toggle = |ui: &mut egui::Ui, glyph, label, pane| {
                    theme::tool_button(ui, glyph, label, open == Some(pane)).clicked()
                };
                if toggle(ui, icon::CLOCK_COUNTER_CLOCKWISE, t("Versions"), Pane::Versions) {
                    state.sure = false;
                    if open == Some(Pane::Versions) {
                        state.open = None;
                    } else {
                        state.open = Some((game.game_id.clone(), Pane::Versions));
                        state.versions = None;
                    }
                }
                if toggle(ui, icon::GEAR, t("Settings"), Pane::Settings) {
                    state.sure = false;
                    state.edit = None;
                    state.open = if open == Some(Pane::Settings) {
                        None
                    } else {
                        Some((game.game_id.clone(), Pane::Settings))
                    };
                }
                if theme::tool_button(ui, icon::CHART_LINE, t("Numbers"), false).clicked() {
                    crate::report::open_url(&format!("https://playlibre.net/creator/{}", game.game_id));
                }
                if let Some(path) = state.files.get(&game.game_id)
                    && theme::tool_button(ui, icon::PENCIL_SIMPLE, t("Edit"), false).clicked()
                {
                    pick = Some(Game::File(path.clone()));
                }
            });
        });
        match open {
            Some(Pane::Settings) => settings(state, ui, platform, game),
            Some(Pane::Versions) => versions(state, ui, platform, game),
            None => {}
        }
    });
    ui.add_space(8.0);
    pick
}

fn settings(state: &mut MyGames, ui: &mut egui::Ui, platform: &Platform, game: &GameInfo) {
    let edit = state.edit.get_or_insert_with(|| Edit {
        title: game.title.clone(),
        description: game.description.clone(),
        private: game.private,
        max_players: game.max_players,
    });
    ui.add_space(8.0);
    egui::Grid::new(("settings", &game.game_id)).num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
        ui.label(t("Name"));
        ui.add(egui::TextEdit::singleline(&mut edit.title).char_limit(120).desired_width(320.0));
        ui.end_row();
        ui.label(t("Description"));
        ui.add(
            egui::TextEdit::multiline(&mut edit.description)
                .char_limit(2000)
                .desired_rows(3)
                .desired_width(320.0),
        );
        ui.end_row();
        ui.label(t("Who can play"));
        ui.horizontal(|ui| {
            ui.selectable_value(&mut edit.private, false, t("Public"));
            ui.selectable_value(&mut edit.private, true, t("Private"));
        });
        ui.end_row();
        ui.label(t("Max players"));
        ui.add(
            egui::DragValue::new(&mut edit.max_players)
                .range(0..=MOST_PLAYERS)
                .custom_formatter(|n, _| if n == 0.0 { t("Any").to_owned() } else { format!("{n}") }),
        );
        ui.end_row();
    });
    let edit = edit.clone();
    ui.horizontal(|ui| {
        let ready = !state.busy && !edit.title.trim().is_empty();
        if ui.add_enabled(ready, egui::Button::new(format!("{} {}", icon::CHECK, t("Save")))).clicked() {
            let change = Change::Settings {
                title: edit.title.trim().to_owned(),
                description: edit.description.trim().to_owned(),
                private: edit.private,
                max_players: edit.max_players,
            };
            state.change(platform, game.game_id.clone(), change, t("Saved.").to_owned());
        }
        let label = if state.sure { t("Remove?") } else { t("Remove") };
        if ui.add_enabled(!state.busy, egui::Button::new(format!("{} {label}", icon::TRASH))).clicked() {
            if state.sure {
                state.open = None;
                state.change(
                    platform,
                    game.game_id.clone(),
                    Change::Remove,
                    tf("Removed {title}.", &[("title", &game.title)]),
                );
            } else {
                state.sure = true;
            }
        }
    });
}

fn versions(state: &mut MyGames, ui: &mut egui::Ui, platform: &Platform, game: &GameInfo) {
    ui.add_space(8.0);
    let Some(list) = state.versions.clone() else {
        state.load_versions(platform, game.game_id.clone());
        ui.label(RichText::new(t("Loading…")).color(theme::MUTED));
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(150));
        return;
    };
    egui::Grid::new(("versions", &game.game_id)).num_columns(3).striped(true).spacing([16.0, 6.0]).show(
        ui,
        |ui| {
            for v in &list {
                ui.label(RichText::new(tf("Version {n}", &[("n", &v.version)])).strong());
                ui.label(RichText::new(crate::browse::ago(v.published_secs_ago)).color(theme::MUTED));
                if v.version == game.version {
                    ui.label(RichText::new(t("Live")).color(theme::GREEN));
                } else if ui
                    .add_enabled(
                        !state.busy,
                        egui::Button::new(format!("{} {}", icon::ARROW_COUNTER_CLOCKWISE, t("Restore"))),
                    )
                    .clicked()
                {
                    state.change(
                        platform,
                        game.game_id.clone(),
                        Change::Restore { version: v.version },
                        tf("Version {n} is live again.", &[("n", &v.version)]),
                    );
                }
                ui.end_row();
            }
        },
    );
}
