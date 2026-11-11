//! Draws screen GUIs (frames, labels and buttons) over the 3D view.
//!
//! While playing, these are the player's own copies; while editing, the
//! starter GUIs are shown as a preview so they can be laid out and selected.

use bevy_egui::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke, StrokeKind};
use libroxel_instance::{
    ClassKind, Color3, InstanceId, InstanceTree, Property, PropertyValue, UDim2, Vec2, Vec3,
};

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

/// Draws a proximity prompt bubble centred at `at`: the key to press, what
/// it does and what it's on, and a bar while the key is held. Returns its
/// rectangle, which can be clicked to use the prompt.
pub fn draw_prompt(
    ui: &egui::Ui,
    tree: &InstanceTree,
    prompt: InstanceId,
    key: &str,
    progress: f32,
    at: Pos2,
) -> Rect {
    let action = tree.get_string(prompt, Property::ActionText).unwrap_or_default();
    let object = tree.get_string(prompt, Property::ObjectText).unwrap_or_default();
    let rect = Rect::from_center_size(at, egui::vec2(200.0, 56.0));
    let painter = ui.painter();
    painter.rect_filled(rect, 8.0, Color32::from_black_alpha(170));
    let key_box = Rect::from_center_size(rect.left_center() + egui::vec2(30.0, 0.0), egui::vec2(36.0, 36.0));
    painter.rect_filled(key_box, 6.0, Color32::from_gray(235));
    painter.text(
        key_box.center(),
        Align2::CENTER_CENTER,
        key,
        FontId::proportional(18.0),
        Color32::from_gray(20),
    );
    if progress > 0.0 && progress < 1.0 {
        // A bar along the bottom of the bubble fills up while the key is held.
        let bar = Rect::from_min_size(
            rect.left_bottom() + egui::vec2(8.0, -6.0),
            egui::vec2((rect.width() - 16.0) * progress, 4.0),
        );
        painter.rect_filled(bar, 1.0, Color32::from_rgb(255, 215, 60));
    }
    let text_x = key_box.right() + 12.0;
    if !object.is_empty() {
        painter.text(
            egui::pos2(text_x, rect.center().y - 9.0),
            Align2::LEFT_CENTER,
            &object,
            FontId::proportional(12.0),
            Color32::from_gray(200),
        );
    }
    let action_y = if object.is_empty() { 0.0 } else { 8.0 };
    painter.text(
        egui::pos2(text_x, rect.center().y + action_y),
        Align2::LEFT_CENTER,
        action,
        FontId::proportional(17.0),
        Color32::WHITE,
    );
    rect
}

/// Draws every enabled billboard in the world at its part, facing the
/// camera, and returns the topmost GUI object under the pointer.
pub fn billboards(
    ui: &egui::Ui,
    tree: &InstanceTree,
    camera: (&bevy::prelude::Camera, &bevy::prelude::GlobalTransform),
    selected: Option<InstanceId>,
) -> GuiHit {
    use bevy::prelude::Vec3 as BVec3;
    let (cam, gt) = camera;
    let area = ui.max_rect();
    let pointer = ui.ctx().input(|i| i.pointer.hover_pos()).filter(|p| area.contains(*p));
    let painter = ui.painter().with_clip_rect(area);
    let eye = gt.translation();
    let right = gt.right().as_vec3();
    let mut hit = None;
    for board in tree.descendants_of_class(tree.workspace(), ClassKind::BillboardGui) {
        if tree.get_bool(board, Property::Enabled) == Some(false) {
            continue;
        }
        let Some(part) = tree.parent(board).ok().flatten() else { continue };
        let Some(pos) = tree.get_vec3(part, Property::Position) else { continue };
        let offset = tree.get_vec3(board, Property::StudsOffset).unwrap_or_default();
        let at = pos + offset;
        let world = BVec3::new(at.x, at.y, at.z);
        let distance = (world - eye).length();
        if distance > tree.get_number(board, Property::MaxDistance).unwrap_or(100.0) as f32 {
            continue;
        }
        // Hidden behind other parts unless it's always on top.
        if tree.get_bool(board, Property::AlwaysOnTop) != Some(true) {
            // The part's own model (such as the character a name tag is on) doesn't count.
            let model = tree.parent(part).ok().flatten().filter(|m| tree.class(*m) == Ok(ClassKind::Model));
            let own = |p: InstanceId| {
                p == part || model.is_some_and(|m| tree.is_descendant_of(p, m).unwrap_or(false))
            };
            let toward = at - Vec3::new(eye.x, eye.y, eye.z);
            let blocked = libroxel_instance::geometry::raycast(
                tree,
                Vec3::new(eye.x, eye.y, eye.z),
                toward * 0.98,
                |p| !own(p),
            );
            if blocked.is_some() {
                continue;
            }
        }
        let (Ok(centre), Ok(beside)) =
            (cam.world_to_viewport(gt, world), cam.world_to_viewport(gt, world + right))
        else {
            continue;
        };
        let per_stud = (beside - centre).length();
        let size = udim2(tree, board, Property::GuiSize);
        let size =
            egui::vec2(size.x_scale * per_stud + size.x_offset, size.y_scale * per_stud + size.y_offset);
        let rect = Rect::from_center_size(Pos2::new(centre.x, centre.y), size);
        if selected == Some(board) {
            painter.rect_stroke(
                rect,
                0.0,
                Stroke::new(1.0, Color32::from_rgb(80, 170, 255)),
                StrokeKind::Outside,
            );
        }
        draw_children(&painter, tree, board, rect, pointer, selected, &mut hit);
    }
    hit
}

/// Draws the player's tool slots along the bottom of the view. Returns each
/// slot's rectangle, so clicks can pick a tool.
pub fn hotbar(ui: &egui::Ui, tree: &InstanceTree, tools: &[(InstanceId, bool)]) -> Vec<(usize, Rect)> {
    let area = ui.max_rect();
    let (slot, gap) = (64.0, 8.0);
    let count = tools.len().min(9);
    let width = count as f32 * slot + count.saturating_sub(1) as f32 * gap;
    let mut x = area.center().x - width / 2.0;
    let y = area.bottom() - slot - 20.0;
    let painter = ui.painter();
    let mut rects = Vec::new();
    for (i, (tool, held)) in tools.iter().take(9).enumerate() {
        let rect = Rect::from_min_size(Pos2::new(x, y), egui::vec2(slot, slot));
        x += slot + gap;
        painter.rect_filled(rect, 6.0, Color32::from_black_alpha(if *held { 210 } else { 150 }));
        if *held {
            painter.rect_stroke(
                rect,
                6.0,
                Stroke::new(2.0, Color32::from_rgb(255, 215, 60)),
                StrokeKind::Inside,
            );
        }
        painter.text(
            rect.left_top() + egui::vec2(6.0, 4.0),
            Align2::LEFT_TOP,
            (i + 1).to_string(),
            FontId::proportional(12.0),
            Color32::from_gray(200),
        );
        let name = tree.name(*tool).unwrap_or("?");
        painter.with_clip_rect(rect.shrink(3.0)).text(
            rect.center() + egui::vec2(0.0, 6.0),
            Align2::CENTER_CENTER,
            name,
            FontId::proportional(13.0),
            Color32::WHITE,
        );
        rects.push((i, rect));
    }
    rects
}
