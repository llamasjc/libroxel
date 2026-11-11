//! Draws screen GUIs (frames, labels and buttons) over the 3D view.
//!
//! While playing, these are the player's own copies; while editing, the
//! starter GUIs are shown as a preview so they can be laid out and selected.

use bevy_egui::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke, StrokeKind};
use libroxel_instance::{ClassKind, Color3, InstanceId, InstanceTree, Property, PropertyValue, UDim2, Vec2};

/// The GUI object under the pointer, if any, and its class.
pub type GuiHit = Option<(InstanceId, ClassKind)>;

fn color(c: Color3, alpha: f32) -> Color32 {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(byte(c.r), byte(c.g), byte(c.b), byte(alpha))
}

fn udim2(tree: &InstanceTree, id: InstanceId, property: Property) -> UDim2 {
    match tree.get(id, property) {
        Ok(PropertyValue::UDim2(u)) => u,
        _ => UDim2::default(),
    }
}

fn vec2(tree: &InstanceTree, id: InstanceId, property: Property) -> Vec2 {
    match tree.get(id, property) {
        Ok(PropertyValue::Vec2(v)) => v,
        _ => Vec2::ZERO,
    }
}

/// Where a GUI object sits inside its parent's rectangle.
pub fn layout(tree: &InstanceTree, id: InstanceId, parent: Rect) -> Rect {
    let parent_size = Vec2::new(parent.width(), parent.height());
    let size = udim2(tree, id, Property::GuiSize).resolve(parent_size);
    let pos = udim2(tree, id, Property::GuiPosition).resolve(parent_size);
    let anchor = vec2(tree, id, Property::AnchorPoint);
    let min = Pos2::new(parent.min.x + pos.x - anchor.x * size.x, parent.min.y + pos.y - anchor.y * size.y);
    Rect::from_min_size(min, egui::vec2(size.x.max(0.0), size.y.max(0.0)))
}

/// Draws every enabled screen GUI inside `container` over `area`, and
/// returns the topmost GUI object under the pointer.
pub fn draw(
    ui: &egui::Ui,
    tree: &InstanceTree,
    container: InstanceId,
    area: Rect,
    selected: Option<InstanceId>,
) -> GuiHit {
    let pointer = ui.ctx().input(|i| i.pointer.hover_pos()).filter(|p| area.contains(*p));
    let painter = ui.painter().with_clip_rect(area);
    let mut hit = None;
    for &screen in tree.children(container).unwrap_or_default() {
        if tree.class(screen) != Ok(ClassKind::ScreenGui)
            || tree.get_bool(screen, Property::Enabled) == Some(false)
        {
            continue;
        }
        draw_children(&painter, tree, screen, area, pointer, selected, &mut hit);
    }
    hit
}

fn draw_children(
    painter: &egui::Painter,
    tree: &InstanceTree,
    parent: InstanceId,
    rect: Rect,
    pointer: Option<Pos2>,
    selected: Option<InstanceId>,
    hit: &mut GuiHit,
) {
    for &child in tree.children(parent).unwrap_or_default() {
        let Ok(class) = tree.class(child) else { continue };
        if !class.is_gui_object() || tree.get_bool(child, Property::Visible) == Some(false) {
            continue;
        }
        let r = layout(tree, child, rect);
        let hovered = pointer.is_some_and(|p| r.contains(p));
        if hovered {
            *hit = Some((child, class));
        }

        let alpha = 1.0 - tree.get_number(child, Property::BackgroundTransparency).unwrap_or(0.0) as f32;
        let mut background = tree.get_color(child, Property::BackgroundColor).unwrap_or(Color3::WHITE);
        if class == ClassKind::TextButton && hovered {
            background = background.lerp(Color3::new(0.0, 0.0, 0.0), 0.15);
        }
        if alpha > 0.0 {
            painter.rect_filled(r, 0.0, color(background, alpha));
        }
        if let Some(text) = tree.get_string(child, Property::Text).filter(|t| !t.is_empty()) {
            let size = tree.get_number(child, Property::TextSize).unwrap_or(14.0).max(1.0) as f32;
            let text_color =
                tree.get_color(child, Property::TextColor).unwrap_or(Color3::new(0.1, 0.16, 0.2));
            painter.with_clip_rect(r.intersect(painter.clip_rect())).text(
                r.center(),
                Align2::CENTER_CENTER,
                text,
                FontId::proportional(size),
                color(text_color, 1.0),
            );
        }
        if selected == Some(child) {
            painter.rect_stroke(
                r,
                0.0,
                Stroke::new(2.0, Color32::from_rgb(80, 170, 255)),
                StrokeKind::Outside,
            );
        }
        draw_children(painter, tree, child, r, pointer, selected, hit);
    }
}
