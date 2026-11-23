use bevy_egui::egui::{self, RichText};
use libroxel_hub::{
    Birthday, Chat, Consent, Family, FamilyChange, FamilySettings, HubClient, Messages, ParentEmail,
};

use super::{Slot, libroxel_hub_now_ms};

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

#[derive(Default)]
pub struct FamilyUi {
    pub family: Option<Family>,
    found: Slot<Family>,
    busy: bool,
    status: Option<Result<String, String>>,
    year: i64,
    month: i64,
    day: i64,
    pub parent_email: String,
    code: String,
    pin: String,
    new_pin: String,
    edit: Option<FamilySettings>,
}

fn today() -> i64 {
    libroxel_hub_now_ms() / 1000 / 86_400
}

fn this_year() -> i64 {
    1970 + today() * 400 / 146_097
}

fn pin_ok(pin: &str) -> bool {
    pin.len() == 4 && pin.chars().all(|c| c.is_ascii_digit())
}

impl FamilyUi {
    pub fn tick(&mut self) {
        if let Some(result) = self.found.lock().unwrap().take() {
            self.busy = false;
            match result {
                Ok(family) => {
                    if family.consent && self.family.as_ref().is_some_and(|f| !f.consent) {
                        self.status = Some(Ok("Approved.".into()));
                    }
                    self.family = Some(family);
                }
                Err(e) => self.status = Some(Err(e)),
            }
        }
    }

    pub fn forget(&mut self) {
        *self = Self::default();
    }

    pub fn fetch(&mut self, client: HubClient) {
        let out = self.found.clone();
        std::thread::spawn(move || {
            let result = crate::identity().and_then(|me| {
                let (id, time) = (me.player_id(), libroxel_hub_now_ms());
                client.family(&id, time, &me.sign(&libroxel_hub::family_read_message(&id, time)))
            });
            *out.lock().unwrap() = Some(result);
        });
    }

    fn run(&mut self, work: impl FnOnce() -> Result<Family, String> + Send + 'static) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.status = None;
        let out = self.found.clone();
        std::thread::spawn(move || *out.lock().unwrap() = Some(work()));
    }

    pub fn born(&self) -> Option<String> {
        let date = format!("{:04}-{:02}-{:02}", self.year, self.month, self.day);
        libroxel_hub::parse_date(&date).map(|_| date)
    }

    pub fn under_13(&self) -> bool {
        self.born()
            .and_then(|b| libroxel_hub::parse_date(&b))
            .is_some_and(|b| libroxel_hub::age_on(b, today()) < i64::from(libroxel_hub::CONSENT_AGE))
    }

    pub fn ready(&self) -> Option<(String, Option<String>)> {
        let born = self.born()?;
        let email = self.parent_email.trim().to_owned();
        if !self.under_13() || email.is_empty() {
            Some((born, None))
        } else {
            libroxel_hub::valid_email(&email).then_some((born, Some(email)))
        }
    }

    pub fn birthday_rows(&mut self, ui: &mut egui::Ui) {
        ui.label("Birthday");
        ui.horizontal(|ui| {
            let month = if self.month == 0 { "Month" } else { MONTHS[self.month as usize - 1] };
            egui::ComboBox::from_id_salt("born-month").selected_text(month).show_ui(ui, |ui| {
                for (i, m) in MONTHS.iter().enumerate() {
                    ui.selectable_value(&mut self.month, i as i64 + 1, *m);
                }
            });
            let day = if self.day == 0 { "Day".to_owned() } else { self.day.to_string() };
            egui::ComboBox::from_id_salt("born-day").selected_text(day).width(50.0).show_ui(ui, |ui| {
                for d in 1..=31 {
                    ui.selectable_value(&mut self.day, d, d.to_string());
                }
            });
            let year = if self.year == 0 { "Year".to_owned() } else { self.year.to_string() };
            egui::ComboBox::from_id_salt("born-year").selected_text(year).width(60.0).show_ui(ui, |ui| {
                let now = this_year();
                for y in (now - 100..=now).rev() {
                    ui.selectable_value(&mut self.year, y, y.to_string());
                }
            });
        });
        ui.end_row();
        if self.under_13() {
            ui.label("Parent's email");
            ui.add(
                egui::TextEdit::singleline(&mut self.parent_email)
                    .char_limit(254)
                    .hint_text("parent@example.com (optional)"),
            );
            ui.end_row();
        }
    }

    pub fn send_birthday(&mut self, client: HubClient) {
        let Some((born, parent_email)) = self.ready() else { return };
        self.run(move || set_birthday(&client, born, parent_email));
    }

    pub fn birthday_page(&mut self, ui: &mut egui::Ui, client: HubClient) {
        crate::theme::heading(ui, "When's your birthday?");
        ui.add_space(8.0);
        egui::Grid::new("birthday").num_columns(2).show(ui, |ui| self.birthday_rows(ui));
        ui.horizontal(|ui| {
            if ui.add_enabled(self.ready().is_some() && !self.busy, egui::Button::new("Save")).clicked() {
                self.send_birthday(client);
            }
            if self.busy {
                ui.spinner();
            }
        });
        self.show_status(ui);
    }

    fn show_status(&self, ui: &mut egui::Ui) {
        match &self.status {
            Some(Ok(said)) => {
                ui.colored_label(crate::theme::GREEN, said);
            }
            Some(Err(why)) => crate::report::error(ui, why),
            None => {}
        }
    }

    pub fn parent_ui(&mut self, ui: &mut egui::Ui, client: HubClient) {
        let Some(family) = self.family.clone().filter(|f| f.has_birthday) else { return };
        ui.add_space(10.0);
        ui.label(RichText::new("Parent settings").strong());
        if family.under_13 && !family.consent {
            self.consent_ui(ui, client, &family);
        } else if let Some(edit) = self.edit {
            self.settings_ui(ui, client, &family, edit);
        } else {
            ui.horizontal(|ui| {
                let label = if family.has_pin { "PIN" } else { "New PIN" };
                ui.label(label);
                ui.add(
                    egui::TextEdit::singleline(&mut self.pin)
                        .password(true)
                        .char_limit(4)
                        .desired_width(50.0),
                );
                let button = if family.has_pin { "Open" } else { "Set PIN" };
                if ui.add_enabled(pin_ok(&self.pin), egui::Button::new(button)).clicked() {
                    if !family.has_pin {
                        self.new_pin = self.pin.clone();
                        self.pin.clear();
                    }
                    self.edit = Some(family.settings);
                }
            });
        }
        self.show_status(ui);
    }

    fn consent_ui(&mut self, ui: &mut egui::Ui, client: HubClient, family: &Family) {
        let to = family.parent_email.as_deref().unwrap_or("parent@example.com");
        ui.colored_label(crate::theme::AMBER, "Friend requests are off until a parent approves.");
        egui::Grid::new("consent").num_columns(2).show(ui, |ui| {
            ui.label("Code from the mail");
            ui.add(egui::TextEdit::singleline(&mut self.code).char_limit(6).desired_width(70.0));
            ui.end_row();
            ui.label("Parent's new PIN");
            ui.add(
                egui::TextEdit::singleline(&mut self.new_pin)
                    .password(true)
                    .char_limit(4)
                    .desired_width(50.0),
            );
            ui.end_row();
        });
        ui.horizontal(|ui| {
            let ready = self.code.trim().len() == 6 && pin_ok(&self.new_pin) && !self.busy;
            if ui.add_enabled(ready, egui::Button::new("Approve")).clicked() {
                let (code, pin) = (self.code.trim().to_owned(), std::mem::take(&mut self.new_pin));
                self.code.clear();
                let client = client.clone();
                self.run(move || {
                    let me = crate::identity()?;
                    let mut c = Consent {
                        player_id: me.player_id(),
                        code,
                        pin,
                        time: libroxel_hub_now_ms(),
                        signature: String::new(),
                    };
                    c.signature = me.sign(&libroxel_hub::consent_message(&c));
                    client.consent(&c)
                });
            }
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.parent_email)
                    .char_limit(254)
                    .hint_text(to)
                    .desired_width(200.0),
            );
            let email = Some(self.parent_email.trim().to_owned())
                .filter(|e| !e.is_empty())
                .or_else(|| family.parent_email.clone())
                .unwrap_or_default();
            let ready = libroxel_hub::valid_email(&email) && !self.busy;
            if ui.add_enabled(ready, egui::Button::new("Send code")).clicked() {
                let client = client.clone();
                self.run(move || {
                    let me = crate::identity()?;
                    let (id, time) = (me.player_id(), libroxel_hub_now_ms());
                    client.ask_parent(&ParentEmail {
                        signature: me.sign(&libroxel_hub::parent_email_message(&id, &email, time)),
                        player_id: id.clone(),
                        parent_email: email,
                        time,
                    })?;
                    client.family(&id, time, &me.sign(&libroxel_hub::family_read_message(&id, time)))
                });
            }
        });
    }

    fn settings_ui(
        &mut self,
        ui: &mut egui::Ui,
        client: HubClient,
        family: &Family,
        mut edit: FamilySettings,
    ) {
        egui::Grid::new("parent-settings").num_columns(2).show(ui, |ui| {
            ui.label("Chat");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut edit.chat, Chat::Off, "Off");
                ui.selectable_value(&mut edit.chat, Chat::Strict, "Strict");
                ui.add_enabled_ui(!family.under_13, |ui| {
                    ui.selectable_value(&mut edit.chat, Chat::Standard, "Standard")
                });
            });
            ui.end_row();
            ui.label("Messages");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut edit.messages, Messages::Everyone, "Everyone");
                ui.selectable_value(&mut edit.messages, Messages::Friends, "Friends");
                ui.selectable_value(&mut edit.messages, Messages::Off, "Off");
            });
            ui.end_row();
            ui.label("Friend requests");
            ui.checkbox(&mut edit.friend_requests, "");
            ui.end_row();
            limit_row(ui, "Playtime a day", &mut edit.playtime_mins, 60, 1440, " min");
            limit_row(ui, "Spending a month", &mut edit.spend_limit, 0, 100_000, "");
            ui.label("New PIN");
            ui.add(
                egui::TextEdit::singleline(&mut self.new_pin)
                    .password(true)
                    .char_limit(4)
                    .desired_width(50.0),
            );
            ui.end_row();
        });
        if let Some(mins) = edit.playtime_mins {
            ui.label(
                RichText::new(format!("Played today: {} of {mins} min", family.played_mins))
                    .color(crate::theme::MUTED),
            );
        }
        self.edit = Some(edit);
        ui.horizontal(|ui| {
            let new_ok = self.new_pin.is_empty() || pin_ok(&self.new_pin);
            if ui.add_enabled(new_ok && !self.busy, egui::Button::new("Save")).clicked() {
                let pin = self.pin.clone();
                let new_pin = Some(std::mem::take(&mut self.new_pin)).filter(|p| !p.is_empty());
                if let Some(p) = &new_pin {
                    self.pin = p.clone();
                }
                self.run(move || {
                    let me = crate::identity()?;
                    let mut c = FamilyChange {
                        player_id: me.player_id(),
                        pin,
                        new_pin,
                        settings: edit,
                        time: libroxel_hub_now_ms(),
                        signature: String::new(),
                    };
                    c.signature = me.sign(&libroxel_hub::family_message(&c));
                    client.change_family(&c)
                });
                self.status = None;
            }
            if ui.button("Close").clicked() {
                self.edit = None;
                self.pin.clear();
                self.new_pin.clear();
            }
            if self.busy {
                ui.spinner();
            }
        });
        if matches!(&self.status, Some(Err(_))) {
            self.edit = Some(edit);
        }
    }
}

fn limit_row(ui: &mut egui::Ui, label: &str, value: &mut Option<u32>, start: u32, most: u32, suffix: &str) {
    ui.label(label);
    ui.horizontal(|ui| {
        let mut on = value.is_some();
        if ui.checkbox(&mut on, "").changed() {
            *value = on.then_some(start);
        }
        if let Some(v) = value {
            ui.add(egui::DragValue::new(v).range(0..=most).suffix(suffix));
        }
    });
    ui.end_row();
}

pub fn set_birthday(
    client: &HubClient,
    born: String,
    parent_email: Option<String>,
) -> Result<Family, String> {
    let me = crate::identity()?;
    let mut b = Birthday {
        player_id: me.player_id(),
        born,
        parent_email,
        utc_offset_mins: 0,
        time: libroxel_hub_now_ms(),
        signature: String::new(),
    };
    b.signature = me.sign(&libroxel_hub::birthday_message(&b));
    client.set_birthday(&b)
}
