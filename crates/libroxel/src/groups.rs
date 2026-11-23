use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy_egui::egui::{self, Color32, RichText};
use libroxel_hub::{GroupAction, GroupBadge, GroupChange, GroupInfo, GroupPage, GroupRole, HubClient};
use libroxel_instance::serde_json::Value as Json;

use crate::browse::Platform;
use crate::hub_home::{HomeState, View, person_row, search_box};
use crate::menu::Tab;
use crate::theme::{self, icon};

type Slot<T> = Arc<Mutex<Option<Result<T, String>>>>;
type Found = Arc<Mutex<Vec<(String, Option<String>)>>>;

pub const BADGE: Color32 = Color32::from_rgb(120, 80, 200);

pub fn badge(ui: &mut egui::Ui, group: &GroupBadge) -> egui::Response {
    ui.label(
        RichText::new(format!(" {} ", group.tag))
            .small()
            .strong()
            .color(Color32::WHITE)
            .background_color(BADGE),
    )
    .on_hover_text(&group.name)
}

#[derive(Default)]
pub struct Groups {
    search: String,
    list: Vec<GroupInfo>,
    list_slot: Slot<Vec<GroupInfo>>,
    listed: bool,
    open: Option<String>,
    page: Option<GroupPage>,
    page_slot: Slot<GroupPage>,
    changed: Slot<String>,
    busy: bool,
    status: Option<Result<String, String>>,
    name: String,
    tag: String,
    about: String,
    post: String,
    edit: Option<(String, String, String)>,
    clock: i64,
}

impl Groups {
    fn stamp(&mut self) -> i64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64);
        self.clock = now.max(self.clock + 1);
        self.clock
    }

    fn take(&mut self) {
        if let Some(result) = self.list_slot.lock().unwrap().take() {
            match result {
                Ok(list) => self.list = list,
                Err(e) => self.status = Some(Err(e)),
            }
        }
        if let Some(result) = self.page_slot.lock().unwrap().take() {
            match result {
                Ok(page) if Some(&page.group.group_id) == self.open.as_ref() => self.page = Some(page),
                Ok(_) => {}
                Err(e) => {
                    self.status = Some(Err(e));
                    self.open = None;
                }
            }
        }
    }

    fn find(&mut self, platform: &Platform) {
        self.listed = true;
        let client = platform.client();
        let search = self.search.trim().to_owned();
        let out = self.list_slot.clone();
        std::thread::spawn(move || *out.lock().unwrap() = Some(client.groups(&search)));
    }

    fn load(&mut self, platform: &Platform, group_id: &str) {
        let client = platform.client();
        let id = group_id.to_owned();
        let out = self.page_slot.clone();
        std::thread::spawn(move || *out.lock().unwrap() = Some(client.group(&id)));
    }

    fn open(&mut self, platform: &Platform, group_id: &str) {
        self.open = Some(group_id.to_owned());
        self.page = None;
        self.edit = None;
        self.status = None;
        self.load(platform, group_id);
    }

    fn change(
        &mut self,
        platform: &Platform,
        action: GroupAction,
        group_id: &str,
        said: String,
        edit: impl FnOnce(&mut GroupChange) + Send + 'static,
    ) {
        let time = self.stamp();
        self.busy = true;
        self.status = None;
        let client = platform.client();
        let out = self.changed.clone();
        let group_id = group_id.to_owned();
        std::thread::spawn(move || {
            let result = crate::identity().and_then(|me| {
                let mut change = GroupChange::new(&me.player_id(), action, &group_id, time);
                edit(&mut change);
                change.signature = me.sign(&libroxel_hub::group_message(&change));
                client.change_group(&change).map(|c| c.group_id)
            });
            *out.lock().unwrap() = Some(result.map(|id| format!("{id}\n{said}")));
        });
    }

    fn settle(&mut self, platform: &mut Platform) {
        let Some(result) = self.changed.lock().unwrap().take() else { return };
        self.busy = false;
        match result {
            Ok(text) => {
                let (id, said) = text.split_once('\n').unwrap_or((&text, ""));
                let id = id.to_owned();
                if !said.is_empty() {
                    self.status = Some(Ok(said.to_owned()));
                }
                platform.refresh_account();
                self.listed = false;
                if id.is_empty() {
                    self.open = None;
                    self.page = None;
                } else if self.open.as_deref() == Some(id.as_str()) {
                    self.load(platform, &id);
                } else {
                    self.open(platform, &id);
                    self.status = Some(Ok(said.to_owned()));
                }
            }
            Err(e) => self.status = Some(Err(e)),
        }
    }
}

pub fn groups_tab(
    groups: &mut Groups,
    home: &mut HomeState,
    ui: &mut egui::Ui,
    platform: &mut Platform,
    tab: &mut Tab,
) {
    groups.take();
    groups.settle(platform);
    match &groups.status {
        Some(Ok(said)) if !said.is_empty() => {
            theme::status(ui, theme::GREEN, said);
        }
        Some(Err(why)) => crate::report::status(ui, why),
        _ => {}
    }
    if groups.open.is_some() {
        group_page(groups, home, ui, platform, tab);
        return;
    }
    if !groups.listed {
        groups.find(platform);
    }
    theme::heading(ui, "Groups");
    ui.add_space(8.0);
    let mut open = None;
    ui.columns(2, |columns| {
        let ui = &mut columns[0];
        if search_box(ui, &mut groups.search, "A group name or tag") {
            groups.find(platform);
        }
        ui.add_space(6.0);
        if groups.list.is_empty() {
            ui.label(RichText::new("No groups found.").color(theme::MUTED));
        }
        for g in &groups.list {
            let status = format!("{} members · owned by {}", g.members, g.owner);
            person_row(ui, &Json::Null, &format!("{}  [{}]", g.name, g.tag), (&status, theme::MUTED), |ui| {
                if ui.button("Open").clicked() {
                    open = Some(g.group_id.clone());
                }
            });
        }
        let ui = &mut columns[1];
        ui.label(RichText::new("Make a group").font(theme::display(18.0)).color(theme::TEXT));
        ui.add_space(4.0);
        if platform.account.is_none() {
            ui.label(RichText::new("Make an account on the Avatar tab first.").color(theme::MUTED));
            return;
        }
        ui.add(egui::TextEdit::singleline(&mut groups.name).hint_text("Name").char_limit(30));
        ui.add(egui::TextEdit::singleline(&mut groups.tag).hint_text("Tag").char_limit(5));
        ui.add(
            egui::TextEdit::multiline(&mut groups.about).hint_text("About").desired_rows(3).char_limit(1000),
        );
        let ready = !groups.busy && !groups.name.trim().is_empty() && !groups.tag.trim().is_empty();
        if ui
            .add_enabled_ui(ready, |ui| {
                theme::block_button(ui, theme::Kind::Primary, icon::CHECK, "Make", 34.0)
            })
            .inner
            .clicked()
        {
            let (name, tag, about) = (
                groups.name.trim().to_owned(),
                groups.tag.trim().to_ascii_uppercase(),
                groups.about.trim().to_owned(),
            );
            groups.name.clear();
            groups.tag.clear();
            groups.about.clear();
            let said = format!("Made {name}.");
            groups.change(platform, GroupAction::Create, "", said, move |c| {
                c.name = name;
                c.tag = tag;
                c.about = about;
            });
        }
    });
    if let Some(id) = open {
        groups.open(platform, &id);
    }
}

fn role_name(role: GroupRole) -> &'static str {
    match role {
        GroupRole::Owner => "Owner",
        GroupRole::Admin => "Admin",
        GroupRole::Member => "Member",
    }
}

fn group_page(
    groups: &mut Groups,
    home: &mut HomeState,
    ui: &mut egui::Ui,
    platform: &mut Platform,
    tab: &mut Tab,
) {
    if ui.button(format!("{}  Groups", icon::ARROW_LEFT)).clicked() {
        groups.open = None;
        groups.page = None;
        groups.status = None;
        return;
    }
    let Some(page) = groups.page.clone() else {
        ui.label(RichText::new("Loading…").color(theme::MUTED));
        return;
    };
    let id = page.group.group_id.clone();
    let me = platform.account.as_ref().map(|a| a.player_id.clone());
    let mine = page.members.iter().find(|m| Some(&m.player_id) == me.as_ref()).map(|m| m.role);
    let admin = mine.is_some_and(|r| r >= GroupRole::Admin);
    let shown = platform.account.as_ref().and_then(|a| a.badge.as_ref()).is_some_and(|b| b.group_id == id);
    let busy = groups.busy;
    let mut act: Option<(GroupAction, String, String, String, Option<GroupRole>, String)> = None;
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(&page.group.name).font(theme::display(26.0)).color(theme::HEADING));
        badge(
            ui,
            &GroupBadge { group_id: id.clone(), name: page.group.name.clone(), tag: page.group.tag.clone() },
        );
    });
    ui.label(
        RichText::new(format!("{} members · owned by {}", page.group.members, page.group.owner))
            .color(theme::MUTED),
    );
    if !page.group.about.is_empty() {
        ui.add_space(4.0);
        ui.label(RichText::new(&page.group.about).color(theme::TEXT));
    }
    ui.add_space(6.0);
    ui.add_enabled_ui(!busy, |ui| {
        ui.horizontal(|ui| {
            if me.is_some()
                && mine.is_none()
                && theme::block_button(ui, theme::Kind::Primary, icon::USER_PLUS, "Join", 32.0).clicked()
            {
                act = Some((
                    GroupAction::Join,
                    id.clone(),
                    String::new(),
                    String::new(),
                    None,
                    format!("You joined {}.", page.group.name),
                ));
            }
            if mine.is_some() {
                let (label, target, said) = if shown {
                    ("Hide badge", String::new(), "Your badge is hidden.".to_owned())
                } else {
                    ("Show badge", id.clone(), format!("Your badge is {} now.", page.group.tag))
                };
                if ui.button(label).clicked() {
                    act = Some((GroupAction::Show, target, String::new(), String::new(), None, said));
                }
                if ui.button("Leave").clicked() {
                    act = Some((
                        GroupAction::Leave,
                        id.clone(),
                        String::new(),
                        String::new(),
                        None,
                        format!("You left {}.", page.group.name),
                    ));
                }
            }
            if mine == Some(GroupRole::Owner) && groups.edit.is_none() && ui.button("Edit").clicked() {
                groups.edit =
                    Some((page.group.name.clone(), page.group.tag.clone(), page.group.about.clone()));
            }
        });
    });
    let mut save = None;
    if let Some((name, tag, about)) = &mut groups.edit {
        theme::panel().show(ui, |ui| {
            ui.add(egui::TextEdit::singleline(name).char_limit(30));
            ui.add(egui::TextEdit::singleline(tag).char_limit(5));
            ui.add(egui::TextEdit::multiline(about).desired_rows(3).char_limit(1000));
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    save = Some(Some((
                        name.trim().to_owned(),
                        tag.trim().to_ascii_uppercase(),
                        about.trim().to_owned(),
                    )));
                }
                if ui.button("Cancel").clicked() {
                    save = Some(None);
                }
            });
        });
    }
    if let Some(saved) = save {
        groups.edit = None;
        if let Some((name, tag, about)) = saved {
            groups.change(platform, GroupAction::Edit, &id, "Saved.".into(), move |c| {
                c.name = name;
                c.tag = tag;
                c.about = about;
            });
        }
    }
    ui.add_space(10.0);
    ui.label(
        RichText::new(format!("Games ({})", page.games.len())).font(theme::display(18.0)).color(theme::TEXT),
    );
    ui.horizontal_wrapped(|ui| {
        for game in &page.games {
            if ui.button(format!("{}  {}", icon::GAME_CONTROLLER, game.title)).clicked() {
                home.view = View::Game(game.game_id.clone());
                *tab = Tab::Home;
            }
            if admin && ui.small_button(icon::X).on_hover_text("Remove from the group").clicked() {
                act = Some((
                    GroupAction::RemoveGame,
                    id.clone(),
                    game.game_id.clone(),
                    String::new(),
                    None,
                    "Removed.".into(),
                ));
            }
        }
    });
    if admin {
        let theirs: Vec<_> = platform
            .account
            .as_ref()
            .map(|a| {
                a.games
                    .iter()
                    .filter(|g| !page.games.iter().any(|x| x.game_id == g.game_id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        if !theirs.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Add:").color(theme::MUTED));
                for game in theirs {
                    if ui.add_enabled(!busy, egui::Button::new(&game.title)).clicked() {
                        act = Some((
                            GroupAction::AddGame,
                            id.clone(),
                            game.game_id.clone(),
                            String::new(),
                            None,
                            "Added.".into(),
                        ));
                    }
                }
            });
        }
    }
    ui.add_space(10.0);
    let mut post = std::mem::take(&mut groups.post);
    ui.columns(2, |columns| {
        let ui = &mut columns[0];
        ui.label(RichText::new("Wall").font(theme::display(18.0)).color(theme::TEXT));
        ui.add_space(4.0);
        if mine.is_some() {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut post)
                        .hint_text("Say something")
                        .char_limit(300)
                        .desired_width(ui.available_width() - 70.0),
                );
                if ui.add_enabled(!busy && !post.trim().is_empty(), egui::Button::new("Post")).clicked() {
                    act = Some((
                        GroupAction::Post,
                        id.clone(),
                        String::new(),
                        post.trim().to_owned(),
                        None,
                        String::new(),
                    ));
                    post.clear();
                }
            });
        }
        if page.posts.is_empty() {
            ui.label(RichText::new("Nothing here yet.").color(theme::MUTED));
        }
        for p in &page.posts {
            theme::panel().inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&p.name).strong().color(theme::TEXT));
                    ui.label(RichText::new(crate::browse::ago(p.secs_ago)).small().color(theme::MUTED));
                    if (admin || me.as_ref() == Some(&p.player_id))
                        && ui.small_button(icon::X).on_hover_text("Remove").clicked()
                    {
                        act = Some((
                            GroupAction::Unpost,
                            id.clone(),
                            p.post_id.to_string(),
                            String::new(),
                            None,
                            String::new(),
                        ));
                    }
                });
                ui.label(RichText::new(&p.text).color(theme::TEXT));
            });
            ui.add_space(3.0);
        }
        let ui = &mut columns[1];
        ui.label(
            RichText::new(format!("Members ({})", page.members.len()))
                .font(theme::display(18.0))
                .color(theme::TEXT),
        );
        ui.add_space(4.0);
        for m in &page.members {
            person_row(ui, &Json::Null, &m.name, (role_name(m.role), theme::MUTED), |ui| {
                if busy {
                    return;
                }
                if mine.is_some_and(|r| r > m.role && r >= GroupRole::Admin)
                    && ui.small_button(icon::USER_MINUS).on_hover_text("Remove").clicked()
                {
                    act = Some((
                        GroupAction::Kick,
                        id.clone(),
                        m.player_id.clone(),
                        String::new(),
                        None,
                        format!("Removed {}.", m.name),
                    ));
                }
                if mine == Some(GroupRole::Owner) && m.role != GroupRole::Owner {
                    for role in [GroupRole::Owner, GroupRole::Admin, GroupRole::Member] {
                        if role != m.role && ui.small_button(role_name(role)).clicked() {
                            act = Some((
                                GroupAction::Role,
                                id.clone(),
                                m.player_id.clone(),
                                String::new(),
                                Some(role),
                                format!("{} is {} now.", m.name, role_name(role).to_lowercase()),
                            ));
                        }
                    }
                }
            });
        }
    });
    groups.post = post;
    if let Some((action, group_id, other_id, text, role, said)) = act {
        groups.change(platform, action, &group_id, said, move |c| {
            c.other_id = other_id;
            c.text = text;
            c.role = role;
        });
    }
}

#[derive(Default)]
pub struct Badges {
    known: HashMap<String, (Instant, Option<String>)>,
    slot: Found,
}

const BADGE_EVERY: Duration = Duration::from_secs(120);

impl Badges {
    pub fn tag(&mut self, client: impl FnOnce() -> HubClient, name: &str) -> Option<String> {
        for (who, tag) in self.slot.lock().unwrap().drain(..) {
            self.known.insert(who, (Instant::now(), tag));
        }
        match self.known.get(name) {
            Some((at, tag)) if at.elapsed() < BADGE_EVERY => tag.clone(),
            old => {
                let tag = old.and_then(|(_, t)| t.clone());
                self.known.insert(name.to_owned(), (Instant::now(), tag.clone()));
                let client = client();
                let out = self.slot.clone();
                let who = name.to_owned();
                std::thread::spawn(move || {
                    let tag = client.profile(&who).ok().flatten().and_then(|p| p.badge).map(|b| b.tag);
                    out.lock().unwrap().push((who, tag));
                });
                tag
            }
        }
    }
}
