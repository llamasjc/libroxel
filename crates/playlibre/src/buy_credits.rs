use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy_egui::egui;
use playlibre_hub::{Checkout, CheckoutUrl, HubClient, Packs};

use crate::i18n::{t, tf};

type Slot<T> = Arc<Mutex<Option<Result<T, String>>>>;

fn put<T: Send + 'static>(work: impl FnOnce() -> Result<T, String> + Send + 'static) -> Slot<T> {
    let slot: Slot<T> = Arc::default();
    let put = slot.clone();
    std::thread::spawn(move || *put.lock().unwrap() = Some(work()));
    slot
}

const WATCH_FOR: Duration = Duration::from_secs(10 * 60);

#[derive(Default)]
pub struct BuyCredits {
    open: bool,
    packs: Option<Result<Packs, String>>,
    loading: Option<Slot<Packs>>,
    going: Option<Slot<CheckoutUrl>>,
    error: Option<String>,
    paying_since: Option<Instant>,
    clock: i64,
}

impl BuyCredits {
    pub fn open(&mut self) {
        self.open = true;
        self.error = None;
        if !matches!(self.packs, Some(Ok(_))) {
            self.packs = None;
        }
    }

    pub fn paying(&self) -> bool {
        self.paying_since.is_some_and(|t| t.elapsed() < WATCH_FOR)
    }

    pub fn ui(&mut self, ctx: &egui::Context, hub: &HubClient, credits: Option<i64>) {
        if let Some(found) = self.loading.as_ref().and_then(|s| s.lock().unwrap().take()) {
            self.packs = Some(found);
            self.loading = None;
        }
        if let Some(found) = self.going.as_ref().and_then(|s| s.lock().unwrap().take()) {
            self.going = None;
            match found {
                Ok(c) => {
                    crate::report::open_url(&c.url);
                    self.paying_since = Some(Instant::now());
                    self.open = false;
                }
                Err(e) => self.error = Some(e),
            }
        }
        if !self.open {
            return;
        }
        if self.packs.is_none() && self.loading.is_none() {
            let hub = hub.clone();
            self.loading = Some(put(move || hub.credit_packs()));
        }
        let mut pick = None;
        let busy = self.going.is_some();
        let shown = egui::Modal::new(egui::Id::new("buy-credits")).show(ctx, |ui| {
            ui.set_max_width(420.0);
            ui.heading(t("Buy Credits"));
            if let Some(have) = credits {
                ui.weak(tf("You have {credits} Credits", &[("credits", &have)]));
            }
            ui.add_space(8.0);
            match &self.packs {
                None => {
                    ui.spinner();
                }
                Some(Err(e)) => {
                    ui.colored_label(egui::Color32::from_rgb(220, 90, 80), e);
                }
                Some(Ok(p)) if !p.on => {
                    ui.weak(t("Not available yet."));
                }
                Some(Ok(p)) => {
                    egui::Grid::new("credit-packs").spacing([8.0, 8.0]).show(ui, |ui| {
                        for (i, pack) in p.packs.iter().enumerate() {
                            let label = format!(
                                "{}  {}\n${}.{:02}",
                                crate::theme::icon::COIN,
                                pack.credits,
                                pack.cents / 100,
                                pack.cents % 100
                            );
                            let button = egui::Button::new(label).min_size(egui::vec2(124.0, 52.0));
                            if ui.add_enabled(!busy, button).clicked() {
                                pick = Some(pack.id);
                            }
                            if i % 3 == 2 {
                                ui.end_row();
                            }
                        }
                    });
                }
            }
            if let Some(e) = &self.error {
                ui.colored_label(egui::Color32::from_rgb(220, 90, 80), e);
            }
            ui.add_space(8.0);
            if ui.button(t("Close")).clicked() {
                ui.close();
            }
        });
        if shown.should_close() {
            self.open = false;
        }
        if let Some(pack) = pick {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as i64);
            self.clock = now.max(self.clock + 1);
            self.error = None;
            let (time, hub) = (self.clock, hub.clone());
            self.going = Some(put(move || {
                let me = crate::identity()?;
                let player_id = me.player_id();
                let signature = me.sign(&playlibre_hub::checkout_message(&player_id, pack, time));
                hub.checkout(&Checkout { player_id, pack, time, signature })
            }));
        }
    }
}
