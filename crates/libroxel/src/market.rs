use std::sync::{Arc, Mutex};

use bevy_egui::egui::{self, RichText};
use libroxel_hub::{Asset, AssetData, AssetKind, HubClient, NewAsset, Report, Unmarket};
use libroxel_instance::InstanceId;

use crate::plugins::{self, Plugin, permission_words};
use crate::theme;

type Slot<T> = Arc<Mutex<Option<Result<T, String>>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Models,
    Plugins,
    Installed,
    Mine,
}

#[derive(Clone)]
pub struct PublishForm {
    pub kind: AssetKind,
    pub target: InstanceId,
    pub asset_id: Option<String>,
    pub name: String,
    pub about: String,
    pub permissions: [bool; 3],
}

impl PublishForm {
    pub fn permissions(&self) -> Vec<String> {
        libroxel_hub::PERMISSIONS
            .iter()
            .zip(self.permissions)
            .filter(|(_, on)| *on)
            .map(|(p, _)| (*p).to_owned())
            .collect()
    }
}

pub enum Do {
    Insert(String, String),
    Run(Plugin),
    Publish(PublishForm),
}

#[derive(Default)]
pub struct Market {
    pub open: bool,
    tab: Tab,
    search: String,
    searched: Option<(String, Tab)>,
    list: Vec<Asset>,
    found: Slot<Vec<Asset>>,
    got: Slot<(Asset, AssetData)>,
    approving: Option<(Asset, AssetData)>,
    plugins: Option<Vec<Plugin>>,
    said: Slot<String>,
    status: Option<Result<String, String>>,
    busy: bool,
    pub form: Option<PublishForm>,
}

fn take<T>(slot: &Slot<T>) -> Option<Result<T, String>> {
    slot.lock().unwrap().take()
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

impl Market {
    fn plugins(&mut self) -> &mut Vec<Plugin> {
        self.plugins.get_or_insert_with(plugins::load)
    }

    fn refresh(&mut self, client: HubClient) {
        let (search, tab) = (self.search.trim().to_owned(), self.tab);
        self.searched = Some((search.clone(), tab));
        let out = self.found.clone();
        std::thread::spawn(move || {
            let result = match tab {
                Tab::Models => client.market(&search, Some(AssetKind::Model), None),
                Tab::Plugins => client.market(&search, Some(AssetKind::Plugin), None),
                Tab::Mine => {
                    crate::identity().and_then(|me| client.market(&search, None, Some(&me.player_id())))
                }
                Tab::Installed => Ok(Vec::new()),
            };
            *out.lock().unwrap() = Some(result);
        });
    }

    fn fetch(&mut self, client: HubClient, asset: Asset) {
        self.busy = true;
        let out = self.got.clone();
        std::thread::spawn(move || {
            let result = client.market_data(&asset.asset_id).map(|data| (asset, data));
            *out.lock().unwrap() = Some(result);
        });
    }

    fn report(&mut self, client: HubClient, asset: &Asset) {
        let target = asset.asset_id.clone();
        let out = self.said.clone();
        std::thread::spawn(move || {
            let result = crate::identity().and_then(|me| {
                let reporter_id = me.player_id();
                let (time, reason) = (now(), "Not OK".to_owned());
                let message =
                    libroxel_hub::report_message(&reporter_id, "market", &target, &reason, "", time);
                client.report(&Report {
                    signature: me.sign(&message),
                    reporter_id,
                    kind: "market".into(),
                    target,
                    reason,
                    evidence: String::new(),
                    time,
                })?;
                Ok("Reported. Thanks.".to_owned())
            });
            *out.lock().unwrap() = Some(result);
        });
    }

    fn take_down(&mut self, client: HubClient, asset: &Asset) {
        let id = asset.asset_id.clone();
        let out = self.said.clone();
        std::thread::spawn(move || {
            let result = crate::identity().and_then(|me| {
                let time = now();
                client.unmarket(
                    &id,
                    &Unmarket { time, signature: me.sign(&libroxel_hub::unmarket_message(&id, time)) },
                )?;
                Ok("Taken down.".to_owned())
            });
            *out.lock().unwrap() = Some(result);
        });
        self.searched = None;
    }

    pub fn publish(&mut self, client: HubClient, form: &PublishForm, data: String) {
        self.busy = true;
        self.status = None;
        let (asset_id, kind, name, about, permissions) = (
            form.asset_id.clone(),
            form.kind,
            form.name.trim().to_owned(),
            form.about.trim().to_owned(),
            form.permissions(),
        );
        let out = self.said.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let data = match kind {
                    AssetKind::Model => crate::hub_assets::upload(&client, &data)?,
                    AssetKind::Plugin => data,
                };
                let me = crate::identity()?;
                let creator_id = me.player_id();
                let time = now();
                let message = libroxel_hub::asset_message(
                    &creator_id,
                    asset_id.as_deref(),
                    kind,
                    &name,
                    &about,
                    &data,
                    &permissions,
                    0,
                    time,
                );
                let done = client.publish_asset(&NewAsset {
                    asset_id,
                    creator_id,
                    kind,
                    name,
                    about,
                    data,
                    permissions,
                    price: 0,
                    time,
                    signature: me.sign(&message),
                })?;
                Ok(if done.version == 1 {
                    format!("{} is in the marketplace.", done.name)
                } else {
                    format!("Version {} of {} is out.", done.version, done.name)
                })
            })();
            *out.lock().unwrap() = Some(result);
        });
        self.searched = None;
    }

    fn catch_up(&mut self, hub: &str) -> Option<Do> {
        if let Some(found) = take(&self.found) {
            match found {
                Ok(list) => self.list = list,
                Err(e) => self.status = Some(Err(e)),
            }
        }
        if let Some(said) = take(&self.said) {
            self.busy = false;
            if said.is_ok() {
                self.form = None;
            }
            self.status = Some(said);
        }
        let (asset, data) = match take(&self.got)? {
            Ok(got) => got,
            Err(e) => {
                self.busy = false;
                self.status = Some(Err(e));
                return None;
            }
        };
        self.busy = false;
        match data.kind {
            AssetKind::Model => {
                self.status = Some(Ok(format!("Added {}.", asset.name)));
                Some(Do::Insert(asset.name, data.data))
            }
            AssetKind::Plugin => {
                let had =
                    self.plugins().iter().find(|p| p.asset_id == asset.asset_id && p.hub == hub).cloned();
                let more =
                    had.is_none_or(|had| data.permissions.iter().any(|p| !had.permissions.contains(p)));
                if more {
                    self.approving = Some((asset, data));
                } else {
                    self.install(hub, &asset, data);
                }
                None
            }
        }
    }

    fn install(&mut self, hub: &str, asset: &Asset, data: AssetData) {
        let plugin = Plugin {
            asset_id: asset.asset_id.clone(),
            hub: hub.to_owned(),
            name: asset.name.clone(),
            creator: asset.creator.clone(),
            version: data.version,
            permissions: data.permissions,
            source: data.data,
        };
        let list = self.plugins();
        list.retain(|p| !(p.asset_id == plugin.asset_id && p.hub == plugin.hub));
        list.push(plugin);
        let saved = plugins::save(list);
        self.status = Some(saved.map(|_| format!("Installed {}. Find it under Installed.", asset.name)));
    }

    pub fn active(&self) -> bool {
        self.open || self.busy
    }

    pub fn fail(&mut self, why: String) {
        self.status = Some(Err(why));
    }

    pub fn window(
        &mut self,
        ctx: &egui::Context,
        client: HubClient,
        selection: Option<InstanceId>,
    ) -> Option<Do> {
        let hub = client.url().to_owned();
        let mut todo = self.catch_up(&hub);
        if !self.open {
            return todo;
        }
        if self.tab != Tab::Installed
            && self.searched.as_ref().is_none_or(|(s, t)| *t != self.tab || *s != self.search.trim())
        {
            self.refresh(client.clone());
        }
        let mut open = true;
        egui::Window::new("Marketplace").open(&mut open).default_size([460.0, 520.0]).show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (tab, name) in [
                    (Tab::Models, "Models"),
                    (Tab::Plugins, "Plugins"),
                    (Tab::Installed, "Installed"),
                    (Tab::Mine, "Mine"),
                ] {
                    ui.selectable_value(&mut self.tab, tab, name);
                }
            });
            if self.tab != Tab::Installed {
                ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text("Search")
                        .desired_width(f32::INFINITY),
                );
            }
            if let Some(status) = &self.status {
                let (text, colour) = match status {
                    Ok(t) => (t.as_str(), theme::GREEN),
                    Err(t) => (t.as_str(), theme::RED),
                };
                ui.label(RichText::new(text).color(colour));
            }
            ui.separator();
            if self.form.is_some() {
                todo = self.form_ui(ui).or(todo.take());
                return;
            }
            if self.approving.is_some() {
                self.approve_ui(ui, &hub);
                return;
            }
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.tab {
                Tab::Installed => todo = self.installed_ui(ui).or(todo.take()),
                Tab::Mine => self.mine_ui(ui, &client, selection),
                _ => self.list_ui(ui, &client, &hub),
            });
        });
        self.open = open;
        todo
    }

    fn list_ui(&mut self, ui: &mut egui::Ui, client: &HubClient, hub: &str) {
        if self.list.is_empty() {
            ui.label(RichText::new("Nothing here yet.").color(theme::MUTED));
        }
        let installed: Vec<(String, u32)> =
            self.plugins().iter().filter(|p| p.hub == hub).map(|p| (p.asset_id.clone(), p.version)).collect();
        let mut fetch = None;
        let mut report = None;
        for asset in &self.list {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(&asset.name).strong());
                    ui.label(
                        RichText::new(format!(
                            "by {} · {} uses · v{}",
                            asset.creator, asset.installs, asset.version
                        ))
                        .color(theme::MUTED),
                    );
                    if !asset.about.is_empty() {
                        ui.label(&asset.about);
                    }
                    if asset.kind == AssetKind::Plugin {
                        let can = if asset.permissions.is_empty() {
                            "Can only look".to_owned()
                        } else {
                            asset
                                .permissions
                                .iter()
                                .map(|p| permission_words(p))
                                .collect::<Vec<_>>()
                                .join(", ")
                        };
                        ui.label(RichText::new(can).color(theme::AMBER));
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Report").clicked() {
                        report = Some(asset.clone());
                    }
                    let label = match (asset.kind, installed.iter().find(|(id, _)| *id == asset.asset_id)) {
                        (AssetKind::Model, _) => Some("Insert"),
                        (_, None) => Some("Install"),
                        (_, Some((_, v))) if *v < asset.version => Some("Update"),
                        _ => None,
                    };
                    match label {
                        Some(label) => {
                            if ui.add_enabled(!self.busy, egui::Button::new(label)).clicked() {
                                fetch = Some(asset.clone());
                            }
                        }
                        None => {
                            ui.label(RichText::new("Installed").color(theme::MUTED));
                        }
                    }
                });
            });
            ui.separator();
        }
        if let Some(asset) = fetch {
            self.fetch(client.clone(), asset);
        }
        if let Some(asset) = report {
            self.report(client.clone(), &asset);
        }
    }

    fn approve_ui(&mut self, ui: &mut egui::Ui, hub: &str) {
        let Some((asset, data)) = &self.approving else { return };
        ui.label(RichText::new(format!("Install {} by {}?", asset.name, asset.creator)).strong());
        if data.permissions.is_empty() {
            ui.label("It can only look at your game.");
        } else {
            ui.label("It can look at your game, and:");
            for p in &data.permissions {
                ui.label(format!("• {}", permission_words(p)));
            }
        }
        ui.label(RichText::new("It can't reach the internet or your files.").color(theme::MUTED));
        ui.horizontal(|ui| {
            if ui.button("Install").clicked() {
                let (asset, data) = self.approving.take().expect("shown above");
                self.install(hub, &asset, data);
            }
            if ui.button("Cancel").clicked() {
                self.approving = None;
            }
        });
    }

    fn installed_ui(&mut self, ui: &mut egui::Ui) -> Option<Do> {
        let mut todo = None;
        let mut remove = None;
        let list = self.plugins().clone();
        if list.is_empty() {
            ui.label(RichText::new("No plugins yet. Find some under Plugins.").color(theme::MUTED));
        }
        for (i, plugin) in list.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(&plugin.name).strong());
                    ui.label(
                        RichText::new(format!("by {} · v{}", plugin.creator, plugin.version))
                            .color(theme::MUTED),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Remove").clicked() {
                        remove = Some(i);
                    }
                    if ui.button("Run").clicked() {
                        todo = Some(Do::Run(plugin.clone()));
                    }
                });
            });
            ui.separator();
        }
        if let Some(i) = remove {
            let list = self.plugins();
            list.remove(i);
            let saved = plugins::save(list);
            if let Err(e) = saved {
                self.status = Some(Err(e));
            }
        }
        todo
    }

    fn mine_ui(&mut self, ui: &mut egui::Ui, client: &HubClient, selection: Option<InstanceId>) {
        let new_form = |kind, target, asset: Option<&Asset>| PublishForm {
            kind,
            target,
            asset_id: asset.map(|a| a.asset_id.clone()),
            name: asset.map(|a| a.name.clone()).unwrap_or_default(),
            about: asset.map(|a| a.about.clone()).unwrap_or_default(),
            permissions: libroxel_hub::PERMISSIONS
                .map(|p| asset.is_some_and(|a| a.permissions.iter().any(|q| q == p))),
        };
        ui.add_enabled_ui(selection.is_some(), |ui| {
            ui.horizontal(|ui| {
                if ui.button("Publish selection as a model").clicked()
                    && let Some(id) = selection
                {
                    self.form = Some(new_form(AssetKind::Model, id, None));
                }
                if ui.button("Publish selected script as a plugin").clicked()
                    && let Some(id) = selection
                {
                    self.form = Some(new_form(AssetKind::Plugin, id, None));
                }
            });
        });
        ui.separator();
        let mut take_down = None;
        for asset in &self.list {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(&asset.name).strong());
                    ui.label(
                        RichText::new(format!(
                            "{} · {} uses · v{}",
                            if asset.kind == AssetKind::Model { "Model" } else { "Plugin" },
                            asset.installs,
                            asset.version
                        ))
                        .color(theme::MUTED),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Take down").clicked() {
                        take_down = Some(asset.clone());
                    }
                    if ui
                        .add_enabled(selection.is_some(), egui::Button::new("Update from selection"))
                        .clicked()
                        && let Some(id) = selection
                    {
                        self.form = Some(new_form(asset.kind, id, Some(asset)));
                    }
                });
            });
            ui.separator();
        }
        if let Some(asset) = take_down {
            self.take_down(client.clone(), &asset);
        }
    }

    fn form_ui(&mut self, ui: &mut egui::Ui) -> Option<Do> {
        let busy = self.busy;
        let form = self.form.as_mut()?;
        let what = if form.kind == AssetKind::Model { "model" } else { "plugin" };
        ui.label(
            RichText::new(if form.asset_id.is_some() {
                format!("New version of this {what}")
            } else {
                format!("Publish a {what}")
            })
            .strong(),
        );
        ui.add(egui::TextEdit::singleline(&mut form.name).hint_text("Name").desired_width(f32::INFINITY));
        ui.add(
            egui::TextEdit::multiline(&mut form.about)
                .hint_text("What it is")
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        if form.kind == AssetKind::Plugin {
            ui.label("It may:");
            for (on, p) in form.permissions.iter_mut().zip(libroxel_hub::PERMISSIONS) {
                ui.checkbox(on, permission_words(p));
            }
        }
        let mut todo = None;
        let mut cancel = false;
        ui.horizontal(|ui| {
            if ui.add_enabled(!busy && !form.name.trim().is_empty(), egui::Button::new("Publish")).clicked() {
                todo = Some(Do::Publish(form.clone()));
            }
            cancel = ui.button("Cancel").clicked();
        });
        if cancel {
            self.form = None;
        }
        todo
    }
}
