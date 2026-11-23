use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy_egui::egui::{self, Color32, RichText};
use libroxel_hub::{
    GROUP_COLORS, GroupAction, GroupBadge, GroupChange, GroupInfo, GroupJoin, GroupPage, GroupRole,
    HubClient, MAX_TAG, group_color,
};
use libroxel_instance::serde_json::Value as Json;

use crate::browse::Platform;
use crate::hub_home::{HomeState, View, person_row, search_box};
use crate::menu::Tab;
use crate::theme::{self, icon};

type Slot<T> = Arc<Mutex<Option<Result<T, String>>>>;
type Found = Arc<Mutex<Vec<(String, Option<GroupBadge>)>>>;
type Act = (GroupAction, String, String, String, Option<GroupRole>, String);

pub fn colour(name: &str) -> Color32 {
    let [r, g, b] = group_color(name);
    Color32::from_rgb(r, g, b)
}

pub fn badge(ui: &mut egui::Ui, group: &GroupBadge) -> egui::Response {
    ui.label(
        RichText::new(format!(" {} ", group.tag))
            .small()
            .strong()
            .color(Color32::WHITE)
            .background_color(colour(&group.color)),
    )
    .on_hover_text(&group.name)
}

fn join_name(join: GroupJoin) -> &'static str {
    match join {
        GroupJoin::Open => "Anyone can join",
        GroupJoin::Request => "Ask to join",
        GroupJoin::Invite => "Invite only",
    }
}

fn pick_colour(ui: &mut egui::Ui, id: &str, chosen: &mut String) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(RichText::new(chosen.as_str()).color(colour(chosen)))
        .show_ui(ui, |ui| {
            for (name, _) in GROUP_COLORS {
                ui.selectable_value(chosen, name.to_owned(), RichText::new(name).color(colour(name)));
            }
        });
}

fn pick_join(ui: &mut egui::Ui, id: &str, chosen: &mut GroupJoin) {
    egui::ComboBox::from_id_salt(id).selected_text(join_name(*chosen)).show_ui(ui, |ui| {
        for join in [GroupJoin::Open, GroupJoin::Request, GroupJoin::Invite] {
            ui.selectable_value(chosen, join, join_name(join));
        }
    });
}

#[derive(Clone)]
struct Edit {
    name: String,
    tag: String,
    about: String,
    color: String,
    join: GroupJoin,
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
    color: String,
    join: GroupJoin,
    post: String,
    edit: Option<Edit>,
    invite: String,
    invites: Vec<GroupInfo>,
    invites_slot: Slot<Vec<GroupInfo>>,
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
        if let Some(Ok(invites)) = self.invites_slot.lock().unwrap().take() {
            self.invites = invites;
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
        let invites = self.invites_slot.clone();
        let time = self.stamp();
        let signed_in = platform.account.is_some();
        std::thread::spawn(move || {
            *out.lock().unwrap() = Some(client.groups(&search));
            if signed_in && let Ok(me) = crate::identity() {
                let id = me.player_id();
                let signature = me.sign(&libroxel_hub::group_invites_message(&id, time));
                *invites.lock().unwrap() = Some(client.group_invites(&id, time, &signature));
            }
        });
    }

    fn load(&mut self, platform: &Platform, group_id: &str) {
        let client = platform.client();
        let id = group_id.to_owned();
        let out = self.page_slot.clone();
        let time = self.stamp();
        let signed_in = platform.account.is_some();
        std::thread::spawn(move || {
            let page = match crate::identity() {
                Ok(me) if signed_in => {
                    let who = me.player_id();
                    let signature = me.sign(&libroxel_hub::group_view_message(&who, &id, time));
                    client.group_as(&id, &who, time, &signature)
                }
                _ => client.group(&id),
            };
            *out.lock().unwrap() = Some(page);
        });
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
        self.send(platform, action, group_id, said, None, edit);
    }

    fn send(
        &mut self,
        platform: &Platform,
        action: GroupAction,
        group_id: &str,
        said: String,
        lookup: Option<String>,
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
                if let Some(name) = &lookup {
                    let found = client.profile(name)?.ok_or_else(|| format!("Nobody is called {name}."))?;
                    change.other_id = found.player_id;
                }
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
            let status = format!("{} members · owned by {} · {}", g.members, g.owner, join_name(g.join));
            person_row(ui, &Json::Null, &format!("{}  [{}]", g.name, g.tag), (&status, theme::MUTED), |ui| {
                if ui.button("Open").clicked() {
                    open = Some(g.group_id.clone());
                }
            });
        }
        let ui = &mut columns[1];
        if !groups.invites.is_empty() {
            ui.label(
                RichText::new(format!("Invites ({})", groups.invites.len()))
                    .font(theme::display(18.0))
                    .color(theme::AMBER),
            );
            for g in &groups.invites {
                person_row(
                    ui,
                    &Json::Null,
                    &format!("{}  [{}]", g.name, g.tag),
                    ("Invited you", theme::MUTED),
                    |ui| {
                        if ui.button("Open").clicked() {
                            open = Some(g.group_id.clone());
                        }
                    },
                );
            }
            ui.add_space(10.0);
        }
        ui.label(RichText::new("Make a group").font(theme::display(18.0)).color(theme::TEXT));
        ui.add_space(4.0);
        if platform.account.is_none() {
            ui.label(RichText::new("Make an account on the Avatar tab first.").color(theme::MUTED));
            return;
        }
        ui.add(egui::TextEdit::singleline(&mut groups.name).hint_text("Name").char_limit(30));
        if groups.color.is_empty() {
            groups.color = "purple".into();
        }
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut groups.tag)
                    .hint_text(format!("Tag (2 to {MAX_TAG})"))
                    .char_limit(MAX_TAG)
                    .desired_width(110.0),
            );
            pick_colour(ui, "new-group-colour", &mut groups.color);
            pick_join(ui, "new-group-join", &mut groups.join);
        });
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
            let (name, tag, about, color, join) = (
                groups.name.trim().to_owned(),
                groups.tag.trim().to_ascii_uppercase(),
                groups.about.trim().to_owned(),
                groups.color.clone(),
                groups.join,
            );
            groups.name.clear();
            groups.tag.clear();
            groups.about.clear();
            let said = format!("Made {name}.");
            groups.change(platform, GroupAction::Create, "", said, move |c| {
                c.name = name;
                c.tag = tag;
                c.about = about;
                c.color = color;
                c.join = Some(join);
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
    let mut act: Option<Act> = None;
    let plain = |action: GroupAction, said: String| -> Act {
        (action, id.clone(), String::new(), String::new(), None, said)
    };
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(&page.group.name).font(theme::display(26.0)).color(theme::HEADING));
        badge(
            ui,
            &GroupBadge {
                group_id: id.clone(),
                name: page.group.name.clone(),
                tag: page.group.tag.clone(),
                color: page.group.color.clone(),
            },
        );
    });
    ui.label(
        RichText::new(format!(
            "{} members · owned by {} · {}",
            page.group.members,
            page.group.owner,
            join_name(page.group.join)
        ))
        .color(theme::MUTED),
    );
    if !page.group.about.is_empty() {
        ui.add_space(4.0);
        ui.label(RichText::new(&page.group.about).color(theme::TEXT));
    }
    ui.add_space(6.0);
    ui.add_enabled_ui(!busy, |ui| {
        ui.horizontal(|ui| {
            if me.is_some() && mine.is_none() {
                let joined = format!("You joined {}.", page.group.name);
                if page.invited {
                    if theme::block_button(ui, theme::Kind::Primary, icon::CHECK, "Accept invite", 32.0)
                        .clicked()
                    {
                        act = Some(plain(GroupAction::Join, joined));
                    }
                    if ui.button("Decline").clicked() {
                        act = Some(plain(GroupAction::Leave, "Declined.".into()));
                    }
                } else if page.requested {
                    ui.label(RichText::new("Request sent").color(theme::MUTED));
                    if ui.button("Cancel").clicked() {
                        act = Some(plain(GroupAction::Leave, "Request taken back.".into()));
                    }
                } else {
                    match page.group.join {
                        GroupJoin::Open => {
                            if theme::block_button(ui, theme::Kind::Primary, icon::USER_PLUS, "Join", 32.0)
                                .clicked()
                            {
                                act = Some(plain(GroupAction::Join, joined));
                            }
                        }
                        GroupJoin::Request => {
                            if theme::block_button(
                                ui,
                                theme::Kind::Primary,
                                icon::USER_PLUS,
                                "Ask to join",
                                32.0,
                            )
                            .clicked()
                            {
                                act = Some(plain(
                                    GroupAction::Join,
                                    format!("Asked to join {}.", page.group.name),
                                ));
                            }
                        }
                        GroupJoin::Invite => {
                            ui.label(RichText::new("Invite only").color(theme::MUTED));
                        }
                    }
                }
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
                groups.edit = Some(Edit {
                    name: page.group.name.clone(),
                    tag: page.group.tag.clone(),
                    about: page.group.about.clone(),
                    color: page.group.color.clone(),
                    join: page.group.join,
                });
            }
        });
    });
    let mut save = None;
    if let Some(edit) = &mut groups.edit {
        theme::panel().show(ui, |ui| {
            ui.add(egui::TextEdit::singleline(&mut edit.name).char_limit(30));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut edit.tag).char_limit(MAX_TAG).desired_width(110.0));
                pick_colour(ui, "edit-group-colour", &mut edit.color);
                pick_join(ui, "edit-group-join", &mut edit.join);
            });
            ui.add(egui::TextEdit::multiline(&mut edit.about).desired_rows(3).char_limit(1000));
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    save = Some(Some(edit.clone()));
                }
                if ui.button("Cancel").clicked() {
                    save = Some(None);
                }
            });
        });
    }
    if let Some(saved) = save {
        groups.edit = None;
        if let Some(edit) = saved {
            groups.change(platform, GroupAction::Edit, &id, "Saved.".into(), move |c| {
                c.name = edit.name.trim().to_owned();
                c.tag = edit.tag.trim().to_ascii_uppercase();
                c.about = edit.about.trim().to_owned();
                c.color = edit.color;
                c.join = Some(edit.join);
            });
        }
    }
    if admin {
        ui.add_space(6.0);
        let mut invite = None;
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut groups.invite).hint_text("Their name").desired_width(180.0),
            );
            if ui
                .add_enabled(!busy && !groups.invite.trim().is_empty(), egui::Button::new("Invite"))
                .clicked()
            {
                invite = Some(groups.invite.trim().to_owned());
            }
        });
        for p in &page.requests {
            person_row(ui, &Json::Null, &p.name, ("Wants to join", theme::AMBER), |ui| {
                if ui.add_enabled(!busy, egui::Button::new("Decline")).clicked() {
                    act = Some((
                        GroupAction::Decline,
                        id.clone(),
                        p.player_id.clone(),
                        String::new(),
                        None,
                        "Declined.".into(),
                    ));
                }
                if ui.add_enabled(!busy, egui::Button::new("Accept")).clicked() {
                    act = Some((
                        GroupAction::Accept,
                        id.clone(),
                        p.player_id.clone(),
                        String::new(),
                        None,
                        format!("{} joined.", p.name),
                    ));
                }
            });
        }
        for p in &page.invites {
            person_row(ui, &Json::Null, &p.name, ("Invited", theme::MUTED), |ui| {
                if ui.add_enabled(!busy, egui::Button::new("Cancel")).clicked() {
                    act = Some((
                        GroupAction::Decline,
                        id.clone(),
                        p.player_id.clone(),
                        String::new(),
                        None,
                        "Invite taken back.".into(),
                    ));
                }
            });
        }
        if let Some(name) = invite {
            groups.invite.clear();
            let said = format!("Invited {name}.");
            groups.send(platform, GroupAction::Invite, &id, said, Some(name), |_| {});
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
    known: HashMap<String, (Instant, Option<GroupBadge>)>,
    slot: Found,
}

const BADGE_EVERY: Duration = Duration::from_secs(120);

impl Badges {
    pub fn of(&mut self, client: impl FnOnce() -> HubClient, name: &str) -> Option<GroupBadge> {
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
                    let tag = client.profile(&who).ok().flatten().and_then(|p| p.badge);
                    out.lock().unwrap().push((who, tag));
                });
                tag
            }
        }
    }
}
