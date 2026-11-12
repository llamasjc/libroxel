//! The editor's panels: menu and toolbar, explorer, properties, output,
//! the script editor and file dialogs.

use std::path::PathBuf;

use bevy::camera::Viewport;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::EguiContexts;
use bevy_egui::egui::{self, Color32, Key, KeyboardShortcut, Modifiers, RichText};
use libroxel_engine::view::{CameraMode, CameraRig, EngineState, InputCapture};
use libroxel_engine::{LogEntry, LogLevel, PlaySession};
use libroxel_instance::{
    ClassKind, EnumValue, InstanceId, InstanceTree, PartShape, Property, PropertyValue, place,
};
use libroxel_script::names;

use crate::{Dialog, Editor, Tool, Tunnel, games_dir, gui, templates, viewport};

/// Things the UI asks for; applied after drawing so nothing is borrowed twice.
enum Action {
    New,
    /// One of the starter games, by name.
    OpenStarter(&'static str),
    Open(PathBuf),
    Save,
    SaveTo(PathBuf),
    Play,
    /// Play, letting other computers join.
    Host,
    /// Join a game another computer hosts.
    Join {
        address: String,
        name: String,
    },
    Stop,
    Undo,
    Redo,
    /// Use the terrain brush where a ray met something.
    Brush(libroxel_instance::Vec3, libroxel_instance::Vec3),
    /// Make a hill around a point on the ground.
    MakeHill(libroxel_instance::Vec3),
    ClearTerrain,
    Insert(ClassKind),
    InsertShape(PartShape),
    InsertInto(ClassKind, InstanceId),
    /// Turn the selection a quarter turn around the vertical axis.
    Rotate90(InstanceId),
    Delete(InstanceId),
    Duplicate(InstanceId),
    Select(Option<InstanceId>),
    /// While playing: the player clicked a GUI button.
    ClickButton(InstanceId),
    /// The pointer is over this GUI object (or none), for MouseEnter and MouseLeave.
    GuiHover(Option<InstanceId>),
    /// The mouse wheel moved over a scrolling frame by this much; it can scroll this far.
    Scroll(InstanceId, egui::Vec2, egui::Vec2),
    /// While playing: the player clicked a part in the world.
    ClickPart(InstanceId),
    /// While playing: the player clicked a proximity prompt.
    TriggerPrompt(InstanceId),
    /// While playing: a mouse button went down (true) or up over the view.
    Mouse(libroxel_script::UserInput, bool, bool),
    /// Forget the current game's saved data.
    ClearData,
    /// While playing: hold or put away the tool in a hotbar slot.
    ToggleTool(usize),
    /// A model with a body, a head and a humanoid that scripts can walk around.
    InsertNpc,
    InsertKart,
    /// While playing: start typing into a text box, or stop (saying whether Enter was pressed).
    FocusTextBox(Option<InstanceId>, bool),
    Reparent(InstanceId, InstanceId),
    OpenScript(InstanceId),
    Rename(InstanceId, String),
    RunCommand(String),
    ClearOutput,
}

const INSERTABLE: &[ClassKind] = &[
    ClassKind::Part,
    ClassKind::SpawnLocation,
    ClassKind::Script,
    ClassKind::LocalScript,
    ClassKind::ModuleScript,
    ClassKind::Model,
    ClassKind::Folder,
    ClassKind::IntValue,
    ClassKind::NumberValue,
    ClassKind::StringValue,
    ClassKind::BoolValue,
    ClassKind::ClickDetector,
    ClassKind::ProximityPrompt,
    ClassKind::Tool,
    ClassKind::BillboardGui,
    ClassKind::PointLight,
    ClassKind::SpotLight,
    ClassKind::RemoteEvent,
    ClassKind::BindableEvent,
    ClassKind::ScreenGui,
    ClassKind::Frame,
    ClassKind::TextLabel,
    ClassKind::TextButton,
    ClassKind::TextBox,
    ClassKind::UICorner,
    ClassKind::UIListLayout,
    ClassKind::ScrollingFrame,
    ClassKind::ImageLabel,
    ClassKind::ImageButton,
    ClassKind::UIStroke,
    ClassKind::UIPadding,
    ClassKind::UIGridLayout,
    ClassKind::BodyColors,
    ClassKind::Accessory,
    ClassKind::UIAspectRatioConstraint,
    ClassKind::UIScale,
    ClassKind::Team,
    ClassKind::WeldConstraint,
    ClassKind::Seat,
    ClassKind::VehicleSeat,
    ClassKind::RemoteFunction,
    ClassKind::BindableFunction,
    ClassKind::ObjectValue,
    ClassKind::Vector3Value,
    ClassKind::Color3Value,
    ClassKind::Attachment,
    ClassKind::LinearVelocity,
    ClassKind::VectorForce,
    ClassKind::AlignPosition,
    ClassKind::RopeConstraint,
    ClassKind::SpringConstraint,
    ClassKind::BodyVelocity,
    ClassKind::BodyPosition,
    ClassKind::BodyForce,
    ClassKind::HingeConstraint,
    ClassKind::AngularVelocity,
    ClassKind::BodyAngularVelocity,
    ClassKind::Decal,
    ClassKind::Texture,
    ClassKind::Sound,
    ClassKind::ParticleEmitter,
    ClassKind::Fire,
    ClassKind::Smoke,
    ClassKind::Sparkles,
    ClassKind::Highlight,
];

const SHAPES: &[(PartShape, &str)] = &[
    (PartShape::Block, "Block"),
    (PartShape::Ball, "Ball"),
    (PartShape::Cylinder, "Cylinder"),
    (PartShape::Wedge, "Wedge"),
];

fn icon(class: ClassKind) -> &'static str {
    match class {
        ClassKind::DataModel => "🎮",
        ClassKind::Workspace => "🌐",
        ClassKind::Folder => "📁",
        ClassKind::Model => "📦",
        ClassKind::Part => "⬛",
        ClassKind::SpawnLocation => "🏁",
        ClassKind::Script => "📜",
        ClassKind::ModuleScript => "📘",
        ClassKind::Humanoid => "👤",
        ClassKind::Players => "👥",
        ClassKind::Player => "🙂",
        ClassKind::ReplicatedStorage => "🗄",
        ClassKind::IntValue | ClassKind::NumberValue => "🔢",
        ClassKind::StringValue => "🔤",
        ClassKind::BoolValue => "☑",
        ClassKind::LocalScript => "📄",
        ClassKind::StarterGui | ClassKind::PlayerGui => "🖼",
        ClassKind::ScreenGui => "🖥",
        ClassKind::GuiObject | ClassKind::Frame => "▭",
        ClassKind::TextLabel => "🏷",
        ClassKind::TextButton => "🔘",
        ClassKind::ClickDetector => "👆",
        ClassKind::Lighting => "☀",
        ClassKind::ServerScriptService => "⚙",
        ClassKind::ProximityPrompt => "💬",
        ClassKind::RemoteEvent => "📡",
        ClassKind::BindableEvent => "🔔",
        ClassKind::Tool => "🔧",
        ClassKind::StarterPack | ClassKind::Backpack => "🎒",
        ClassKind::BillboardGui => "🪧",
        ClassKind::Camera => "🎥",
        ClassKind::PointLight | ClassKind::SpotLight => "💡",
        ClassKind::TextBox => "⌨",
        ClassKind::UICorner => "◜",
        ClassKind::UIListLayout => "☰",
        ClassKind::Explosion => "💥",
        ClassKind::WeldConstraint => "🔗",
        ClassKind::Seat => "🪑",
        ClassKind::VehicleSeat => "🏎",
        ClassKind::RemoteFunction => "📞",
        ClassKind::BindableFunction => "🔔",
        ClassKind::ObjectValue => "🔗",
        ClassKind::Vector3Value => "📐",
        ClassKind::Color3Value => "🎨",
        ClassKind::ServerStorage => "🗄",
        ClassKind::StarterPlayer => "👤",
        ClassKind::Attachment => "📍",
        ClassKind::LinearVelocity | ClassKind::BodyVelocity => "➡",
        ClassKind::VectorForce | ClassKind::BodyForce => "💨",
        ClassKind::AlignPosition | ClassKind::BodyPosition => "🎯",
        ClassKind::RopeConstraint => "➰",
        ClassKind::SpringConstraint => "🌀",
        ClassKind::HingeConstraint => "🚪",
        ClassKind::AngularVelocity | ClassKind::BodyAngularVelocity => "🔄",
        ClassKind::Terrain => "🗻",
        ClassKind::Decal => "🏷",
        ClassKind::Sound => "🔊",
        ClassKind::Texture => "🏁",
        ClassKind::ParticleEmitter | ClassKind::Sparkles => "✨",
        ClassKind::Fire => "🔥",
        ClassKind::Smoke => "☁",
        ClassKind::Highlight => "🔆",
        ClassKind::ScrollingFrame => "📜",
        ClassKind::ImageLabel => "🖼",
        ClassKind::ImageButton => "🖲",
        ClassKind::UIStroke => "▢",
        ClassKind::UIPadding => "⬚",
        ClassKind::UIGridLayout => "▦",
        ClassKind::BodyColors => "🎨",
        ClassKind::Accessory => "🎩",
        ClassKind::UIAspectRatioConstraint => "⬌",
        ClassKind::UIScale => "🔍",
        ClassKind::Teams | ClassKind::Team => "🚩",
    }
}

/// Classes that hold Luau source and open in the script editor.
fn is_code(class: ClassKind) -> bool {
    matches!(class, ClassKind::Script | ClassKind::LocalScript | ClassKind::ModuleScript)
}

fn level_color(level: LogLevel) -> Color32 {
    match level {
        LogLevel::Info => Color32::from_gray(220),
        LogLevel::Warn => Color32::from_rgb(240, 190, 60),
        LogLevel::Error => Color32::from_rgb(255, 110, 100),
    }
}

fn log(engine: &mut EngineState, level: LogLevel, text: impl Into<String>) {
    let time = engine
        .session
        .as_ref()
        .map(|s| s.runtime().clock())
        .or_else(|| engine.log.last().map(|l| l.time))
        .unwrap_or(0.0);
    engine.log.push(LogEntry { level, time, text: text.into() });
}

#[allow(clippy::too_many_arguments)]
pub fn editor_ui(
    mut contexts: EguiContexts,
    mut engine: NonSendMut<EngineState>,
    mut editor: NonSendMut<Editor>,
    mut capture: ResMut<InputCapture>,
    mut cameras: Query<(&mut Camera, &GlobalTransform, &mut Transform, &mut CameraRig)>,
    window: Query<&Window, With<PrimaryWindow>>,
    time: Res<Time>,
) -> Result {
    let ctx = contexts.ctx_mut()?.clone();
    let now = time.elapsed_secs_f64();
    let engine = &mut *engine;
    let editor = &mut *editor;
    let tree_rc = engine.tree.clone();
    let playing = engine.is_playing();
    let mut actions: Vec<Action> = Vec::new();
    // A game that has ended no longer needs its internet connection.
    if engine.session.is_none() {
        editor.tunnel = None;
    }
    let hosting = engine.session.as_ref().filter(|s| s.host_address().is_some()).map(|s| {
        let code = match &editor.tunnel {
            Some(Tunnel::Host(tunnel)) => tunnel.code(),
            _ => None,
        };
        (code, s.joined_count())
    });
    editor.platform.tick(hosting);

    // A game played from the main menu ended: back to the menu.
    if editor.player_mode && engine.session.is_none() {
        editor.player_mode = false;
        editor.menu.open = true;
        editor.menu.scanned = false;
    }
    if editor.menu.open {
        capture.pointer = true;
        capture.keyboard = true;
        let mut picks: Vec<crate::menu::Pick> =
            editor.menu.show(&ctx, &mut editor.platform).into_iter().collect();
        if let Some((code, name)) = editor.platform.window(&ctx) {
            picks.push(crate::menu::Pick::Join { code, name });
        }
        for pick in picks {
            menu_pick(pick, engine, editor, now);
        }
        return Ok(());
    }

    shortcuts(&ctx, editor, playing, &mut actions);

    let mut root = egui::Ui::new(
        ctx.clone(),
        "libroxel-root".into(),
        egui::UiBuilder::new().layer_id(egui::LayerId::background()).max_rect(ctx.viewport_rect()),
    );

    if !editor.player_mode {
        egui::Panel::top("menu").show(&mut root, |ui| {
            menu_bar(ui, editor, playing, &mut actions);
        });
    }
    egui::Panel::top("toolbar").show(&mut root, |ui| {
        toolbar(ui, editor, engine.session.as_ref(), &mut actions);
    });
    if !editor.player_mode {
        egui::Panel::left("explorer").resizable(true).default_size(260.0).show(&mut root, |ui| {
            ui.heading("Explorer");
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let tree = tree_rc.borrow();
                explorer_node(ui, &tree, tree.root(), editor, &mut actions, 0);
                editor.reveal_selection = false;
            });
        });
        egui::Panel::right("properties").resizable(true).default_size(300.0).show(&mut root, |ui| {
            ui.heading("Properties");
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                properties(ui, editor, &mut tree_rc.borrow_mut(), now, playing, &mut actions);
            });
        });
        egui::Panel::bottom("output").resizable(true).default_size(170.0).show(&mut root, |ui| {
            output(ui, engine, editor, playing, &mut actions);
        });
    }

    // The space left over is the 3D view.
    let mut gizmo_hot = false;
    let mut gui_hit: gui::GuiHit = None;
    let mut prompt_hit: Option<(InstanceId, egui::Rect)> = None;
    let mut hotbar_hit: Vec<(usize, egui::Rect)> = Vec::new();
    let gui_root = if playing {
        engine.session.as_ref().and_then(|s| s.player_gui())
    } else {
        tree_rc.borrow().service(ClassKind::StarterGui)
    };
    // A script may have moved the keyboard to another text box, or away.
    if let Some(session) = engine.session.as_ref() {
        let focused = session.focused_text_box();
        if focused != editor.typing {
            editor.typing = focused;
            editor.focus_typing = focused.is_some();
        }
    } else {
        editor.typing = None;
    }
    let mut boxes = gui::TextBoxes { typing: editor.typing, ..Default::default() };
    // Pictures are looked for beside the game, then in the games folder.
    let mut folders = Vec::new();
    if let Some(dir) = editor.file.as_ref().and_then(|f| f.parent()) {
        folders.push(dir.to_path_buf());
        folders.push(dir.join("images"));
    }
    folders.push(games_dir().join("images"));
    crate::images::set_folders(folders);
    let central = egui::CentralPanel::no_frame()
        .show(&mut root, |ui| {
            if let Ok((cam, gt, _, _)) = cameras.single() {
                gizmo_hot = viewport::gizmo(
                    ui,
                    editor,
                    &mut tree_rc.borrow_mut(),
                    (cam, gt),
                    ui.max_rect(),
                    now,
                    !playing,
                );
            }
            if let Ok((cam, gt, _, _)) = cameras.single() {
                let selected = if playing { None } else { editor.selection };
                if let Some(hit) = gui::billboards(ui, &tree_rc.borrow(), (cam, gt), selected, &mut boxes) {
                    gui_hit = Some(hit);
                }
            }
            if let Some(root) = gui_root {
                let selected = if playing { None } else { editor.selection };
                if let Some(hit) = gui::draw(ui, &tree_rc.borrow(), root, ui.max_rect(), selected, &mut boxes)
                {
                    gui_hit = Some(hit);
                }
            }
            if playing {
                text_field(ui, editor, &mut tree_rc.borrow_mut(), &boxes, &mut actions);
            }
            if let Some(session) = engine.session.as_ref() {
                hotbar_hit = gui::hotbar(ui, &tree_rc.borrow(), &session.hotbar());
            }
            if let (Some(active), Ok((cam, gt, _, _))) =
                (engine.session.as_ref().and_then(|s| s.active_prompt()), cameras.single())
            {
                let tree = tree_rc.borrow();
                // Just above the part, so the character doesn't hide it.
                let at = tree
                    .get_vec3(active.part, Property::Position)
                    .zip(tree.get_vec3(active.part, Property::Size))
                    .map(|(p, s)| Vec3::new(p.x, p.y + s.y / 2.0 + 1.5, p.z));
                if let Some(at) = at
                    && let Ok(screen) = cam.world_to_viewport(gt, at)
                {
                    let key = names::enum_item_name(EnumValue::KeyCode(active.key));
                    let rect = gui::draw_prompt(
                        ui,
                        &tree,
                        active.prompt,
                        key,
                        active.progress,
                        egui::pos2(screen.x, screen.y),
                    );
                    prompt_hit = Some((active.prompt, rect));
                }
            }
            hud(ui, playing);
            if playing {
                leaderboard(ui, &tree_rc.borrow());
            }
            ui.max_rect()
        })
        .inner;

    script_editor(&ctx, editor, &mut tree_rc.borrow_mut(), now, playing);
    dialogs(&ctx, editor, &mut actions);
    if let Some((address, name)) = editor.platform.window(&ctx) {
        actions.push(Action::Join { address, name });
    }

    // Decide who gets the mouse and keyboard this frame.
    let pointer = ctx.input(|i| i.pointer.hover_pos());
    let over_view = pointer.is_some_and(|p| {
        central.contains(p) && ctx.layer_id_at(p).is_none_or(|l| l.order == egui::Order::Background)
    });
    let pressed_in_view = ctx.input(|i| i.pointer.press_origin()).is_none_or(|p| central.contains(p));
    // The mouse wheel over a scrolling frame scrolls it, and not the camera.
    let scrolling = boxes.scroll.filter(|_| over_view);
    capture.pointer = !(over_view && pressed_in_view) || gizmo_hot || scrolling.is_some();
    if let Some((frame, max)) = scrolling {
        let delta = ctx.input(|i| i.smooth_scroll_delta);
        if delta != egui::Vec2::ZERO {
            actions.push(Action::Scroll(frame, delta, max));
        }
    }
    if playing {
        actions.push(Action::GuiHover(gui_hit.map(|(id, _)| id).filter(|_| over_view)));
    }
    capture.keyboard = ctx.egui_wants_keyboard_input();
    editor.gizmo_active = gizmo_hot;
    if let Some(session) = engine.session.as_mut() {
        session.set_viewport(central.width(), central.height());
    }

    // Where the mouse points, for scripts' `Player:GetMouse()`.
    if let Some(session) = engine.session.as_mut() {
        let ray = pointer
            .filter(|_| over_view)
            .zip(cameras.single().ok())
            .and_then(|(pos, (cam, gt, _, _))| cam.viewport_to_world(gt, Vec2::new(pos.x, pos.y)).ok())
            .map(|ray| {
                let (o, d) = (ray.origin, ray.direction.as_vec3());
                libroxel_script::MouseRay {
                    origin: libroxel_instance::Vec3::new(o.x, o.y, o.z),
                    direction: libroxel_instance::Vec3::new(d.x, d.y, d.z),
                    x: pointer.map_or(0.0, |p| p.x - central.min.x),
                    y: pointer.map_or(0.0, |p| p.y - central.min.y),
                }
            });
        session.set_mouse(ray);
    }
    let on_hotbar = hotbar_hit.iter().copied().find(|(_, r)| pointer.is_some_and(|p| r.contains(p)));

    // Mouse buttons for the input service, while playing.
    if playing && over_view {
        let on_ui = gui_hit.is_some()
            || on_hotbar.is_some()
            || prompt_hit.is_some_and(|(_, r)| pointer.is_some_and(|p| r.contains(p)));
        ctx.input(|i| {
            for (button, input) in [
                (egui::PointerButton::Primary, libroxel_script::UserInput::MouseButton1),
                (egui::PointerButton::Secondary, libroxel_script::UserInput::MouseButton2),
            ] {
                if i.pointer.button_pressed(button) {
                    actions.push(Action::Mouse(input, true, on_ui));
                }
                if i.pointer.button_released(button) {
                    actions.push(Action::Mouse(input, false, on_ui));
                }
            }
        });
    }

    // The terrain brush works while the mouse is held down.
    if editor.tool == Tool::Terrain
        && !playing
        && over_view
        && gui_hit.is_none()
        && now >= editor.brush_ready
        && ctx.input(|i| i.pointer.primary_down())
        && let (Some(pos), Ok((cam, gt, _, _))) = (pointer, cameras.single())
        && let Ok(ray) = cam.viewport_to_world(gt, Vec2::new(pos.x, pos.y))
    {
        let dir = ray.direction.as_vec3() * 10_000.0;
        let from = libroxel_instance::Vec3::new(ray.origin.x, ray.origin.y, ray.origin.z);
        let to = libroxel_instance::Vec3::new(dir.x, dir.y, dir.z);
        if let Some(hit) = libroxel_instance::geometry::raycast(&tree_rc.borrow(), from, to, |_| true) {
            actions.push(Action::Brush(hit.position, hit.normal));
            editor.brush_ready = now + 0.08;
        }
    }
    // Asking for a hill: put it where the middle of the view looks at the ground.
    for action in actions.iter_mut() {
        if let Action::MakeHill(at) = action
            && let Ok((cam, gt, _, _)) = cameras.single()
            && let Some(size) = cam.logical_viewport_size()
        {
            let middle = cam.logical_viewport_rect().map_or(size / 2.0, |r| r.center());
            if let Ok(ray) = cam.viewport_to_world(gt, middle) {
                let dir = ray.direction.as_vec3();
                let from = libroxel_instance::Vec3::new(ray.origin.x, ray.origin.y, ray.origin.z);
                let reach = libroxel_instance::Vec3::new(dir.x, dir.y, dir.z) * 2000.0;
                *at = match libroxel_instance::geometry::raycast(&tree_rc.borrow(), from, reach, |_| true) {
                    Some(hit) => hit.position,
                    // Nothing there: wherever the view meets the ground plane.
                    None if dir.y < -0.05 => {
                        from + libroxel_instance::Vec3::new(dir.x, dir.y, dir.z) * (-from.y / dir.y)
                    }
                    None => libroxel_instance::Vec3::ZERO,
                };
            }
        }
    }

    // Click to select (the terrain tool uses clicks for itself).
    let clicked = ctx.input(|i| i.pointer.primary_clicked()) && editor.tool != Tool::Terrain;
    let on_prompt = prompt_hit.filter(|(_, r)| pointer.is_some_and(|p| r.contains(p)));
    if clicked
        && over_view
        && !gizmo_hot
        && let (Some(pos), Ok((cam, gt, _, _))) = (pointer, cameras.single())
        && let Ok(ray) = cam.viewport_to_world(gt, Vec2::new(pos.x, pos.y))
    {
        match gui_hit {
            _ if on_hotbar.is_some() => {
                if let Some((slot, _)) = on_hotbar {
                    actions.push(Action::ToggleTool(slot));
                }
            }
            _ if on_prompt.is_some() => {
                if let Some((prompt, _)) = on_prompt {
                    actions.push(Action::TriggerPrompt(prompt));
                }
            }
            Some((id, class)) if playing && class.is_button() => actions.push(Action::ClickButton(id)),
            Some((id, ClassKind::TextBox)) if playing => {
                if editor.typing != Some(id) {
                    actions.push(Action::FocusTextBox(Some(id), false));
                }
            }
            Some(_) if playing => {}
            Some((id, _)) => {
                editor.reveal_selection = true;
                actions.push(Action::Select(Some(id)));
            }
            None => {
                let hit = viewport::pick(&tree_rc.borrow(), ray);
                match hit {
                    Some(part) if playing => actions.push(Action::ClickPart(part)),
                    _ => {
                        editor.reveal_selection = hit.is_some();
                        actions.push(Action::Select(hit));
                    }
                }
            }
        }
    }

    // Fit the 3D camera to the free space between panels.
    if let (Ok(window), Ok((mut cam, _, mut transform, rig))) = (window.single(), cameras.single_mut()) {
        let scale = window.scale_factor();
        let min = UVec2::new((central.min.x * scale) as u32, (central.min.y * scale) as u32);
        let max = UVec2::new(
            ((central.max.x * scale) as u32).min(window.physical_width()),
            ((central.max.y * scale) as u32).min(window.physical_height()),
        );
        if max.x > min.x + 8 && max.y > min.y + 8 {
            cam.viewport = Some(Viewport { physical_position: min, physical_size: max - min, ..default() });
        }
        if std::mem::take(&mut editor.focus_selection)
            && rig.mode == CameraMode::Fly
            && let Some(id) = editor.selection
        {
            viewport::frame_selection(&tree_rc.borrow(), id, &mut transform);
        }
    }

    editor.history.tick(now, gizmo_hot && ctx.input(|i| i.pointer.primary_down()));
    for action in actions {
        apply(action, engine, editor, now);
    }

    // Remember the editing camera while playing, and put it back after.
    if let Ok((_, _, mut transform, mut rig)) = cameras.single_mut() {
        if engine.is_playing() && !playing {
            editor.saved_camera = Some((*transform, *rig));
            rig.distance = 16.0;
        } else if !engine.is_playing()
            && playing
            && let Some((saved_transform, saved_rig)) = editor.saved_camera.take()
        {
            *transform = saved_transform;
            *rig = saved_rig;
        }
    }
    Ok(())
}

/// A walking dummy standing on `ground`: a body part (its root), a head on
/// top, a humanoid, and a script that walks it back and forth.
fn new_npc(tree: &mut InstanceTree, ground: libroxel_instance::Vec3) -> Option<InstanceId> {
    use libroxel_instance::{Color3, Vec3 as V};
    let ws = tree.workspace();
    let model = tree.create(ClassKind::Model, "Dummy").ok()?;
    let root = tree.create(ClassKind::Part, names::character::ROOT_PART).ok()?;
    let _ = tree.set(root, Property::Size, PropertyValue::Vec3(V::new(2.0, 4.0, 1.5)));
    let _ = tree.set(root, Property::Position, PropertyValue::Vec3(ground + V::new(0.0, 2.1, 0.0)));
    let _ = tree.set(root, Property::Transparency, PropertyValue::Number(1.0));
    tree.set_parent(root, Some(model)).ok()?;
    // A body like a player's, in its own colours.
    let colors = libroxel_engine::character::BodyColors {
        skin: Color3::from_rgb8(205, 170, 130),
        shirt: Color3::from_rgb8(150, 150, 160),
        pants: Color3::from_rgb8(90, 90, 100),
    };
    let (head, limbs) = libroxel_engine::character::build_rig(tree, model, colors);
    libroxel_engine::character::pose_rig(tree, root, head, &limbs, Default::default());
    let humanoid = tree.create(ClassKind::Humanoid, names::character::HUMANOID).ok()?;
    tree.set_parent(humanoid, Some(model)).ok()?;
    let script = tree.create(ClassKind::Script, "Walk").ok()?;
    let source = "-- Walks back and forth between two points.\nlocal model = script.Parent\nlocal humanoid = model.Humanoid\nlocal start = model.HumanoidRootPart.Position\n\nwhile true do\n\thumanoid:MoveTo(start + Vector3.new(10, 0, 0))\n\thumanoid.MoveToFinished:Wait()\n\thumanoid:MoveTo(start)\n\thumanoid.MoveToFinished:Wait()\nend\n";
    let _ = tree.set(script, Property::Source, PropertyValue::String(source.into()));
    tree.set_parent(script, Some(model)).ok()?;
    tree.set_parent(model, Some(ws)).ok()?;
    Some(model)
}

/// While playing, the text field for the text box being typed into, drawn
/// over the box. Typing writes straight into the box's Text.
fn text_field(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    tree: &mut InstanceTree,
    boxes: &gui::TextBoxes,
    actions: &mut Vec<Action>,
) {
    let Some(id) = editor.typing else { return };
    let Some(&(_, rect)) = boxes.found.iter().find(|(b, _)| *b == id) else {
        // It was hidden or removed: stop typing.
        actions.push(Action::FocusTextBox(None, false));
        return;
    };
    let mut text = tree.get_string(id, Property::Text).unwrap_or_default().to_owned();
    let size = tree.get_number(id, Property::TextSize).unwrap_or(14.0).max(1.0) as f32;
    let c = tree.get_color(id, Property::TextColor).unwrap_or(libroxel_instance::Color3::new(0.1, 0.16, 0.2));
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let hint = tree.get_string(id, Property::PlaceholderText).unwrap_or_default().to_owned();
    let field = egui::TextEdit::singleline(&mut text)
        .frame(egui::Frame::NONE)
        .font(egui::FontId::proportional(size))
        .text_color(Color32::from_rgb(byte(c.r), byte(c.g), byte(c.b)))
        .hint_text(hint)
        .horizontal_align(egui::Align::Center)
        .vertical_align(egui::Align::Center);
    let response = ui.put(rect, field);
    if std::mem::take(&mut editor.focus_typing) {
        response.request_focus();
    }
    if response.changed() {
        let _ = tree.set(id, Property::Text, PropertyValue::String(text));
    }
    if response.lost_focus() {
        let enter = ui.input(|i| i.key_pressed(Key::Enter));
        actions.push(Action::FocusTextBox(None, enter));
    }
}

fn shortcuts(ctx: &egui::Context, editor: &mut Editor, playing: bool, actions: &mut Vec<Action>) {
    let typing = ctx.egui_wants_keyboard_input();
    ctx.input_mut(|i| {
        let cmd = |key| KeyboardShortcut::new(Modifiers::COMMAND, key);
        if i.consume_shortcut(&cmd(Key::S)) {
            actions.push(Action::Save);
        }
        if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z))
            || i.consume_shortcut(&cmd(Key::Y))
        {
            actions.push(Action::Redo);
        }
        if i.consume_shortcut(&cmd(Key::Z)) {
            actions.push(Action::Undo);
        }
        if i.consume_key(Modifiers::NONE, Key::F5) {
            actions.push(if playing { Action::Stop } else { Action::Play });
        }
        if playing && i.consume_key(Modifiers::NONE, Key::Escape) {
            actions.push(Action::Stop);
        }
        if typing {
            return;
        }
        for (key, tool) in [
            (Key::Num1, Tool::Select),
            (Key::Num2, Tool::Move),
            (Key::Num3, Tool::Scale),
            (Key::Num4, Tool::Rotate),
            (Key::Num5, Tool::Terrain),
        ] {
            if i.consume_shortcut(&cmd(key)) {
                editor.tool = tool;
            }
        }
        if let Some(id) = editor.selection {
            if i.consume_shortcut(&cmd(Key::D)) {
                actions.push(Action::Duplicate(id));
            }
            if i.consume_shortcut(&cmd(Key::R)) {
                actions.push(Action::Rotate90(id));
            }
            if i.consume_key(Modifiers::NONE, Key::Delete) {
                actions.push(Action::Delete(id));
            }
        }
    });
}

fn menu_bar(ui: &mut egui::Ui, editor: &mut Editor, playing: bool, actions: &mut Vec<Action>) {
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("File", |ui| {
            if ui.add_enabled(!playing, egui::Button::new("Main menu")).clicked() {
                editor.menu.open = true;
                editor.menu.scanned = false;
            }
            ui.separator();
            if ui.button("New").clicked() {
                actions.push(Action::New);
            }
            ui.menu_button("Open a starter game", |ui| {
                for game in crate::starter_games::STARTER_GAMES {
                    if ui.button(format!("{} {}", game.symbol, game.name)).on_hover_text(game.about).clicked()
                    {
                        actions.push(Action::OpenStarter(game.name));
                    }
                }
            });
            if ui
                .add_enabled(!playing, egui::Button::new("Clear saved data"))
                .on_hover_text("Forget what this game's data stores saved in earlier play tests")
                .clicked()
            {
                actions.push(Action::ClearData);
            }
            if ui.button("Open…").clicked() {
                let path = games_dir().join("untitled.lrx").display().to_string();
                editor.dialog = Some(Dialog::Open { path });
            }
            ui.separator();
            if ui.add_enabled(!playing, egui::Button::new("Save        Ctrl+S")).clicked() {
                actions.push(Action::Save);
            }
            if ui.add_enabled(!playing, egui::Button::new("Save as…")).clicked() {
                let path = editor
                    .file
                    .clone()
                    .unwrap_or_else(|| games_dir().join("untitled.lrx"))
                    .display()
                    .to_string();
                editor.dialog = Some(Dialog::SaveAs { path });
            }
        });
        ui.menu_button("Edit", |ui| {
            if ui
                .add_enabled(!playing && editor.history.can_undo(), egui::Button::new("Undo   Ctrl+Z"))
                .clicked()
            {
                actions.push(Action::Undo);
            }
            if ui
                .add_enabled(!playing && editor.history.can_redo(), egui::Button::new("Redo   Ctrl+Y"))
                .clicked()
            {
                actions.push(Action::Redo);
            }
            ui.separator();
            let selected = editor.selection;
            if ui.add_enabled(selected.is_some(), egui::Button::new("Duplicate   Ctrl+D")).clicked()
                && let Some(id) = selected
            {
                actions.push(Action::Duplicate(id));
            }
            if ui.add_enabled(selected.is_some(), egui::Button::new("Delete   Del")).clicked()
                && let Some(id) = selected
            {
                actions.push(Action::Delete(id));
            }
            if ui.add_enabled(selected.is_some(), egui::Button::new("Turn 90°   Ctrl+R")).clicked()
                && let Some(id) = selected
            {
                actions.push(Action::Rotate90(id));
            }
        });
        ui.menu_button("Insert", |ui| {
            for &class in INSERTABLE {
                if ui.button(format!("{} {}", icon(class), names::class_name(class))).clicked() {
                    actions.push(Action::Insert(class));
                }
            }
            ui.separator();
            for &(shape, label) in SHAPES {
                if ui.button(format!("{} part", label)).clicked() {
                    actions.push(Action::InsertShape(shape));
                }
            }
            ui.separator();
            if ui.button("👤 Character (NPC)").clicked() {
                actions.push(Action::InsertNpc);
            }
            if ui.button("🏎 Go-kart").clicked() {
                actions.push(Action::InsertKart);
            }
        });
        ui.menu_button("View", |ui| {
            ui.checkbox(&mut editor.show_grid, "Show grid");
        });
        ui.menu_button("Help", |ui| {
            ui.label("Right mouse + WASD/QE: fly the camera (Shift = faster)");
            ui.label("Scroll: move forward/back");
            ui.label("Click: select · Del: delete · Ctrl+D: duplicate · Ctrl+R: turn 90°");
            ui.label("Ctrl+1..4: select, move, resize, rotate tools");
            ui.label("F5: play / stop · Esc: stop");
            ui.label("While playing: WASD walk, Space jump, right mouse turns the camera");
        });
    });
}

fn toolbar(ui: &mut egui::Ui, editor: &mut Editor, session: Option<&PlaySession>, actions: &mut Vec<Action>) {
    let playing = session.is_some();
    ui.horizontal(|ui| {
        if playing {
            let stop = if editor.player_mode { "⏹ Leave game" } else { "⏹ Stop" };
            if ui.button(RichText::new(stop).strong().color(Color32::from_rgb(255, 120, 110))).clicked() {
                actions.push(Action::Stop);
            }
        } else {
            if ui.button(RichText::new("▶ Play").strong().color(Color32::from_rgb(120, 220, 120))).clicked()
            {
                actions.push(Action::Play);
            }
            if ui.button("🌐 Host").on_hover_text("Play, and let other computers join").clicked() {
                actions.push(Action::Host);
            }
            if ui.button("🔗 Join").on_hover_text("Join a game another computer hosts").clicked() {
                editor.dialog = Some(Dialog::Join { address: "127.0.0.1".into(), name: "Player".into() });
            }
            if ui.button("🌍 Browse").on_hover_text("Find games being hosted right now").clicked() {
                editor.platform.open = true;
            }
        }
        if let Some(session) = session {
            let red = Color32::from_rgb(255, 120, 110);
            if let Some(address) = session.host_address() {
                let joined = session.joined_count();
                let who = if joined == 1 { "1 joined".to_owned() } else { format!("{joined} joined") };
                ui.label(format!("🌐 Hosting on port {} · {who}", address.port()))
                    .on_hover_text("Others on your network join with this computer's address and this port");
                if let Some(Tunnel::Host(tunnel)) = &editor.tunnel {
                    if let Some(code) = tunnel.code() {
                        let copy = ui
                            .button("📋 Copy join code")
                            .on_hover_text(format!("Friends anywhere paste this into Join:\n{code}"));
                        if copy.clicked() {
                            ui.ctx().copy_text(code);
                        }
                    } else if let Some(error) = tunnel.error() {
                        ui.colored_label(red, "No join code").on_hover_text(error);
                    } else {
                        ui.label("⏳ Getting a join code…");
                    }
                }
            } else if session.is_joined() {
                let tunnel = match &editor.tunnel {
                    Some(Tunnel::Join(tunnel)) => Some(tunnel),
                    _ => None,
                };
                if let Some(error) = tunnel.and_then(|t| t.error()) {
                    ui.colored_label(red, "🔗 Could not reach the host").on_hover_text(error);
                } else if session.disconnected() {
                    ui.colored_label(red, "🔗 Disconnected");
                } else if tunnel.is_some_and(|t| !t.connected()) {
                    ui.label("⏳ Connecting…");
                } else {
                    ui.label("🔗 Joined");
                }
            }
        }
        if editor.player_mode {
            return;
        }
        ui.separator();
        ui.selectable_value(&mut editor.tool, Tool::Select, "🖱 Select");
        ui.selectable_value(&mut editor.tool, Tool::Move, "✥ Move");
        ui.selectable_value(&mut editor.tool, Tool::Scale, "⤢ Resize");
        ui.selectable_value(&mut editor.tool, Tool::Rotate, "⟳ Rotate");
        if !playing {
            ui.selectable_value(&mut editor.tool, Tool::Terrain, "🗻 Terrain")
                .on_hover_text("Add, dig or paint terrain by clicking and dragging in the view");
        }
        ui.separator();
        ui.label("Snap");
        egui::ComboBox::from_id_salt("snap")
            .selected_text(if editor.snap > 0.0 { format!("{} stud", editor.snap) } else { "Off".into() })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut editor.snap, 0.0, "Off");
                for s in [0.25, 0.5, 1.0, 2.0, 4.0] {
                    ui.selectable_value(&mut editor.snap, s, format!("{s} stud"));
                }
            });
        egui::ComboBox::from_id_salt("angle-snap")
            .selected_text(if editor.angle_snap > 0.0 {
                format!("{}°", editor.angle_snap)
            } else {
                "Free".into()
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut editor.angle_snap, 0.0, "Free");
                for a in [5.0, 15.0, 45.0, 90.0] {
                    ui.selectable_value(&mut editor.angle_snap, a, format!("{a}°"));
                }
            });
        ui.separator();
        ui.add_enabled_ui(!playing, |ui| {
            ui.menu_button("+ Part", |ui| {
                for &(shape, label) in SHAPES {
                    if ui.button(label).clicked() {
                        actions.push(Action::InsertShape(shape));
                        ui.close();
                    }
                }
            });
            if ui.button("+ Script").clicked() {
                actions.push(Action::Insert(ClassKind::Script));
            }
        });
        if playing {
            ui.separator();
            ui.label(RichText::new("Playing: edits are undone when you stop").italics());
        }
    });
    if editor.tool == Tool::Terrain && !playing {
        terrain_bar(ui, editor, actions);
    }
}

/// The terrain tool's options, under the toolbar.
fn terrain_bar(ui: &mut egui::Ui, editor: &mut Editor, actions: &mut Vec<Action>) {
    use crate::terrain_tools::BrushMode;
    ui.horizontal(|ui| {
        ui.label("🗻 Terrain:");
        let brush = &mut editor.brush;
        ui.selectable_value(&mut brush.mode, BrushMode::Add, "Add");
        ui.selectable_value(&mut brush.mode, BrushMode::Dig, "Dig");
        ui.selectable_value(&mut brush.mode, BrushMode::Paint, "Paint");
        ui.separator();
        egui::ComboBox::from_id_salt("terrain-material")
            .selected_text(libroxel_script::names::enum_item_name(EnumValue::Material(brush.material)))
            .show_ui(ui, |ui| {
                for &m in libroxel_instance::voxels::TERRAIN_MATERIALS {
                    ui.selectable_value(
                        &mut brush.material,
                        m,
                        libroxel_script::names::enum_item_name(EnumValue::Material(m)),
                    );
                }
            });
        ui.add(egui::Slider::new(&mut brush.size, 1.0..=32.0).text("studs across"));
        ui.separator();
        if ui.button("Make a hill").on_hover_text("A grassy hill where the view is looking").clicked() {
            actions.push(Action::MakeHill(libroxel_instance::Vec3::ZERO));
        }
        if ui.button("Clear terrain").clicked() {
            actions.push(Action::ClearTerrain);
        }
    });
}

fn explorer_node(
    ui: &mut egui::Ui,
    tree: &InstanceTree,
    id: InstanceId,
    editor: &mut Editor,
    actions: &mut Vec<Action>,
    depth: usize,
) {
    let Ok(class) = tree.class(id) else { return };
    let name = tree.name(id).unwrap_or("?");
    let children = tree.children(id).map(|c| c.to_vec()).unwrap_or_default();
    let selected = editor.selection == Some(id);

    let mut label = |ui: &mut egui::Ui| {
        if let Some((rid, text)) = editor.renaming.as_mut()
            && *rid == id
        {
            let response = ui.text_edit_singleline(text);
            response.request_focus();
            if response.lost_focus() {
                actions.push(Action::Rename(id, text.clone()));
            }
            return;
        }
        let text = format!("{} {}", icon(class), name);
        let inner = ui.add(
            egui::Button::selectable(selected, text)
                .frame_when_inactive(false)
                .sense(egui::Sense::click_and_drag()),
        );
        if class.is_user_creatable() {
            inner.dnd_set_drag_payload(id);
        }
        if inner.clicked() {
            actions.push(Action::Select(Some(id)));
        }
        if inner.double_clicked() {
            if is_code(class) {
                actions.push(Action::OpenScript(id));
            } else {
                editor.focus_selection = true;
            }
        }
        if let Some(dragged) = inner.dnd_release_payload::<InstanceId>() {
            actions.push(Action::Reparent(*dragged, id));
        }
        inner.context_menu(|ui| {
            actions.push(Action::Select(Some(id)));
            ui.menu_button("Insert", |ui| {
                for &c in INSERTABLE {
                    if ui.button(format!("{} {}", icon(c), names::class_name(c))).clicked() {
                        actions.push(Action::InsertInto(c, id));
                        ui.close();
                    }
                }
            });
            let editable = class.is_user_creatable();
            if ui.add_enabled(editable, egui::Button::new("Rename")).clicked() {
                editor.renaming = Some((id, name.to_owned()));
                ui.close();
            }
            if ui.add_enabled(editable, egui::Button::new("Duplicate")).clicked() {
                actions.push(Action::Duplicate(id));
                ui.close();
            }
            if ui.add_enabled(editable, egui::Button::new("Delete")).clicked() {
                actions.push(Action::Delete(id));
                ui.close();
            }
            if is_code(class) && ui.button("Edit script").clicked() {
                actions.push(Action::OpenScript(id));
                ui.close();
            }
        });
    };

    if children.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(18.0);
            label(ui);
        });
    } else {
        let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
            ui.ctx(),
            egui::Id::new(("explorer-open", id)),
            depth < 2,
        );
        if editor.reveal_selection
            && editor.selection.is_some_and(|sel| tree.is_descendant_of(sel, id).unwrap_or(false))
        {
            state.set_open(true);
        }
        state.show_header(ui, |ui| label(ui)).body(|ui| {
            for child in children {
                explorer_node(ui, tree, child, editor, actions, depth + 1);
            }
        });
    }
}

fn properties(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    tree: &mut InstanceTree,
    now: f64,
    playing: bool,
    actions: &mut Vec<Action>,
) {
    let Some(id) = editor.selection.filter(|id| tree.exists(*id) && !tree.is_destroyed(*id).unwrap_or(true))
    else {
        ui.label("Select something in the Explorer or click a part in the view.");
        return;
    };
    let Ok(class) = tree.class(id) else { return };
    ui.label(RichText::new(format!("{} {}", icon(class), names::class_name(class))).strong());
    ui.add_space(4.0);

    let mut edited: Option<(Property, PropertyValue)> = None;
    egui::Grid::new("props").num_columns(2).striped(true).spacing([8.0, 6.0]).show(ui, |ui| {
        for &property in class.properties() {
            let Ok(value) = tree.get(id, property) else { continue };
            ui.label(names::property_name(property));
            if let PropertyValue::Ref(target) = &value {
                if matches!(
                    property,
                    Property::Part0
                        | Property::Part1
                        | Property::Attachment0
                        | Property::Attachment1
                        | Property::ObjectValue
                        | Property::Adornee
                ) {
                    if let Some(new) = part_picker(ui, tree, id, property, *target) {
                        edited = Some((property, PropertyValue::Ref(new)));
                    }
                } else {
                    let name = target.and_then(|t| tree.name(t).ok()).unwrap_or("(none)");
                    ui.label(RichText::new(name).weak());
                }
                ui.end_row();
                continue;
            }
            let locked =
                (property == Property::Name && !class.is_user_creatable()) || property == Property::Character;
            ui.add_enabled_ui(!locked, |ui| {
                if let Some(new) = property_editor(ui, property, value, id, actions) {
                    edited = Some((property, new));
                }
            });
            ui.end_row();
        }
    });
    if let Some((property, value)) = edited {
        if !playing {
            editor.history.begin_edit(tree, now);
            editor.dirty = true;
        }
        let _ = tree.set(id, property, value);
    }
    attributes_and_tags(ui, editor, tree, id, now, playing, actions);

    let hint = match class {
        ClassKind::Script => Some(
            "Scripts start when you press Play. `script` is this Script; `script.Parent` is what it's inside.",
        ),
        ClassKind::ModuleScript => Some(
            "Module scripts don't run on their own. Another script loads one with `require(path.to.Module)` \
             and gets back whatever the module returns.",
        ),
        ClassKind::Players => Some(
            "While playing, each player appears here. Scripts can add a `leaderstats` folder with values to show them on the leaderboard.",
        ),
        ClassKind::ReplicatedStorage => Some("A place for modules and templates that aren't in the world."),
        ClassKind::LocalScript => Some(
            "Local scripts run for the player, from inside a screen GUI in StarterGui. \
             `game.Players.LocalPlayer` is the player.",
        ),
        ClassKind::ServerScriptService => {
            Some("Scripts in here start when you press Play, without being part of the world.")
        }
        ClassKind::StarterGui => Some(
            "Screen GUIs in here are copied to each player when they spawn, and drawn over the view. \
             Click a GUI object in the view to select it.",
        ),
        ClassKind::ScreenGui => {
            Some("Put frames, text labels and text buttons inside to show them on screen.")
        }
        ClassKind::TextButton => {
            Some("A local script can listen with `button.MouseButton1Click:Connect(function() ... end)`.")
        }
        ClassKind::ClickDetector => Some(
            "Put this inside a part. While playing, clicking the part fires `MouseClick` with the player, \
             if their character is close enough.",
        ),
        ClassKind::ProximityPrompt => Some(
            "Put this inside a part. While playing, walking close shows a prompt; pressing its key (or \
             clicking it) fires `Triggered` with the player. HoldDuration makes the player hold the key.",
        ),
        ClassKind::RemoteEvent => Some(
            "Local scripts call `FireServer(...)` and game scripts hear `OnServerEvent(player, ...)`. \
             Game scripts call `FireClient(player, ...)` or `FireAllClients(...)` and local scripts hear `OnClientEvent`.",
        ),
        ClassKind::Tool => Some(
            "Tools in StarterPack are given to each player. While playing, press 1-9 or click the hotbar to hold one; \
             clicking then fires `Activated`. The part called Handle is what the character holds.",
        ),
        ClassKind::StarterPack => {
            Some("Tools in here go into every player's backpack when their character spawns.")
        }
        ClassKind::BillboardGui => Some(
            "Put this inside a part, with text labels or frames inside it. It floats StudsOffset above the part, \
             facing the camera. Size scale is in studs; offset is in pixels.",
        ),
        ClassKind::BindableEvent => {
            Some("Any script can call `Fire(...)`; everything connected to `Event` gets the same values.")
        }
        ClassKind::PointLight | ClassKind::SpotLight => Some(
            "Put this inside a part. It lights everything within Range studs; a spot light shines out of the \
             part's front in a cone Angle degrees wide. Lights show best at night (Lighting.ClockTime).",
        ),
        ClassKind::TextBox => Some(
            "While playing, click it to type. A local script can read `box.Text`, and \
             `box.FocusLost:Connect(function(enterPressed) ... end)` runs when the player is done.",
        ),
        ClassKind::UICorner => {
            Some("Put this inside a frame, label, button or text box to round its corners.")
        }
        ClassKind::UIListLayout => Some(
            "Put this inside a frame: the other things in that frame line up in a column (or a row), \
             lowest LayoutOrder first, with Padding between them.",
        ),
        ClassKind::Camera => Some(
            "The player's view. Scripts reach it as `workspace.CurrentCamera`; set CameraType to Scriptable \
             and move its CFrame to take over the view.",
        ),
        ClassKind::Team => Some(
            "Players joining go on the AutoAssignable team with the fewest players. A SpawnLocation with \
             Neutral off and the same TeamColor is where that team spawns. Scripts can set `player.Team`.",
        ),
        ClassKind::Teams => {
            Some("Teams in here split the players up; each one has a colour (Insert > Team).")
        }
        ClassKind::Humanoid => Some(
            "A model with a Humanoid and an unanchored part called HumanoidRootPart walks: \
             `humanoid:MoveTo(point)` sends it, `MoveToFinished` says when it got there, and setting `Jump` jumps.",
        ),
        ClassKind::Seat => Some(
            "Characters that touch it sit down; jumping gets up. Scripts can use `seat:Sit(humanoid)`, \
             read `Occupant`, and set `Humanoid.Sit` to false. Disabled stops anyone sitting.",
        ),
        ClassKind::VehicleSeat => Some(
            "Sit in it to drive: WASD sets Throttle and Steer, and it pushes whatever it's welded to \
             (unanchored) at up to MaxSpeed. Insert > Go-kart builds a ready-made kart.",
        ),
        ClassKind::RemoteFunction => Some(
            "A local script calls `InvokeServer(...)` and waits; the game script's \
             `remote.OnServerInvoke = function(player, ...) return answer end` answers it.",
        ),
        ClassKind::BindableFunction => Some(
            "Set `OnInvoke = function(...) return answer end` in one script; any other script can call \
             `Invoke(...)` and gets the answer back.",
        ),
        ClassKind::ServerStorage => {
            Some("A place for templates and modules only game scripts use. Nothing in here is in the world.")
        }
        ClassKind::StarterPlayer => {
            Some("CharacterWalkSpeed and CharacterJumpPower are what each new character starts with.")
        }
        ClassKind::Lighting => Some(
            "ClockTime sets the time of day: 6 is sunrise, 12 noon, 18 sunset. Ambient lights the shadows, \
             and things further than FogStart fade into FogColor until they vanish at FogEnd.",
        ),
        ClassKind::Attachment => Some(
            "A point on its part (Position is relative to the part). Movers and constraints hold \
             attachments: insert one with a part selected and it gets an attachment on that part.",
        ),
        ClassKind::LinearVelocity => Some(
            "While playing, pushes Attachment0's part to move at VectorVelocity (studs per second), \
             with up to MaxForce. Unanchor the part first.",
        ),
        ClassKind::VectorForce => Some(
            "While playing, pushes Attachment0's part with Force all the time. A force of mass × 196.2 \
             straight up cancels gravity.",
        ),
        ClassKind::AlignPosition => Some(
            "While playing, pulls Attachment0's part to Attachment1, or to Position if there's no \
             Attachment1. Responsiveness is how quickly; RigidityEnabled snaps straight there.",
        ),
        ClassKind::RopeConstraint => Some(
            "Keeps Attachment0 and Attachment1 no further apart than Length studs, so a loose part \
             hangs and swings from an anchored one. Visible draws the rope.",
        ),
        ClassKind::SpringConstraint => Some(
            "Pulls Attachment0 and Attachment1 towards FreeLength studs apart. Stiffness is how hard, \
             Damping how quickly the bouncing stops.",
        ),
        ClassKind::HingeConstraint => Some(
            "Joins Attachment1's part to Attachment0's so it can only turn about Attachment0's X axis              (turn the attachment to aim it). ActuatorType None swings freely, Motor turns at              AngularVelocity radians a second, Servo turns to TargetAngle degrees at AngularSpeed.",
        ),
        ClassKind::AngularVelocity => Some(
            "While playing, spins Attachment0's part at AngularVelocity (radians a second about X, Y              and Z), with up to MaxTorque. Unanchor the part first.",
        ),
        ClassKind::Terrain => Some(
            "Ground made of one-stud cubes. Pick the Terrain tool (5) to add, dig or paint it by              dragging in the view, or to make a hill. Scripts use workspace.Terrain:FillBlock,              FillBall and FillCylinder (Enum.Material.Air digs) and Clear.",
        ),
        ClassKind::Decal | ClassKind::Texture => Some(
            "Put one inside a part to show a PNG picture on one Face of it. A Decal stretches the \
             picture over the whole side; a Texture repeats it every StudsPerTileU by StudsPerTileV \
             studs. Texture is a file name, found like an ImageLabel's; libroxel://smile, star, arrow, \
             checker and coin are built in. Color3 tints it.",
        ),
        ClassKind::BodyColors => Some(
            "Put one in a character (StarterCharacter, or from a script on CharacterAdded) to colour \
             its head, torso, arms and legs. Players pick their own colours on the Avatar tab; this \
             overrides them.",
        ),
        ClassKind::Accessory => Some(
            "Something a character wears, like a hat. Put parts inside it and give each a HeadOffset \
             attribute (a Vector3, in studs from the middle of the head) to say where it sits; they \
             follow the head as the character moves.",
        ),
        ClassKind::Sound => Some(
            "Plays a WAV or OGG file while the game runs: tick Playing, or call Play, Stop, Pause and \
             Resume from a script (Ended fires when it finishes). SoundId is found like a picture; \
             libroxel://coin, jump, boom, click, splash, chime and hum are built in. Inside a part it \
             gets quieter further from the camera.",
        ),
        ClassKind::BodyAngularVelocity => Some(
            "The older spinner: put one inside a loose part to spin it at AngularVelocity, with up to              MaxTorque about each axis.",
        ),
        ClassKind::BodyVelocity | ClassKind::BodyPosition | ClassKind::BodyForce => Some(
            "The older movers: put one inside a loose part. BodyVelocity pushes it at Velocity, \
             BodyPosition pulls it to Position, BodyForce pushes with Force. MaxForce limits each axis.",
        ),
        ClassKind::ImageLabel | ClassKind::ImageButton => Some(
            "Shows a PNG picture. Image is a file name, looked for beside the game file, in an images \
             folder beside it, or in the games folder's images folder; libroxel://coin is built in. \
             ScaleType says whether it stretches, fits or crops; ImageColor3 tints it.",
        ),
        ClassKind::ScrollingFrame => Some(
            "A frame you can scroll with the mouse wheel while playing. CanvasSize is how big its inside is \
             (2 in Y scale is twice as tall as the frame); CanvasPosition is how far it's scrolled.",
        ),
        ClassKind::UIStroke => Some(
            "Inside a frame or button, outlines it. Inside a text label, outlines the letters. \
             Thickness is in pixels.",
        ),
        ClassKind::UIPadding => {
            Some("Keeps the text and everything inside this GUI object away from its edges.")
        }
        ClassKind::UIGridLayout => Some(
            "Lines up the other things in this frame in a grid of CellSize cells, CellPadding apart, \
             filling rows first (or columns, with FillDirection Vertical).",
        ),
        ClassKind::UIAspectRatioConstraint => Some(
            "Keeps this GUI object's width divided by its height at AspectRatio, shrinking the longer side.",
        ),
        ClassKind::UIScale => Some("Draws this GUI object Scale times bigger (or smaller, below 1)."),
        ClassKind::ParticleEmitter => Some(
            "Put this inside a part: it sends particles out of the part's top. Rate is how many a second, \
             Lifetime and Speed are ranges (min, max), SpreadAngle scatters them, Acceleration pulls them \
             (try 0, -20, 0 to make them fall), and LightEmission makes them glow. Scripts can call `Emit(count)`.",
        ),
        ClassKind::Fire | ClassKind::Smoke | ClassKind::Sparkles => Some(
            "Put this inside a part to set it alight, make it smoke or make it sparkle. Turn Enabled off to stop it.",
        ),
        ClassKind::Highlight => Some(
            "Outlines and tints its Adornee (a part or model), or what it's inside if Adornee is empty. \
             Good for showing what a player can pick up or who is selected.",
        ),
        ClassKind::WeldConstraint => Some(
            "Holds Part0 and Part1 together while playing: they move as one, keeping how they sit now. \
             Unanchor one part and the whole assembly falls, rolls and gets pushed together.",
        ),
        ClassKind::Explosion => Some(
            "Goes off as soon as it is in the workspace while playing: parts within BlastRadius are hit, \
             characters die and loose parts are thrown.",
        ),
        _ => None,
    };
    if let Some(hint) = hint {
        ui.add_space(8.0);
        ui.label(hint);
    }
}

/// The kinds of value a new attribute can start as, with their first value.
fn attribute_kinds() -> [(&'static str, PropertyValue); 5] {
    [
        ("number", PropertyValue::Number(0.0)),
        ("string", PropertyValue::String(String::new())),
        ("boolean", PropertyValue::Bool(false)),
        ("Vector3", PropertyValue::Vec3(libroxel_instance::Vec3::ZERO)),
        ("Color3", PropertyValue::Color3(libroxel_instance::Color3::WHITE)),
    ]
}

/// Attribute and tag editing below an instance's properties.
fn attributes_and_tags(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    tree: &mut InstanceTree,
    id: InstanceId,
    now: f64,
    playing: bool,
    actions: &mut Vec<Action>,
) {
    enum Edit {
        Set(String, Option<PropertyValue>),
        AddTag(String),
        RemoveTag(String),
    }
    let mut edits: Vec<Edit> = Vec::new();
    ui.add_space(10.0);
    ui.label(RichText::new("Attributes").strong());
    let attributes = tree.attributes(id).cloned().unwrap_or_default();
    egui::Grid::new("attributes").num_columns(3).striped(true).spacing([8.0, 6.0]).show(ui, |ui| {
        for (name, value) in attributes {
            ui.label(&name);
            ui.push_id(&name, |ui| {
                if let Some(new) = property_editor(ui, Property::Name, value, id, actions) {
                    edits.push(Edit::Set(name.clone(), Some(new)));
                }
            });
            if ui.small_button("✖").on_hover_text("Remove this attribute").clicked() {
                edits.push(Edit::Set(name, None));
            }
            ui.end_row();
        }
    });
    ui.horizontal(|ui| {
        let (name, kind) = &mut editor.new_attribute;
        ui.add(egui::TextEdit::singleline(name).hint_text("New attribute").desired_width(110.0));
        let kinds = attribute_kinds();
        egui::ComboBox::from_id_salt("attribute-kind").selected_text(kinds[*kind].0).show_ui(ui, |ui| {
            for (i, (label, _)) in kinds.iter().enumerate() {
                ui.selectable_value(kind, i, *label);
            }
        });
        let valid = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if ui.add_enabled(valid, egui::Button::new("Add")).clicked() {
            edits.push(Edit::Set(std::mem::take(name), Some(kinds[*kind].1.clone())));
        }
    });

    ui.add_space(8.0);
    ui.label(RichText::new("Tags").strong());
    let tags = tree.tags(id).map(|t| t.to_vec()).unwrap_or_default();
    ui.horizontal_wrapped(|ui| {
        for tag in tags {
            if ui.button(format!("{tag}  ✖")).on_hover_text("Remove this tag").clicked() {
                edits.push(Edit::RemoveTag(tag));
            }
        }
    });
    ui.horizontal(|ui| {
        let response =
            ui.add(egui::TextEdit::singleline(&mut editor.new_tag).hint_text("New tag").desired_width(150.0));
        let entered = response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        let ready = !editor.new_tag.trim().is_empty();
        if (ui.add_enabled(ready, egui::Button::new("Add")).clicked() || entered) && ready {
            edits.push(Edit::AddTag(editor.new_tag.trim().to_owned()));
            editor.new_tag.clear();
        }
    });

    if edits.is_empty() {
        return;
    }
    if !playing {
        editor.history.begin_edit(tree, now);
        editor.dirty = true;
    }
    for edit in edits {
        let _ = match edit {
            Edit::Set(name, value) => tree.set_attribute(id, &name, value),
            Edit::AddTag(tag) => tree.add_tag(id, &tag),
            Edit::RemoveTag(tag) => tree.remove_tag(id, &tag),
        };
    }
}

fn property_editor(
    ui: &mut egui::Ui,
    property: Property,
    value: PropertyValue,
    id: InstanceId,
    actions: &mut Vec<Action>,
) -> Option<PropertyValue> {
    match value {
        PropertyValue::String(mut s) => {
            if property == Property::Source {
                let lines = s.lines().count();
                if ui.button(format!("📝 Edit ({lines} lines)")).clicked() {
                    actions.push(Action::OpenScript(id));
                }
                None
            } else {
                ui.text_edit_singleline(&mut s).changed().then_some(PropertyValue::String(s))
            }
        }
        PropertyValue::Bool(mut b) => ui.checkbox(&mut b, "").changed().then_some(PropertyValue::Bool(b)),
        PropertyValue::Number(mut n) => {
            let fraction = matches!(property, Property::Transparency | Property::BackgroundTransparency);
            let speed = match property {
                _ if fraction => 0.01,
                Property::ClockTime | Property::Brightness => 0.05,
                _ => 0.5,
            };
            let drag = egui::DragValue::new(&mut n).speed(speed);
            let drag = if fraction { drag.range(0.0..=1.0) } else { drag };
            ui.add(drag).changed().then_some(PropertyValue::Number(n))
        }
        PropertyValue::Vec3(mut v) => {
            let mut changed = false;
            ui.horizontal(|ui| {
                for (axis, value) in [("X", &mut v.x), ("Y", &mut v.y), ("Z", &mut v.z)] {
                    ui.label(RichText::new(axis).small().weak());
                    changed |= ui.add(egui::DragValue::new(value).speed(0.1).max_decimals(3)).changed();
                }
            });
            if property == Property::Size {
                v = libroxel_instance::Vec3::new(v.x.max(0.05), v.y.max(0.05), v.z.max(0.05));
            }
            changed.then_some(PropertyValue::Vec3(v))
        }
        PropertyValue::Color3(c) => {
            let mut rgb = [c.r, c.g, c.b];
            ui.color_edit_button_rgb(&mut rgb)
                .changed()
                .then(|| PropertyValue::Color3(libroxel_instance::Color3::new(rgb[0], rgb[1], rgb[2])))
        }
        PropertyValue::Enum(current) => {
            let mut chosen = current;
            egui::ComboBox::from_id_salt(("enum", property.file_name()))
                .selected_text(names::enum_item_name(current))
                .show_ui(ui, |ui| {
                    for &item in current.kind().items() {
                        ui.selectable_value(&mut chosen, item, names::enum_item_name(item));
                    }
                });
            (chosen != current).then_some(PropertyValue::Enum(chosen))
        }
        PropertyValue::Vec2(mut v) => {
            let mut changed = false;
            ui.horizontal(|ui| {
                for (axis, value) in [("X", &mut v.x), ("Y", &mut v.y)] {
                    ui.label(RichText::new(axis).small().weak());
                    changed |= ui.add(egui::DragValue::new(value).speed(0.01).max_decimals(3)).changed();
                }
            });
            changed.then_some(PropertyValue::Vec2(v))
        }
        PropertyValue::UDim2(mut u) => {
            let mut changed = false;
            ui.vertical(|ui| {
                for (axis, scale, offset) in
                    [("X", &mut u.x_scale, &mut u.x_offset), ("Y", &mut u.y_scale, &mut u.y_offset)]
                {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(axis).small().weak());
                        changed |= ui
                            .add(egui::DragValue::new(scale).speed(0.01).max_decimals(3).prefix("scale "))
                            .changed();
                        changed |= ui
                            .add(egui::DragValue::new(offset).speed(1.0).max_decimals(1).prefix("px "))
                            .changed();
                    });
                }
            });
            changed.then_some(PropertyValue::UDim2(u))
        }
        PropertyValue::UDim(mut u) => {
            let mut changed = false;
            ui.horizontal(|ui| {
                changed |= ui
                    .add(egui::DragValue::new(&mut u.scale).speed(0.01).max_decimals(3).prefix("scale "))
                    .changed();
                changed |= ui
                    .add(egui::DragValue::new(&mut u.offset).speed(1.0).max_decimals(1).prefix("px "))
                    .changed();
            });
            changed.then_some(PropertyValue::UDim(u))
        }
        // Shown by the caller, which can look up the name.
        PropertyValue::Ref(_) => None,
    }
}

/// While playing: everyone's `leaderstats` values in the top-right corner.
fn leaderboard(ui: &mut egui::Ui, tree: &InstanceTree) {
    let Some(players) = tree.service(ClassKind::Players) else { return };
    // Each player: their name, their stats (name and value), and their team.
    type Row = (String, Vec<(String, String)>, Option<InstanceId>);
    let mut rows: Vec<Row> = tree
        .children(players)
        .unwrap_or_default()
        .iter()
        .filter(|p| tree.class(**p) == Ok(ClassKind::Player))
        .map(|&player| {
            let stats = tree
                .find_first_child(player, "leaderstats")
                .ok()
                .flatten()
                .map(|folder| {
                    tree.children(folder)
                        .unwrap_or_default()
                        .iter()
                        .filter_map(|&v| {
                            let class = tree.class(v).ok()?;
                            let value = match class {
                                ClassKind::IntValue | ClassKind::NumberValue => {
                                    let n = tree.get_number(v, Property::NumberValue)?;
                                    if n.fract() == 0.0 { format!("{n:.0}") } else { format!("{n:.2}") }
                                }
                                ClassKind::StringValue => tree.get_string(v, Property::StringValue)?,
                                ClassKind::BoolValue => {
                                    if tree.get_bool(v, Property::BoolValue)? {
                                        "Yes".into()
                                    } else {
                                        "No".into()
                                    }
                                }
                                _ => return None,
                            };
                            Some((tree.name(v).ok()?.to_owned(), value))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let team = tree
                .get_ref(player, Property::Team)
                .filter(|_| tree.get_bool(player, Property::Neutral) == Some(false));
            (tree.name(player).unwrap_or("?").to_owned(), stats, team)
        })
        .collect();
    // Players on teams are listed under their team, in the order of the Teams service.
    let team_order = |t: Option<InstanceId>| {
        t.and_then(|t| {
            let teams = tree.service(ClassKind::Teams)?;
            tree.children(teams).ok()?.iter().position(|c| *c == t)
        })
        .unwrap_or(usize::MAX)
    };
    rows.sort_by_key(|(_, _, team)| team_order(*team));
    if rows.is_empty() {
        return;
    }
    let columns: Vec<String> =
        rows.iter().flat_map(|(_, s, _)| s.iter().map(|(n, _)| n.clone())).fold(Vec::new(), |mut acc, n| {
            if !acc.contains(&n) {
                acc.push(n);
            }
            acc
        });
    let rect = ui.max_rect();
    egui::Area::new(egui::Id::new("leaderboard"))
        .fixed_pos(egui::pos2(rect.right() - 12.0, rect.top() + 12.0))
        .pivot(egui::Align2::RIGHT_TOP)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).fill(Color32::from_black_alpha(170)).show(ui, |ui| {
                egui::Grid::new("leaderboard-grid").spacing([16.0, 4.0]).show(ui, |ui| {
                    ui.label(RichText::new("Player").strong().color(Color32::WHITE));
                    for c in &columns {
                        ui.label(RichText::new(c).strong().color(Color32::WHITE));
                    }
                    ui.end_row();
                    let mut last_team = None;
                    for (name, stats, team) in &rows {
                        if *team != last_team {
                            last_team = *team;
                            let (label, color) = match team {
                                Some(t) => {
                                    let c = tree
                                        .get_color(*t, Property::TeamColor)
                                        .unwrap_or(libroxel_instance::Color3::WHITE);
                                    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                                    (
                                        tree.name(*t).unwrap_or("?").to_owned(),
                                        Color32::from_rgb(byte(c.r), byte(c.g), byte(c.b)),
                                    )
                                }
                                None => ("Neutral".to_owned(), Color32::from_gray(200)),
                            };
                            ui.label(RichText::new(label).strong().color(color));
                            ui.end_row();
                        }
                        ui.label(RichText::new(name).color(Color32::WHITE));
                        for c in &columns {
                            let v =
                                stats.iter().find(|(n, _)| n == c).map(|(_, v)| v.as_str()).unwrap_or("-");
                            ui.label(RichText::new(v).color(Color32::from_rgb(255, 220, 120)));
                        }
                        ui.end_row();
                    }
                });
            });
        });
}

fn output(
    ui: &mut egui::Ui,
    engine: &mut EngineState,
    editor: &mut Editor,
    playing: bool,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        ui.heading("Output");
        if ui.small_button("Clear").clicked() {
            actions.push(Action::ClearOutput);
        }
        ui.separator();
        let hint =
            if playing { "Run Luau in the running game…" } else { "Press Play to run Luau commands" };
        let response = ui.add_enabled(
            playing,
            egui::TextEdit::singleline(&mut editor.command).hint_text(hint).desired_width(f32::INFINITY),
        );
        if response.lost_focus()
            && ui.input(|i| i.key_pressed(Key::Enter))
            && !editor.command.trim().is_empty()
        {
            actions.push(Action::RunCommand(std::mem::take(&mut editor.command)));
            response.request_focus();
        }
    });
    ui.separator();
    egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
        let start = engine.log.len().saturating_sub(1000);
        for entry in &engine.log[start..] {
            ui.label(
                RichText::new(format!("[{:>6.1}s] {}", entry.time, entry.text))
                    .monospace()
                    .color(level_color(entry.level)),
            );
        }
    });
}

fn hud(ui: &mut egui::Ui, playing: bool) {
    let text = if playing {
        "WASD walk · Space jump · right mouse turn · scroll zoom · Esc stop"
    } else {
        "Right mouse + WASD fly · click to select · F5 play"
    };
    let rect = ui.max_rect();
    ui.painter().text(
        rect.right_bottom() + egui::vec2(-8.0, -8.0),
        egui::Align2::RIGHT_BOTTOM,
        text,
        egui::FontId::proportional(13.0),
        Color32::from_white_alpha(170),
    );
}

fn script_editor(ctx: &egui::Context, editor: &mut Editor, tree: &mut InstanceTree, now: f64, playing: bool) {
    let Some(id) = editor.open_script else { return };
    let Ok(PropertyValue::String(mut source)) = tree.get(id, Property::Source) else {
        editor.open_script = None;
        return;
    };
    let name = tree.name(id).unwrap_or("Script").to_owned();
    let mut open = true;
    let mut changed = false;
    egui::Window::new(format!("📜 {name}"))
        .id(egui::Id::new("script-editor"))
        .open(&mut open)
        .default_size([620.0, 460.0])
        .default_pos([300.0, 90.0])
        .resizable(true)
        .show(ctx, |ui| {
            if playing {
                ui.label(
                    RichText::new("Changes made while playing apply next time you press Play.").italics(),
                );
            }
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                changed = ui
                    .add(
                        egui::TextEdit::multiline(&mut source)
                            .code_editor()
                            .lock_focus(true)
                            .desired_width(f32::INFINITY)
                            .desired_rows(24),
                    )
                    .changed();
            });
        });
    if changed {
        if !playing {
            editor.history.begin_edit(tree, now);
            editor.dirty = true;
        }
        let _ = tree.set(id, Property::Source, PropertyValue::String(source));
    }
    if !open {
        editor.open_script = None;
    }
}

fn list_places() -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(games_dir())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "lrx"))
        .collect();
    found.sort();
    found
}

fn dialogs(ctx: &egui::Context, editor: &mut Editor, actions: &mut Vec<Action>) {
    let Some(dialog) = editor.dialog.as_mut() else { return };
    if let Dialog::Join { address, name } = dialog {
        let mut close = false;
        egui::Window::new("Join a game").collapsible(false).resizable(false).default_width(360.0).show(
            ctx,
            |ui| {
                egui::Grid::new("join").num_columns(2).show(ui, |ui| {
                    ui.label("Join code or address");
                    ui.add(egui::TextEdit::singleline(address).hint_text("lrx1… or 192.168.1.20"));
                    ui.end_row();
                    ui.label("Your name");
                    ui.add(egui::TextEdit::singleline(name).char_limit(20));
                    ui.end_row();
                });
                ui.label(format!(
                    "Paste the join code the host copied to play from anywhere, or type an address on \
                     your network (without a port, {} is used).",
                    libroxel_engine::net::DEFAULT_PORT
                ));
                ui.horizontal(|ui| {
                    if ui.button("Join").clicked() {
                        actions.push(Action::Join {
                            address: address.trim().to_owned(),
                            name: name.trim().to_owned(),
                        });
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            },
        );
        if close {
            editor.dialog = None;
        }
        return;
    }
    let (title, button) = match dialog {
        Dialog::Open { .. } => ("Open place", "Open"),
        Dialog::SaveAs { .. } => ("Save place as", "Save"),
        Dialog::Join { .. } => return,
    };
    let opening = matches!(dialog, Dialog::Open { .. });
    let mut close = false;
    egui::Window::new(title).collapsible(false).resizable(false).default_width(480.0).show(ctx, |ui| {
        let path = match dialog {
            Dialog::Open { path } | Dialog::SaveAs { path } => path,
            Dialog::Join { .. } => return,
        };
        ui.label("File path (places use the .lrx extension):");
        ui.add(egui::TextEdit::singleline(path).desired_width(f32::INFINITY));
        let places = list_places();
        if !places.is_empty() {
            ui.label(format!("In {}:", games_dir().display()));
            for p in places {
                let label = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                if ui.link(label).clicked() {
                    *path = p.display().to_string();
                }
            }
        }
        ui.horizontal(|ui| {
            if ui.button(button).clicked() {
                let mut chosen = PathBuf::from(path.trim());
                if chosen.extension().is_none() {
                    chosen.set_extension("lrx");
                }
                actions.push(if opening { Action::Open(chosen) } else { Action::SaveTo(chosen) });
                close = true;
            }
            if ui.button("Cancel").clicked() {
                close = true;
            }
        });
    });
    if close {
        editor.dialog = None;
    }
}

// ---------------------------------------------------------------------------
// Applying actions
// ---------------------------------------------------------------------------

/// Where play tests keep the current game's saved data: beside its file.
fn data_path(editor: &Editor) -> PathBuf {
    match &editor.file {
        Some(file) => file.with_extension("data.json"),
        None => games_dir().join("unsaved.data.json"),
    }
}

/// Ends the play session, if there is one: the player leaves, then saved
/// data is written. Returns whether a session was running.
fn end_session(engine: &mut EngineState, editor: &Editor) -> bool {
    let Some(mut session) = engine.session.take() else { return false };
    session.stop();
    let lines = session.take_log();
    engine.log.extend(lines);
    // A joined game's data is kept by whoever hosts it.
    if session.is_joined() {
        return true;
    }
    let json = session.save_data();
    let path = data_path(editor);
    if json.trim() != "{}" || path.exists() {
        let written =
            path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(&path, json));
        if let Err(e) = written {
            log(engine, LogLevel::Error, format!("Could not keep saved data: {e}"));
        }
    }
    true
}

fn replace_world(engine: &mut EngineState, editor: &mut Editor, tree: InstanceTree, file: Option<PathBuf>) {
    if end_session(engine, editor) {
        editor.edit_snapshot = None;
    }
    *engine.tree.borrow_mut() = tree;
    editor.history.clear();
    editor.selection = None;
    editor.open_script = None;
    editor.file = file;
    editor.dirty = false;
}

fn save_to(engine: &mut EngineState, editor: &mut Editor, path: PathBuf) {
    let text = {
        let tree = engine.tree.borrow();
        let source = editor.edit_snapshot.as_ref().unwrap_or(&tree);
        place::save(source)
    };
    let result = text.map_err(|e| e.to_string()).and_then(|text| {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, text).map_err(|e| e.to_string())
    });
    match result {
        Ok(()) => {
            log(engine, LogLevel::Info, format!("Saved {}", path.display()));
            editor.file = Some(path);
            editor.dirty = false;
        }
        Err(err) => log(engine, LogLevel::Error, format!("Could not save {}: {err}", path.display())),
    }
}

/// Where a new frame, label or button goes: inside the selected GUI, or the
/// first screen GUI in StarterGui (made if there is none).
fn gui_parent(tree: &mut InstanceTree, selection: Option<InstanceId>) -> Option<InstanceId> {
    if let Some(sel) = selection
        && tree.class(sel).is_ok_and(|c| c == ClassKind::ScreenGui || c.is_gui_object())
    {
        return Some(sel);
    }
    let starter = tree.service(ClassKind::StarterGui)?;
    if let Some(screen) = tree.find_first_child_of_class(starter, ClassKind::ScreenGui).ok().flatten() {
        return Some(screen);
    }
    let screen = tree.create(ClassKind::ScreenGui, names::class_name(ClassKind::ScreenGui)).ok()?;
    tree.set_parent(screen, Some(starter)).ok()?;
    Some(screen)
}

/// A drop-down of the workspace's parts, for a weld's Part0 or Part1.
/// Classes that hold an `Attachment0`, which inserting fills in.
fn needs_attachment(class: ClassKind) -> bool {
    class.has_property(Property::Attachment0)
}

/// What a link property may point at.
fn ref_choices(tree: &InstanceTree, property: Property) -> Vec<InstanceId> {
    let ws = tree.workspace();
    match property {
        Property::Part0 | Property::Part1 => tree.descendants_of_class(ws, ClassKind::Part),
        Property::Attachment0 | Property::Attachment1 => tree.descendants_of_class(ws, ClassKind::Attachment),
        _ => tree.descendants(tree.root()).unwrap_or_default(),
    }
}

fn part_picker(
    ui: &mut egui::Ui,
    tree: &InstanceTree,
    id: InstanceId,
    property: Property,
    current: Option<InstanceId>,
) -> Option<Option<InstanceId>> {
    let label = |p: InstanceId| {
        let name = tree.name(p).unwrap_or("?");
        match tree.parent(p).ok().flatten().filter(|q| *q != tree.workspace()) {
            Some(q) => format!("{} › {name}", tree.name(q).unwrap_or("?")),
            None => name.to_owned(),
        }
    };
    let shown = current.filter(|t| tree.exists(*t)).map(label).unwrap_or_else(|| "(none)".into());
    let mut picked = None;
    egui::ComboBox::from_id_salt(("part_picker", id, property as u32)).selected_text(shown).show_ui(
        ui,
        |ui| {
            if ui.selectable_label(current.is_none(), "(none)").clicked() {
                picked = Some(None);
            }
            for part in ref_choices(tree, property) {
                if ui.selectable_label(current == Some(part), label(part)).clicked() {
                    picked = Some(Some(part));
                }
            }
        },
    );
    picked.filter(|p| *p != current)
}

/// The part whose box touches or overlaps `part`'s the most, if any.
fn touching_part(tree: &InstanceTree, part: InstanceId) -> Option<InstanceId> {
    let bounds = |p: InstanceId| {
        let pos = tree.get_vec3(p, Property::Position)?;
        let half = tree.get_vec3(p, Property::Size)? / 2.0;
        Some((pos - half, pos + half))
    };
    let (amin, amax) = bounds(part)?;
    let mut best: Option<(f32, InstanceId)> = None;
    for other in tree.descendants_of_class(tree.workspace(), ClassKind::Part) {
        if other == part {
            continue;
        }
        let Some((bmin, bmax)) = bounds(other) else { continue };
        let gap = |lo: f32, hi: f32| hi - lo;
        let ox = gap(amin.x.max(bmin.x), amax.x.min(bmax.x));
        let oy = gap(amin.y.max(bmin.y), amax.y.min(bmax.y));
        let oz = gap(amin.z.max(bmin.z), amax.z.min(bmax.z));
        const SLACK: f32 = 0.05;
        if ox < -SLACK || oy < -SLACK || oz < -SLACK {
            continue;
        }
        // Rank by the area (or volume) shared, with faces that only touch counting a little.
        let score = ox.max(0.01) * oy.max(0.01) * oz.max(0.01);
        if best.is_none_or(|(s, _)| score > s) {
            best = Some((score, other));
        }
    }
    best.map(|(_, p)| p)
}

fn new_instance(
    tree: &mut InstanceTree,
    class: ClassKind,
    parent: InstanceId,
    near: Option<InstanceId>,
) -> Option<InstanceId> {
    let id = tree.create(class, names::class_name(class)).ok()?;
    if class.is_physical() {
        // Drop new parts on top of the selection, or at the origin.
        let above = near
            .and_then(|n| Some((tree.get_vec3(n, Property::Position)?, tree.get_vec3(n, Property::Size)?)))
            .map(|(p, s)| p + libroxel_instance::Vec3::new(0.0, s.y / 2.0 + 0.5, 0.0))
            .unwrap_or(libroxel_instance::Vec3::new(0.0, 0.5, 0.0));
        let _ = tree.set(id, Property::Position, PropertyValue::Vec3(above));
        let _ = tree.set(id, Property::Anchored, PropertyValue::Bool(true));
    }
    if class == ClassKind::Script {
        let _ = tree.set(
            id,
            Property::Source,
            PropertyValue::String("print(\"Hello from \" .. script.Name)\n".into()),
        );
    }
    if class == ClassKind::Tool {
        // A tool needs a part to hold it by.
        let handle = tree.create(ClassKind::Part, names::TOOL_HANDLE).ok()?;
        let _ = tree.set(
            handle,
            Property::Size,
            PropertyValue::Vec3(libroxel_instance::Vec3::new(0.6, 0.6, 3.0)),
        );
        let _ = tree.set(handle, Property::CanCollide, PropertyValue::Bool(false));
        tree.set_parent(handle, Some(id)).ok()?;
        let script = tree.create(ClassKind::Script, "ToolScript").ok()?;
        let source = "local tool = script.Parent\n\ntool.Activated:Connect(function()\n\tprint(\"Used \" .. tool.Name)\nend)\n";
        let _ = tree.set(script, Property::Source, PropertyValue::String(source.into()));
        tree.set_parent(script, Some(id)).ok()?;
    }
    if class == ClassKind::BillboardGui {
        let label = tree.create(ClassKind::TextLabel, "Label").ok()?;
        let _ = tree.set(
            label,
            Property::GuiSize,
            PropertyValue::UDim2(libroxel_instance::UDim2::new(1.0, 0.0, 1.0, 0.0)),
        );
        let _ = tree.set(label, Property::BackgroundTransparency, PropertyValue::Number(1.0));
        let _ = tree.set(label, Property::TextColor, PropertyValue::Color3(libroxel_instance::Color3::WHITE));
        let _ = tree.set(label, Property::TextSize, PropertyValue::Number(20.0));
        let _ = tree.set(label, Property::Text, PropertyValue::String("Hello!".into()));
        tree.set_parent(label, Some(id)).ok()?;
    }
    if class == ClassKind::LocalScript {
        let source = "local player = game.Players.LocalPlayer\nprint(\"Hello, \" .. player.Name)\n";
        let _ = tree.set(id, Property::Source, PropertyValue::String(source.into()));
    }
    if class == ClassKind::ModuleScript {
        let source = "local module = {}\n\nfunction module.hello()\n\tprint(\"Hello from \" .. script.Name)\nend\n\nreturn module\n";
        let _ = tree.set(id, Property::Source, PropertyValue::String(source.into()));
    }
    tree.set_parent(id, Some(parent)).ok()?;
    Some(id)
}

/// Does what a main menu button asked for.
fn menu_pick(pick: crate::menu::Pick, engine: &mut EngineState, editor: &mut Editor, now: f64) {
    use crate::menu::{Game, Pick};
    let open = |game: &Game, engine: &mut EngineState, editor: &mut Editor| -> bool {
        match game {
            Game::Starter(name) => apply(Action::OpenStarter(name), engine, editor, now),
            Game::Blank => apply(Action::New, engine, editor, now),
            Game::File(path) => {
                apply(Action::Open(path.clone()), engine, editor, now);
                if editor.file.as_ref() != Some(path) {
                    editor.menu.error = Some(format!("Could not open {}.", path.display()));
                    return false;
                }
            }
        }
        true
    };
    let start = |action: Action, engine: &mut EngineState, editor: &mut Editor| {
        apply(action, engine, editor, now);
        match engine.session.is_some() {
            true => {
                editor.player_mode = true;
                editor.menu.open = false;
            }
            false => {
                let why = engine.log.last().map(|l| l.text.clone()).unwrap_or_default();
                editor.menu.error = Some(format!("Could not start: {why}"));
            }
        }
    };
    match pick {
        Pick::Edit(game) => {
            if open(&game, engine, editor) {
                editor.menu.open = false;
            }
        }
        Pick::Play(game) => {
            if open(&game, engine, editor) {
                start(Action::Play, engine, editor);
            }
        }
        Pick::Host(game) => {
            if open(&game, engine, editor) {
                start(Action::Host, engine, editor);
            }
        }
        Pick::Join { code, name } => {
            apply(Action::New, engine, editor, now);
            start(Action::Join { address: code, name }, engine, editor);
        }
        Pick::Browse => editor.platform.open = true,
    }
}

fn apply(action: Action, engine: &mut EngineState, editor: &mut Editor, now: f64) {
    let playing = engine.is_playing();
    match action {
        Action::New => replace_world(engine, editor, templates::blank(), None),
        Action::OpenStarter(name) => {
            if let Some(game) = crate::starter_games::starter(name) {
                replace_world(engine, editor, (game.build)(), None);
                log(engine, LogLevel::Info, format!("Opened {}. Press Play to try it.", game.name));
            }
        }
        Action::Open(path) => {
            match std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|t| place::load(&t).map_err(|e| e.to_string()))
            {
                Ok((tree, report)) => {
                    replace_world(engine, editor, tree, Some(path.clone()));
                    log(engine, LogLevel::Info, format!("Opened {}", path.display()));
                    for w in report.warnings {
                        log(engine, LogLevel::Warn, w);
                    }
                }
                Err(err) => log(engine, LogLevel::Error, format!("Could not open {}: {err}", path.display())),
            }
        }
        Action::Save => match editor.file.clone() {
            Some(path) => save_to(engine, editor, path),
            None => {
                let path = games_dir().join("untitled.lrx").display().to_string();
                editor.dialog = Some(Dialog::SaveAs { path });
            }
        },
        Action::SaveTo(path) => save_to(engine, editor, path),
        action @ (Action::Play | Action::Host | Action::Join { .. }) => {
            if playing {
                return;
            }
            editor.history.tick(f64::INFINITY, false);
            editor.edit_snapshot = Some(engine.tree.borrow().clone());
            let saved = std::fs::read_to_string(data_path(editor)).ok();
            let (started, what) = match &action {
                Action::Host => (
                    PlaySession::host(
                        engine.tree.clone(),
                        libroxel_engine::net::DEFAULT_PORT,
                        saved.as_deref(),
                        true,
                    ),
                    "🌐 Hosting".to_owned(),
                ),
                Action::Join { address, name } if libroxel_net::is_join_code(address) => {
                    let name = if name.is_empty() { "Player" } else { name.as_str() };
                    let tunnel = crate::identity().and_then(|me| {
                        libroxel_net::JoinTunnel::start(&me, libroxel_net::Network::Internet, address)
                    });
                    match tunnel {
                        Ok(tunnel) => {
                            let local = tunnel.local_addr().to_string();
                            editor.tunnel = Some(Tunnel::Join(tunnel));
                            (
                                PlaySession::join(engine.tree.clone(), &local, name),
                                "🔗 Joining with a code".to_owned(),
                            )
                        }
                        Err(e) => (Err(e), String::new()),
                    }
                }
                Action::Join { address, name } => {
                    let name = if name.is_empty() { "Player" } else { name.as_str() };
                    // The session says where it is joining.
                    (PlaySession::join(engine.tree.clone(), address, name), String::new())
                }
                _ => (
                    match saved {
                        Some(json) => PlaySession::start_with_data(engine.tree.clone(), &json),
                        None => PlaySession::start(engine.tree.clone(), true),
                    },
                    "▶ Playing".to_owned(),
                ),
            };
            match started {
                Ok(mut session) => {
                    session.set_local_look(editor.menu.look());
                    let lines = session.take_log();
                    if let Some(address) = session.host_address() {
                        log(engine, LogLevel::Info, format!("{what} on port {}", address.port()));
                        let tunnel = crate::identity().and_then(|me| {
                            libroxel_net::HostTunnel::start(
                                &me,
                                libroxel_net::Network::Internet,
                                address.port(),
                            )
                        });
                        match tunnel {
                            Ok(tunnel) => {
                                editor.tunnel = Some(Tunnel::Host(tunnel));
                                if let Ok(me) = crate::identity() {
                                    let game = editor.file.as_ref().and_then(|f| f.file_stem()).map_or_else(
                                        || "Untitled game".to_owned(),
                                        |s| s.to_string_lossy().into_owned(),
                                    );
                                    editor.platform.announce(me.player_id(), game);
                                }
                                log(
                                    engine,
                                    LogLevel::Info,
                                    "Getting a join code so friends anywhere can join…",
                                );
                            }
                            Err(e) => log(engine, LogLevel::Warn, format!("Only your network can join: {e}")),
                        }
                    } else if !what.is_empty() {
                        log(engine, LogLevel::Info, what);
                    }
                    engine.session = Some(session);
                    engine.log.extend(lines);
                }
                Err(err) => {
                    log(engine, LogLevel::Error, err);
                    editor.tunnel = None;
                    if let Some(snapshot) = editor.edit_snapshot.take() {
                        *engine.tree.borrow_mut() = snapshot;
                    }
                }
            }
        }
        Action::Stop => {
            if end_session(engine, editor) {
                if let Some(snapshot) = editor.edit_snapshot.take() {
                    *engine.tree.borrow_mut() = snapshot;
                }
                if editor.selection.is_some_and(|id| !engine.tree.borrow().exists(id)) {
                    editor.selection = None;
                }
                log(engine, LogLevel::Info, "⏹ Stopped");
            }
        }
        Action::Undo | Action::Redo if playing => {}
        Action::Undo => {
            let mut tree = engine.tree.borrow_mut();
            if editor.history.undo(&mut tree) {
                editor.dirty = true;
            }
        }
        Action::Redo => {
            let mut tree = engine.tree.borrow_mut();
            if editor.history.redo(&mut tree) {
                editor.dirty = true;
            }
        }
        Action::Insert(class) => {
            let tree_rc = engine.tree.clone();
            let mut tree = tree_rc.borrow_mut();
            let parent = tree.workspace();
            if !playing {
                editor.history.begin_edit(&tree, now);
                editor.dirty = true;
            }
            let near = editor.selection.filter(|s| tree.class(*s).is_ok_and(|c| c.is_physical()));
            // Scripts go inside the selected part, so `script.Parent` is useful straight away.
            // Modules live in storage unless something else is selected.
            let parent = match class {
                ClassKind::ModuleScript => tree.service(ClassKind::ReplicatedStorage).unwrap_or(parent),
                ClassKind::ScreenGui => tree.service(ClassKind::StarterGui).unwrap_or(parent),
                ClassKind::RemoteEvent | ClassKind::RemoteFunction => {
                    tree.service(ClassKind::ReplicatedStorage).unwrap_or(parent)
                }
                ClassKind::Tool => tree.service(ClassKind::StarterPack).unwrap_or(parent),
                ClassKind::Team => tree.service(ClassKind::Teams).unwrap_or(parent),
                // Corners and layouts go inside the selected GUI object.
                ClassKind::UICorner
                | ClassKind::UIListLayout
                | ClassKind::UIStroke
                | ClassKind::UIPadding
                | ClassKind::UIGridLayout
                | ClassKind::UIAspectRatioConstraint
                | ClassKind::UIScale => editor
                    .selection
                    .filter(|s| tree.class(*s).is_ok_and(|c| c.is_gui_object() || c == ClassKind::ScreenGui))
                    .or_else(|| gui_parent(&mut tree, editor.selection))
                    .unwrap_or(parent),
                c if c.is_gui_object() => gui_parent(&mut tree, editor.selection).unwrap_or(parent),
                _ => parent,
            };
            let goes_inside = matches!(
                class,
                ClassKind::Script
                    | ClassKind::LocalScript
                    | ClassKind::ClickDetector
                    | ClassKind::ProximityPrompt
                    | ClassKind::BillboardGui
                    | ClassKind::PointLight
                    | ClassKind::SpotLight
                    | ClassKind::BindableEvent
                    | ClassKind::BindableFunction
                    | ClassKind::ObjectValue
                    | ClassKind::Vector3Value
                    | ClassKind::Color3Value
                    | ClassKind::ModuleScript
                    | ClassKind::IntValue
                    | ClassKind::NumberValue
                    | ClassKind::StringValue
                    | ClassKind::BoolValue
                    | ClassKind::WeldConstraint
                    | ClassKind::Attachment
                    | ClassKind::LinearVelocity
                    | ClassKind::VectorForce
                    | ClassKind::AlignPosition
                    | ClassKind::RopeConstraint
                    | ClassKind::SpringConstraint
                    | ClassKind::BodyVelocity
                    | ClassKind::BodyPosition
                    | ClassKind::BodyForce
                    | ClassKind::HingeConstraint
                    | ClassKind::AngularVelocity
                    | ClassKind::BodyAngularVelocity
                    | ClassKind::Decal
                    | ClassKind::Texture
                    | ClassKind::Sound
                    | ClassKind::ParticleEmitter
                    | ClassKind::Fire
                    | ClassKind::Smoke
                    | ClassKind::Sparkles
                    | ClassKind::Highlight
            );
            let parent = match editor.selection {
                Some(sel)
                    if goes_inside
                        && tree.class(sel).is_ok_and(|c| !is_code(c) && c != ClassKind::DataModel) =>
                {
                    sel
                }
                _ => parent,
            };
            if let Some(id) = new_instance(&mut tree, class, parent, near) {
                if needs_attachment(class)
                    && let Some(part) = near
                {
                    // Movers and constraints hold attachments: give it one on the selected part.
                    if let Ok(at) =
                        tree.create(ClassKind::Attachment, names::class_name(ClassKind::Attachment))
                        && tree.set_parent(at, Some(part)).is_ok()
                    {
                        let _ = tree.set(id, Property::Attachment0, PropertyValue::Ref(Some(at)));
                    }
                }
                if class == ClassKind::WeldConstraint
                    && let Some(part) = near
                {
                    // Weld the selected part to whatever it touches.
                    let _ = tree.set(id, Property::Part0, PropertyValue::Ref(Some(part)));
                    let other = touching_part(&tree, part);
                    let _ = tree.set(id, Property::Part1, PropertyValue::Ref(other));
                }
                editor.selection = Some(id);
                if is_code(class) {
                    editor.open_script = Some(id);
                }
            }
        }
        Action::InsertNpc => {
            let tree_rc = engine.tree.clone();
            let mut tree = tree_rc.borrow_mut();
            if !playing {
                editor.history.begin_edit(&tree, now);
                editor.dirty = true;
            }
            let near = editor
                .selection
                .filter(|s| tree.class(*s).is_ok_and(|c| c.is_physical()))
                .and_then(|n| {
                    Some((tree.get_vec3(n, Property::Position)?, tree.get_vec3(n, Property::Size)?))
                })
                .map(|(p, s)| p + libroxel_instance::Vec3::new(0.0, s.y / 2.0, 0.0))
                .unwrap_or(libroxel_instance::Vec3::ZERO);
            if let Some(model) = new_npc(&mut tree, near) {
                editor.selection = Some(model);
            }
        }
        Action::InsertKart => {
            let tree_rc = engine.tree.clone();
            let mut tree = tree_rc.borrow_mut();
            if !playing {
                editor.history.begin_edit(&tree, now);
                editor.dirty = true;
            }
            let at = editor
                .selection
                .filter(|s| tree.class(*s).is_ok_and(|c| c.is_physical()))
                .and_then(|n| {
                    Some((tree.get_vec3(n, Property::Position)?, tree.get_vec3(n, Property::Size)?))
                })
                .map(|(p, s)| p + libroxel_instance::Vec3::new(0.0, s.y / 2.0, 0.0))
                .unwrap_or(libroxel_instance::Vec3::ZERO);
            let ws = tree.workspace();
            let model = crate::templates::kart(&mut tree, ws, at, 0.0, [60, 120, 220]);
            editor.selection = Some(model);
        }
        Action::Brush(point, normal) if !playing => {
            let tree_rc = engine.tree.clone();
            let mut tree = tree_rc.borrow_mut();
            editor.history.begin_edit(&tree, now);
            let Ok(terrain) = tree.terrain_or_make() else { return };
            let brush = editor.brush;
            if tree
                .edit_voxels(terrain, |v| crate::terrain_tools::apply_brush(v, brush, point, normal))
                .ok()
                .flatten()
                > Some(0)
            {
                editor.dirty = true;
            }
        }
        Action::MakeHill(at) if !playing => {
            let tree_rc = engine.tree.clone();
            let mut tree = tree_rc.borrow_mut();
            editor.history.begin_edit(&tree, now);
            let Ok(terrain) = tree.terrain_or_make() else { return };
            let seed = (now * 1000.0) as u32;
            let ground = libroxel_instance::Vec3::new(at.x, at.y.round(), at.z);
            let _ = tree.edit_voxels(terrain, |v| crate::terrain_tools::hill(v, ground, 24.0, 12.0, seed));
            editor.dirty = true;
        }
        Action::ClearTerrain if !playing => {
            let tree_rc = engine.tree.clone();
            let mut tree = tree_rc.borrow_mut();
            if let Some(terrain) = tree.terrain() {
                editor.history.begin_edit(&tree, now);
                let _ = tree.edit_voxels(terrain, |v| v.clear());
                editor.dirty = true;
            }
        }
        Action::Brush(..) | Action::MakeHill(_) | Action::ClearTerrain => {}
        Action::InsertShape(shape) => {
            let tree_rc = engine.tree.clone();
            let mut tree = tree_rc.borrow_mut();
            if !playing {
                editor.history.begin_edit(&tree, now);
                editor.dirty = true;
            }
            let near = editor.selection.filter(|s| tree.class(*s).is_ok_and(|c| c.is_physical()));
            let parent = tree.workspace();
            if let Some(id) = new_instance(&mut tree, ClassKind::Part, parent, near) {
                let size = match shape {
                    PartShape::Block => libroxel_instance::Vec3::new(4.0, 1.0, 2.0),
                    PartShape::Ball => libroxel_instance::Vec3::new(2.0, 2.0, 2.0),
                    PartShape::Cylinder => libroxel_instance::Vec3::new(2.0, 2.0, 2.0),
                    PartShape::Wedge => libroxel_instance::Vec3::new(4.0, 2.0, 4.0),
                };
                let _ = tree.set(id, Property::Shape, PropertyValue::Enum(EnumValue::Shape(shape)));
                let _ = tree.set(id, Property::Size, PropertyValue::Vec3(size));
                if let Some(pos) = tree.get_vec3(id, Property::Position) {
                    let lifted = pos + libroxel_instance::Vec3::new(0.0, size.y / 2.0 - 0.5, 0.0);
                    let _ = tree.set(id, Property::Position, PropertyValue::Vec3(lifted));
                }
                let name = SHAPES.iter().find(|(s, _)| *s == shape).map(|(_, n)| *n).unwrap_or("Part");
                if shape != PartShape::Block {
                    let _ = tree.set_name(id, name);
                }
                editor.selection = Some(id);
            }
        }
        Action::Rotate90(id) => {
            let tree_rc = engine.tree.clone();
            let mut tree = tree_rc.borrow_mut();
            let Some(o) = tree.get_vec3(id, Property::Orientation) else { return };
            if !playing {
                editor.history.begin_edit(&tree, now);
                editor.dirty = true;
            }
            let turned = libroxel_instance::Vec3::new(o.x, (o.y + 90.0).rem_euclid(360.0), o.z);
            let _ = tree.set(id, Property::Orientation, PropertyValue::Vec3(turned));
        }
        Action::InsertInto(class, parent) => {
            let tree_rc = engine.tree.clone();
            let mut tree = tree_rc.borrow_mut();
            if !playing {
                editor.history.begin_edit(&tree, now);
                editor.dirty = true;
            }
            let near = Some(parent).filter(|p| tree.class(*p).is_ok_and(|c| c.is_physical()));
            if let Some(id) = new_instance(&mut tree, class, parent, near) {
                editor.selection = Some(id);
                if is_code(class) {
                    editor.open_script = Some(id);
                }
            }
        }
        Action::Delete(id) => {
            let tree_rc = engine.tree.clone();
            let mut tree = tree_rc.borrow_mut();
            if !tree.class(id).is_ok_and(|c| c.is_user_creatable()) {
                return;
            }
            if !playing {
                editor.history.begin_edit(&tree, now);
                editor.dirty = true;
            }
            if tree.destroy(id).is_ok() {
                tree.purge_destroyed_if_editing(playing);
                editor.selection = None;
            }
        }
        Action::Duplicate(id) => {
            let tree_rc = engine.tree.clone();
            let mut tree = tree_rc.borrow_mut();
            if !playing {
                editor.history.begin_edit(&tree, now);
                editor.dirty = true;
            }
            let parent = tree.parent(id).ok().flatten().unwrap_or(tree.workspace());
            if let Ok(copy) = tree.clone_subtree(id) {
                // Offset copies so they don't hide inside the original.
                if let (Some(pos), Some(size)) =
                    (tree.get_vec3(copy, Property::Position), tree.get_vec3(copy, Property::Size))
                {
                    let moved = pos + libroxel_instance::Vec3::new(0.0, size.y, 0.0);
                    let _ = tree.set(copy, Property::Position, PropertyValue::Vec3(moved));
                }
                let _ = tree.set_parent(copy, Some(parent));
                editor.selection = Some(copy);
            }
        }
        Action::Select(id) => editor.selection = id,
        Action::GuiHover(id) => {
            if let Some(session) = engine.session.as_mut() {
                session.gui_hover(id);
            }
        }
        Action::Scroll(frame, delta, max) => {
            let mut tree = engine.tree.borrow_mut();
            if let Ok(PropertyValue::Vec2(at)) = tree.get(frame, Property::CanvasPosition) {
                // Wheel down (a negative delta) moves further down the canvas.
                let next = libroxel_instance::Vec2::new(
                    (at.x - delta.x).clamp(0.0, max.x.max(0.0)),
                    (at.y - delta.y).clamp(0.0, max.y.max(0.0)),
                );
                if next != at {
                    let _ = tree.set(frame, Property::CanvasPosition, PropertyValue::Vec2(next));
                }
            }
        }
        Action::ClickButton(id) => {
            if let Some(session) = engine.session.as_mut() {
                session.click_button(id);
            }
        }
        Action::FocusTextBox(id, enter) => {
            if let Some(session) = engine.session.as_mut() {
                session.focus_text_box(id, enter);
            }
            editor.typing = id;
            editor.focus_typing = id.is_some();
        }
        Action::ToggleTool(slot) => {
            if let Some(session) = engine.session.as_mut() {
                session.toggle_tool(slot);
            }
        }
        Action::TriggerPrompt(id) => {
            if let Some(session) = engine.session.as_mut() {
                session.trigger_prompt(id);
            }
        }
        Action::Mouse(input, down, processed) => {
            if let Some(session) = engine.session.as_mut() {
                if down {
                    session.input_began(input, processed);
                } else {
                    session.input_ended(input, processed);
                }
            }
        }
        Action::ClearData => {
            let path = data_path(editor);
            match std::fs::remove_file(&path) {
                Ok(()) => log(engine, LogLevel::Info, format!("Cleared saved data ({})", path.display())),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    log(engine, LogLevel::Info, "This game has no saved data yet")
                }
                Err(e) => log(engine, LogLevel::Error, format!("Could not clear saved data: {e}")),
            }
        }
        Action::ClickPart(id) => {
            let clickable = engine.session.as_mut().is_some_and(|s| s.click_part(id));
            if !clickable {
                editor.reveal_selection = true;
                editor.selection = Some(id);
            }
        }
        Action::Reparent(child, parent) => {
            let tree_rc = engine.tree.clone();
            let mut tree = tree_rc.borrow_mut();
            if child == parent {
                return;
            }
            let before = tree.clone();
            match tree.set_parent(child, Some(parent)) {
                Ok(()) => {
                    if !playing {
                        editor.history.begin_edit(&before, now);
                        editor.dirty = true;
                    }
                }
                Err(err) => {
                    drop(tree);
                    log(engine, LogLevel::Warn, format!("Can't move that there: {err}"));
                }
            }
        }
        Action::OpenScript(id) => {
            editor.selection = Some(id);
            editor.open_script = Some(id);
        }
        Action::Rename(id, name) => {
            editor.renaming = None;
            let tree_rc = engine.tree.clone();
            let mut tree = tree_rc.borrow_mut();
            if !name.trim().is_empty() && tree.name(id).is_ok_and(|n| n != name) {
                if !playing {
                    editor.history.begin_edit(&tree, now);
                    editor.dirty = true;
                }
                let _ = tree.set_name(id, name.trim());
            }
        }
        Action::RunCommand(code) => {
            log(engine, LogLevel::Info, format!("> {code}"));
            if let Some(session) = engine.session.as_mut() {
                let result = session.runtime().run("command", &code);
                let mut lines = session.take_log();
                for line in session.runtime().take_output() {
                    let level = match line.level {
                        libroxel_script::OutputLevel::Print => LogLevel::Info,
                        libroxel_script::OutputLevel::Warn => LogLevel::Warn,
                    };
                    lines.push(LogEntry { level, time: session.runtime().clock(), text: line.text });
                }
                engine.log.extend(lines);
                if let Err(err) = result {
                    log(engine, LogLevel::Error, err.to_string());
                }
            }
        }
        Action::ClearOutput => engine.log.clear(),
    }
}

trait PurgeExt {
    fn purge_destroyed_if_editing(&mut self, playing: bool);
}

impl PurgeExt for InstanceTree {
    /// While editing nothing else holds ids, so deleted instances can be freed.
    fn purge_destroyed_if_editing(&mut self, playing: bool) {
        if !playing {
            self.purge_destroyed();
        }
    }
}
