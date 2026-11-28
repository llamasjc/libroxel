//! Game passes and developer products inside a game: `UserOwnsGamePassAsync`
//! answers from the Hub, and `PromptGamePassPurchase` and
//! `PromptProductPurchase` ask the player, whose own copy of the app buys
//! with their Credits (only they can sign for them). A product's receipt
//! goes to `ProcessReceipt` until a script grants it.
//!
//! Whoever hosts the game (a server, or the app when it plays a published
//! game itself) runs [`HostPasses`]. Each player's app shows the prompt
//! for a [`PassOffer`] and answers with [`PassOffer::answer`].

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy_egui::egui;
use playlibre_engine::PlaySession;
use playlibre_hub::{HubClient, Pass, Product, Receipt};
use playlibre_instance::InstanceId;
use playlibre_instance::serde_json::{Value, json};
use playlibre_net::Identity;

type Slot<T> = Arc<Mutex<Option<Result<T, String>>>>;

fn put<T: Send + 'static>(work: impl FnOnce() -> Result<T, String> + Send + 'static) -> Slot<T> {
    let slot: Slot<T> = Arc::default();
    let put = slot.clone();
    std::thread::spawn(move || *put.lock().unwrap() = Some(work()));
    slot
}

fn take<T>(slot: &Slot<T>) -> Option<Result<T, String>> {
    slot.lock().unwrap().take()
}

/// The host's side: looks up passes and checks purchases.
pub struct HostPasses {
    hub: HubClient,
    game_id: String,
    info: Option<Slot<Vec<Pass>>>,
    passes: Vec<Pass>,
    /// Players whose passes are being (or have been) looked up.
    looked_up: HashMap<InstanceId, Option<Slot<Vec<i64>>>>,
    /// Purchases a player said they made, being checked with the Hub.
    checks: Vec<(InstanceId, i64, Slot<bool>)>,
    identity: Option<Identity>,
    product_info: Option<Slot<Vec<Product>>>,
    products: Vec<Product>,
    /// Players whose waiting receipts are being looked up.
    receipt_lookups: Vec<(InstanceId, Slot<Vec<Receipt>>)>,
    /// Product purchases a player said they made: player, product, receipt.
    product_checks: Vec<(InstanceId, i64, i64, Slot<Vec<Receipt>>)>,
    receipts: Vec<Waiting>,
}

/// A receipt not yet granted, or granted but not yet told to the Hub.
struct Waiting {
    player: InstanceId,
    receipt: Receipt,
    granted: bool,
    next: Instant,
    telling: Option<Slot<()>>,
}

const RETRY: Duration = Duration::from_secs(10);

impl HostPasses {
    /// For a published game (not a starter game, which sells nothing).
    pub fn new(hub: HubClient, game_id: &str) -> Option<Self> {
        if game_id.is_empty() || game_id.starts_with("starter:") {
            return None;
        }
        let (client, id) = (hub.clone(), game_id.to_owned());
        let (client2, id2) = (hub.clone(), game_id.to_owned());
        Some(Self {
            info: Some(put(move || client.passes(&id, None))),
            product_info: Some(put(move || client2.products(&id2))),
            hub,
            game_id: game_id.to_owned(),
            passes: Vec::new(),
            looked_up: HashMap::new(),
            checks: Vec::new(),
            identity: None,
            products: Vec::new(),
            receipt_lookups: Vec::new(),
            product_checks: Vec::new(),
            receipts: Vec::new(),
        })
    }

    /// Who tells the Hub that receipts were granted.
    pub fn with_identity(mut self, identity: Option<Identity>) -> Self {
        self.identity = identity;
        self
    }

    fn account(session: &PlaySession, player: InstanceId, local_account: Option<&str>) -> Option<String> {
        if Some(player) == session.local_player() {
            local_account.map(str::to_owned)
        } else {
            session.account_of(player)
        }
    }

    /// Run every frame. `local_account` is the account of the player on
    /// this computer, when there is one.
    pub fn tick(&mut self, session: &mut PlaySession, local_account: Option<&str>) {
        if let Some(slot) = &self.info
            && let Some(result) = take(slot)
        {
            self.info = None;
            // A Hub that can't say leaves passes as they were: nobody owns
            // any, and prompts close unbought.
            if let Ok(list) = result {
                session.set_passes(
                    list.iter()
                        .map(|p| playlibre_script::PassInfo {
                            id: p.pass_id,
                            name: p.name.clone(),
                            description: p.about.clone(),
                            price: p.price,
                        })
                        .collect(),
                );
                self.passes = list;
            }
        }
        if self.info.is_some() {
            return;
        }
        self.tick_products(session, local_account);
        let local = session.local_player();
        for player in session.players() {
            if self.looked_up.contains_key(&player) {
                continue;
            }
            let account = if Some(player) == local {
                local_account.map(str::to_owned)
            } else {
                session.account_of(player)
            };
            match account {
                Some(account) => {
                    let (hub, game, who) = (self.hub.clone(), self.game_id.clone(), account.clone());
                    self.receipt_lookups.push((player, put(move || hub.waiting_receipts(&game, &who))));
                    let (hub, game) = (self.hub.clone(), self.game_id.clone());
                    let slot = put(move || {
                        hub.passes(&game, Some(&account))
                            .map(|l| l.into_iter().filter(|p| p.owned).map(|p| p.pass_id).collect())
                    });
                    self.looked_up.insert(player, Some(slot));
                }
                // No account: owns nothing.
                None => {
                    session.add_owned_passes(player, Vec::new());
                    self.looked_up.insert(player, None);
                }
            }
        }
        for (player, slot) in &mut self.looked_up {
            if let Some(s) = slot
                && let Some(result) = take(s)
            {
                session.add_owned_passes(*player, result.unwrap_or_default());
                *slot = None;
            }
        }
        let here: Vec<InstanceId> = session.players();
        self.looked_up.retain(|p, _| here.contains(p));

        for (player, pass) in session.take_pass_prompts() {
            let Some(info) = self.passes.iter().find(|p| p.pass_id == pass) else {
                session.pass_prompt_finished(player, pass, false);
                continue;
            };
            session.ask_player(
                player,
                json!({
                    "kind": "pass",
                    "game_id": self.game_id,
                    "id": pass,
                    "name": info.name,
                    "about": info.about,
                    "price": info.price,
                }),
            );
        }
        let mut others = Vec::new();
        for (player, answer) in session.take_player_answers() {
            if answer.get("kind").and_then(Value::as_str) == Some("product") {
                let Some(product) = answer.get("id").and_then(Value::as_i64) else { continue };
                let receipt = answer.get("receipt").and_then(Value::as_i64);
                let bought = answer.get("bought").and_then(Value::as_bool) == Some(true);
                match (Self::account(session, player, local_account), receipt.filter(|_| bought)) {
                    (Some(account), Some(receipt)) => {
                        let (hub, game) = (self.hub.clone(), self.game_id.clone());
                        let slot = put(move || hub.waiting_receipts(&game, &account));
                        self.product_checks.push((player, product, receipt, slot));
                    }
                    _ => session.product_prompt_finished(player, product, false),
                }
                continue;
            }
            if answer.get("kind").and_then(Value::as_str) != Some("pass") {
                others.push((player, answer));
                continue;
            }
            let Some(pass) = answer.get("id").and_then(Value::as_i64) else { continue };
            let bought = answer.get("bought").and_then(Value::as_bool) == Some(true);
            let account = if Some(player) == local {
                local_account.map(str::to_owned)
            } else {
                session.account_of(player)
            };
            match account.filter(|_| bought) {
                // Taken on the player's word only once the Hub agrees.
                Some(account) => {
                    let hub = self.hub.clone();
                    self.checks.push((player, pass, put(move || hub.owns_pass(pass, &account))));
                }
                None => session.pass_prompt_finished(player, pass, false),
            }
        }
        session.put_back_answers(others);
        self.checks.retain(|(player, pass, slot)| match take(slot) {
            Some(result) => {
                session.pass_prompt_finished(*player, *pass, result.unwrap_or(false));
                false
            }
            None => true,
        });
    }
}

impl HostPasses {
    fn tick_products(&mut self, session: &mut PlaySession, local_account: Option<&str>) {
        if let Some(slot) = &self.product_info
            && let Some(result) = take(slot)
        {
            self.product_info = None;
            if let Ok(list) = result {
                session.set_products(
                    list.iter()
                        .map(|p| playlibre_script::ProductInfo {
                            id: p.product_id,
                            name: p.name.clone(),
                            description: p.about.clone(),
                            price: p.price,
                        })
                        .collect(),
                );
                self.products = list;
            }
        }
        for (player, product) in session.take_product_prompts() {
            let Some(info) = self.products.iter().find(|p| p.product_id == product) else {
                session.product_prompt_finished(player, product, false);
                continue;
            };
            session.ask_player(
                player,
                json!({
                    "kind": "product",
                    "game_id": self.game_id,
                    "id": product,
                    "name": info.name,
                    "about": info.about,
                    "price": info.price,
                }),
            );
        }
        let mut found = Vec::new();
        self.receipt_lookups.retain(|(player, slot)| match take(slot) {
            Some(result) => {
                found.extend(result.unwrap_or_default().into_iter().map(|r| (*player, r)));
                false
            }
            None => true,
        });
        self.product_checks.retain(|(player, product, receipt, slot)| match take(slot) {
            Some(result) => {
                let r = result.unwrap_or_default().into_iter().find(|r| r.receipt_id == *receipt);
                let ok = r.as_ref().is_some_and(|r| r.product_id == *product);
                session.product_prompt_finished(*player, *product, ok);
                found.extend(r.filter(|_| ok).map(|r| (*player, r)));
                false
            }
            None => true,
        });
        let now = Instant::now();
        for (player, receipt) in found {
            if !self.receipts.iter().any(|w| w.receipt.receipt_id == receipt.receipt_id) {
                self.receipts.push(Waiting { player, receipt, granted: false, next: now, telling: None });
            }
        }
        for (id, granted) in session.take_receipt_results() {
            if let Some(w) = self.receipts.iter_mut().find(|w| w.receipt.receipt_id == id) {
                w.granted |= granted;
                w.next = if granted { now } else { now + RETRY };
            }
        }
        let here = session.players();
        let identity = self.identity.clone();
        let hub = self.hub.clone();
        self.receipts.retain_mut(|w| {
            if let Some(slot) = &w.telling
                && let Some(result) = take(slot)
            {
                w.telling = None;
                if result.is_ok() {
                    return false;
                }
                w.next = now + RETRY;
            }
            if w.granted {
                if w.telling.is_none() && w.next <= now {
                    let Some(identity) = identity.clone() else { return false };
                    let (hub, id) = (hub.clone(), w.receipt.receipt_id);
                    w.telling = Some(put(move || {
                        let server_id = identity.player_id();
                        let signature = identity.sign(&playlibre_hub::granted_message(&server_id, id));
                        hub.grant_receipt(id, &playlibre_hub::Granted { server_id, signature })
                    }));
                }
                return true;
            }
            if !here.contains(&w.player)
                || Self::account(session, w.player, local_account).as_deref() != Some(&w.receipt.player_id)
            {
                return false;
            }
            if w.next <= now {
                w.next = now + Duration::from_secs(3600);
                session.process_receipt(
                    w.player,
                    w.receipt.receipt_id,
                    w.receipt.product_id,
                    w.receipt.price,
                );
            }
            true
        });
    }
}

/// A pass or product the host asked this player to buy.
#[derive(Debug, Clone, PartialEq)]
pub struct PassOffer {
    pub product: bool,
    pub game_id: String,
    pub pass_id: i64,
    pub name: String,
    pub about: String,
    pub price: u32,
}

impl PassOffer {
    pub fn from_ask(ask: &Value) -> Option<Self> {
        let product = match ask.get("kind")?.as_str()? {
            "pass" => false,
            "product" => true,
            _ => return None,
        };
        Some(Self {
            product,
            game_id: ask.get("game_id")?.as_str()?.chars().take(80).collect(),
            pass_id: ask.get("id")?.as_i64()?,
            name: ask.get("name").and_then(Value::as_str).unwrap_or("").chars().take(60).collect(),
            about: ask.get("about").and_then(Value::as_str).unwrap_or("").chars().take(300).collect(),
            price: ask.get("price").and_then(Value::as_u64).unwrap_or(0) as u32,
        })
    }

    /// The answer to send the host.
    pub fn answer(&self, bought: bool) -> Value {
        json!({ "kind": if self.product { "product" } else { "pass" }, "id": self.pass_id, "bought": bought })
    }

    fn bought_product(&self, receipt: i64) -> Value {
        json!({ "kind": "product", "id": self.pass_id, "bought": true, "receipt": receipt })
    }
}

/// The player's side, in the app: shows the host's purchase prompts and
/// buys passes with the player's Credits.
#[derive(Default)]
pub struct Shop {
    /// The pass being offered, and why the last try failed.
    offer: Option<(PassOffer, Option<String>)>,
    waiting: Vec<PassOffer>,
    /// What was bought: a product's receipt, or nothing for a pass.
    buying: Option<Slot<Option<i64>>>,
    /// The host passes when this app hosts a published game itself.
    host: Option<(String, HostPasses)>,
    clock: i64,
}

impl Shop {
    /// Run every frame with the play session, if there is one.
    pub fn tick(
        &mut self,
        session: Option<&mut PlaySession>,
        hub: &HubClient,
        game_id: Option<&str>,
        account: Option<&str>,
    ) {
        let Some(session) = session else {
            *self = Self { clock: self.clock, ..Self::default() };
            return;
        };
        // Hosting (or playing alone) a published game: this app is the host.
        if !session.is_joined()
            && let Some(id) = game_id
        {
            if self.host.as_ref().is_none_or(|(g, _)| g != id) {
                self.host = HostPasses::new(hub.clone(), id)
                    .map(|h| (id.to_owned(), h.with_identity(crate::identity().ok())));
            }
            if let Some((_, host)) = self.host.as_mut() {
                host.tick(session, account);
            }
        }
        let mut others = Vec::new();
        for ask in session.take_host_asks() {
            match PassOffer::from_ask(&ask) {
                Some(offer) => self.waiting.push(offer),
                None => others.push(ask),
            }
        }
        session.put_back_asks(others);
        if self.offer.is_none() && !self.waiting.is_empty() {
            self.offer = Some((self.waiting.remove(0), None));
        }
        if let Some(slot) = &self.buying
            && let Some(result) = take(slot)
        {
            self.buying = None;
            if let Some((offer, error)) = self.offer.take() {
                let _ = error;
                match result {
                    Ok(Some(receipt)) => session.answer_host(offer.bought_product(receipt)),
                    Ok(None) => session.answer_host(offer.answer(true)),
                    Err(e) if !offer.product && e.contains("already have") => {
                        session.answer_host(offer.answer(true))
                    }
                    Err(e) => self.offer = Some((offer, Some(e))),
                }
            }
        }
    }

    /// Draws the prompt, when there is one. Closing it answers the host.
    pub fn ui(
        &mut self,
        ctx: &egui::Context,
        session: Option<&mut PlaySession>,
        hub: &HubClient,
        credits: Option<i64>,
        signed_in: bool,
    ) {
        use crate::i18n::{t, tf};
        let Some(session) = session else { return };
        let Some((offer, error)) = &self.offer else { return };
        let (offer, error) = (offer.clone(), error.clone());
        let mut buy = false;
        let mut cancel = false;
        egui::Modal::new(egui::Id::new("pass-prompt")).show(ctx, |ui| {
            ui.set_max_width(380.0);
            ui.heading(&offer.name);
            if !offer.about.is_empty() {
                ui.label(&offer.about);
            }
            ui.add_space(6.0);
            ui.label(tf("{price} Credits", &[("price", &offer.price.to_string())]));
            if let Some(have) = credits {
                ui.weak(tf("You have {credits} Credits", &[("credits", &have.to_string())]));
            }
            if let Some(e) = &error {
                ui.colored_label(egui::Color32::from_rgb(220, 90, 80), e);
            }
            if !signed_in {
                ui.weak(t("Sign in to buy."));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let busy = self.buying.is_some();
                let can = signed_in && !busy && credits.is_none_or(|c| c >= offer.price as i64);
                buy = ui
                    .add_enabled(can, egui::Button::new(if busy { t("Buying…") } else { t("Buy") }))
                    .clicked();
                cancel = ui.add_enabled(!busy, egui::Button::new(t("Cancel"))).clicked();
            });
        });
        if cancel {
            self.offer = None;
            session.answer_host(offer.answer(false));
        } else if buy {
            let now = playlibre_hub::hub_now_ms();
            self.clock = now.max(self.clock + 1);
            let (time, hub, id, price) = (self.clock, hub.clone(), offer.pass_id, offer.price);
            let product = offer.product;
            self.buying = Some(put(move || {
                let me = crate::identity()?;
                let player_id = me.player_id();
                if product {
                    let signature = me.sign(&playlibre_hub::buy_product_message(&player_id, id, price, time));
                    let buy = playlibre_hub::BuyProduct { player_id, product_id: id, price, time, signature };
                    return hub.buy_product(&buy).map(|r| Some(r.receipt_id));
                }
                let signature = me.sign(&playlibre_hub::buy_pass_message(&player_id, id, price, time));
                hub.buy_pass(&playlibre_hub::BuyPass { player_id, pass_id: id, price, time, signature })
                    .map(|_| None)
            }));
        }
    }
}
