//! Draws screen GUIs (frames, labels and buttons) over the 3D view.
//!
//! While playing, these are the player's own copies; while editing, the
//! starter GUIs are shown as a preview so they can be laid out and selected.

use bevy_egui::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke, StrokeKind};
use libroxel_instance::{
    ClassKind, Color3, InstanceId, InstanceTree, Property, PropertyValue, UDim, UDim2, Vec2, Vec3,
};

/// The GUI object under the pointer, if any, and its class.
pub type GuiHit = Option<(InstanceId, ClassKind)>;

/// Text boxes: where each one was drawn (so the caller can put a text
/// field there), and the one being typed into, whose text the field draws.
#[derive(Debug, Default)]
pub struct TextBoxes {
    pub found: Vec<(InstanceId, Rect)>,
    pub typing: Option<InstanceId>,
    /// The innermost scrolling frame under the pointer, and how far it can
    /// scroll (in pixels, on each axis), so the mouse wheel can move it.
    pub scroll: Option<(InstanceId, egui::Vec2)>,
    /// Where each screen GUI object was drawn, from the view's top left
    /// corner, for scripts' AbsolutePosition and AbsoluteSize.
    pub rects: Vec<(InstanceId, Rect)>,
    /// The topmost object under the pointer that takes input while playing:
    /// a button, a text box or an Active object. Labels and frames let the
    /// press through to what's under them, as in Roblox.
    pub input: GuiHit,
}

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

fn udim(tree: &InstanceTree, id: InstanceId, property: Property) -> UDim {
    match tree.get(id, property) {
        Ok(PropertyValue::UDim(u)) => u,
        _ => UDim::default(),
    }
}

/// The first child of `id` of `class`.
fn child_of_class(tree: &InstanceTree, id: InstanceId, class: ClassKind) -> Option<InstanceId> {
    tree.find_first_child_of_class(id, class).ok().flatten()
}

/// An enumeration kept in an instance's extras, by its number.
/// Whether a text object reads tags in its text (its RichText is on).
fn is_rich(tree: &InstanceTree, id: InstanceId) -> bool {
    matches!(tree.get_extra(id, "RichText"), Ok(Some(PropertyValue::Bool(true))))
}

fn extra_enum(tree: &InstanceTree, id: InstanceId, name: &str) -> Option<i64> {
    match tree.get_extra(id, name) {
        Ok(Some(PropertyValue::Number(n))) => Some(n as i64),
        _ => None,
    }
}

/// The space Roblox's top bar takes at the top of the screen, in pixels.
/// Screen GUIs start below it unless they ignore the inset.
pub const GUI_INSET: f32 = libroxel_script::GUI_INSET;

/// Whether a screen GUI covers the top bar too (IgnoreGuiInset, or
/// ScreenInsets set to None or DeviceSafeInsets).
pub fn ignores_inset(tree: &InstanceTree, screen: InstanceId) -> bool {
    matches!(tree.get_extra(screen, "IgnoreGuiInset"), Ok(Some(PropertyValue::Bool(true))))
        || matches!(extra_enum(tree, screen, "ScreenInsets"), Some(0 | 1))
}

/// `u` resolved against `span`, with its offset (in pixels) times `k`, the
/// UI scale it is under.
fn res1(u: UDim, span: f32, k: f32) -> f32 {
    u.scale * span + u.offset * k
}

/// `u` resolved against `parent`, offsets times `k`.
fn res2(u: UDim2, parent: Vec2, k: f32) -> Vec2 {
    Vec2::new(u.x_scale * parent.x + u.x_offset * k, u.y_scale * parent.y + u.y_offset * k)
}

/// How much a GUI object's own UI scale (a UIScale inside it) enlarges it
/// and everything in it.
fn own_scale(tree: &InstanceTree, id: InstanceId) -> f32 {
    child_of_class(tree, id, ClassKind::UIScale)
        .map_or(1.0, |s| tree.get_number(s, Property::Scale).unwrap_or(1.0).max(0.0) as f32)
}

/// A GUI object's size in its parent: its Size, with its SizeConstraint
/// (RelativeXX and RelativeYY size both axes from one side of the parent).
fn size_in(tree: &InstanceTree, id: InstanceId, parent: Vec2, k: f32) -> Vec2 {
    let base = match tree.get_extra(id, "SizeConstraint") {
        Ok(Some(PropertyValue::Number(n))) if n as i64 == 1 => Vec2::new(parent.x, parent.x),
        Ok(Some(PropertyValue::Number(n))) if n as i64 == 2 => Vec2::new(parent.y, parent.y),
        _ => parent,
    };
    res2(udim2(tree, id, Property::GuiSize), base, k)
}

/// How round a GUI object's corners are, in pixels, from a UI corner inside it.
fn rounding(tree: &InstanceTree, id: InstanceId, rect: Rect, k: f32) -> f32 {
    child_of_class(tree, id, ClassKind::UICorner).map_or(0.0, |corner| {
        let short = rect.width().min(rect.height());
        res1(udim(tree, corner, Property::CornerRadius), short, k).clamp(0.0, short / 2.0)
    })
}

/// The space inside `rect` left after a UI padding in `id`.
fn padded(tree: &InstanceTree, id: InstanceId, rect: Rect, k: f32) -> Rect {
    let Some(pad) = child_of_class(tree, id, ClassKind::UIPadding) else { return rect };
    let (w, h) = (rect.width(), rect.height());
    let left = res1(udim(tree, pad, Property::PaddingLeft), w, k);
    let right = res1(udim(tree, pad, Property::PaddingRight), w, k);
    let top = res1(udim(tree, pad, Property::PaddingTop), h, k);
    let bottom = res1(udim(tree, pad, Property::PaddingBottom), h, k);
    let min = Pos2::new(rect.min.x + left, rect.min.y + top);
    let max = Pos2::new((rect.max.x - right).max(min.x), (rect.max.y - bottom).max(min.y));
    Rect::from_min_max(min, max)
}

fn by_layout_order(tree: &InstanceTree, items: &mut [(InstanceId, ClassKind)]) {
    // Lowest LayoutOrder first; ties keep their order in the tree.
    items.sort_by(|a, b| {
        let order = |id| tree.get_number(id, Property::LayoutOrder).unwrap_or(0.0);
        order(a.0).total_cmp(&order(b.0))
    });
}

/// The visible GUI objects inside `parent`, in drawing order, each with its
/// rectangle. A list or grid layout inside `parent` lines them up instead of
/// using their positions. Higher ZIndex is drawn later (on top).
#[cfg(test)]
fn arrange(tree: &InstanceTree, parent: InstanceId, rect: Rect) -> Vec<(InstanceId, ClassKind, Rect)> {
    arrange_scaled(tree, parent, rect, 1.0)
}

/// [`arrange`] for children under a UI scale of `k` (the parent's own
/// scale included).
fn arrange_scaled(
    tree: &InstanceTree,
    parent: InstanceId,
    rect: Rect,
    k: f32,
) -> Vec<(InstanceId, ClassKind, Rect)> {
    let mut out = arrange_unsorted(tree, parent, rect, k);
    out.sort_by(|a, b| {
        let z = |id| tree.get_number(id, Property::ZIndex).unwrap_or(1.0);
        z(a.0).total_cmp(&z(b.0))
    });
    out
}

fn arrange_unsorted(
    tree: &InstanceTree,
    parent: InstanceId,
    rect: Rect,
    k: f32,
) -> Vec<(InstanceId, ClassKind, Rect)> {
    let rect = padded(tree, parent, rect, k);
    let mut items: Vec<(InstanceId, ClassKind)> = tree
        .children(parent)
        .unwrap_or_default()
        .iter()
        .filter_map(|&c| Some((c, tree.class(c).ok()?)))
        .filter(|(c, class)| class.is_gui_object() && tree.get_bool(*c, Property::Visible) != Some(false))
        .collect();
    if let Some(grid) = child_of_class(tree, parent, ClassKind::UIGridLayout) {
        by_layout_order(tree, &mut items);
        let parent_size = Vec2::new(rect.width(), rect.height());
        let cell = res2(udim2(tree, grid, Property::CellSize), parent_size, k);
        let gap = res2(udim2(tree, grid, Property::CellPadding), parent_size, k);
        let (cw, ch) = (cell.x.max(0.0), cell.y.max(0.0));
        let rows_first = tree.get_enum(grid, Property::FillDirection)
            != Some(libroxel_instance::EnumValue::FillDirection(libroxel_instance::FillDirection::Vertical));
        // How many cells fit along the direction being filled (at least one).
        let fit =
            |span: f32, size: f32, gap: f32| (((span + gap) / (size + gap).max(1.0)).floor() as usize).max(1);
        let per_line = if rows_first { fit(rect.width(), cw, gap.x) } else { fit(rect.height(), ch, gap.y) };
        return items
            .into_iter()
            .enumerate()
            .map(|(i, (c, class))| {
                let (line, along) = (i / per_line, i % per_line);
                let (col, row) = if rows_first { (along, line) } else { (line, along) };
                let min =
                    Pos2::new(rect.min.x + col as f32 * (cw + gap.x), rect.min.y + row as f32 * (ch + gap.y));
                (c, class, Rect::from_min_size(min, egui::vec2(cw, ch)))
            })
            .collect();
    }
    let Some(list) = child_of_class(tree, parent, ClassKind::UIListLayout) else {
        return items.into_iter().map(|(c, class)| (c, class, layout(tree, c, rect, k))).collect();
    };
    let horizontal = tree.get_enum(list, Property::FillDirection)
        == Some(libroxel_instance::EnumValue::FillDirection(libroxel_instance::FillDirection::Horizontal));
    let along = if horizontal { rect.width() } else { rect.height() };
    let gap = res1(udim(tree, list, Property::Padding), along, k);
    by_layout_order(tree, &mut items);
    let parent_size = Vec2::new(rect.width(), rect.height());
    let sized: Vec<(InstanceId, ClassKind, egui::Vec2)> = items
        .into_iter()
        .map(|(c, class)| {
            let size = constrained(tree, c, size_in(tree, c, parent_size, k), k);
            (c, class, egui::vec2(size.x.max(0.0), size.y.max(0.0)))
        })
        .collect();
    // Alignment, as Roblox numbers it: Center = 0, Left/Top = 1, Right/Bottom = 2.
    let h_align = extra_enum(tree, list, "HorizontalAlignment").unwrap_or(1);
    let v_align = extra_enum(tree, list, "VerticalAlignment").unwrap_or(1);
    let place = |align: i64, free: f32| match align {
        0 => free / 2.0,
        2 => free,
        _ => 0.0,
    };
    let total: f32 = sized.iter().map(|(_, _, s)| if horizontal { s.x } else { s.y }).sum::<f32>()
        + gap * sized.len().saturating_sub(1) as f32;
    let mut cursor =
        if horizontal { place(h_align, rect.width() - total) } else { place(v_align, rect.height() - total) };
    sized
        .into_iter()
        .map(|(c, class, size)| {
            let min = if horizontal {
                Pos2::new(rect.min.x + cursor, rect.min.y + place(v_align, rect.height() - size.y))
            } else {
                Pos2::new(rect.min.x + place(h_align, rect.width() - size.x), rect.min.y + cursor)
            };
            cursor += if horizontal { size.x } else { size.y } + gap;
            (c, class, Rect::from_min_size(min, size))
        })
        .collect()
}

/// A GUI object's size after its UI scale, size constraint and aspect
/// ratio constraint.
fn constrained(tree: &InstanceTree, id: InstanceId, size: Vec2, k: f32) -> Vec2 {
    let mut size = automatic(tree, id, size, k);
    if let Some(limits) = child_of_class(tree, id, ClassKind::UISizeConstraint) {
        let lo = vec2(tree, limits, Property::MinSize);
        let hi = vec2(tree, limits, Property::MaxSize);
        let (lo, hi) = (Vec2::new(lo.x * k, lo.y * k), Vec2::new(hi.x * k, hi.y * k));
        size = Vec2::new(size.x.clamp(lo.x, hi.x.max(lo.x)), size.y.clamp(lo.y, hi.y.max(lo.y)));
    }
    let own = own_scale(tree, id);
    size = Vec2::new(size.x * own, size.y * own);
    if let Some(aspect) = child_of_class(tree, id, ClassKind::UIAspectRatioConstraint) {
        let ratio = tree.get_number(aspect, Property::AspectRatio).unwrap_or(1.0).max(0.001) as f32;
        // Shrink whichever side is too long.
        if size.x > size.y * ratio {
            size.x = size.y * ratio;
        } else {
            size.y = size.x / ratio;
        }
    }
    size
}

thread_local! {
    /// The context GUI text is laid out with, so sizes match what's drawn.
    static TEXT_CONTEXT: std::cell::RefCell<Option<egui::Context>> = const { std::cell::RefCell::new(None) };
}

/// How wide and tall `text` is at `size` points, wrapped at `width`: laid
/// out with the drawing fonts while drawing, or estimated otherwise.
fn measure_text(text: &str, size: f32, width: f32, rich: Option<f32>) -> (f32, f32) {
    if text.is_empty() {
        return (0.0, 0.0);
    }
    let laid = TEXT_CONTEXT.with_borrow(|ctx| {
        ctx.as_ref().map(|ctx| {
            let job = match rich {
                Some(k) => crate::rich_text::job(text, size, Color32::WHITE, width, k),
                None => egui::text::LayoutJob::simple(
                    text.to_owned(),
                    FontId::proportional(size),
                    Color32::WHITE,
                    width,
                ),
            };
            let galley = ctx.fonts_mut(|f| f.layout_job(job));
            (galley.size().x, galley.size().y)
        })
    });
    laid.unwrap_or_else(|| {
        let per_line =
            if width.is_finite() { (width / (size * 0.55)).floor().max(1.0) } else { f32::INFINITY };
        let mut lines = 0.0;
        let mut widest: f32 = 0.0;
        for line in text.lines() {
            let n = line.chars().count() as f32;
            lines += (n / per_line).ceil().max(1.0);
            widest = widest.max(n.min(per_line));
        }
        (widest * size * 0.55, lines.max(1.0) * size * 1.2)
    })
}

/// A GUI object's size grown to fit what's inside it, along the axes its
/// AutomaticSize names (X = 1, Y = 2, XY = 3): its children, and its text.
fn automatic(tree: &InstanceTree, id: InstanceId, size: Vec2, k: f32) -> Vec2 {
    let axes = extra_enum(tree, id, "AutomaticSize").unwrap_or(0);
    if axes == 0 {
        return size;
    }
    let (grow_x, grow_y) = (axes & 1 != 0, axes & 2 != 0);
    let origin = Rect::from_min_size(Pos2::ZERO, egui::vec2(size.x.max(0.0), size.y.max(0.0)));
    let inner = padded(tree, id, origin, k);
    // Read the far-side padding itself: an empty box clamps its inner rect.
    let (pad_right, pad_bottom) = match child_of_class(tree, id, ClassKind::UIPadding) {
        Some(pad) => (
            res1(udim(tree, pad, Property::PaddingRight), origin.width(), k),
            res1(udim(tree, pad, Property::PaddingBottom), origin.height(), k),
        ),
        None => (0.0, 0.0),
    };
    let mut extent = Vec2::new(0.0, 0.0);
    for (_, _, r) in arrange_unsorted(tree, id, origin, k) {
        extent.x = extent.x.max(r.max.x + pad_right);
        extent.y = extent.y.max(r.max.y + pad_bottom);
    }
    if let Ok(class) = tree.class(id)
        && is_text(class)
    {
        let text = tree.get_string(id, Property::Text).unwrap_or_default();
        let text_size = tree.get_number(id, Property::TextSize).unwrap_or(14.0).max(1.0) as f32 * k;
        // Wrapped text that can't grow sideways wraps at its own width, so it
        // grows downwards by as many lines as it wraps to, as in Roblox.
        let wraps = tree.get_bool(id, Property::TextWrapped) == Some(true) && !grow_x;
        let width = if wraps { inner.width().max(1.0) } else { f32::INFINITY };
        let (w, h) = measure_text(&text, text_size, width, is_rich(tree, id).then_some(k));
        extent.x = extent.x.max(inner.min.x + w + pad_right);
        extent.y = extent.y.max(inner.min.y + h + pad_bottom);
    }
    Vec2::new(
        if grow_x { size.x.max(extent.x) } else { size.x },
        if grow_y { size.y.max(extent.y) } else { size.y },
    )
}

/// Where a GUI object sits inside its parent's rectangle.
pub fn layout(tree: &InstanceTree, id: InstanceId, parent: Rect, k: f32) -> Rect {
    let parent_size = Vec2::new(parent.width(), parent.height());
    let size = constrained(tree, id, size_in(tree, id, parent_size, k), k);
    let pos = res2(udim2(tree, id, Property::GuiPosition), parent_size, k);
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
    boxes: &mut TextBoxes,
) -> GuiHit {
    let pointer = ui.ctx().input(|i| i.pointer.hover_pos()).filter(|p| area.contains(*p));
    let painter = ui.painter().with_clip_rect(area);
    let mut hit = None;
    // Higher DisplayOrder is drawn later, on top.
    let mut screens: Vec<InstanceId> = tree
        .children(container)
        .unwrap_or_default()
        .iter()
        .copied()
        .filter(|s| {
            tree.class(*s) == Ok(ClassKind::ScreenGui) && tree.get_bool(*s, Property::Enabled) != Some(false)
        })
        .collect();
    let order = |s: InstanceId| match tree.get_extra(s, "DisplayOrder") {
        Ok(Some(PropertyValue::Number(n))) => n,
        _ => 0.0,
    };
    screens.sort_by(|a, b| order(*a).total_cmp(&order(*b)));
    boxes.rects.clear();
    TEXT_CONTEXT.with_borrow_mut(|c| *c = Some(ui.ctx().clone()));
    for screen in screens {
        let k = own_scale(tree, screen);
        let mut screen_area = area;
        if !ignores_inset(tree, screen) {
            screen_area.min.y = (area.min.y + GUI_INSET).min(area.max.y);
        }
        draw_children(&painter, tree, screen, screen_area, k, pointer, selected, &mut hit, boxes);
    }
    // Roblox measures AbsolutePosition from just below the top bar.
    let origin = area.min.to_vec2() + egui::vec2(0.0, GUI_INSET);
    for (_, r) in &mut boxes.rects {
        *r = r.translate(-origin);
    }
    hit
}

/// Draws `text` inside `rect` as a text object's properties say: aligned,
/// wrapped or scaled to fit, and outlined when `stroke` is given.
#[allow(clippy::too_many_arguments)]
fn draw_text(
    painter: &egui::Painter,
    tree: &InstanceTree,
    id: InstanceId,
    rect: Rect,
    text: String,
    size: f32,
    colour: Color32,
    stroke: Option<(Color32, f32)>,
    scale: f32,
) {
    use libroxel_instance::{EnumValue, TextXAlignment, TextYAlignment};
    let x = match tree.get_enum(id, Property::TextXAlignment) {
        Some(EnumValue::TextXAlignment(a)) => a,
        _ => TextXAlignment::Center,
    };
    let y = match tree.get_enum(id, Property::TextYAlignment) {
        Some(EnumValue::TextYAlignment(a)) => a,
        _ => TextYAlignment::Center,
    };
    let wrapped = tree.get_bool(id, Property::TextWrapped) == Some(true);
    let scaled = tree.get_bool(id, Property::TextScaled) == Some(true);
    let halign = match x {
        TextXAlignment::Left => egui::Align::LEFT,
        TextXAlignment::Center => egui::Align::Center,
        TextXAlignment::Right => egui::Align::RIGHT,
    };
    let rich = is_rich(tree, id);
    let lay = |size: f32| {
        let width = if wrapped || scaled { rect.width().max(1.0) } else { f32::INFINITY };
        let mut job = if rich {
            crate::rich_text::job(&text, size, colour, width, scale)
        } else {
            egui::text::LayoutJob::simple(text.clone(), FontId::proportional(size), colour, width)
        };
        job.halign = halign;
        painter.layout_job(job)
    };
    // A text size constraint keeps the size (scaled or not) within its limits.
    let (least, most) = match child_of_class(tree, id, ClassKind::UITextSizeConstraint) {
        Some(c) => {
            let lo = tree.get_number(c, Property::MinTextSize).unwrap_or(1.0).max(1.0) as f32;
            (lo, (tree.get_number(c, Property::MaxTextSize).unwrap_or(100.0) as f32).max(lo))
        }
        None => (1.0, 100.0),
    };
    let galley = if scaled {
        // The biggest size that fits, found by halving the gap a few times.
        let (mut lo, mut hi) = (least, most);
        for _ in 0..8 {
            let mid = (lo + hi) / 2.0;
            let g = lay(mid);
            if g.size().x <= rect.width() + 0.5 && g.size().y <= rect.height() + 0.5 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        lay(lo)
    } else {
        lay(size.clamp(least, most))
    };
    let bounds = galley.rect;
    let left = match x {
        TextXAlignment::Left => rect.min.x,
        TextXAlignment::Center => rect.center().x - bounds.width() / 2.0,
        TextXAlignment::Right => rect.max.x - bounds.width(),
    };
    let top = match y {
        TextYAlignment::Top => rect.min.y,
        TextYAlignment::Center => rect.center().y - bounds.height() / 2.0,
        TextYAlignment::Bottom => rect.max.y - bounds.height(),
    };
    let pos = Pos2::new(left, top) - bounds.min.to_vec2();
    if let Some((stroke_colour, width)) = stroke {
        let w = width.max(0.5);
        for (dx, dy) in [
            (-1.0, 0.0),
            (1.0, 0.0),
            (0.0, -1.0),
            (0.0, 1.0),
            (-0.7, -0.7),
            (0.7, -0.7),
            (-0.7, 0.7),
            (0.7, 0.7),
        ] {
            painter.galley_with_override_text_color(
                pos + egui::vec2(dx * w, dy * w),
                galley.clone(),
                stroke_colour,
            );
        }
    }
    painter.galley(pos, galley, colour);
}

/// A UI stroke inside `id`, if it has an enabled one: its colour and thickness.
fn stroke_of(tree: &InstanceTree, id: InstanceId) -> Option<(Color32, f32)> {
    let s = child_of_class(tree, id, ClassKind::UIStroke)?;
    if tree.get_bool(s, Property::Enabled) == Some(false) {
        return None;
    }
    let alpha = 1.0 - tree.get_number(s, Property::Transparency).unwrap_or(0.0) as f32;
    let c = tree.get_color(s, Property::Color).unwrap_or(Color3::new(0.0, 0.0, 0.0));
    Some((color(c, alpha), tree.get_number(s, Property::Thickness).unwrap_or(1.0).max(0.0) as f32))
}

/// An image label's or button's picture, tinted and scaled as it says.
fn draw_image(painter: &egui::Painter, tree: &InstanceTree, id: InstanceId, rect: Rect, darker: bool) {
    let name = tree.get_string(id, Property::Image).unwrap_or_default();
    let Some((texture, size)) = crate::images::texture(painter.ctx(), &name) else { return };
    let scale = match tree.get_enum(id, Property::ScaleType) {
        Some(libroxel_instance::EnumValue::ScaleType(s)) => s,
        _ => libroxel_instance::ScaleType::Stretch,
    };
    let (at, uv) = crate::images::placement(rect, size, scale);
    let mut tint = tree.get_color(id, Property::ImageColor).unwrap_or(Color3::WHITE);
    if darker {
        tint = tint.lerp(Color3::new(0.0, 0.0, 0.0), 0.15);
    }
    let alpha = 1.0 - tree.get_number(id, Property::ImageTransparency).unwrap_or(0.0) as f32;
    if alpha > 0.0 {
        painter.with_clip_rect(rect.intersect(painter.clip_rect())).image(
            texture,
            at,
            uv,
            color(tint, alpha),
        );
    }
}

fn is_text(class: ClassKind) -> bool {
    matches!(class, ClassKind::TextLabel | ClassKind::TextButton | ClassKind::TextBox)
}

#[allow(clippy::too_many_arguments)]
fn draw_children(
    painter: &egui::Painter,
    tree: &InstanceTree,
    parent: InstanceId,
    rect: Rect,
    scale: f32,
    pointer: Option<Pos2>,
    selected: Option<InstanceId>,
    hit: &mut GuiHit,
    boxes: &mut TextBoxes,
) {
    for (child, class, r) in arrange_scaled(tree, parent, rect, scale) {
        // The child and what's in it are under its own UI scale too.
        let k = scale * own_scale(tree, child);
        boxes.rects.push((child, r));
        let round = rounding(tree, child, r, k);
        // Only what shows (inside any clipping frame) can be pointed at.
        let hovered = pointer.is_some_and(|p| r.contains(p) && painter.clip_rect().contains(p));
        if hovered {
            *hit = Some((child, class));
            if class.is_button()
                || class == ClassKind::TextBox
                || matches!(tree.get_extra(child, "Active"), Ok(Some(PropertyValue::Bool(true))))
            {
                boxes.input = Some((child, class));
            }
        }

        let alpha = 1.0 - tree.get_number(child, Property::BackgroundTransparency).unwrap_or(0.0) as f32;
        let mut background = tree.get_color(child, Property::BackgroundColor).unwrap_or(Color3::WHITE);
        if class.is_button() && hovered {
            background = background.lerp(Color3::new(0.0, 0.0, 0.0), 0.15);
        }
        if alpha > 0.0 {
            match gradient_of(tree, child) {
                Some(g) => fill_gradient(painter, r, round, background, alpha, &g),
                None => {
                    painter.rect_filled(r, round, color(background, alpha));
                }
            }
        }
        let border = tree.get_number(child, Property::BorderSizePixel).unwrap_or(0.0) as f32 * k;
        if border > 0.0 && alpha > 0.0 {
            let c = tree.get_color(child, Property::BorderColor).unwrap_or(Color3::new(0.1, 0.16, 0.2));
            painter.rect_stroke(r, round, Stroke::new(border, color(c, alpha)), StrokeKind::Outside);
        }
        let stroke = stroke_of(tree, child).map(|(c, w)| (c, w * k));
        if let Some((c, width)) = stroke.filter(|_| !is_text(class)) {
            painter.rect_stroke(r, round, Stroke::new(width, c), StrokeKind::Outside);
        }
        if matches!(class, ClassKind::ImageLabel | ClassKind::ImageButton) {
            draw_image(
                painter,
                tree,
                child,
                padded(tree, child, r, k),
                class == ClassKind::ImageButton && hovered,
            );
        }
        let size = tree.get_number(child, Property::TextSize).unwrap_or(14.0).max(1.0) as f32 * k;
        let text_color = tree.get_color(child, Property::TextColor).unwrap_or(Color3::new(0.1, 0.16, 0.2));
        let mut text = tree.get_string(child, Property::Text).unwrap_or_default().to_owned();
        let mut text_alpha = 1.0 - tree.get_number(child, Property::TextTransparency).unwrap_or(0.0) as f32;
        if class == ClassKind::TextBox {
            boxes.found.push((child, r));
            if boxes.typing == Some(child) {
                text.clear();
            } else if text.is_empty() {
                text = tree.get_string(child, Property::PlaceholderText).unwrap_or_default().to_owned();
                text_alpha *= 0.45;
            }
        }
        let inner = padded(tree, child, r, k);
        if !text.is_empty() && text_alpha > 0.0 {
            let clipped = painter.with_clip_rect(r.intersect(painter.clip_rect()));
            let outline = stroke.filter(|_| is_text(class));
            draw_text(&clipped, tree, child, inner, text, size, color(text_color, text_alpha), outline, k);
        }
        if selected == Some(child) {
            painter.rect_stroke(
                r,
                round,
                Stroke::new(2.0, Color32::from_rgb(80, 170, 255)),
                StrokeKind::Outside,
            );
        }
        if class == ClassKind::ScrollingFrame {
            // The children sit on a canvas that can be bigger than the frame, moved by scrolling.
            let canvas = res2(udim2(tree, child, Property::CanvasSize), Vec2::new(r.width(), r.height()), k);
            let canvas = egui::vec2(canvas.x.max(r.width()), canvas.y.max(r.height()));
            let max = canvas - r.size();
            let at = vec2(tree, child, Property::CanvasPosition);
            let at = egui::vec2(at.x.clamp(0.0, max.x), at.y.clamp(0.0, max.y));
            if hovered {
                boxes.scroll = Some((child, max));
            }
            let clip = painter.with_clip_rect(r.intersect(painter.clip_rect()));
            let area = Rect::from_min_size(r.min - at, canvas);
            draw_children(&clip, tree, child, area, k, pointer, selected, hit, boxes);
            let bar = tree.get_number(child, Property::ScrollBarThickness).unwrap_or(12.0) as f32 * k;
            if max.y > 0.5 && bar > 0.0 {
                let track = Rect::from_min_max(Pos2::new(r.max.x - bar, r.min.y), r.max);
                let thumb_h = (r.height() * r.height() / canvas.y).max(bar);
                let thumb_y = track.min.y + (track.height() - thumb_h) * (at.y / max.y);
                let thumb = Rect::from_min_size(Pos2::new(track.min.x, thumb_y), egui::vec2(bar, thumb_h));
                painter.rect_filled(thumb.shrink(2.0), bar / 2.0, Color32::from_black_alpha(120));
            }
            continue;
        }
        if tree.get_bool(child, Property::ClipsDescendants) == Some(true) {
            let clip = painter.with_clip_rect(r.intersect(painter.clip_rect()));
            draw_children(&clip, tree, child, r, k, pointer, selected, hit, boxes);
        } else {
            draw_children(painter, tree, child, r, k, pointer, selected, hit, boxes);
        }
    }
}

/// A gradient over a GUI object's background: its two end colours and
/// transparencies, which way it runs (degrees clockwise) and its shift.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gradient {
    pub colours: (Color3, Color3),
    pub transparency: (f32, f32),
    pub rotation: f32,
    pub offset: Vec2,
}

/// The enabled UIGradient inside `id`, if there is one.
pub fn gradient_of(tree: &InstanceTree, id: InstanceId) -> Option<Gradient> {
    let g = child_of_class(tree, id, ClassKind::UIGradient)?;
    if tree.get_bool(g, Property::Enabled) == Some(false) {
        return None;
    }
    let c = |p| tree.get_color(g, p).unwrap_or(Color3::WHITE);
    let n = |p| tree.get_number(g, p).unwrap_or(0.0) as f32;
    Some(Gradient {
        colours: (c(Property::GradientColor0), c(Property::GradientColor1)),
        transparency: (n(Property::GradientTransparency0), n(Property::GradientTransparency1)),
        rotation: n(Property::GradientRotation),
        offset: vec2(tree, g, Property::GradientOffset),
    })
}

impl Gradient {
    /// How far along the gradient (0 at its start, 1 at its end) a point
    /// in `rect` is. Turned, it still runs corner to corner.
    pub fn along(&self, rect: Rect, at: Pos2) -> f32 {
        let (s, c) = self.rotation.to_radians().sin_cos();
        let dir = egui::vec2(c, s);
        let centre = rect.center() + egui::vec2(self.offset.x * rect.width(), self.offset.y * rect.height());
        let half = (rect.width() * c.abs() + rect.height() * s.abs()) / 2.0;
        if half <= 0.0 {
            return 0.0;
        }
        (((at - centre).dot(dir) / half + 1.0) / 2.0).clamp(0.0, 1.0)
    }

    /// The colour multiplier and opacity at `t` along it.
    pub fn at(&self, t: f32) -> (Color3, f32) {
        let colour = self.colours.0.lerp(self.colours.1, t);
        let transparency = self.transparency.0 + (self.transparency.1 - self.transparency.0) * t;
        (colour, (1.0 - transparency).clamp(0.0, 1.0))
    }
}

/// Fills `rect` (with rounded corners) in `base` tinted by a gradient.
fn fill_gradient(painter: &egui::Painter, rect: Rect, round: f32, base: Color3, alpha: f32, g: &Gradient) {
    let ctx = painter.ctx();
    let shape = egui::epaint::ClippedShape {
        clip_rect: painter.clip_rect(),
        shape: egui::Shape::rect_filled(rect, round, Color32::WHITE),
    };
    for primitive in ctx.tessellate(vec![shape], ctx.pixels_per_point()) {
        let egui::epaint::Primitive::Mesh(mut mesh) = primitive.primitive else { continue };
        for v in &mut mesh.vertices {
            let (tint, opacity) = g.at(g.along(rect, v.pos));
            // The edge's softening comes through as the vertex's own alpha.
            let edge = v.color.a() as f32 / 255.0;
            let c = Color3::new(base.r * tint.r, base.g * tint.g, base.b * tint.b);
            v.color = color(c, alpha * opacity * edge);
        }
        painter.add(egui::Shape::mesh(mesh));
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
    use crate::theme;
    let action = tree.get_string(prompt, Property::ActionText).unwrap_or_default();
    let object = tree.get_string(prompt, Property::ObjectText).unwrap_or_default();
    let rect = Rect::from_center_size(at, egui::vec2(210.0, 56.0));
    let painter = ui.painter();
    // A hard offset shadow, then the slab, with an accent strip down the left.
    painter.rect_filled(rect.translate(egui::vec2(4.0, 4.0)), theme::CORNER, Color32::from_black_alpha(150));
    painter.rect_filled(rect, theme::CORNER, theme::INK1);
    painter.rect_stroke(rect, theme::CORNER, Stroke::new(1.0, theme::LINE), StrokeKind::Inside);
    // The key, as a keycap standing on its edge.
    let key_box = Rect::from_center_size(rect.left_center() + egui::vec2(30.0, -2.0), egui::vec2(34.0, 32.0));
    painter.rect_filled(key_box.translate(egui::vec2(0.0, 4.0)), theme::CORNER, theme::EMBER);
    painter.rect_filled(key_box, theme::CORNER, theme::TANGERINE);
    painter.text(key_box.center(), Align2::CENTER_CENTER, key, theme::display_bold(17.0), theme::INK0);
    if progress > 0.0 && progress < 1.0 {
        // A bar along the bottom of the bubble fills up while the key is held.
        let bar = Rect::from_min_size(
            rect.left_bottom() + egui::vec2(0.0, -4.0),
            egui::vec2(rect.width() * progress, 4.0),
        );
        painter.rect_filled(bar, 0.0, theme::SUN);
    }
    let text_x = key_box.right() + 12.0;
    let width = rect.right() - text_x - 8.0;
    if !object.is_empty() {
        theme::text_line(
            painter,
            egui::pos2(text_x, rect.center().y - 17.0),
            &object.to_uppercase(),
            theme::display(11.0),
            theme::DUST,
            width,
        );
    }
    let action_y = if object.is_empty() { -9.0 } else { -2.0 };
    theme::text_line(
        painter,
        egui::pos2(text_x, rect.center().y + action_y),
        &action,
        theme::display(17.0),
        theme::CHALK,
        width,
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
    boxes: &mut TextBoxes,
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
        draw_children(
            &painter,
            tree,
            board,
            rect,
            own_scale(tree, board),
            pointer,
            selected,
            &mut hit,
            boxes,
        );
    }
    hit
}

/// Where a surface GUI is drawn: the face it covers (a unit square facing
/// +Z, stretched over the side) and its canvas size in GUI pixels.
pub fn surface_canvas(
    tree: &InstanceTree,
    board: InstanceId,
) -> Option<(bevy::prelude::Transform, egui::Vec2)> {
    use libroxel_instance::{EnumValue, NormalId};
    if tree.get_bool(board, Property::Enabled) == Some(false) {
        return None;
    }
    let part = tree.get_ref(board, Property::Adornee).or_else(|| tree.parent(board).ok().flatten())?;
    if !tree.class(part).is_ok_and(|c| c.is_physical()) {
        return None;
    }
    let pos = tree.get_vec3(part, Property::Position)?;
    let size = tree.get_vec3(part, Property::Size)?;
    let orientation = tree.get_vec3(part, Property::Orientation).unwrap_or_default();
    let face = match tree.get_enum(board, Property::Face) {
        Some(EnumValue::NormalId(face)) => face,
        _ => NormalId::Front,
    };
    let (transform, studs) = libroxel_engine::view::decal_transform(pos, size, orientation, face);
    let per_stud = (tree.get_number(board, Property::PixelsPerStud).unwrap_or(50.0) as f32).clamp(1.0, 400.0);
    Some((transform, egui::vec2(studs.x * per_stud, studs.y * per_stud)))
}

/// A point on a surface GUI's canvas (pixels from its top left) in the world.
pub fn canvas_to_world(face: &bevy::prelude::Transform, canvas: egui::Vec2, at: Pos2) -> bevy::prelude::Vec3 {
    let local = bevy::prelude::Vec3::new(at.x / canvas.x - 0.5, 0.5 - at.y / canvas.y, 0.0);
    face.transform_point(local)
}

/// Draws every surface GUI in the world on its part's face, and returns
/// the topmost GUI object under the pointer.
pub fn surfaces(
    ui: &egui::Ui,
    tree: &InstanceTree,
    camera: (&bevy::prelude::Camera, &bevy::prelude::GlobalTransform),
    selected: Option<InstanceId>,
) -> GuiHit {
    let (cam, gt) = camera;
    let ctx = ui.ctx();
    let area = ui.max_rect();
    let pointer = ctx.input(|i| i.pointer.hover_pos()).filter(|p| area.contains(*p));
    let screen = ui.painter().with_clip_rect(area);
    let eye = gt.translation();
    let mut hit = None;
    for board in tree.descendants_of_class(tree.workspace(), ClassKind::SurfaceGui) {
        let Some((face, canvas)) = surface_canvas(tree, board) else { continue };
        let centre = face.translation;
        let normal = face.rotation * bevy::prelude::Vec3::Z;
        // Only its front shows.
        if (eye - centre).dot(normal) <= 0.0 {
            continue;
        }
        if tree.get_bool(board, Property::AlwaysOnTop) != Some(true) {
            let part = tree.get_ref(board, Property::Adornee).or_else(|| tree.parent(board).ok().flatten());
            let from = Vec3::new(eye.x, eye.y, eye.z);
            let toward = Vec3::new(centre.x, centre.y, centre.z) - from;
            if libroxel_instance::geometry::raycast(tree, from, toward * 0.98, |p| Some(p) != part).is_some()
            {
                continue;
            }
        }
        // Where the pointer falls on the canvas: along its ray to the face.
        let on_canvas = pointer.and_then(|p| {
            let ray = cam.viewport_to_world(gt, bevy::prelude::Vec2::new(p.x, p.y)).ok()?;
            let along = ray.direction.dot(normal);
            if along.abs() < 1e-5 {
                return None;
            }
            let t = (centre - ray.origin).dot(normal) / along;
            let local = face.compute_affine().inverse().transform_point3(ray.origin + ray.direction * t);
            (t > 0.0 && local.x.abs() <= 0.5 && local.y.abs() <= 0.5)
                .then(|| Pos2::new((local.x + 0.5) * canvas.x, (0.5 - local.y) * canvas.y))
        });
        // Draw it flat on a hidden layer, then bend each triangle onto the face.
        let layer = egui::LayerId::new(egui::Order::Background, egui::Id::new(("surface gui", board)));
        let rect = Rect::from_min_size(Pos2::ZERO, canvas);
        let flat = egui::Painter::new(ctx.clone(), layer, rect);
        let mut scratch = TextBoxes::default();
        draw_children(
            &flat,
            tree,
            board,
            rect,
            own_scale(tree, board),
            on_canvas,
            selected,
            &mut hit,
            &mut scratch,
        );
        let shapes: Vec<egui::epaint::ClippedShape> = ctx.graphics_mut(|g| {
            let list = g.entry(layer);
            let shapes = list.all_entries().cloned().collect();
            for i in 0..list.next_idx().0 {
                list.reset_shape(egui::layers::ShapeIdx(i));
            }
            shapes
        });
        let mut meshes = Vec::new();
        let mut behind = false;
        for primitive in ctx.tessellate(shapes, ctx.pixels_per_point()) {
            let egui::epaint::Primitive::Mesh(mut mesh) = primitive.primitive else { continue };
            for v in &mut mesh.vertices {
                let at = Pos2::new(v.pos.x.clamp(0.0, canvas.x), v.pos.y.clamp(0.0, canvas.y));
                match cam.world_to_viewport(gt, canvas_to_world(&face, canvas, at)) {
                    Ok(p) => v.pos = Pos2::new(p.x, p.y),
                    Err(_) => behind = true,
                }
            }
            meshes.push(egui::Shape::mesh(mesh));
        }
        if behind {
            continue;
        }
        screen.extend(meshes);
        if selected == Some(board) {
            let corners = [(0.0, 0.0), (canvas.x, 0.0), (canvas.x, canvas.y), (0.0, canvas.y), (0.0, 0.0)];
            let outline: Option<Vec<Pos2>> = corners
                .iter()
                .map(|(x, y)| {
                    let p =
                        cam.world_to_viewport(gt, canvas_to_world(&face, canvas, Pos2::new(*x, *y))).ok()?;
                    Some(Pos2::new(p.x, p.y))
                })
                .collect();
            if let Some(points) = outline {
                screen.add(egui::Shape::line(points, Stroke::new(1.0, Color32::from_rgb(80, 170, 255))));
            }
        }
    }
    hit
}

/// Draws the player's tool slots along the bottom of the view. Returns each
/// slot's rectangle, so clicks can pick a tool.
pub fn hotbar(ui: &egui::Ui, tree: &InstanceTree, tools: &[(InstanceId, bool)]) -> Vec<(usize, Rect)> {
    use crate::theme;
    let area = ui.max_rect();
    let (slot, gap) = (64.0, 6.0);
    let count = tools.len().min(9);
    let width = count as f32 * slot + count.saturating_sub(1) as f32 * gap;
    let mut x = area.center().x - width / 2.0;
    let y = area.bottom() - slot - 22.0;
    let painter = ui.painter();
    let mut rects = Vec::new();
    for (i, (tool, held)) in tools.iter().take(9).enumerate() {
        let rect = Rect::from_min_size(Pos2::new(x, y), egui::vec2(slot, slot));
        x += slot + gap;
        // The held tool lifts out of the row on its accent edge.
        let face = if *held { rect.translate(egui::vec2(0.0, -6.0)) } else { rect };
        painter.rect_filled(
            rect.translate(egui::vec2(0.0, 4.0)),
            theme::CORNER,
            Color32::from_black_alpha(140),
        );
        if *held {
            painter.rect_filled(face.translate(egui::vec2(0.0, 4.0)), theme::CORNER, theme::EMBER);
        }
        painter.rect_filled(
            face,
            theme::CORNER,
            if *held { theme::INK2 } else { theme::INK1.gamma_multiply(0.92) },
        );
        let line = if *held { Stroke::new(2.0, theme::TANGERINE) } else { Stroke::new(1.0, theme::LINE) };
        painter.rect_stroke(face, theme::CORNER, line, StrokeKind::Inside);
        // The slot's number in a corner tab.
        let tab = Rect::from_min_size(face.left_top(), egui::vec2(16.0, 15.0));
        painter.rect_filled(tab, 0.0, if *held { theme::TANGERINE } else { theme::INK3 });
        painter.text(
            tab.center(),
            Align2::CENTER_CENTER,
            (i + 1).to_string(),
            theme::display_bold(11.0),
            if *held { theme::INK0 } else { theme::DUST },
        );
        let name = tree.name(*tool).unwrap_or("?");
        let galley = painter.layout(name.to_owned(), theme::display(12.5), theme::CHALK, slot - 8.0);
        painter.with_clip_rect(face.shrink(3.0)).galley(
            face.center() + egui::vec2(-galley.size().x / 2.0, 6.0 - galley.size().y / 2.0),
            galley,
            theme::CHALK,
        );
        rects.push((i, face));
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gui(tree: &mut InstanceTree, class: ClassKind, parent: InstanceId, name: &str) -> InstanceId {
        let id = tree.create(class, name).unwrap();
        tree.set_parent(id, Some(parent)).unwrap();
        id
    }

    fn set(tree: &mut InstanceTree, id: InstanceId, p: Property, v: PropertyValue) {
        tree.set(id, p, v).unwrap();
    }

    #[test]
    fn grids_padding_aspect_and_z_order() {
        let mut tree = InstanceTree::new();
        let root = tree.root();
        let screen = gui(&mut tree, ClassKind::ScreenGui, root, "Screen");
        let frame = gui(&mut tree, ClassKind::Frame, screen, "Frame");
        let pad = gui(&mut tree, ClassKind::UIPadding, frame, "UIPadding");
        set(&mut tree, pad, Property::PaddingLeft, PropertyValue::UDim(UDim::new(0.0, 10.0)));
        set(&mut tree, pad, Property::PaddingTop, PropertyValue::UDim(UDim::new(0.1, 0.0)));
        let grid = gui(&mut tree, ClassKind::UIGridLayout, frame, "UIGridLayout");
        set(&mut tree, grid, Property::CellSize, PropertyValue::UDim2(UDim2::new(0.0, 40.0, 0.0, 30.0)));
        let cells: Vec<_> =
            (0..5).map(|i| gui(&mut tree, ClassKind::Frame, frame, &format!("Cell{i}"))).collect();
        // Put the last one first.
        set(&mut tree, cells[4], Property::LayoutOrder, PropertyValue::Number(-1.0));
        let area = Rect::from_min_size(Pos2::ZERO, egui::vec2(140.0, 200.0));
        let placed = arrange(&tree, frame, area);
        let at = |id| placed.iter().find(|(c, _, _)| *c == id).unwrap().2;
        // 130 wide after padding: 40 + 5 + 40 + 5 + 40 fits three to a row.
        assert_eq!(at(cells[4]).min, Pos2::new(10.0, 20.0));
        assert_eq!(at(cells[0]).min, Pos2::new(55.0, 20.0));
        assert_eq!(at(cells[2]).min, Pos2::new(10.0, 55.0));
        assert_eq!(at(cells[2]).size(), egui::vec2(40.0, 30.0));

        let square = gui(&mut tree, ClassKind::Frame, screen, "Square");
        set(&mut tree, square, Property::GuiSize, PropertyValue::UDim2(UDim2::new(0.0, 300.0, 0.0, 100.0)));
        gui(&mut tree, ClassKind::UIAspectRatioConstraint, square, "Aspect");
        let big = gui(&mut tree, ClassKind::Frame, screen, "Big");
        set(&mut tree, big, Property::ZIndex, PropertyValue::Number(5.0));
        let scale = gui(&mut tree, ClassKind::UIScale, big, "UIScale");
        set(&mut tree, scale, Property::Scale, PropertyValue::Number(2.0));
        let placed = arrange(&tree, screen, Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0)));
        let names: Vec<_> = placed.iter().map(|(c, _, _)| tree.name(*c).unwrap().to_owned()).collect();
        assert_eq!(names.last().unwrap(), "Big", "highest ZIndex is drawn last");
        let rect = |name: &str| placed.iter().find(|(c, _, _)| tree.name(*c) == Ok(name)).unwrap().2;
        assert_eq!(rect("Square").size(), egui::vec2(100.0, 100.0));
        assert_eq!(rect("Big").size(), egui::vec2(200.0, 200.0));
    }

    #[test]
    fn automatic_size_and_list_alignment_work_like_roblox() {
        let mut tree = InstanceTree::new();
        let root = tree.root();
        let screen = gui(&mut tree, ClassKind::ScreenGui, root, "Screen");
        // A stack that grows to fit its rows, centred on the screen.
        let stack = gui(&mut tree, ClassKind::Frame, screen, "Stack");
        set(&mut tree, stack, Property::GuiSize, PropertyValue::UDim2(UDim2::new(0.0, 200.0, 0.0, 0.0)));
        set(&mut tree, stack, Property::GuiPosition, PropertyValue::UDim2(UDim2::new(0.5, 0.0, 0.5, 0.0)));
        set(&mut tree, stack, Property::AnchorPoint, PropertyValue::Vec2(Vec2::new(0.5, 0.5)));
        tree.set_extra(stack, "AutomaticSize", Some(PropertyValue::Number(2.0))).unwrap();
        let list = gui(&mut tree, ClassKind::UIListLayout, stack, "UIListLayout");
        tree.set_extra(list, "HorizontalAlignment", Some(PropertyValue::Number(0.0))).unwrap();
        let rows: Vec<_> =
            (0..3).map(|i| gui(&mut tree, ClassKind::Frame, stack, &format!("Row{i}"))).collect();
        for r in &rows {
            set(&mut tree, *r, Property::GuiSize, PropertyValue::UDim2(UDim2::new(0.0, 100.0, 0.0, 50.0)));
        }
        let area = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let placed = arrange(&tree, screen, area);
        let stack_rect = placed.iter().find(|(c, _, _)| *c == stack).unwrap().2;
        assert_eq!(stack_rect, Rect::from_min_size(Pos2::new(300.0, 225.0), egui::vec2(200.0, 150.0)));
        let inside = arrange(&tree, stack, stack_rect);
        let at = |id| inside.iter().find(|(c, _, _)| *c == id).unwrap().2;
        assert_eq!(at(rows[0]).min, Pos2::new(350.0, 225.0));
        assert_eq!(at(rows[2]).min, Pos2::new(350.0, 325.0));
    }

    #[test]
    fn an_automatic_box_keeps_its_padding_and_grows_for_wrapped_text() {
        let mut tree = InstanceTree::new();
        let root = tree.root();
        let screen = gui(&mut tree, ClassKind::ScreenGui, root, "Screen");
        let boxed = gui(&mut tree, ClassKind::Frame, screen, "Box");
        set(&mut tree, boxed, Property::GuiSize, PropertyValue::UDim2(UDim2::new(0.0, 100.0, 0.0, 0.0)));
        tree.set_extra(boxed, "AutomaticSize", Some(PropertyValue::Number(2.0))).unwrap();
        let pad = gui(&mut tree, ClassKind::UIPadding, boxed, "UIPadding");
        set(&mut tree, pad, Property::PaddingTop, PropertyValue::UDim(UDim::new(0.0, 10.0)));
        set(&mut tree, pad, Property::PaddingBottom, PropertyValue::UDim(UDim::new(0.0, 10.0)));
        let row = gui(&mut tree, ClassKind::Frame, boxed, "Row");
        set(&mut tree, row, Property::GuiSize, PropertyValue::UDim2(UDim2::new(1.0, 0.0, 0.0, 30.0)));
        let area = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let at =
            |tree: &InstanceTree| arrange(tree, screen, area).iter().find(|(c, _, _)| *c == boxed).unwrap().2;
        // Padding above and below the row, not just above it.
        assert_eq!(at(&tree).height(), 50.0);
        // Long wrapped text grows taller than one line.
        let label = gui(&mut tree, ClassKind::TextLabel, screen, "Label");
        set(&mut tree, label, Property::GuiSize, PropertyValue::UDim2(UDim2::new(0.0, 100.0, 0.0, 0.0)));
        tree.set_extra(label, "AutomaticSize", Some(PropertyValue::Number(2.0))).unwrap();
        set(&mut tree, label, Property::TextWrapped, PropertyValue::Bool(true));
        set(&mut tree, label, Property::TextSize, PropertyValue::Number(10.0));
        set(&mut tree, label, Property::Text, PropertyValue::String("word ".repeat(20)));
        let placed = arrange(&tree, screen, area);
        let tall = placed.iter().find(|(c, _, _)| *c == label).unwrap().2.height();
        assert!(tall > 30.0, "wrapped text is {tall} tall");
    }

    #[test]
    fn a_gradient_runs_from_its_start_colour_to_its_end_colour() {
        let g = Gradient {
            colours: (Color3::new(1.0, 0.0, 0.0), Color3::new(0.0, 0.0, 1.0)),
            transparency: (0.0, 1.0),
            rotation: 0.0,
            offset: Vec2::ZERO,
        };
        let r = Rect::from_min_size(Pos2::ZERO, egui::vec2(200.0, 100.0));
        assert_eq!(g.along(r, Pos2::new(0.0, 50.0)), 0.0);
        assert_eq!(g.along(r, Pos2::new(200.0, 0.0)), 1.0);
        assert_eq!(g.at(0.5), (Color3::new(0.5, 0.0, 0.5), 0.5));
        // Turned a quarter, it runs top to bottom.
        let down = Gradient { rotation: 90.0, ..g };
        assert!((down.along(r, Pos2::new(0.0, 100.0)) - 1.0).abs() < 1e-5);
        assert!(down.along(r, Pos2::new(200.0, 0.0)).abs() < 1e-5);
    }

    #[test]
    fn a_surface_gui_covers_its_face_at_pixels_per_stud() {
        use libroxel_instance::{EnumValue, NormalId};
        let mut tree = InstanceTree::new();
        let part = tree.create(ClassKind::Part, "Sign").unwrap();
        tree.set(part, Property::Position, PropertyValue::Vec3(Vec3::new(0.0, 5.0, 0.0))).unwrap();
        tree.set(part, Property::Size, PropertyValue::Vec3(Vec3::new(8.0, 4.0, 1.0))).unwrap();
        tree.set_parent(part, Some(tree.workspace())).unwrap();
        let board = tree.create(ClassKind::SurfaceGui, "Screen").unwrap();
        tree.set_parent(board, Some(part)).unwrap();
        let (face, canvas) = surface_canvas(&tree, board).unwrap();
        assert_eq!(canvas, egui::vec2(400.0, 200.0));
        // The canvas's top left is the front's top left, seen from the front.
        let corner = canvas_to_world(&face, canvas, Pos2::ZERO);
        assert!((corner - bevy::prelude::Vec3::new(4.0, 7.0, -0.51)).length() < 1e-3, "{corner}");
        let middle = canvas_to_world(&face, canvas, Pos2::new(200.0, 100.0));
        assert!((middle - bevy::prelude::Vec3::new(0.0, 5.0, -0.51)).length() < 1e-3, "{middle}");
        tree.set(board, Property::Face, PropertyValue::Enum(EnumValue::NormalId(NormalId::Top))).unwrap();
        tree.set(board, Property::PixelsPerStud, PropertyValue::Number(10.0)).unwrap();
        assert_eq!(surface_canvas(&tree, board).unwrap().1, egui::vec2(80.0, 10.0));
        tree.set(board, Property::Enabled, PropertyValue::Bool(false)).unwrap();
        assert!(surface_canvas(&tree, board).is_none());
    }

    #[test]
    fn size_constraints_keep_objects_within_limits() {
        let mut tree = InstanceTree::new();
        let root = tree.root();
        let screen = gui(&mut tree, ClassKind::ScreenGui, root, "Screen");
        let wide = gui(&mut tree, ClassKind::Frame, screen, "Wide");
        set(&mut tree, wide, Property::GuiSize, PropertyValue::UDim2(UDim2::new(1.0, 0.0, 0.0, 10.0)));
        let limits = gui(&mut tree, ClassKind::UISizeConstraint, wide, "Limits");
        set(
            &mut tree,
            limits,
            Property::MinSize,
            PropertyValue::Vec2(libroxel_instance::Vec2::new(0.0, 40.0)),
        );
        set(
            &mut tree,
            limits,
            Property::MaxSize,
            PropertyValue::Vec2(libroxel_instance::Vec2::new(300.0, 500.0)),
        );
        let placed = arrange(&tree, screen, Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0)));
        let rect = placed.iter().find(|(c, _, _)| *c == wide).unwrap().2;
        assert_eq!(rect.size(), egui::vec2(300.0, 40.0));
    }
}
