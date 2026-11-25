//! On-screen controls for touch screens, shown while playing once the
//! screen has been touched (or View → Touch controls is on), laid out as
//! phone games usually are: a thumb put down anywhere in the bottom left
//! brings up a stick there and walks, a big button in the bottom right
//! jumps, dragging anywhere else looks round, pinching zooms, and tapping
//! clicks (as the mouse does). Everything grows with the screen, so a
//! thumb reaches it on a phone and a tablet alike. The game's own buttons
//! keep their taps, even inside the stick's corner.

use std::collections::HashMap;

use bevy_egui::egui::{self, Color32, Pos2, Rect, Stroke, TouchPhase, Vec2};

/// How big the controls are for a view: 1 on a screen 450 points high.
fn scale(view: Rect) -> f32 {
    (view.width().min(view.height()) / 450.0).clamp(0.85, 1.4)
}

/// How far the stick's knob goes from the middle, in points.
pub fn stick_radius(view: Rect) -> f32 {
    56.0 * scale(view)
}

/// The jump button's radius, in points.
pub fn jump_radius(view: Rect) -> f32 {
    46.0 * scale(view)
}

/// How far in from the corner the stick rests and the jump button sits.
fn margin(view: Rect) -> f32 {
    30.0 * scale(view)
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Finger {
    /// Walking: where the finger went down, and where it is now.
    Stick {
        origin: Pos2,
        at: Pos2,
    },
    Jump,
    /// On one of the game's action buttons (which one is in `held`).
    Action,
    Look {
        at: Pos2,
    },
    /// On one of the game's buttons: the button has it.
    Gui,
}

/// The fingers on the screen and what each is doing.
#[derive(Debug, Default)]
pub struct TouchPad {
    fingers: HashMap<u64, Finger>,
    /// Someone has touched the screen (the controls show from then on).
    pub seen: bool,
    look: Vec2,
    zoom: f32,
    /// A finger on the stick or the jump button came up this frame, so
    /// its tap isn't also a click on the world.
    released: bool,
    /// The game hid the stick and jump button (every finger then looks
    /// round, or presses the game's buttons).
    pub hidden: bool,
    /// The game's action buttons this frame (see [`lay_out_actions`]).
    pub actions: Vec<ActionButton>,
    /// Which action each finger on an action button holds down.
    held: HashMap<u64, String>,
    /// Action buttons pressed (true) or let go since last taken.
    presses: Vec<(String, bool)>,
}

/// A button a game asked for with ContextActionService:BindAction: its
/// action, what it says, and where its middle is.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionButton {
    pub name: String,
    pub label: String,
    pub center: Pos2,
}

/// An action button's radius, in points: a little smaller than jump's.
pub fn action_radius(view: Rect) -> f32 {
    jump_radius(view) * 0.72
}

/// Where the game's action buttons go: where a script put one with
/// SetPosition (its corner, as a UDim2 across the view), and the rest in
/// arcs above and left of the jump button, where the thumb already is, as
/// the reference platform lays them out.
pub fn lay_out_actions(view: Rect, buttons: &[playlibre_script::TouchButton]) -> Vec<ActionButton> {
    let (jump, jr, r) = (jump_center(view), jump_radius(view), action_radius(view));
    let gap = 12.0 * scale(view);
    let mut free = 0;
    buttons
        .iter()
        .map(|b| {
            let center = match b.position {
                Some([sx, ox, sy, oy]) => {
                    Pos2::new(view.min.x + sx * view.width() + ox, view.min.y + sy * view.height() + oy)
                        + Vec2::splat(r)
                }
                None => {
                    // Three on the first arc, four on the next, and so on.
                    let (ring, slot) =
                        if free < 3 { (0, free) } else { (1 + (free - 3) / 4, (free - 3) % 4) };
                    free += 1;
                    let per_ring = if ring == 0 { 3.0 } else { 4.0 };
                    let distance = jr + r + gap + ring as f32 * (2.0 * r + gap);
                    let angle = (180.0 - slot as f32 * 90.0 / (per_ring - 1.0)).to_radians();
                    jump + Vec2::new(angle.cos(), -angle.sin()) * distance
                }
            };
            let label = b.title.clone().unwrap_or_else(|| b.name.clone());
            ActionButton { name: b.name.clone(), label: label.chars().take(10).collect(), center }
        })
        .collect()
}

/// A finger's touch this frame, for scripts: its number, what it did,
/// where, and whether the controls or the game's buttons took it.
pub type Touched = (u64, TouchPhase, Pos2, bool);

/// Where the stick rests in `view` when no thumb is on it.
pub fn stick_home(view: Rect) -> Pos2 {
    let r = stick_radius(view);
    Pos2::new(view.min.x + margin(view) + r, view.max.y - margin(view) - r)
}

pub fn jump_center(view: Rect) -> Pos2 {
    let r = jump_radius(view);
    Pos2::new(view.max.x - margin(view) - r * 1.2, view.max.y - margin(view) - r * 1.2)
}

/// Where a thumb going down brings up the stick: the bottom left, two
/// fifths of the way across and the lower two thirds of the way up.
pub fn stick_zone(view: Rect) -> Rect {
    Rect::from_min_max(
        Pos2::new(view.min.x, view.min.y + view.height() / 3.0),
        Pos2::new(view.min.x + view.width() * 0.4, view.max.y),
    )
}

impl TouchPad {
    /// Takes one touch event in a view of `view`. `on_gui` says whether a
    /// point is on one of the game's buttons or text boxes. Returns whether
    /// the controls or the game's buttons took the finger.
    pub fn touch(
        &mut self,
        id: u64,
        phase: TouchPhase,
        pos: Pos2,
        view: Rect,
        on_gui: impl Fn(Pos2) -> bool,
    ) -> bool {
        let taken = |f: Option<&Finger>| {
            matches!(f, Some(Finger::Stick { .. } | Finger::Jump | Finger::Action | Finger::Gui))
        };
        let action_at = |pos: Pos2| {
            self.actions.iter().find(|b| pos.distance(b.center) <= action_radius(view) * 1.15).cloned()
        };
        match phase {
            TouchPhase::Start => {
                self.seen = true;
                let action = if self.hidden { None } else { action_at(pos) };
                let finger = if !self.hidden && pos.distance(jump_center(view)) <= jump_radius(view) * 1.35 {
                    Finger::Jump
                } else if let Some(button) = action {
                    self.presses.push((button.name.clone(), true));
                    self.held.insert(id, button.name);
                    Finger::Action
                } else if on_gui(pos) {
                    Finger::Gui
                } else if !self.hidden
                    && stick_zone(view).contains(pos)
                    && !self.fingers.values().any(|f| matches!(f, Finger::Stick { .. }))
                {
                    Finger::Stick { origin: pos, at: pos }
                } else {
                    Finger::Look { at: pos }
                };
                self.fingers.insert(id, finger);
                taken(Some(&finger))
            }
            TouchPhase::Move => {
                let pinch_before = self.pinch_width();
                let moved = match self.fingers.get_mut(&id) {
                    Some(Finger::Gui | Finger::Jump | Finger::Action) => return true,
                    None => return false,
                    Some(Finger::Stick { origin, at }) => {
                        *at = pos;
                        // Past the edge, the stick follows the thumb, so
                        // turning back doesn't need the whole way.
                        let r = stick_radius(view);
                        let d = *at - *origin;
                        if d.length() > r {
                            *origin = *at - d.normalized() * r;
                        }
                        return true;
                    }
                    Some(Finger::Look { at }) => {
                        let moved = pos - *at;
                        *at = pos;
                        moved
                    }
                };
                match (pinch_before, self.pinch_width()) {
                    (Some(a), Some(b)) => self.zoom += (b - a) / 40.0,
                    _ => self.look += moved,
                }
                false
            }
            TouchPhase::End | TouchPhase::Cancel => {
                let finger = self.fingers.remove(&id);
                if matches!(finger, Some(Finger::Stick { .. } | Finger::Jump | Finger::Action)) {
                    self.released = true;
                }
                if let Some(name) = self.held.remove(&id) {
                    self.presses.push((name, false));
                }
                taken(finger.as_ref())
            }
        }
    }

    /// With exactly two fingers looking, how far apart they are.
    fn pinch_width(&self) -> Option<f32> {
        let mut looking = self.fingers.values().filter_map(|f| match f {
            Finger::Look { at } => Some(*at),
            _ => None,
        });
        let (a, b) = (looking.next()?, looking.next()?);
        looking.next().is_none().then(|| a.distance(b))
    }

    /// The stick's push in `view`: x to the right, y forwards (up the
    /// screen), at most 1 long.
    pub fn stick(&self, view: Rect) -> Vec2 {
        self.fingers
            .values()
            .find_map(|f| match f {
                Finger::Stick { origin, at } => {
                    let d = (*at - *origin) / stick_radius(view);
                    let d = if d.length() > 1.0 { d.normalized() } else { d };
                    Some(Vec2::new(d.x, -d.y))
                }
                _ => None,
            })
            .unwrap_or(Vec2::ZERO)
    }

    /// The jump button is held.
    pub fn jump(&self) -> bool {
        self.fingers.values().any(|f| *f == Finger::Jump)
    }

    /// Whether a finger is on the stick or jump button (or just left it),
    /// so the press isn't a click on the world as well.
    pub fn has_pointer(&self) -> bool {
        self.released
            || self
                .fingers
                .values()
                .any(|f| matches!(f, Finger::Stick { .. } | Finger::Jump | Finger::Action))
    }

    /// The action buttons pressed (true) or let go since last asked.
    pub fn take_presses(&mut self) -> Vec<(String, bool)> {
        std::mem::take(&mut self.presses)
    }

    /// How far fingers dragged to look since last asked, in points.
    pub fn take_look(&mut self) -> Vec2 {
        std::mem::take(&mut self.look)
    }

    /// How much pinching zoomed since last asked, in mouse-wheel lines.
    pub fn take_zoom(&mut self) -> f32 {
        std::mem::take(&mut self.zoom)
    }

    /// Lets go of every finger (when the game stops).
    pub fn reset(&mut self) {
        self.fingers.clear();
        self.held.clear();
        self.presses.clear();
        self.look = Vec2::ZERO;
        self.zoom = 0.0;
        self.released = false;
    }

    /// Reads this frame's touches, then draws the controls over `view`.
    /// `gui` holds where the game's buttons and text boxes are, which keep
    /// their taps. Returns this frame's touches, for scripts.
    pub fn show(&mut self, ctx: &egui::Context, view: Rect, gui: &[Rect]) -> Vec<Touched> {
        self.released = false;
        let touches: Vec<(u64, TouchPhase, Pos2)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Touch { id, phase, pos, .. } => Some((id.0, *phase, *pos)),
                    _ => None,
                })
                .collect()
        });
        let mut touched = Vec::with_capacity(touches.len());
        for (id, phase, pos) in touches {
            let taken = self.touch(id, phase, pos, view, |p| gui.iter().any(|r| r.contains(p)));
            touched.push((id, phase, pos, taken));
        }
        if !self.seen || self.hidden {
            return touched;
        }
        let (r, jr) = (stick_radius(view), jump_radius(view));
        let home = stick_home(view);
        let (base, knob, held) = match self.fingers.values().find(|f| matches!(f, Finger::Stick { .. })) {
            Some(Finger::Stick { origin, at }) => {
                let d = *at - *origin;
                (*origin, *origin + if d.length() > r { d.normalized() * r } else { d }, true)
            }
            _ => (home, home, false),
        };
        let jump = jump_center(view);
        // The jump button takes its taps from the world and the game alike.
        let jump_rect = Rect::from_center_size(jump, Vec2::splat(jr * 2.7));
        egui::Area::new(egui::Id::new("touch jump"))
            .order(egui::Order::Foreground)
            .fixed_pos(jump_rect.min)
            .show(ctx, |ui| {
                ui.allocate_rect(jump_rect, egui::Sense::click_and_drag());
            });
        let painter =
            ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("touch controls")));
        // Faint while resting, clearer under a thumb.
        let (ring, fill) = if held { (150, 50) } else { (90, 25) };
        painter.circle(
            base,
            r,
            Color32::from_white_alpha(fill),
            Stroke::new(2.5, Color32::from_white_alpha(ring)),
        );
        painter.circle_filled(knob, r * 0.45, Color32::from_white_alpha(if held { 190 } else { 120 }));
        let pressed = self.jump();
        painter.circle(
            jump,
            jr,
            Color32::from_white_alpha(if pressed { 120 } else { 35 }),
            Stroke::new(2.5, Color32::from_white_alpha(140)),
        );
        painter.text(
            jump,
            egui::Align2::CENTER_CENTER,
            crate::theme::icon::ARROW_FAT_UP,
            egui::FontId::proportional(jr * 0.75),
            Color32::WHITE,
        );
        // The game's action buttons, lit while held.
        let ar = action_radius(view);
        for button in &self.actions {
            let rect = Rect::from_center_size(button.center, Vec2::splat(ar * 2.3));
            egui::Area::new(egui::Id::new(("touch action", &button.name)))
                .order(egui::Order::Foreground)
                .fixed_pos(rect.min)
                .show(ctx, |ui| {
                    ui.allocate_rect(rect, egui::Sense::click_and_drag());
                });
            let pressed = self.held.values().any(|n| *n == button.name);
            painter.circle(
                button.center,
                ar,
                Color32::from_white_alpha(if pressed { 120 } else { 35 }),
                Stroke::new(2.5, Color32::from_white_alpha(140)),
            );
            painter.text(
                button.center,
                egui::Align2::CENTER_CENTER,
                &button.label,
                egui::FontId::proportional((ar * 0.42).clamp(11.0, 18.0)),
                Color32::WHITE,
            );
        }
        touched
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> Rect {
        Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 600.0))
    }

    #[test]
    fn a_finger_in_the_bottom_left_walks_and_one_on_the_button_jumps() {
        let mut pad = TouchPad::default();
        let v = view();
        assert!(!pad.seen);
        pad.touch(1, TouchPhase::Start, Pos2::new(120.0, 480.0), v, |_| false);
        assert!(pad.seen);
        assert_eq!(pad.stick(v), Vec2::ZERO);
        // Up the screen is forwards; past the edge it stays at full push.
        pad.touch(1, TouchPhase::Move, Pos2::new(120.0, 480.0 - stick_radius(v) * 3.0), v, |_| false);
        assert!((pad.stick(v) - Vec2::new(0.0, 1.0)).length() < 1e-5, "{:?}", pad.stick(v));
        pad.touch(2, TouchPhase::Start, jump_center(v), v, |_| false);
        assert!(pad.jump());
        pad.touch(2, TouchPhase::End, jump_center(v), v, |_| false);
        assert!(!pad.jump());
        pad.touch(1, TouchPhase::End, Pos2::new(120.0, 300.0), v, |_| false);
        assert_eq!(pad.stick(v), Vec2::ZERO);
        assert_eq!(pad.take_look(), Vec2::ZERO, "walking doesn't look round");
    }

    #[test]
    fn dragging_elsewhere_looks_and_pinching_zooms() {
        let mut pad = TouchPad::default();
        let v = view();
        pad.touch(7, TouchPhase::Start, Pos2::new(600.0, 200.0), v, |_| false);
        pad.touch(7, TouchPhase::Move, Pos2::new(650.0, 190.0), v, |_| false);
        assert_eq!(pad.take_look(), Vec2::new(50.0, -10.0));
        assert_eq!(pad.take_look(), Vec2::ZERO);
        // A second finger: spreading them apart zooms in, without looking.
        pad.touch(8, TouchPhase::Start, Pos2::new(750.0, 190.0), v, |_| false);
        pad.touch(8, TouchPhase::Move, Pos2::new(830.0, 190.0), v, |_| false);
        assert_eq!(pad.take_look(), Vec2::ZERO);
        assert!((pad.take_zoom() - 2.0).abs() < 1e-5);
        assert_eq!(pad.stick(v), Vec2::ZERO);
        assert!(!pad.jump());
    }

    #[test]
    fn the_stick_comes_up_under_the_thumb_and_follows_it() {
        let mut pad = TouchPad::default();
        let v = view();
        // Well away from where the stick rests, but still in its corner.
        let down = Pos2::new(300.0, 350.0);
        assert!(stick_zone(v).contains(down));
        pad.touch(1, TouchPhase::Start, down, v, |_| false);
        assert!(pad.has_pointer(), "the stick's tap isn't a click on the world");
        // Far to the right, then back: the stick turned round straight away.
        pad.touch(1, TouchPhase::Move, down + Vec2::new(stick_radius(v) * 4.0, 0.0), v, |_| false);
        assert!((pad.stick(v) - Vec2::new(1.0, 0.0)).length() < 1e-5);
        pad.touch(1, TouchPhase::Move, down + Vec2::new(stick_radius(v) * 2.0, 0.0), v, |_| false);
        assert!(pad.stick(v).x < -0.99, "{:?}", pad.stick(v));
    }

    #[test]
    fn the_games_buttons_keep_their_taps() {
        let mut pad = TouchPad::default();
        let v = view();
        let button = Rect::from_min_size(Pos2::new(100.0, 400.0), Vec2::splat(80.0));
        pad.touch(1, TouchPhase::Start, Pos2::new(140.0, 440.0), v, |p| button.contains(p));
        pad.touch(1, TouchPhase::Move, Pos2::new(180.0, 440.0), v, |p| button.contains(p));
        assert_eq!(pad.stick(v), Vec2::ZERO);
        assert_eq!(pad.take_look(), Vec2::ZERO);
        assert!(!pad.has_pointer());
    }

    #[test]
    fn action_buttons_sit_by_the_jump_button_and_press_their_action() {
        let v = view();
        let button = |name: &str, position| playlibre_script::TouchButton {
            name: name.to_owned(),
            title: None,
            image: None,
            position,
        };
        let actions = lay_out_actions(
            v,
            &[button("Sprint", None), button("Attack", None), button("Menu", Some([0.0, 10.0, 0.0, 10.0]))],
        );
        // Near the jump button, not on it, nor on each other, and on screen.
        let (a, b) = (actions[0].center, actions[1].center);
        let reach = jump_radius(v) + action_radius(v);
        for p in [a, b] {
            assert!(
                v.contains(p)
                    && p.distance(jump_center(v)) >= reach
                    && p.distance(jump_center(v)) < reach * 2.0
            );
        }
        assert!(a.distance(b) >= action_radius(v) * 2.0);
        // Where the script put it.
        assert_eq!(actions[2].center, Pos2::new(10.0, 10.0) + Vec2::splat(action_radius(v)));

        let mut pad = TouchPad { actions, ..Default::default() };
        assert!(pad.touch(3, TouchPhase::Start, a, v, |_| false));
        assert!(pad.has_pointer(), "pressing it doesn't click the world");
        pad.touch(3, TouchPhase::End, a, v, |_| false);
        assert_eq!(pad.take_presses(), vec![("Sprint".to_owned(), true), ("Sprint".to_owned(), false)]);
    }

    #[test]
    fn a_game_can_hide_the_stick_and_jump_button() {
        let mut pad = TouchPad { hidden: true, ..Default::default() };
        let v = view();
        assert!(!pad.touch(1, TouchPhase::Start, jump_center(v), v, |_| false));
        assert!(!pad.jump(), "no jump button to press");
        pad.touch(1, TouchPhase::End, jump_center(v), v, |_| false);
        pad.touch(2, TouchPhase::Start, Pos2::new(120.0, 480.0), v, |_| false);
        pad.touch(2, TouchPhase::Move, Pos2::new(160.0, 480.0), v, |_| false);
        assert_eq!(pad.stick(v), Vec2::ZERO, "no stick either: the finger looks round");
        assert_eq!(pad.take_look(), Vec2::new(40.0, 0.0));
    }

    #[test]
    fn controls_grow_with_the_screen() {
        let phone = Rect::from_min_size(Pos2::ZERO, Vec2::new(740.0, 360.0));
        let tablet = Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0));
        assert!(jump_radius(tablet) > jump_radius(phone));
        // A thumb reaches the jump button on a phone: no smaller than a fingertip.
        assert!(jump_radius(phone) * 2.0 >= 72.0);
        for v in [phone, tablet] {
            assert!(v.contains(jump_center(v)) && !stick_zone(v).contains(jump_center(v)));
        }
    }
}
