//! On-screen controls for touch screens, shown while playing once the
//! screen has been touched (or View → Touch controls is on): a stick in the bottom left walks, a button in
//! the bottom right jumps, dragging anywhere else looks round, pinching
//! zooms, and tapping clicks (as the mouse does).

use std::collections::HashMap;

use bevy_egui::egui::{self, Color32, Pos2, Rect, Stroke, TouchPhase, Vec2};

/// How far the stick's knob goes from the middle, in points.
pub const STICK_RADIUS: f32 = 56.0;
/// The jump button's radius, in points.
pub const JUMP_RADIUS: f32 = 44.0;
/// How far in from the corner the stick and jump button sit.
const MARGIN: f32 = 36.0;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Finger {
    /// Walking: where the finger went down, and where it is now.
    Stick {
        origin: Pos2,
        at: Pos2,
    },
    Jump,
    Look {
        at: Pos2,
    },
}

/// The fingers on the screen and what each is doing.
#[derive(Debug, Default)]
pub struct TouchPad {
    fingers: HashMap<u64, Finger>,
    /// Someone has touched the screen (the controls show from then on).
    pub seen: bool,
    look: Vec2,
    zoom: f32,
}

/// Where the controls sit in `view`.
pub fn stick_home(view: Rect) -> Pos2 {
    Pos2::new(view.min.x + MARGIN + STICK_RADIUS, view.max.y - MARGIN - STICK_RADIUS)
}

pub fn jump_center(view: Rect) -> Pos2 {
    Pos2::new(view.max.x - MARGIN - JUMP_RADIUS, view.max.y - MARGIN - JUMP_RADIUS)
}

/// Where a finger going down starts the stick: round its home in the
/// bottom left, kept small so the game's own buttons nearby still work.
pub fn stick_zone(view: Rect) -> Rect {
    Rect::from_center_size(stick_home(view), Vec2::splat(STICK_RADIUS * 3.2))
}

impl TouchPad {
    /// Takes one touch event in a view of `view`.
    pub fn touch(&mut self, id: u64, phase: TouchPhase, pos: Pos2, view: Rect) {
        match phase {
            TouchPhase::Start => {
                self.seen = true;
                let finger = if pos.distance(jump_center(view)) <= JUMP_RADIUS * 1.3 {
                    Finger::Jump
                } else if stick_zone(view).contains(pos) {
                    Finger::Stick { origin: pos, at: pos }
                } else {
                    Finger::Look { at: pos }
                };
                self.fingers.insert(id, finger);
            }
            TouchPhase::Move => {
                let pinch_before = self.pinch_width();
                let moved = match self.fingers.get_mut(&id) {
                    Some(Finger::Stick { at, .. }) => {
                        *at = pos;
                        return;
                    }
                    Some(Finger::Look { at }) => {
                        let moved = pos - *at;
                        *at = pos;
                        moved
                    }
                    _ => return,
                };
                match (pinch_before, self.pinch_width()) {
                    (Some(a), Some(b)) => self.zoom += (b - a) / 40.0,
                    _ => self.look += moved,
                }
            }
            TouchPhase::End | TouchPhase::Cancel => {
                self.fingers.remove(&id);
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

    /// The stick's push: x to the right, y forwards (up the screen), at most 1 long.
    pub fn stick(&self) -> Vec2 {
        self.fingers
            .values()
            .find_map(|f| match f {
                Finger::Stick { origin, at } => {
                    let d = (*at - *origin) / STICK_RADIUS;
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
        self.look = Vec2::ZERO;
        self.zoom = 0.0;
    }

    /// Reads this frame's touches, then draws the controls over `view`.
    /// The stick and jump button take the pointer, so a finger there
    /// doesn't also click the world.
    pub fn show(&mut self, ctx: &egui::Context, view: Rect) {
        let touches: Vec<(u64, TouchPhase, Pos2)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Touch { id, phase, pos, .. } => Some((id.0, *phase, *pos)),
                    _ => None,
                })
                .collect()
        });
        for (id, phase, pos) in touches {
            self.touch(id, phase, pos, view);
        }
        if !self.seen {
            return;
        }
        let home = stick_home(view);
        let (base, knob) = match self.fingers.values().find(|f| matches!(f, Finger::Stick { .. })) {
            Some(Finger::Stick { origin, at }) => {
                let d = *at - *origin;
                (*origin, *origin + if d.length() > STICK_RADIUS { d.normalized() * STICK_RADIUS } else { d })
            }
            _ => (home, home),
        };
        let jump = jump_center(view);
        let zone = stick_zone(view);
        let jump_rect = Rect::from_center_size(jump, Vec2::splat(JUMP_RADIUS * 2.6));
        for (name, rect) in [("touch stick", zone), ("touch jump", jump_rect)] {
            egui::Area::new(egui::Id::new(name)).order(egui::Order::Foreground).fixed_pos(rect.min).show(
                ctx,
                |ui| {
                    ui.allocate_rect(
                        Rect::from_min_size(rect.min, rect.size()),
                        egui::Sense::click_and_drag(),
                    );
                },
            );
        }
        let painter =
            ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("touch controls")));
        let faint = Color32::from_white_alpha(40);
        let line = Stroke::new(2.0, Color32::from_white_alpha(120));
        painter.circle(base, STICK_RADIUS, faint, line);
        painter.circle_filled(knob, STICK_RADIUS * 0.45, Color32::from_white_alpha(150));
        let held = if self.jump() { Color32::from_white_alpha(110) } else { faint };
        painter.circle(jump, JUMP_RADIUS, held, line);
        painter.text(
            jump,
            egui::Align2::CENTER_CENTER,
            "⬆",
            egui::FontId::proportional(30.0),
            Color32::WHITE,
        );
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
        pad.touch(1, TouchPhase::Start, Pos2::new(120.0, 480.0), v);
        assert!(pad.seen);
        assert_eq!(pad.stick(), Vec2::ZERO);
        // Up the screen is forwards; past the edge it stays at full push.
        pad.touch(1, TouchPhase::Move, Pos2::new(120.0, 480.0 - STICK_RADIUS * 3.0), v);
        assert!((pad.stick() - Vec2::new(0.0, 1.0)).length() < 1e-5, "{:?}", pad.stick());
        pad.touch(2, TouchPhase::Start, jump_center(v), v);
        assert!(pad.jump());
        pad.touch(2, TouchPhase::End, jump_center(v), v);
        assert!(!pad.jump());
        pad.touch(1, TouchPhase::End, Pos2::new(120.0, 300.0), v);
        assert_eq!(pad.stick(), Vec2::ZERO);
        assert_eq!(pad.take_look(), Vec2::ZERO, "walking doesn't look round");
    }

    #[test]
    fn dragging_elsewhere_looks_and_pinching_zooms() {
        let mut pad = TouchPad::default();
        let v = view();
        pad.touch(7, TouchPhase::Start, Pos2::new(600.0, 200.0), v);
        pad.touch(7, TouchPhase::Move, Pos2::new(650.0, 190.0), v);
        assert_eq!(pad.take_look(), Vec2::new(50.0, -10.0));
        assert_eq!(pad.take_look(), Vec2::ZERO);
        // A second finger: spreading them apart zooms in, without looking.
        pad.touch(8, TouchPhase::Start, Pos2::new(750.0, 190.0), v);
        pad.touch(8, TouchPhase::Move, Pos2::new(830.0, 190.0), v);
        assert_eq!(pad.take_look(), Vec2::ZERO);
        assert!((pad.take_zoom() - 2.0).abs() < 1e-5);
        assert_eq!(pad.stick(), Vec2::ZERO);
        assert!(!pad.jump());
    }
}
