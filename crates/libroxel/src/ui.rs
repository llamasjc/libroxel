//! The editor's panels: menu and toolbar, explorer, properties, output,
//! the script editor and file dialogs.

use std::path::PathBuf;

use crate::i18n::{t, tf};
use crate::theme::{self, icon};
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
    /// Put the selection in a new model (Ctrl+G).
    Group(InstanceId),
    /// Put a model's or folder's contents where it is, and remove it (Ctrl+U).
    Ungroup(InstanceId),
    /// Anchor or unanchor the selected part.
    ToggleAnchor(InstanceId),
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
    /// Join the game whose connection dropped again, now.
    Reconnect,
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
    /// Add a 3D model file (dropped on the window) as a part.
    ImportMesh(PathBuf),
    /// Add the instances in a model file (.rbxm or .rbxmx) to the workspace.
    InsertModelFile(PathBuf),
    Delete(InstanceId),
    Duplicate(InstanceId),
    Select(Option<InstanceId>),
    /// While playing: the player clicked a GUI button.
    ClickButton(InstanceId),
    /// While playing: the left mouse button went down (or up) on a GUI button.
    PressButton(InstanceId, bool),
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
    ResetCharacter,
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
    /// Let other computers edit this game too.
    ShareEdits,
    /// Edit the game another computer shares (a join code or an address).
    JoinEdits {
        address: String,
        name: String,
    },
    StopEdits,
    /// While hosting: remove a joined player from the game.
    Kick(InstanceId),
    /// Share a model (or part) as a hat in the catalog.
    ShareHat(InstanceId),
    OpenMarket,
    InsertMarketModel(String, String),
    RunPlugin(crate::plugins::Plugin),
    PublishAsset(crate::market::PublishForm),
    /// Put the open game on the Hub for anyone to play.
    Publish {
        title: String,
        description: String,
    },
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
    ClassKind::SurfaceGui,
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
    ClassKind::Animation,
    ClassKind::UIAspectRatioConstraint,
    ClassKind::UIScale,
    ClassKind::UIGradient,
    ClassKind::Team,
    ClassKind::WeldConstraint,
    ClassKind::Weld,
    ClassKind::Motor6D,
    ClassKind::Configuration,
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
    ClassKind::PrismaticConstraint,
    ClassKind::BallSocketConstraint,
    ClassKind::AngularVelocity,
    ClassKind::BodyAngularVelocity,
    ClassKind::AlignOrientation,
    ClassKind::BodyGyro,
    ClassKind::Torque,
    ClassKind::RodConstraint,
    ClassKind::NoCollisionConstraint,
    ClassKind::Decal,
    ClassKind::Texture,
    ClassKind::Sound,
    ClassKind::ParticleEmitter,
    ClassKind::Fire,
    ClassKind::Smoke,
    ClassKind::Sparkles,
    ClassKind::Highlight,
    ClassKind::SelectionBox,
    ClassKind::Beam,
    ClassKind::Trail,
    ClassKind::Sky,
    ClassKind::Atmosphere,
    ClassKind::BloomEffect,
    ClassKind::ColorCorrectionEffect,
    ClassKind::UISizeConstraint,
    ClassKind::UITextSizeConstraint,
];

const SHAPES: &[(PartShape, &str)] = &[
    (PartShape::Block, "Block"),
    (PartShape::Ball, "Ball"),
    (PartShape::Cylinder, "Cylinder"),
    (PartShape::Wedge, "Wedge"),
];

fn icon(class: ClassKind) -> &'static str {
    crate::theme::class_glyph(class).0
}

/// Classes that hold Luau source and open in the script editor.
fn is_code(class: ClassKind) -> bool {
    matches!(class, ClassKind::Script | ClassKind::LocalScript | ClassKind::ModuleScript)
}

fn level_color(level: LogLevel) -> Color32 {
    match level {
        LogLevel::Info => theme::TEXT,
        LogLevel::Warn => theme::AMBER,
        LogLevel::Error => theme::RED,
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
    mut touch_input: ResMut<libroxel_engine::devices::VirtualInput>,
    mut screen_touches: ResMut<libroxel_engine::devices::ScreenTouches>,
    render_scale: Res<crate::render_scale::RenderScale>,
    adapter: Option<Res<bevy::render::renderer::RenderAdapterInfo>>,
    mut quality: Option<ResMut<libroxel_engine::view::GraphicsQuality>>,
) -> Result {
    let ui_began = std::time::Instant::now();
    let ctx = contexts.ctx_mut()?.clone();
    if !crate::theme::install(&ctx) {
        ctx.request_repaint();
        return Ok(());
    }
    let now = time.elapsed_secs_f64();
    let engine = &mut *engine;
    let editor = &mut *editor;
    let tree_rc = engine.tree.clone();
    let playing = engine.is_playing();
    if editor.graphics_adapter.is_empty()
        && let Some(info) = adapter
    {
        editor.graphics_adapter = format!("{} ({:?}, {:?})", info.name, info.backend, info.device_type);
    }
    let mut actions: Vec<Action> = Vec::new();
    // Everything in the Output also goes to a log file.
    editor.log_file.write_new(&engine.log);
    crate::report::crash_screen(&ctx, &mut editor.crash);
    recovery_screen(&ctx, engine, editor);
    keep_unsaved_copy(engine, editor, now);
    // Model files dropped on the window come in as parts.
    for file in ctx.input(|i| i.raw.dropped_files.clone()) {
        let path = file.path();
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("obj")) {
            actions.push(Action::ImportMesh(path.to_path_buf()));
        } else if crate::open::is_model_file(path) {
            actions.push(Action::InsertModelFile(path.to_path_buf()));
        } else if crate::open::is_game_file(path) {
            actions.push(Action::Open(path.to_path_buf()));
        }
    }
    // A game that has ended no longer needs its internet connection.
    if engine.session.is_none() {
        editor.tunnel = None;
    }
    rejoin_tick(engine, editor);
    let hosting = engine.session.as_ref().filter(|s| s.host_address().is_some()).map(|s| {
        let code = match &editor.tunnel {
            Some(Tunnel::Host(tunnel)) => tunnel.code(),
            _ => None,
        };
        (code, s.joined_count())
    });
    editor.platform.tick(hosting);
    editor.platform.look_up_account();
    if let Some(look) = editor.platform.take_account_look() {
        editor.menu.use_look(&look);
        let look = editor.menu.look();
        if let Some(session) = engine.session.as_mut() {
            session.set_local_look(look);
        }
    }
    editor.platform.social_tick(engine.session.is_some());
    editor.platform.awards_tick(engine.session.as_mut());
    for name in std::mem::take(&mut editor.platform.awards.earned) {
        log(engine, LogLevel::Info, format!("Badge earned: {name}"));
    }
    editor.platform.chats_tick(editor.menu.open && editor.menu.tab == crate::menu::Tab::Messages);
    let hub = editor.platform.client();
    editor.voice.tick(engine.session.as_mut(), || hub);
    if !playing {
        edit_together(engine, editor);
    }
    if editor.player_mode
        && !editor.menu.open
        && editor.platform.social.joined.is_some()
        && let Some(code) = editor.platform.party_follow()
    {
        let name = editor.platform.name.clone();
        menu_pick(crate::menu::Pick::Join { code, name }, engine, editor, now);
        return Ok(());
    }
    match editor.platform.item_status.take() {
        Some(Ok(said)) => log(engine, LogLevel::Info, said),
        Some(Err(why)) => log(engine, LogLevel::Error, format!("Could not share the hat: {why}")),
        None => {}
    }

    if engine.session.is_none()
        && let Some(snapshot) = editor.edit_snapshot.take()
    {
        *engine.tree.borrow_mut() = snapshot;
        editor.tunnel = None;
    }
    // A game played from the main menu ended: back to the menu.
    if editor.player_mode && engine.session.is_none() {
        editor.player_mode = false;
        editor.paused = false;
        editor.menu.open = true;
        editor.menu.scanned = false;
    }
    if editor.menu.open {
        if engine.session.is_none() && !editor.dirty {
            let _ = crate::update::apply(false);
        }
        capture.pointer = true;
        capture.keyboard = true;
        let mut picks: Vec<crate::menu::Pick> =
            editor.menu.show(&ctx, &mut editor.platform).into_iter().collect();
        if let Some((code, name)) = editor.platform.window(&ctx) {
            picks.push(crate::menu::Pick::Join { code, name });
        }
        if picks.is_empty()
            && !editor.dirty
            && let Some(code) = editor.platform.party_follow()
        {
            picks.push(crate::menu::Pick::Join { code, name: editor.platform.name.clone() });
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
        // Inside the safe area: clear of a phone's notch (the whole window elsewhere).
        egui::UiBuilder::new().layer_id(egui::LayerId::background()).max_rect(ctx.content_rect()),
    );

    if !editor.player_mode {
        // Laid out the way Studio users know: the ribbon on top, the
        // toolbox on the left, the explorer over the properties on the
        // right, and the output along the bottom.
        egui::Panel::top("ribbon").show(&mut root, |ui| {
            ribbon(ui, editor, engine.session.as_ref(), &tree_rc.borrow(), &mut actions);
        });
        let side_panels = !playing || editor.panels_in_play;
        if editor.show_toolbox && side_panels {
            egui::Panel::left("toolbox").resizable(true).default_size(190.0).show(&mut root, |ui| {
                toolbox(ui, editor, playing, &mut actions);
            });
        }
        if (editor.show_explorer || editor.show_properties) && side_panels {
            egui::Panel::right("side").resizable(true).default_size(300.0).show(&mut root, |ui| {
                if editor.show_explorer {
                    let explorer = |ui: &mut egui::Ui, editor: &mut Editor, actions: &mut Vec<Action>| {
                        theme::panel_title(ui, icon::TREE_STRUCTURE, "Explorer");
                        ui.add(
                            egui::TextEdit::singleline(&mut editor.explorer_filter)
                                .hint_text(format!(
                                    "{} Filter workspace (Ctrl+Shift+X)",
                                    icon::MAGNIFYING_GLASS
                                ))
                                .id(egui::Id::new("explorer-filter"))
                                .desired_width(f32::INFINITY),
                        );
                        egui::ScrollArea::vertical().id_salt("explorer").auto_shrink([false, false]).show(
                            ui,
                            |ui| {
                                // A tree reads best packed close.
                                ui.spacing_mut().item_spacing.y = 0.0;
                                ui.spacing_mut().interact_size.y = 22.0;
                                let tree = tree_rc.borrow();
                                let filter = ExplorerFilter::new(&tree, &editor.explorer_filter);
                                if filter.as_ref().is_some_and(|f| f.shown.is_empty()) {
                                    ui.label(RichText::new(t("Nothing matches the filter.")).weak());
                                }
                                explorer_node(ui, &tree, tree.root(), editor, actions, 0, filter.as_ref());
                                editor.reveal_selection = false;
                            },
                        );
                    };
                    if editor.show_properties {
                        let half = ui.available_height() * 0.45;
                        egui::Panel::top("explorer")
                            .resizable(true)
                            .default_size(half)
                            .show(ui, |ui| explorer(ui, editor, &mut actions));
                    } else {
                        explorer(ui, editor, &mut actions);
                    }
                }
                if editor.show_properties {
                    theme::panel_title(ui, icon::SLIDERS_HORIZONTAL, "Properties");
                    egui::ScrollArea::vertical().id_salt("properties").auto_shrink([false, false]).show(
                        ui,
                        |ui| {
                            properties(ui, editor, &mut tree_rc.borrow_mut(), now, playing, &mut actions);
                        },
                    );
                }
            });
        }
        if editor.show_output {
            egui::Panel::bottom("output").resizable(true).default_size(170.0).show(&mut root, |ui| {
                output(ui, engine, editor, playing, &mut actions);
            });
        }
    }

    // The space left over is the 3D view.
    let mut gizmo_hot = false;
    let mut gui_hit: gui::GuiHit = None;
    let mut prompt_hits: Vec<(InstanceId, egui::Rect)> = Vec::new();
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
    // Pictures are looked for beside the game, then in the games folder;
    // files named by asset number in an `assets` folder in any of these.
    let mut folders = Vec::new();
    if let Some(dir) = editor.file.as_ref().or(editor.imported_from.as_ref()).and_then(|f| f.parent()) {
        folders.push(dir.to_path_buf());
        folders.push(dir.join("images"));
        folders.push(dir.join("meshes"));
    }
    folders.push(games_dir().join("images"));
    folders.push(crate::mesh_import::meshes_dir());
    // So the games folder's own `assets` folder is searched for asset numbers.
    folders.push(games_dir());
    crate::images::set_folders(folders);
    if editor.picker.is_open() {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    match editor.picker.poll() {
        Some(crate::file_picker::Picked::File(path)) => actions.push(Action::Open(path)),
        // No picker on this computer: ask for the path instead.
        Some(crate::file_picker::Picked::Unavailable) => {
            let path = games_dir().join("untitled.lrx").display().to_string();
            editor.dialog = Some(Dialog::Open { path });
        }
        Some(crate::file_picker::Picked::Cancelled) | None => {}
    }
    // On phones the 3D view is drawn with fewer pixels, into a picture
    // that goes under everything else here (see render_scale.rs).
    let world_picture = render_scale.image.as_ref().map(|image| {
        contexts
            .image_id(image)
            .unwrap_or_else(|| contexts.add_image(bevy_egui::EguiTextureHandle::Weak(image.id())))
    });
    let central = egui::CentralPanel::no_frame()
        .show(&mut root, |ui| {
            if let Some(picture) = world_picture {
                let whole = ctx.viewport_rect();
                let rect = ui.max_rect();
                let uv = egui::Rect::from_min_max(
                    egui::pos2(rect.min.x / whole.width(), rect.min.y / whole.height()),
                    egui::pos2(rect.max.x / whole.width(), rect.max.y / whole.height()),
                );
                ui.painter().image(picture, rect, uv, Color32::WHITE);
            }
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
            // (Not while the 3D view isn't drawn at all, behind a loading screen.)
            if let Ok((cam, gt, _, _)) = cameras.single()
                && cam.is_active
            {
                let selected = if playing { None } else { editor.selection };
                if let Some(hit) = gui::surfaces(ui, &tree_rc.borrow(), (cam, gt), selected) {
                    gui_hit = Some(hit);
                    if hit.1.is_button() {
                        boxes.input = Some(hit);
                    }
                }
                if let Some(hit) = gui::billboards(ui, &tree_rc.borrow(), (cam, gt), gui_root, selected, &mut boxes) {
                    gui_hit = Some(hit);
                }
            }
            // Nothing covers the 3D view unless the game's GUI says so below.
            libroxel_engine::view::VIEW_COVERED.store(false, std::sync::atomic::Ordering::Relaxed);
            if !playing {
                libroxel_script::RENDERING_3D.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            if let Some(root) = gui_root {
                let selected = if playing { None } else { editor.selection };
                if let Some(hit) = gui::draw(ui, &tree_rc.borrow(), root, ui.max_rect(), selected, &mut boxes)
                {
                    gui_hit = Some(hit);
                }
                if let Some(session) = engine.session.as_mut() {
                    let rects =
                        boxes.rects.iter().map(|(id, r)| (*id, [r.min.x, r.min.y, r.width(), r.height()]));
                    session.set_gui_rects(rects.collect());
                }
            }
            if playing {
                text_field(ui, editor, &mut tree_rc.borrow_mut(), &boxes, &mut actions);
            }
            if let Some(session) = engine.session.as_ref() {
                hotbar_hit = gui::hotbar(ui, &tree_rc.borrow(), &session.hotbar());
            }
            editor.voice.ui(ui, &tree_rc.borrow());
            if let (Some(session), Ok((cam, gt, _, _))) = (engine.session.as_ref(), cameras.single()) {
                let tree = tree_rc.borrow();
                for active in session.active_prompts() {
                    // Just above the part, so the character doesn't hide it.
                    let at = tree
                        .get_vec3(active.part, Property::Position)
                        .zip(tree.get_vec3(active.part, Property::Size))
                        .map(|(p, s)| Vec3::new(p.x, p.y + s.y / 2.0 + 1.5, p.z));
                    if let Some(at) = at
                        && let Ok(screen) = cam.world_to_viewport(gt, at)
                    {
                        let key = names::enum_item_name(EnumValue::KeyCode(active.key));
                        let above = prompt_hits.iter().filter(|(p, _)| tree.parent(*p).ok().flatten() == Some(active.part));
                        let lift: f32 = above.map(|(_, r)| r.height() + 6.0).sum();
                        let rect = gui::draw_prompt(
                            ui,
                            &tree,
                            active.prompt,
                            key,
                            active.progress,
                            egui::pos2(screen.x, screen.y - lift),
                        );
                        prompt_hits.push((active.prompt, rect));
                    }
                }
            }
            if playing && ui.input(|i| i.modifiers.shift && i.key_pressed(Key::F1)) {
                editor.stats.shown = !editor.stats.shown;
            }
            if let Some(session) = engine.session.as_mut() {
                let adapter = editor.graphics_adapter.clone();
                let level = quality.as_ref().map_or_else(String::new, |q| {
                    format!("{}{}", q.level, if q.auto { " (auto)" } else { "" })
                });
                let about = || {
                    let tree = tree_rc.borrow();
                    let all = tree.descendants(tree.root()).map(|d| d.len()).unwrap_or(0);
                    let parts = tree.descendants_of_class(tree.workspace(), ClassKind::Part).len();
                    format!(
                        "{}\nGraphics: {}, quality {level}\nWindow: {:.0} x {:.0}\nInstances: {all}, parts in Workspace: {parts}",
                        crate::debugging::device_line(),
                        adapter,
                        ui.ctx().content_rect().width(),
                        ui.ctx().content_rect().height(),
                    )
                };
                editor.stats.update(session, ui.input(|i| i.unstable_dt), about);
            }
            if playing && editor.stats.shown {
                let at = ui.max_rect().left_bottom() + egui::vec2(8.0, -30.0);
                let galley = ui.painter().layout_no_wrap(
                    editor.stats.text.clone(),
                    egui::FontId::monospace(12.0),
                    Color32::WHITE,
                );
                let rect = egui::Align2::LEFT_BOTTOM.anchor_size(at, galley.size()).expand(5.0);
                ui.painter().rect_filled(rect, 4.0, Color32::from_black_alpha(170));
                ui.painter().galley(rect.min + egui::vec2(5.0, 5.0), galley, Color32::WHITE);
            }
            let list_on = engine.session.as_ref().is_none_or(|s| s.runtime().core_gui_enabled("PlayerList"));
            if playing && list_on {
                let game = game_name(editor);
                leaderboard(ui, &tree_rc.borrow(), &mut editor.platform, &game);
            }
            ui.max_rect()
        })
        .inner;

    script_editor(&ctx, editor, &mut tree_rc.borrow_mut(), now, playing);
    dialogs(&ctx, editor, &mut actions);
    if editor.market.active()
        && let Some(todo) = editor.market.window(&ctx, editor.platform.client(), editor.selection)
    {
        actions.push(match todo {
            crate::market::Do::Insert(name, text) => Action::InsertMarketModel(name, text),
            crate::market::Do::Run(plugin) => Action::RunPlugin(plugin),
            crate::market::Do::Publish(form) => Action::PublishAsset(form),
        });
    }
    if let Some((address, name)) = editor.platform.window(&ctx) {
        actions.push(Action::Join { address, name });
    }

    if editor.player_mode {
        game_menu(&ctx, editor, engine.session.as_ref(), central, &mut actions, quality.as_deref_mut());
    }
    match engine.session.as_mut() {
        Some(session) if editor.loading.active() => {
            let filling = libroxel_engine::view::VIEW_FILLING.load(std::sync::atomic::Ordering::Relaxed);
            if editor.loading.show(&ctx, central, now, session.joining(), filling) {
                editor.loading.stop();
                actions.push(Action::Stop);
            }
            if let Some(text) = editor.loading.take_finished() {
                session.note(text);
            }
        }
        Some(_) => {}
        None => editor.loading.stop(),
    }
    libroxel_engine::view::LOADING_SCREEN
        .store(editor.loading.covering(), std::sync::atomic::Ordering::Relaxed);
    if engine.session.as_ref().is_some_and(PlaySession::wrong_version) {
        crate::update::now();
        if !editor.dirty
            && let Err(why) = crate::update::apply(true)
        {
            log(engine, LogLevel::Warn, format!("Could not update: {why}"));
        }
        let text = if crate::update::checking() || crate::update::ready().is_some() {
            t("Updating…")
        } else {
            t("A different version of Libroxel")
        };
        egui::Area::new("version".into())
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .order(egui::Order::Foreground)
            .show(&ctx, |ui| {
                ui.label(RichText::new(text).size(22.0).color(Color32::WHITE));
            });
    }

    // Touch screens get on-screen controls while playing.
    if playing && !editor.menu.open && !editor.paused {
        editor.touch.hidden = screen_touches.controls_hidden;
        editor.touch.actions = crate::touch::lay_out_actions(central, &screen_touches.buttons);
        for (id, phase, pos, processed) in editor.touch.show(&ctx, central, &boxes.inputs) {
            let phase = match phase {
                egui::TouchPhase::Start => libroxel_script::TouchPhase::Began,
                egui::TouchPhase::Move => libroxel_script::TouchPhase::Moved,
                egui::TouchPhase::End | egui::TouchPhase::Cancel => libroxel_script::TouchPhase::Ended,
            };
            let at = pos - central.min;
            screen_touches.touches.push(libroxel_engine::devices::ScreenTouch {
                id,
                phase,
                x: at.x,
                y: at.y,
                processed,
            });
        }
        screen_touches.presses.extend(editor.touch.take_presses());
        let stick = editor.touch.stick(central);
        touch_input.stick = Vec2::new(stick.x, stick.y);
        touch_input.jump = editor.touch.jump();
        // Dragging up looks up, as the right stick does.
        let look = editor.touch.take_look();
        touch_input.look += Vec2::new(look.x, -look.y);
        touch_input.zoom += editor.touch.take_zoom();
    } else {
        editor.touch.reset();
        touch_input.stick = Vec2::ZERO;
        touch_input.jump = false;
    }
    touch_input.touched = editor.touch.seen;

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
    capture.confine = editor.player_mode;
    if editor.paused {
        capture.pointer = true;
        capture.keyboard = true;
    }
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

    // GUI buttons: down when pressed on one, then up and a click when let go
    // over the same one, however long it was held (hold-to-confirm buttons).
    // Only buttons, text boxes and Active objects take a press; a label on
    // a button or a see-through frame over the screen lets it through.
    let button_under = boxes.input.filter(|(_, class)| class.is_button() && over_view).map(|(id, _)| id);
    if playing && on_hotbar.is_none() {
        let (pressed, released) = ctx.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_released()));
        if pressed && let Some(id) = button_under {
            editor.pressed_button = Some(id);
            actions.push(Action::PressButton(id, true));
        }
        if released
            && let Some(id) = editor.pressed_button.take()
            && button_under == Some(id)
        {
            actions.push(Action::PressButton(id, false));
            actions.push(Action::ClickButton(id));
        }
    } else {
        editor.pressed_button = None;
    }

    // Mouse buttons for the input service, while playing.
    if playing && over_view {
        let on_ui = boxes.input.is_some()
            || on_hotbar.is_some()
            || prompt_hits.iter().any(|(_, r)| pointer.is_some_and(|p| r.contains(p)));
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
    let clicked = ctx.input(|i| i.pointer.primary_clicked())
        && editor.tool != Tool::Terrain
        && !(playing && editor.touch.has_pointer());
    let on_prompt = prompt_hits.iter().copied().find(|(_, r)| pointer.is_some_and(|p| r.contains(p)));
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
        // In the window's pixels, or the smaller picture's on a phone.
        let scale = render_scale.pixels_per_point.max(0.01);
        let share = scale / window.scale_factor().max(0.01);
        let (width, height) = (
            (window.physical_width() as f32 * share).ceil() as u32,
            (window.physical_height() as f32 * share).ceil() as u32,
        );
        let min = UVec2::new((central.min.x * scale) as u32, (central.min.y * scale) as u32);
        let max = UVec2::new(
            ((central.max.x * scale) as u32).min(width),
            ((central.max.y * scale) as u32).min(height),
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
    editor.stats.report.ui_seconds += ui_began.elapsed().as_secs_f64();
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
    libroxel_engine::character::add_joints(tree, root, head, &limbs);
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
    if let Some(id) = editor.focus_filter.take() {
        ctx.memory_mut(|m| m.request_focus(id));
    }
    let typing = ctx.egui_wants_keyboard_input();
    ctx.input_mut(|i| {
        let cmd = |key| KeyboardShortcut::new(Modifiers::COMMAND, key);
        if i.consume_shortcut(&cmd(Key::S)) {
            actions.push(Action::Save);
        }
        if i.consume_shortcut(&cmd(Key::O)) {
            editor.picker.open(&games_dir());
        }
        if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z))
            || i.consume_shortcut(&cmd(Key::Y))
        {
            actions.push(Action::Redo);
        }
        if i.consume_shortcut(&cmd(Key::Z)) {
            actions.push(Action::Undo);
        }
        // As in Studio: Ctrl+Shift+X filters the Explorer, Ctrl+Shift+P the Properties.
        if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::X)) {
            editor.show_explorer = true;
            editor.focus_filter = Some(egui::Id::new("explorer-filter"));
        }
        if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::P)) {
            editor.show_properties = true;
            editor.focus_filter = Some(egui::Id::new("properties-filter"));
        }
        // As in Studio: F5 plays, Shift+F5 stops.
        if i.consume_key(Modifiers::SHIFT, Key::F5) {
            actions.push(Action::Stop);
        }
        if i.consume_key(Modifiers::NONE, Key::F5) {
            actions.push(if playing { Action::Stop } else { Action::Play });
        }
        if playing && i.consume_key(Modifiers::NONE, Key::Escape) {
            if editor.player_mode {
                editor.paused = !editor.paused;
            } else {
                actions.push(Action::Stop);
            }
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
            if i.consume_shortcut(&cmd(Key::G)) {
                actions.push(Action::Group(id));
            }
            if i.consume_shortcut(&cmd(Key::U)) {
                actions.push(Action::Ungroup(id));
            }
            // F looks at the selection.
            if i.consume_key(Modifiers::NONE, Key::F) && !playing {
                editor.focus_selection = true;
            }
        }
    });
}

/// How a hosted or joined game is going: who joined, the join code, the connection.
fn session_status(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    session: Option<&PlaySession>,
    actions: &mut Vec<Action>,
) {
    if let Some(session) = session {
        let red = theme::RED;
        if let Some(address) = session.host_address() {
            let joined = session.joined_count();
            let who =
                if joined == 1 { t("1 joined").to_owned() } else { tf("{n} joined", &[("n", &joined)]) };
            theme::status(
                ui,
                theme::GREEN,
                &tf("Hosting on port {port} · {who}", &[("port", &address.port()), ("who", &who)]),
            )
            .on_hover_text(t("Others on your network join with this computer's address and this port"));
            let players = session.joined_players();
            if !players.is_empty() {
                ui.menu_button(format!("{}  {}", icon::USERS_THREE, t("Players")), |ui| {
                    for (player, name) in players {
                        ui.horizontal(|ui| {
                            ui.label(name);
                            if ui
                                .small_button("Remove")
                                .on_hover_text(t("Take them out of this game; they can't join it again"))
                                .clicked()
                            {
                                actions.push(Action::Kick(player));
                                ui.close();
                            }
                        });
                    }
                });
            }
            if let Some(Tunnel::Host(tunnel)) = &editor.tunnel {
                if let Some(code) = tunnel.code() {
                    let copy = ui.button(format!("{}  {}", icon::COPY, t("Copy join code"))).on_hover_text(
                        "Friends anywhere paste it into Join. It isn't shown here, so it stays off streams.",
                    );
                    if copy.clicked() {
                        ui.ctx().copy_text(code);
                    }
                } else if let Some(error) = tunnel.error() {
                    ui.colored_label(red, t("No join code")).on_hover_text(&error);
                    crate::report::button(ui, &format!("No join code: {error}"));
                } else {
                    theme::status(ui, theme::AMBER, "Getting a join code…");
                }
            }
        } else if session.is_joined() {
            let tunnel = match &editor.tunnel {
                Some(Tunnel::Join(tunnel)) => Some(tunnel),
                _ => None,
            };
            if let Some(error) = tunnel.and_then(|t| t.error()) {
                theme::status(ui, theme::RED, "Could not reach the host").on_hover_text(&error);
                crate::report::button(ui, &format!("Could not reach the host: {error}"));
            } else if session.disconnected() && session.can_reconnect() {
                if editor.rejoin.tries < REJOIN_TRIES {
                    theme::status(ui, theme::AMBER, "Connection lost · trying to reconnect…");
                } else {
                    theme::status(ui, theme::RED, "Connection lost");
                }
                if ui.button(t("Reconnect")).clicked() {
                    actions.push(Action::Reconnect);
                }
            } else if session.disconnected() {
                theme::status(ui, theme::RED, "Disconnected");
            } else if tunnel.is_some_and(|t| !t.connected()) {
                theme::status(ui, theme::AMBER, "Connecting…");
            } else {
                theme::status(ui, theme::GREEN, "Joined");
            }
        }
    }
}

/// The File menu: new, open, save, publish and the main menu.
fn file_menu(ui: &mut egui::Ui, editor: &mut Editor, playing: bool, actions: &mut Vec<Action>) {
    if ui.add_enabled(!playing, egui::Button::new(t("Main menu"))).clicked() {
        editor.menu.open = true;
        editor.menu.scanned = false;
    }
    ui.separator();
    if ui.button(t("New")).clicked() {
        actions.push(Action::New);
    }
    ui.menu_button(t("Open a starter game"), |ui| {
        for game in crate::starter_games::STARTER_GAMES {
            if ui.button(format!("{} {}", game.symbol, game.name)).on_hover_text(game.about).clicked() {
                actions.push(Action::OpenStarter(game.name));
            }
        }
    });
    if ui
        .add_enabled(!playing, egui::Button::new(t("Clear saved data")))
        .on_hover_text(t("Forget what this game's data stores saved in earlier play tests"))
        .clicked()
    {
        actions.push(Action::ClearData);
    }
    if ui.button(t("Open…       Ctrl+O")).clicked() {
        editor.picker.open(&games_dir());
    }
    if ui.button(t("Open by path…")).clicked() {
        let path = games_dir().join("untitled.lrx").display().to_string();
        editor.dialog = Some(Dialog::Open { path });
    }
    ui.separator();
    if ui.add_enabled(!playing, egui::Button::new(t("Save        Ctrl+S"))).clicked() {
        actions.push(Action::Save);
    }
    if ui.add_enabled(!playing, egui::Button::new(t("Save as…"))).clicked() {
        let path =
            editor.file.clone().unwrap_or_else(|| games_dir().join("untitled.lrx")).display().to_string();
        editor.dialog = Some(Dialog::SaveAs { path });
    }
    ui.separator();
    if ui
        .add_enabled(
            !playing,
            egui::Button::new(format!("{}  {}", icon::GLOBE_HEMISPHERE_WEST, t("Publish…"))),
        )
        .on_hover_text(t("Put this game on the Hub, so anyone can find it Online and play it"))
        .clicked()
    {
        let title = editor
            .file
            .as_ref()
            .and_then(|f| f.file_stem())
            .map_or_else(|| t("My game").to_owned(), |s| s.to_string_lossy().into_owned());
        editor.platform.publish_status = None;
        editor.dialog = Some(Dialog::Publish { title, description: String::new() });
    }
}

/// Every class the editor can insert, with the ready-made part shapes and models.
fn insert_menu(ui: &mut egui::Ui, actions: &mut Vec<Action>) {
    for &class in INSERTABLE {
        if ui.button(format!("{} {}", icon(class), names::class_name(class))).clicked() {
            actions.push(Action::Insert(class));
        }
    }
    ui.separator();
    for &(shape, label) in SHAPES {
        if ui.button(tf("{shape} part", &[("shape", &t(label))])).clicked() {
            actions.push(Action::InsertShape(shape));
        }
    }
    ui.separator();
    if ui.button(format!("{}  {}", icon::PERSON_SIMPLE, t("Character (NPC)"))).clicked() {
        actions.push(Action::InsertNpc);
    }
    if ui.button(format!("{}  {}", icon::STEERING_WHEEL, t("Go-kart"))).clicked() {
        actions.push(Action::InsertKart);
    }
}

/// The menu Esc opens while playing a game from the main menu.
fn game_menu(
    ctx: &egui::Context,
    editor: &mut Editor,
    session: Option<&PlaySession>,
    view: egui::Rect,
    actions: &mut Vec<Action>,
    quality: Option<&mut libroxel_engine::view::GraphicsQuality>,
) {
    if !editor.paused {
        if editor.touch.seen {
            egui::Area::new("game-menu-button".into()).fixed_pos(view.min + egui::vec2(10.0, 10.0)).show(
                ctx,
                |ui| {
                    if theme::block_button(ui, theme::Kind::Plain, icon::LIST, "", 34.0).clicked() {
                        editor.paused = true;
                    }
                },
            );
        }
        return;
    }
    egui::Area::new("game-menu-shade".into()).fixed_pos(view.min).order(egui::Order::Middle).show(
        ctx,
        |ui| {
            let (rect, response) = ui.allocate_exact_size(view.size(), egui::Sense::click());
            ui.painter().rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(10, 40, 80, 110));
            if response.clicked() {
                editor.paused = false;
            }
        },
    );
    egui::Area::new("game-menu".into())
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            theme::panel().inner_margin(egui::Margin::same(22)).show(ui, |ui| {
                ui.set_width(280.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(t("Paused")).font(theme::light(26.0)).color(theme::HEADING));
                    ui.add_space(14.0);
                    if theme::block_button(ui, theme::Kind::Primary, icon::PLAY, "Resume", 40.0).clicked() {
                        editor.paused = false;
                    }
                    if session.is_some_and(PlaySession::can_reset) {
                        ui.add_space(8.0);
                        let reset = theme::block_button(
                            ui,
                            theme::Kind::Plain,
                            icon::ARROW_CLOCKWISE,
                            "Reset character",
                            40.0,
                        );
                        if reset.clicked() || ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::R)) {
                            editor.paused = false;
                            actions.push(Action::ResetCharacter);
                        }
                    }
                    ui.add_space(8.0);
                    if theme::block_button(ui, theme::Kind::Danger, icon::X, "Leave game", 40.0).clicked() {
                        editor.paused = false;
                        actions.push(Action::Stop);
                    }
                    if let Some(quality) = quality {
                        ui.add_space(14.0);
                        graphics_picker(ui, quality);
                    }
                    ui.add_space(10.0);
                    crate::voice::toggle(ui);
                    if session.is_some_and(|s| s.host_address().is_some() || s.is_joined()) {
                        ui.add_space(12.0);
                        ui.horizontal_wrapped(|ui| session_status(ui, editor, session, actions));
                    }
                });
            });
        });
}

fn graphics_picker(ui: &mut egui::Ui, quality: &mut libroxel_engine::view::GraphicsQuality) {
    use libroxel_engine::view::GraphicsQuality;
    theme::label_caps(ui, "Graphics");
    ui.add_space(4.0);
    let mut pick = None;
    ui.horizontal(|ui| {
        if ui.selectable_label(quality.auto, t("Auto")).clicked() {
            pick = Some((!quality.auto, quality.level));
        }
        ui.add_space(6.0);
        let bar = egui::vec2(14.0, 22.0);
        for level in 1..=GraphicsQuality::MAX {
            let (rect, response) = ui.allocate_exact_size(bar, egui::Sense::click());
            let on = level <= quality.level;
            let colour = match (on, quality.auto) {
                (true, false) => theme::ACCENT,
                (true, true) => theme::MUTED,
                _ => theme::LINE,
            };
            let height = bar.y * (0.4 + 0.6 * f32::from(level) / f32::from(GraphicsQuality::MAX));
            let fill =
                egui::Rect::from_min_max(egui::pos2(rect.left() + 2.0, rect.bottom() - height), rect.max);
            ui.painter().rect_filled(fill, 2.0, colour);
            if response.on_hover_text(level.to_string()).clicked() {
                pick = Some((false, level));
            }
        }
    });
    if let Some((auto, level)) = pick {
        quality.set(auto, level);
        crate::save_graphics(auto, level);
    }
}

/// The terrain tool's options, under the toolbar.
fn terrain_bar(ui: &mut egui::Ui, editor: &mut Editor, actions: &mut Vec<Action>) {
    use crate::terrain_tools::BrushMode;
    ui.horizontal(|ui| {
        theme::label_caps(ui, "Terrain");
        let brush = &mut editor.brush;
        ui.selectable_value(&mut brush.mode, BrushMode::Add, t("Add"));
        ui.selectable_value(&mut brush.mode, BrushMode::Dig, t("Dig"));
        ui.selectable_value(&mut brush.mode, BrushMode::Paint, t("Paint"));
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
        ui.add(egui::Slider::new(&mut brush.size, 1.0..=32.0).text(t("studs across")));
        ui.separator();
        if ui.button(t("Make a hill")).on_hover_text(t("A grassy hill where the view is looking")).clicked() {
            actions.push(Action::MakeHill(libroxel_instance::Vec3::ZERO));
        }
        if ui.button(t("Clear terrain")).clicked() {
            actions.push(Action::ClearTerrain);
        }
    });
}

/// The services in the order Studio lists them; anything else under the
/// game comes after, in its own order.
const SERVICE_ORDER: &[&str] = &[
    "Workspace",
    "Players",
    "Lighting",
    "MaterialService",
    "ReplicatedFirst",
    "ReplicatedStorage",
    "ServerScriptService",
    "ServerStorage",
    "StarterGui",
    "StarterPack",
    "StarterPlayer",
    "Teams",
    "SoundService",
    "TextChatService",
];

/// What the Explorer shows while its filter has text: the instances whose
/// name or class contains it, and their ancestors so the tree still reads.
struct ExplorerFilter {
    matched: std::collections::HashSet<InstanceId>,
    shown: std::collections::HashSet<InstanceId>,
}

impl ExplorerFilter {
    fn new(tree: &InstanceTree, text: &str) -> Option<Self> {
        let needle = text.trim().to_lowercase();
        if needle.is_empty() {
            return None;
        }
        let mut matched = std::collections::HashSet::new();
        let mut shown = std::collections::HashSet::new();
        for id in tree.descendants(tree.root()).unwrap_or_default() {
            let name = tree.name(id).unwrap_or("").to_lowercase();
            let class = tree.class(id).map(names::class_name).unwrap_or("").to_lowercase();
            if name.contains(&needle) || class.contains(&needle) {
                matched.insert(id);
                let mut at = Some(id);
                while let Some(a) = at {
                    if !shown.insert(a) {
                        break;
                    }
                    at = tree.parent(a).ok().flatten();
                }
            }
        }
        Some(Self { matched, shown })
    }
}

fn explorer_node(
    ui: &mut egui::Ui,
    tree: &InstanceTree,
    id: InstanceId,
    editor: &mut Editor,
    actions: &mut Vec<Action>,
    depth: usize,
    filter: Option<&ExplorerFilter>,
) {
    let Ok(class) = tree.class(id) else { return };
    let name = tree.name(id).unwrap_or("?");
    let mut children = tree.children(id).map(|c| c.to_vec()).unwrap_or_default();
    if let Some(f) = filter {
        children.retain(|c| f.shown.contains(c));
    }
    if id == tree.root() {
        let rank = |c: &InstanceId| {
            let n = tree.class(*c).map(names::class_name).unwrap_or("");
            SERVICE_ORDER.iter().position(|s| *s == n).unwrap_or(SERVICE_ORDER.len())
        };
        children.sort_by_key(rank);
    }
    let selected = editor.selection == Some(id);
    let dim = filter.is_some_and(|f| !f.matched.contains(&id));

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
        let mut text = crate::theme::class_label(class, name);
        if dim {
            for section in &mut text.sections {
                section.format.color = section.format.color.gamma_multiply(0.45);
            }
        }
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
            ui.menu_button(t("Insert"), |ui| {
                for &c in INSERTABLE {
                    if ui.button(format!("{} {}", icon(c), names::class_name(c))).clicked() {
                        actions.push(Action::InsertInto(c, id));
                        ui.close();
                    }
                }
            });
            let editable = class.is_user_creatable();
            if ui.add_enabled(editable, egui::Button::new(t("Rename"))).clicked() {
                editor.renaming = Some((id, name.to_owned()));
                ui.close();
            }
            if ui.add_enabled(editable, egui::Button::new(t("Duplicate"))).clicked() {
                actions.push(Action::Duplicate(id));
                ui.close();
            }
            if ui.add_enabled(editable, egui::Button::new("Delete")).clicked() {
                actions.push(Action::Delete(id));
                ui.close();
            }
            if is_code(class) && ui.button(t("Edit script")).clicked() {
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
            // Filtering opens the way to every match, without forgetting
            // what was open before.
            egui::Id::new(("explorer-open", id, filter.is_some())),
            depth < 2 || filter.is_some(),
        );
        if editor.reveal_selection
            && editor.selection.is_some_and(|sel| tree.is_descendant_of(sel, id).unwrap_or(false))
        {
            state.set_open(true);
        }
        state.show_header(ui, |ui| label(ui)).body(|ui| {
            for child in children {
                explorer_node(ui, tree, child, editor, actions, depth + 1, filter);
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
        ui.label(t("Select something in the Explorer or click a part in the view."));
        return;
    };
    let Ok(class) = tree.class(id) else { return };
    ui.label(RichText::new(format!("{} {}", icon(class), names::class_name(class))).strong());
    ui.add_space(4.0);

    ui.add(
        egui::TextEdit::singleline(&mut editor.properties_filter)
            .hint_text(format!("{} {}", icon::MAGNIFYING_GLASS, t("Filter Properties (Ctrl+Shift+P)")))
            .id(egui::Id::new("properties-filter"))
            .desired_width(f32::INFINITY),
    );
    ui.add_space(2.0);
    let needle = editor.properties_filter.trim().to_lowercase();
    let mut groups: Vec<(&'static str, Vec<Property>)> = Vec::new();
    for &property in class.properties() {
        if !needle.is_empty() && !names::property_name(property).to_lowercase().contains(&needle) {
            continue;
        }
        let category = property_category(class, property);
        match groups.iter_mut().find(|(c, _)| *c == category) {
            Some((_, list)) => list.push(property),
            None => groups.push((category, vec![property])),
        }
    }
    groups.sort_by_key(|(c, _)| CATEGORY_ORDER.iter().position(|o| o == c).unwrap_or(CATEGORY_ORDER.len()));
    let mut edited: Option<(Property, PropertyValue)> = None;
    for (category, list) in groups {
        egui::CollapsingHeader::new(RichText::new(category).strong())
            .id_salt(("prop-category", category))
            .default_open(true)
            .show(ui, |ui| {
                egui::Grid::new(("props", category)).num_columns(2).striped(true).spacing([8.0, 6.0]).show(
                    ui,
                    |ui| {
                        for property in list {
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
                            let locked = (property == Property::Name && !class.is_user_creatable())
                                || property == Property::Character;
                            ui.add_enabled_ui(!locked, |ui| {
                                if let Some(new) = property_editor(ui, property, value, id, actions) {
                                    edited = Some((property, new));
                                }
                            });
                            ui.end_row();
                        }
                    },
                );
            });
    }
    if let Some((property, value)) = edited {
        if !playing {
            editor.history.begin_edit(tree, now);
            editor.dirty = true;
        }
        let _ = tree.set(id, property, value);
    }
    attributes_and_tags(ui, editor, tree, id, now, playing, actions);

    let hint = class_hint(class);
    if let Some(hint) = hint {
        ui.add_space(8.0);
        ui.label(t(hint));
    }
}

/// The kinds of value a new attribute can start as, with their first value.
/// Studio's property categories, in the order it shows them.
const CATEGORY_ORDER: &[&str] =
    &["Appearance", "Data", "Text", "Image", "Behavior", "Collision", "Part", "Transform", "Pivot", "Other"];

/// Which of Studio's categories a property is listed under.
fn property_category(class: ClassKind, property: Property) -> &'static str {
    let name = names::property_name(property);
    let gui = class.is_a(ClassKind::GuiObject);
    match name {
        "Name" | "Source" | "Value" | "ClassName" | "Parent" | "Archivable" => "Data",
        "Color"
        | "BrickColor"
        | "Material"
        | "Transparency"
        | "Reflectance"
        | "CastShadow"
        | "BackgroundColor3"
        | "BackgroundTransparency"
        | "BorderColor3"
        | "BorderSizePixel"
        | "Visible"
        | "ZIndex"
        | "Brightness"
        | "Range"
        | "Shadows"
        | "Texture"
        | "Face"
        | "LightEmission"
        | "LightInfluence"
        | "TextureID"
        | "TextureId" => "Appearance",
        "CanCollide" | "CanTouch" | "CanQuery" | "CollisionGroup" => "Collision",
        "Anchored" | "Massless" | "Shape" | "CustomPhysicalProperties" | "RootPriority" => "Part",
        "Position" | "Orientation" | "Rotation" | "AnchorPoint" | "CFrame" => "Transform",
        "Size" if gui || class.is_physical() => "Transform",
        "PivotOffset" | "WorldPivot" => "Pivot",
        "Disabled" | "Enabled" | "Locked" | "Active" | "Selectable" | "Interactable" | "AutoButtonColor"
        | "Modal" | "ResetOnSpawn" | "RunContext" => "Behavior",
        n if n.starts_with("Text") || n.starts_with("Font") || n == "RichText" || n == "LineHeight" => "Text",
        n if n.starts_with("Image") || n == "ScaleType" || n == "SliceCenter" || n == "TileSize" => "Image",
        n if n.ends_with("Color") || n.ends_with("Color3") || n.ends_with("Transparency") => "Appearance",
        _ => "Data",
    }
}

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
/// What the Properties panel says about a class, in English (shown through
/// the language files).
pub(crate) fn class_hint(class: ClassKind) -> Option<&'static str> {
    match class {
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
        ClassKind::SurfaceGui => Some(
            "Put this inside a part, with frames, labels and buttons inside it. They are drawn on the part's Face \
             like a screen or a sign, PixelsPerStud pixels to a stud, and buttons on it can be clicked.",
        ),
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
        ClassKind::RodConstraint => Some(
            "Keeps Attachment0 and Attachment1 exactly Length studs apart, like a stiff bar between them.",
        ),
        ClassKind::AlignOrientation => Some(
            "While playing, turns Attachment0's part so the attachment faces the same way as Attachment1, \
             or as its CFrame when Attachment1 is empty. Responsiveness is how quickly, MaxTorque how hard.",
        ),
        ClassKind::BodyGyro => Some(
            "Put this inside a loose part to turn it to face the way its CFrame does. P is how hard, D how \
             quickly the wobbling stops, MaxTorque the most it pushes about each axis.",
        ),
        ClassKind::Torque => Some(
            "While playing, keeps turning Attachment0's part with a steady Torque about the attachment's own \
             axes. Unanchor the part first.",
        ),
        ClassKind::NoCollisionConstraint => {
            Some("Lets Part0 and Part1 pass through each other while still hitting everything else.")
        }
        ClassKind::SpringConstraint => Some(
            "Pulls Attachment0 and Attachment1 towards FreeLength studs apart. Stiffness is how hard, \
             Damping how quickly the bouncing stops.",
        ),
        ClassKind::HingeConstraint => Some(
            "Joins Attachment1's part to Attachment0's so it can only turn about Attachment0's X axis (turn the attachment to aim it). ActuatorType None swings freely, Motor turns at AngularVelocity radians a second, Servo turns to TargetAngle degrees at AngularSpeed.",
        ),
        ClassKind::PrismaticConstraint => Some(
            "Joins Attachment1's part to Attachment0's so it can only slide along Attachment0's X \
             axis, without turning. With LimitsEnabled it stops at LowerLimit and UpperLimit studs. \
             Add a SpringConstraint beside it for suspension.",
        ),
        ClassKind::BallSocketConstraint => Some(
            "Holds Attachment1 on Attachment0 while its part turns freely any way, like a shoulder \
             or a chain link.",
        ),
        ClassKind::AngularVelocity => Some(
            "While playing, spins Attachment0's part at AngularVelocity (radians a second about X, Y and Z), with up to MaxTorque. Unanchor the part first.",
        ),
        ClassKind::Terrain => Some(
            "Ground made of one-stud cubes. Pick the Terrain tool (5) to add, dig or paint it by dragging in the view, or to make a hill. Scripts use workspace.Terrain:FillBlock, FillBall and FillCylinder (Enum.Material.Air digs) and Clear.",
        ),
        ClassKind::Decal | ClassKind::Texture => Some(
            "Put one inside a part to show a PNG picture on one Face of it. A Decal stretches the \
             picture over the whole side; a Texture repeats it every StudsPerTileU by StudsPerTileV \
             studs. Texture is a file name, found like an ImageLabel's; libroxel://smile, star, arrow, \
             checker, coin and chess pieces (chess-wk to chess-bp) are built in. Color3 tints it.",
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
            "The older spinner: put one inside a loose part to spin it at AngularVelocity, with up to MaxTorque about each axis.",
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
        ClassKind::UIGradient => Some(
            "Shades this GUI object's background from Color0 to Color1 (and Transparency0 to Transparency1), \
             turned Rotation degrees. Scripts set Color to a ColorSequence and Transparency to a NumberSequence.",
        ),
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
        ClassKind::Weld => Some(
            "Holds Part1 to Part0 while playing: Part1 sits at Part0's CFrame * C0 * C1:Inverse(). \
             Scripts change C0 or C1 to move Part1 around Part0, like a door on a hinge.",
        ),
        ClassKind::Motor6D => Some(
            "A weld that turns: Part1 sits at Part0 * C0 * (a turn of CurrentAngle about Z) * C1:Inverse(). \
             Set DesiredAngle (in radians) and it turns there by MaxVelocity each 1/60 of a second.",
        ),
        ClassKind::Configuration => Some("A folder for a model's settings: put value objects inside it."),
        ClassKind::Atmosphere => Some(
            "Put this in Lighting: haze that thickens with distance. Density is how thick (0 to 1), Haze adds more \
             towards the horizon, and Color tints it. It replaces Lighting's fog.",
        ),
        ClassKind::BloomEffect => Some(
            "Put this in Lighting (or the camera): bright things such as neon parts glow. Intensity is how strongly, \
             Size how far it spreads and Threshold how bright something must be.",
        ),
        ClassKind::ColorCorrectionEffect => Some(
            "Put this in Lighting (or the camera) to grade the picture: Brightness, Contrast and Saturation \
             (each 0 for no change; Saturation -1 is black and white), and TintColor to warm or cool it.",
        ),
        ClassKind::SelectionBox => Some(
            "Draws an outline round its Adornee (a part or model), or what it's inside if Adornee is empty.",
        ),
        ClassKind::Trail => Some(
            "Leaves a fading ribbon behind it as Attachment0 and Attachment1 move: put both in something that moves, \
             a little apart. Lifetime is how long the ribbon lasts.",
        ),
        ClassKind::Sky => Some(
            "Put this in Lighting to draw the sun and moon (SunAngularSize, MoonAngularSize) and stars at night \
             (StarCount). They follow Lighting's ClockTime.",
        ),
        ClassKind::Beam => Some(
            "A flat strip between Attachment0 and Attachment1 that always faces the camera: lasers, light shafts, \
             ropes of light. Width0 and Width1 set how wide, LightEmission makes it glow.",
        ),
        ClassKind::UISizeConstraint => {
            Some("Put this in a GUI object to keep its size between MinSize and MaxSize (in pixels).")
        }
        ClassKind::UITextSizeConstraint => Some(
            "Put this in a text object to keep its text size between MinTextSize and MaxTextSize, even with TextScaled.",
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
    }
}

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
    ui.label(RichText::new(t("Attributes")).strong());
    let attributes = tree.attributes(id).cloned().unwrap_or_default();
    egui::Grid::new("attributes").num_columns(3).striped(true).spacing([8.0, 6.0]).show(ui, |ui| {
        for (name, value) in attributes {
            ui.label(&name);
            ui.push_id(&name, |ui| {
                if let Some(new) = property_editor(ui, Property::Name, value, id, actions) {
                    edits.push(Edit::Set(name.clone(), Some(new)));
                }
            });
            if ui.small_button(icon::X).on_hover_text(t("Remove this attribute")).clicked() {
                edits.push(Edit::Set(name, None));
            }
            ui.end_row();
        }
    });
    ui.horizontal(|ui| {
        let (name, kind) = &mut editor.new_attribute;
        ui.add(egui::TextEdit::singleline(name).hint_text(t("New attribute")).desired_width(110.0));
        let kinds = attribute_kinds();
        egui::ComboBox::from_id_salt("attribute-kind").selected_text(kinds[*kind].0).show_ui(ui, |ui| {
            for (i, (label, _)) in kinds.iter().enumerate() {
                ui.selectable_value(kind, i, *label);
            }
        });
        let valid = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if ui.add_enabled(valid, egui::Button::new(t("Add"))).clicked() {
            edits.push(Edit::Set(std::mem::take(name), Some(kinds[*kind].1.clone())));
        }
    });

    ui.add_space(8.0);
    ui.label(RichText::new(t("Tags")).strong());
    let tags = tree.tags(id).map(|t| t.to_vec()).unwrap_or_default();
    ui.horizontal_wrapped(|ui| {
        for tag in tags {
            if ui.button(format!("{tag}  {}", icon::X)).on_hover_text(t("Remove this tag")).clicked() {
                edits.push(Edit::RemoveTag(tag));
            }
        }
    });
    ui.horizontal(|ui| {
        let response = ui.add(
            egui::TextEdit::singleline(&mut editor.new_tag).hint_text(t("New tag")).desired_width(150.0),
        );
        let entered = response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        let ready = !editor.new_tag.trim().is_empty();
        if (ui.add_enabled(ready, egui::Button::new(t("Add"))).clicked() || entered) && ready {
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
                if ui.button(format!("{}  Edit ({lines} lines)", icon::PENCIL_SIMPLE)).clicked() {
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
fn leaderboard(ui: &mut egui::Ui, tree: &InstanceTree, platform: &mut crate::browse::Platform, game: &str) {
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
                                        t("Yes").into()
                                    } else {
                                        t("No").into()
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
            egui::Frame::popup(ui.style()).fill(theme::see_through(248, 252, 255, 230)).show(ui, |ui| {
                egui::Grid::new("leaderboard-grid").spacing([16.0, 4.0]).show(ui, |ui| {
                    ui.label(RichText::new(t("Player")).strong().color(theme::HEADING));
                    for c in &columns {
                        ui.label(RichText::new(c).strong().color(theme::HEADING));
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
                                None => (t("Neutral").to_owned(), theme::MUTED),
                            };
                            ui.label(RichText::new(label).strong().color(color));
                            ui.end_row();
                        }
                        let address = platform.hub_address.clone();
                        let badge = platform.badges.of(|| libroxel_hub::HubClient::new(&address), name);
                        ui.horizontal(|ui| {
                            if let Some(badge) = badge {
                                crate::groups::badge(ui, &badge);
                            }
                            ui.label(RichText::new(name).color(theme::TEXT)).context_menu(|ui| {
                                if ui.button(format!("{} {}", theme::icon::FLAG, t("Report"))).clicked() {
                                    platform.report_player(name, format!("In {game}"));
                                    ui.close();
                                }
                            });
                        });
                        for c in &columns {
                            let v =
                                stats.iter().find(|(n, _)| n == c).map(|(_, v)| v.as_str()).unwrap_or("-");
                            ui.label(RichText::new(v).color(theme::AMBER));
                        }
                        ui.end_row();
                    }
                });
                if platform.reporting_player() {
                    platform.report_row(ui);
                }
            });
        });
}

/// What the Output panel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputView {
    /// Every line, filtered.
    Log,
    /// Errors and warnings grouped by what they say.
    Problems,
    /// API members and classes scripts asked for that Libroxel lacks.
    Missing,
}

/// The game's name, for reports.
fn game_name(editor: &Editor) -> String {
    editor
        .file
        .as_ref()
        .or(editor.imported_from.as_ref())
        .and_then(|p| p.file_name())
        .map_or_else(|| "an unsaved game".to_owned(), |n| n.to_string_lossy().into_owned())
}

/// Opens a folder in the computer's file manager.
fn open_folder(path: &std::path::Path) {
    let _ = std::fs::create_dir_all(path);
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let program = "xdg-open";
    let _ = std::process::Command::new(program).arg(path).spawn();
}

/// The first script called `name`, for jumping to where an error happened.
fn script_named(tree: &InstanceTree, name: &str) -> Option<InstanceId> {
    tree.descendants(tree.root()).ok()?.into_iter().find(|id| {
        tree.name(*id) == Ok(name)
            && matches!(
                tree.class(*id),
                Ok(ClassKind::Script | ClassKind::LocalScript | ClassKind::ModuleScript)
            )
    })
}

/// A small toggle button.
fn toggle(ui: &mut egui::Ui, on: &mut bool, text: String, colour: Color32) {
    let label = RichText::new(text).color(if *on { colour } else { colour.gamma_multiply(0.4) });
    if ui.add(egui::Button::new(label).small().selected(*on)).clicked() {
        *on = !*on;
    }
}

fn output(
    ui: &mut egui::Ui,
    engine: &mut EngineState,
    editor: &mut Editor,
    playing: bool,
    actions: &mut Vec<Action>,
) {
    editor.output_cache.refresh(
        &engine.log,
        &editor.output_search,
        editor.group_repeats,
        [editor.show_prints, editor.show_warnings, editor.show_errors],
    );
    let cache = std::mem::take(&mut editor.output_cache);
    let (problems, missing) = (&cache.problems, &cache.missing);
    let [prints, warnings, errors] = cache.counts;
    ui.horizontal(|ui| {
        theme::panel_name(ui, icon::TERMINAL_WINDOW, "Output");
        let views = [
            (OutputView::Log, t("Log").to_owned()),
            (OutputView::Problems, tf("Problems ({n})", &[("n", &problems.len())])),
            (OutputView::Missing, tf("Errors ({n})", &[("n", &missing.len())])),
        ];
        for (view, label) in views {
            if ui.selectable_label(editor.output_view == view, label).clicked() {
                editor.output_view = view;
            }
        }
        ui.separator();
        if ui.small_button(t("Clear")).clicked() {
            actions.push(Action::ClearOutput);
        }
        // The whole log, or a report of what went wrong, in one click.
        let copied_at = egui::Id::new("output-copied-at");
        let now = ui.input(|i| i.time);
        let copied =
            ui.data(|d| d.get_temp::<(f64, u8)>(copied_at)).filter(|(t, _)| now - t < 1.5).map(|c| c.1);
        let label = |which: u8, text: &str, glyph: &str| {
            if copied == Some(which) {
                format!("{} {}", icon::CHECK, t("Copied"))
            } else {
                format!("{glyph} {text}")
            }
        };
        let copy = ui
            .add_enabled(!engine.log.is_empty(), egui::Button::new(label(0, "Copy all", icon::COPY)).small())
            .on_hover_text(t("Copy everything in the output, to paste into a message or a file"));
        if copy.clicked() {
            ui.ctx().copy_text(output_text(&engine.log));
            ui.data_mut(|d| d.insert_temp(copied_at, (now, 0)));
        }
        let report = ui
            .add_enabled(
                !engine.log.is_empty(),
                egui::Button::new(label(1, "Copy report", icon::BUG)).small(),
            )
            .on_hover_text(t("Copy what's missing and each kind of error once, with where it happened"));
        if report.clicked() {
            let text = crate::debugging::report(&engine.log, &game_name(editor), editor.log_file.path());
            ui.ctx().copy_text(text);
            ui.data_mut(|d| d.insert_temp(copied_at, (now, 1)));
        }
        let last_error = engine.log.iter().rev().find(|e| e.level == LogLevel::Error);
        if ui.add_enabled(last_error.is_some(), egui::Button::new(t("Report")).small()).clicked()
            && let Some(entry) = last_error
        {
            crate::report::open(&entry.text);
        }
        let folder = ui.small_button(format!("{} {}", icon::FOLDER_OPEN, t("Logs"))).on_hover_text(format!(
            "Every Output is also saved to a file here (latest.log is the newest):\n{}",
            crate::debugging::logs_dir().display()
        ));
        if folder.clicked() {
            open_folder(&crate::debugging::logs_dir());
        }
        if copied.is_some() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
        }
    });
    if editor.output_view == OutputView::Log {
        ui.horizontal(|ui| {
            toggle(ui, &mut editor.show_errors, format!("{} {errors}", icon::X_CIRCLE), theme::RED);
            toggle(ui, &mut editor.show_warnings, format!("{} {warnings}", icon::WARNING), theme::AMBER);
            toggle(ui, &mut editor.show_prints, format!("{} {prints}", icon::INFO), theme::TEXT);
            let group = RichText::new(format!("{} {}", icon::STACK, t("Group repeats")));
            if ui
                .add(egui::Button::new(group).small().selected(editor.group_repeats))
                .on_hover_text(t("Show an error or warning that keeps happening once, with how many times"))
                .clicked()
            {
                editor.group_repeats = !editor.group_repeats;
            }
            ui.add(
                egui::TextEdit::singleline(&mut editor.output_search)
                    .hint_text(format!("{} {}", icon::MAGNIFYING_GLASS, t("Filter the output…")))
                    .desired_width(220.0),
            );
            ui.separator();
            let hint = if playing {
                t("Run Luau in the running game…")
            } else {
                t("Press Play to run Luau commands")
            };
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
    }
    ui.separator();
    match editor.output_view {
        OutputView::Log => {
            let row = ui.text_style_height(&egui::TextStyle::Monospace) + ui.spacing().item_spacing.y;
            egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show_rows(
                ui,
                row,
                cache.lines.len(),
                |ui, range| {
                    for &(i, l) in &cache.lines[range] {
                        let Some(entry) = engine.log.get(i) else { continue };
                        let line = entry.text.lines().nth(l).unwrap_or_default();
                        let text = if l > 0 {
                            format!("           {line}")
                        } else {
                            match cache.repeats.get(entry.text.as_str()) {
                                Some(&(n, last)) if n > 1 && entry.level != LogLevel::Info => {
                                    format!("[{:>6.1}s] (×{n}, last at {last:.1}s) {line}", entry.time)
                                }
                                _ => format!("[{:>6.1}s] {line}", entry.time),
                            }
                        };
                        ui.label(RichText::new(text).monospace().color(level_color(entry.level)));
                    }
                },
            );
        }
        OutputView::Problems => {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                if problems.is_empty() {
                    ui.label(t("No errors or warnings."));
                }
                let tree = engine.tree.borrow();
                for (i, p) in problems.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("×{}", p.count)).monospace().strong());
                        let at = match (&p.script, p.line) {
                            (Some(s), Some(l)) => format!("{s}:{l}"),
                            (Some(s), None) => s.clone(),
                            _ => String::new(),
                        };
                        let script = p.script.as_deref().and_then(|s| script_named(&tree, s));
                        if let Some(id) = script {
                            if ui.link(at).on_hover_text(t("Open this script")).clicked() {
                                actions.push(Action::OpenScript(id));
                            }
                        } else if !at.is_empty() {
                            ui.label(RichText::new(at).monospace());
                        }
                        egui::CollapsingHeader::new(
                            RichText::new(&p.message).monospace().color(level_color(p.level)),
                        )
                        .id_salt(("problem", i))
                        .show(ui, |ui| {
                            ui.label(RichText::new(&p.detail).monospace().small());
                        });
                        if p.level == LogLevel::Error {
                            crate::report::button(ui, &p.detail);
                        }
                    });
                }
            });
        }
        OutputView::Missing => {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                if missing.is_empty() {
                    ui.label(t("Nothing missing so far: every member and class scripts asked for exists."));
                } else {
                    ui.label(
                        "Members and classes your scripts asked for that don't exist here: either \
                         Libroxel doesn't have them yet, or the name is misspelled. Copy report puts \
                         this list in your clipboard.",
                    );
                }
                for (m, n) in missing {
                    ui.label(RichText::new(format!("{}.{}   ×{n}", m.on, m.member)).monospace());
                }
            });
        }
    }
    editor.output_cache = cache;
}

/// What the Output panel shows, kept between frames: a big game prints
/// thousands of lines, and going over all of them every frame made the
/// editor crawl. Only lines added since the last frame are looked at.
#[derive(Default)]
pub struct OutputCache {
    /// How many log entries have been looked at.
    seen: usize,
    /// The filters the lines were picked with.
    key: (String, bool, [bool; 3]),
    /// The rows shown: (log entry, line of its text).
    lines: Vec<(usize, usize)>,
    /// Each repeating error or warning: how often, and when last.
    repeats: std::collections::HashMap<String, (usize, f64)>,
    counts: [usize; 3],
    problems: Vec<crate::debugging::Problem>,
    missing: std::collections::BTreeMap<crate::debugging::Missing, usize>,
    /// When the Problems and Errors lists were last worked out, and how long the log was.
    summed: (Option<std::time::Instant>, usize),
}

impl OutputCache {
    fn refresh(&mut self, log: &[LogEntry], search: &str, group: bool, levels: [bool; 3]) {
        let search = search.to_lowercase();
        let key = (search.clone(), group, levels);
        if key != self.key || log.len() < self.seen {
            let summed = self.summed;
            *self = OutputCache { key, summed, ..Default::default() };
        }
        let level_index = |l: LogLevel| match l {
            LogLevel::Error => 2,
            LogLevel::Warn => 1,
            _ => 0,
        };
        for (i, e) in log.iter().enumerate().skip(self.seen) {
            let level = level_index(e.level);
            self.counts[level] += 1;
            if !levels[level] || !(search.is_empty() || e.text.to_lowercase().contains(&search)) {
                continue;
            }
            if group && e.level != LogLevel::Info {
                // The first one stands for all of them.
                if let Some(seen) = self.repeats.get_mut(e.text.as_str()) {
                    seen.0 += 1;
                    seen.1 = e.time;
                    continue;
                }
                self.repeats.insert(e.text.clone(), (1, e.time));
            }
            self.lines.extend((0..e.text.lines().count().max(1)).map(|l| (i, l)));
        }
        self.seen = log.len();
        // The lists of problems go over the whole log: at most twice a second.
        let stale = self.summed.0.is_none_or(|t| t.elapsed().as_secs_f32() > 0.5);
        if self.summed.1 != log.len() && stale {
            self.problems = crate::debugging::problems(log);
            self.missing = crate::debugging::missing(log);
            self.summed = (Some(std::time::Instant::now()), log.len());
        }
    }
}

/// The output log as plain text, one line per entry, as it's shown.
fn output_text<'a>(log: impl IntoIterator<Item = &'a LogEntry>) -> String {
    let mut text = String::new();
    for entry in log {
        text.push_str(&crate::debugging::line_text(entry));
        text.push('\n');
    }
    text
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
    egui::Window::new(format!("{}  {name}", icon::CODE))
        .id(egui::Id::new("script-editor"))
        .open(&mut open)
        .default_size([620.0, 460.0])
        .default_pos([300.0, 90.0])
        .resizable(true)
        .show(ctx, |ui| {
            if playing {
                ui.label(
                    RichText::new(t("Changes made while playing apply next time you press Play.")).italics(),
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
        .filter(|p| crate::open::is_game_file(p))
        .collect();
    found.sort();
    found
}

fn dialogs(ctx: &egui::Context, editor: &mut Editor, actions: &mut Vec<Action>) {
    let Some(dialog) = editor.dialog.as_mut() else { return };
    if let Dialog::Join { address, name } = dialog {
        let mut close = false;
        egui::Window::new(t("Join a game")).collapsible(false).resizable(false).default_width(360.0).show(
            ctx,
            |ui| {
                egui::Grid::new("join").num_columns(2).show(ui, |ui| {
                    ui.label(t("Join code or address"));
                    theme::hidden_field(ui, address, "lrx1… or 192.168.1.20", 220.0);
                    ui.end_row();
                    ui.label(t("Playing as"));
                    ui.label(egui::RichText::new(name.trim()).strong());
                    ui.end_row();
                });
                ui.label(format!(
                    "Paste the join code the host copied to play from anywhere, or type an address on \
                     your network (without a port, {} is used).",
                    libroxel_engine::net::DEFAULT_PORT
                ));
                ui.horizontal(|ui| {
                    if ui.button(t("Join")).clicked() {
                        actions.push(Action::Join {
                            address: address.trim().to_owned(),
                            name: name.trim().to_owned(),
                        });
                        close = true;
                    }
                    if ui.button(t("Cancel")).clicked() {
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
    if let Dialog::Publish { title, description } = dialog {
        let mut close = false;
        let busy = editor.platform.is_publishing();
        egui::Window::new(t("Publish")).collapsible(false).resizable(false).default_width(400.0).show(
            ctx,
            |ui| {
                ui.label(
                    "Puts this game on Libroxel, so anyone finds it Online and can play it, even when \
                     you're away. Publishing again puts out a new version.",
                );
                egui::Grid::new("publish").num_columns(2).show(ui, |ui| {
                    ui.label(t("Title"));
                    ui.add(egui::TextEdit::singleline(title).char_limit(60));
                    ui.end_row();
                    ui.label(t("About it"));
                    ui.add(egui::TextEdit::multiline(description).desired_rows(3).char_limit(2000));
                    ui.end_row();
                    ui.label(t("By"));
                    ui.label(editor.platform.name.trim());
                    ui.end_row();
                });
                match &editor.platform.publish_status {
                    _ if busy => {
                        theme::status(ui, theme::AMBER, "Publishing…");
                    }
                    Some(Ok(said)) => {
                        ui.colored_label(theme::GREEN, said);
                    }
                    Some(Err(why)) => {
                        crate::report::error(ui, why);
                    }
                    None => {}
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !busy && !title.trim().is_empty(),
                            egui::Button::new(format!("{}  {}", icon::GLOBE_HEMISPHERE_WEST, t("Publish"))),
                        )
                        .clicked()
                    {
                        actions.push(Action::Publish {
                            title: title.trim().to_owned(),
                            description: description.trim().to_owned(),
                        });
                    }
                    if ui.button(t("Close")).clicked() {
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
    if let Dialog::EditTogether { address, name } = dialog {
        let mut close = false;
        egui::Window::new(t("Edit together")).collapsible(false).resizable(false).default_width(380.0).show(
            ctx,
            |ui| {
                ui.label(t("Share the game you have open, and others can change it with you as you work."));
                if ui.button(format!("{}  {}", icon::USERS_THREE, t("Share this game"))).clicked() {
                    actions.push(Action::ShareEdits);
                    close = true;
                }
                ui.separator();
                ui.label(t("Or edit a game someone else shares. Its game replaces the one open here."));
                ui.label(t("Ask them for the edit code, or their address then # and the edit password."));
                egui::Grid::new("edit-together").num_columns(2).show(ui, |ui| {
                    ui.label(t("Join code or address"));
                    theme::hidden_field(ui, address, "lrx1… or 192.168.1.20", 220.0);
                    ui.end_row();
                    ui.label(t("Playing as"));
                    ui.label(egui::RichText::new(name.trim()).strong());
                    ui.end_row();
                });
                ui.label(tf(
                    "Without a port, {port} is used.",
                    &[("port", &libroxel_engine::coedit::EDIT_PORT)],
                ));
                ui.horizontal(|ui| {
                    if ui.add_enabled(!address.trim().is_empty(), egui::Button::new(t("Join"))).clicked() {
                        actions.push(Action::JoinEdits {
                            address: address.trim().to_owned(),
                            name: name.trim().to_owned(),
                        });
                        close = true;
                    }
                    if ui.button(t("Cancel")).clicked() {
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
        Dialog::Open { .. } => (t("Open place"), t("Open")),
        Dialog::SaveAs { .. } => (t("Save place as"), t("Save")),
        Dialog::Join { .. } | Dialog::EditTogether { .. } | Dialog::Publish { .. } => return,
    };
    let opening = matches!(dialog, Dialog::Open { .. });
    let mut close = false;
    egui::Window::new(title).collapsible(false).resizable(false).default_width(480.0).show(ctx, |ui| {
        let path = match dialog {
            Dialog::Open { path } | Dialog::SaveAs { path } => path,
            Dialog::Join { .. } | Dialog::EditTogether { .. } | Dialog::Publish { .. } => return,
        };
        ui.label(if opening {
            "File path: a Libroxel game (.lrx), or a place or model from the reference editor (.rbxl, .rbxlx, .rbxm, .rbxmx):"
        } else {
            "File path (places use the .lrx extension):"
        });
        ui.add(egui::TextEdit::singleline(path).desired_width(f32::INFINITY));
        let places = list_places();
        if !places.is_empty() {
            ui.label(tf("In {folder}:", &[("folder", &games_dir().display())]));
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
            if ui.button(t("Cancel")).clicked() {
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
/// How many times a joined game whose connection dropped tries to come
/// back by itself (a second apart, then 2, 4, 8 and 16), before it waits
/// for the Reconnect button.
const REJOIN_TRIES: u32 = 5;

/// Brings a joined game whose connection dropped (Wi-Fi that blinked, a
/// computer that slept) back by itself when the host answers again.
fn rejoin_tick(engine: &mut EngineState, editor: &mut Editor) {
    let Some(session) = engine.session.as_ref() else {
        editor.rejoin = Default::default();
        return;
    };
    if !session.can_reconnect() {
        if session.character().is_some() {
            // Back in and playing: the next drop starts counting again.
            editor.rejoin.tries = 0;
        }
        editor.rejoin.next = None;
        editor.rejoin.probe = None;
        return;
    }
    let rejoin = &mut editor.rejoin;
    if rejoin.tries >= REJOIN_TRIES {
        return;
    }
    let now = std::time::Instant::now();
    if let Some(probe) = &rejoin.probe {
        match probe.try_recv() {
            Ok(true) => {
                rejoin.probe = None;
                rejoin.tries += 1;
                rejoin_now(engine);
            }
            Ok(false) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                rejoin.probe = None;
                rejoin.tries += 1;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
        return;
    }
    let next = *rejoin.next.get_or_insert(now + std::time::Duration::from_secs(1 << rejoin.tries.min(4)));
    if now < next {
        return;
    }
    rejoin.next = None;
    let Some(address) = session.joined_address() else { return };
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        use std::net::ToSocketAddrs;
        let up = address.to_socket_addrs().ok().and_then(|mut a| a.next()).is_some_and(|a| {
            std::net::TcpStream::connect_timeout(&a, std::time::Duration::from_secs(3)).is_ok()
        });
        let _ = tx.send(up);
    });
    rejoin.probe = Some(rx);
}

/// Joins the game whose connection dropped again, as the same player.
fn rejoin_now(engine: &mut EngineState) {
    let Some(session) = engine.session.as_mut() else { return };
    if !session.can_reconnect() {
        return;
    }
    match session.reconnect() {
        Ok(()) => log(engine, LogLevel::Info, "Reconnecting to the game…"),
        Err(e) => log(engine, LogLevel::Warn, format!("Could not reconnect: {e}")),
    }
}

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

/// Sends this frame's edits to the people editing with us and takes in theirs.
fn edit_together(engine: &mut EngineState, editor: &mut Editor) {
    let Some(coedit) = editor.coedit.as_mut() else { return };
    let was_ready = coedit.ready();
    let theirs = coedit.step(&mut engine.tree.borrow_mut());
    let lines = coedit.take_log();
    if !was_ready && coedit.ready() {
        // The shared game arrived in place of ours.
        editor.history.clear();
        editor.selection = None;
        editor.open_script = None;
        editor.file = None;
        editor.dirty = false;
    } else if theirs && !coedit.is_guest() {
        // Someone else changed the game: it needs saving.
        editor.dirty = true;
    }
    if editor.selection.is_some_and(|id| engine.tree.borrow().is_destroyed(id).unwrap_or(true)) {
        editor.selection = None;
    }
    if editor.open_script.is_some_and(|id| engine.tree.borrow().is_destroyed(id).unwrap_or(true)) {
        editor.open_script = None;
    }
    for line in lines {
        log(engine, LogLevel::Info, line);
    }
}

/// The toolbar's word on editing together.
fn together_status(ui: &mut egui::Ui, coedit: &mut libroxel_engine::coedit::CoEdit, tunnel: Option<&Tunnel>) {
    let red = theme::RED;
    // Nobody edits a shared game until the person sharing it lets them in.
    let mut answers = Vec::new();
    for (ticket, name) in coedit.asking() {
        theme::status(ui, theme::AMBER, &tf("{name} asks to edit this game", &[("name", &name)]));
        if ui
            .button(t("Let in"))
            .on_hover_text(t("They will see and can change everything, scripts too"))
            .clicked()
        {
            answers.push((ticket, true));
        }
        if ui.button(t("Turn away")).clicked() {
            answers.push((ticket, false));
        }
    }
    for (ticket, allow) in answers {
        coedit.answer(ticket, allow);
    }
    let password = coedit.password().unwrap_or_default().to_owned();
    if let Some(address) = coedit.local_addr() {
        let people = coedit.people();
        let who = if people.is_empty() { t("no one yet").to_owned() } else { people.join(", ") };
        theme::status(
            ui,
            theme::GREEN,
            &tf("Sharing on port {port} · {who}", &[("port", &address.port()), ("who", &who)]),
        )
        .on_hover_text(t("Others on your network join with this computer's address and this port, then # and the edit password"));
        if ui
            .button(format!("{}  {}", icon::COPY, t("Copy edit password")))
            .on_hover_text(t(
                "For joining by address on your network. It isn't shown here, so it stays off streams.",
            ))
            .clicked()
        {
            ui.ctx().copy_text(password.clone());
        }
        if let Some(Tunnel::Host(tunnel)) = tunnel {
            if let Some(code) = tunnel.code() {
                let code = format!("{code}{}{password}", libroxel_engine::coedit::PASSWORD_MARK);
                let copy = ui
                    .button(format!("{}  {}", icon::COPY, t("Copy edit code")))
                    .on_hover_text(t("Friends anywhere paste it into Edit together. It isn't shown here, so it stays off streams."));
                if copy.clicked() {
                    ui.ctx().copy_text(code);
                }
            } else if let Some(error) = tunnel.error() {
                ui.colored_label(red, t("No edit code")).on_hover_text(&error);
                crate::report::button(ui, &format!("No edit code: {error}"));
            } else {
                theme::status(ui, theme::AMBER, "Getting an edit code…");
            }
        }
    } else if let Some(error) = match tunnel {
        Some(Tunnel::Join(tunnel)) => tunnel.error(),
        _ => None,
    } {
        theme::status(ui, theme::RED, "Could not reach the shared game").on_hover_text(&error);
        crate::report::button(ui, &format!("Could not reach the shared game: {error}"));
    } else if coedit.disconnected() {
        theme::status(ui, theme::RED, "Disconnected");
    } else if coedit.waiting() {
        theme::status(ui, theme::AMBER, "Waiting to be let in…");
    } else if !coedit.ready() {
        theme::status(ui, theme::AMBER, "Getting the shared game…");
    } else {
        theme::status(ui, theme::GREEN, "Editing together");
    }
}

fn replace_world(engine: &mut EngineState, editor: &mut Editor, tree: InstanceTree, file: Option<PathBuf>) {
    if end_session(engine, editor) {
        editor.edit_snapshot = None;
    }
    // A guest opening another game leaves the shared one; the sharing
    // computer opening one shares that instead.
    if editor.coedit.as_ref().is_some_and(|c| c.is_guest()) {
        editor.coedit = None;
        editor.edit_tunnel = None;
        log(engine, LogLevel::Info, "Stopped editing together");
    }
    *engine.tree.borrow_mut() = tree;
    editor.history.clear();
    editor.selection = None;
    editor.open_script = None;
    editor.file = file;
    editor.imported_from = None;
    editor.dirty = false;
}

/// Every so often, a copy of unsaved work goes beside the app's settings,
/// so a crash doesn't lose it (see `autosave`).
fn keep_unsaved_copy(engine: &mut EngineState, editor: &mut Editor, now: f64) {
    // Someone else's shared game is theirs to save.
    let guest = editor.coedit.as_ref().is_some_and(|c| c.is_guest());
    let held = editor.recovered.is_some() || editor.crash.is_some();
    let dir = crate::autosave::dir();
    let failed = {
        let tree = engine.tree.borrow();
        let source = editor.edit_snapshot.as_ref().unwrap_or(&tree);
        let file = editor.file.clone();
        editor.autosave.tick(&dir, now, editor.dirty && !guest, held, |dir| {
            crate::autosave::write(dir, source, file.as_deref())
        })
    };
    if let Some(error) = failed {
        log(engine, LogLevel::Warn, error);
    }
}

/// Offers back the unsaved work a crash or a closed window left behind.
fn recovery_screen(ctx: &egui::Context, engine: &mut EngineState, editor: &mut Editor) {
    let Some(found) = editor.recovered.as_ref() else { return };
    // The crash screen comes first.
    if editor.crash.is_some() {
        return;
    }
    let mut open = false;
    let mut discard = false;
    egui::Modal::new(egui::Id::new("recovered")).show(ctx, |ui| {
        ui.set_max_width(460.0);
        ui.heading(t("Unsaved work"));
        ui.add_space(6.0);
        ui.label(tf(
            "Libroxel closed before {game} was saved. A copy from {when} was kept.",
            &[("game", &found.name()), ("when", &found.age(crate::autosave::now()))],
        ));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            open = ui.button(t("Open the copy")).clicked();
            discard = ui.button(t("Throw it away")).clicked();
        });
    });
    if open {
        let found = editor.recovered.take().expect("shown above");
        match place::load(&found.text) {
            Ok((tree, _)) => {
                replace_world(engine, editor, tree, found.file.clone());
                // It still needs saving, and the copy stays until it is.
                editor.dirty = true;
                editor.autosave.adopt();
                log(
                    engine,
                    LogLevel::Info,
                    format!("Opened the copy of {}; save it to keep it", found.name()),
                );
            }
            Err(e) => {
                log(engine, LogLevel::Error, format!("Could not open the copy of {}: {e}", found.name()))
            }
        }
    } else if discard {
        editor.recovered = None;
        crate::autosave::clear(&crate::autosave::dir());
    }
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
        && tree.class(sel).is_ok_and(|c| {
            matches!(c, ClassKind::ScreenGui | ClassKind::SurfaceGui | ClassKind::BillboardGui)
                || c.is_gui_object()
        })
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
        Property::Part0 | Property::Part1 | Property::PrimaryPart => {
            tree.descendants_of_class(ws, ClassKind::Part)
        }
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
            if ui.selectable_label(current.is_none(), t("(none)")).clicked() {
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
    if class == ClassKind::SurfaceGui {
        let label = tree.create(ClassKind::TextLabel, "Label").ok()?;
        let _ = tree.set(
            label,
            Property::GuiSize,
            PropertyValue::UDim2(libroxel_instance::UDim2::new(1.0, 0.0, 1.0, 0.0)),
        );
        let _ = tree.set(label, Property::TextScaled, PropertyValue::Bool(true));
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
                // A file from the reference editor opens as a new, unsaved game.
                if editor.file.as_ref() != Some(path) && editor.imported_from.as_ref() != Some(path) {
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
    editor.platform.picked(&pick);
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
            match crate::open::open(&path) {
                Ok(opened) => {
                    // A file from another engine is saved as a new .lrx, never over the original.
                    let file = (!opened.imported).then(|| path.clone());
                    replace_world(engine, editor, opened.tree, file);
                    editor.imported_from = opened.imported.then(|| path.clone());
                    log(engine, LogLevel::Info, format!("Opened {}", path.display()));
                    let level = if opened.imported { LogLevel::Info } else { LogLevel::Warn };
                    for note in opened.notes {
                        log(engine, level, note);
                    }
                    if opened.imported {
                        log(
                            engine,
                            LogLevel::Info,
                            "Save it to keep it as a Libroxel game (.lrx).".to_string(),
                        );
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
            // You play as your account's name, in play tests and games you host.
            let me = editor.platform.account.as_ref().map(|a| a.name.clone());
            libroxel_engine::session::set_local_player_name(me.as_deref());
            let (started, what) = match &action {
                Action::Host => (
                    PlaySession::host(
                        engine.tree.clone(),
                        libroxel_engine::net::DEFAULT_PORT,
                        saved.as_deref(),
                        true,
                    ),
                    "Hosting".to_owned(),
                ),
                Action::Join { address, name } if libroxel_net::is_join_code(address) => {
                    let name =
                        me.as_deref().unwrap_or(if name.is_empty() { "Player" } else { name.as_str() });
                    let tunnel = crate::identity().and_then(|me| {
                        libroxel_net::JoinTunnel::start(&me, libroxel_net::Network::Internet, address)
                    });
                    match tunnel {
                        Ok(tunnel) => {
                            let local = tunnel.local_addr().to_string();
                            editor.tunnel = Some(Tunnel::Join(tunnel));
                            (
                                PlaySession::join(engine.tree.clone(), &local, name),
                                "Joining with a code".to_owned(),
                            )
                        }
                        Err(e) => (Err(e), String::new()),
                    }
                }
                Action::Join { address, name } => {
                    let name =
                        me.as_deref().unwrap_or(if name.is_empty() { "Player" } else { name.as_str() });
                    // The session says where it is joining.
                    (PlaySession::join(engine.tree.clone(), address, name), String::new())
                }
                _ => (
                    match saved {
                        Some(json) => PlaySession::start_with_data(engine.tree.clone(), &json),
                        None => PlaySession::start(engine.tree.clone(), true),
                    },
                    "Playing".to_owned(),
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
                                // Players who come through it are known by their keys.
                                session.set_guest_lookup(Box::new(tunnel.guest_lookup()));
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
                    if editor.player_mode || session.is_joined() {
                        editor.loading.start(now);
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
        Action::Reconnect => {
            editor.rejoin = Default::default();
            rejoin_now(engine);
        }
        Action::Stop => {
            if end_session(engine, editor) {
                if let Some(snapshot) = editor.edit_snapshot.take() {
                    *engine.tree.borrow_mut() = snapshot;
                }
                if editor.selection.is_some_and(|id| !engine.tree.borrow().exists(id)) {
                    editor.selection = None;
                }
                log(engine, LogLevel::Info, "Stopped");
            }
        }
        Action::ShareEdits => {
            match libroxel_engine::coedit::CoEdit::host(libroxel_engine::coedit::EDIT_PORT) {
                Ok(coedit) => {
                    let port = coedit.local_addr().map_or(0, |a| a.port());
                    log(engine, LogLevel::Info, format!("Sharing this game for editing on port {port}"));
                    // A key made just for this share, so the edit code says
                    // nothing about who is sharing and can't be worked out
                    // from their player id.
                    let me = libroxel_net::Identity::generate();
                    match libroxel_net::HostTunnel::start(&me, libroxel_net::Network::Internet, port) {
                        Ok(tunnel) => editor.edit_tunnel = Some(Tunnel::Host(tunnel)),
                        Err(e) => log(engine, LogLevel::Warn, format!("Only your network can join: {e}")),
                    }
                    editor.coedit = Some(coedit);
                }
                Err(e) => log(engine, LogLevel::Error, e),
            }
        }
        Action::JoinEdits { address, name } => {
            let name = if name.is_empty() { "Someone" } else { name.as_str() };
            let (target, password) = libroxel_engine::coedit::split_password(&address);
            let password = password.unwrap_or_default();
            let joined = if libroxel_net::is_join_code(target) {
                crate::identity()
                    .and_then(|me| {
                        libroxel_net::JoinTunnel::start(&me, libroxel_net::Network::Internet, target)
                    })
                    .and_then(|tunnel| {
                        let local = format!(
                            "{}{}{password}",
                            tunnel.local_addr(),
                            libroxel_engine::coedit::PASSWORD_MARK
                        );
                        let coedit = libroxel_engine::coedit::CoEdit::join(&local, name)?;
                        Ok((coedit, Some(Tunnel::Join(tunnel))))
                    })
            } else {
                libroxel_engine::coedit::CoEdit::join(&address, name).map(|c| (c, None))
            };
            match joined {
                Ok((mut coedit, tunnel)) => {
                    for line in coedit.take_log() {
                        log(engine, LogLevel::Info, line);
                    }
                    editor.coedit = Some(coedit);
                    editor.edit_tunnel = tunnel;
                }
                Err(e) => log(engine, LogLevel::Error, e),
            }
        }
        Action::Kick(player) => {
            if let Some(session) = engine.session.as_mut() {
                session.kick(player, "The host removed you from the game");
            }
        }
        Action::ShareHat(id) => {
            let made = libroxel_engine::character::CustomHat::from_parts(&engine.tree.borrow(), id);
            match made {
                Ok(hat) => {
                    log(engine, LogLevel::Info, format!("Sharing {} ({} parts)…", hat.name, hat.parts.len()));
                    editor.platform.share_hat(&hat);
                }
                Err(why) => log(engine, LogLevel::Error, format!("That can't be a hat: {why}")),
            }
        }
        Action::Publish { title, description } => {
            if playing {
                return;
            }
            match place::save(&engine.tree.borrow()) {
                Ok(text) => {
                    // Publishing the same file again puts out a new version of it.
                    let key = editor
                        .file
                        .as_ref()
                        .map_or_else(|| format!("untitled:{title}"), |f| f.display().to_string());
                    editor.platform.publish(key, title, description, text);
                }
                Err(e) => editor.platform.publish_status = Some(Err(format!("Could not save the game: {e}"))),
            }
        }
        Action::StopEdits => {
            if editor.coedit.take().is_some() {
                editor.edit_tunnel = None;
                log(engine, LogLevel::Info, "Stopped editing together");
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
                | ClassKind::UIScale
                | ClassKind::UIGradient => editor
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
                    | ClassKind::SurfaceGui
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
                    | ClassKind::PrismaticConstraint
                    | ClassKind::BallSocketConstraint
                    | ClassKind::AngularVelocity
                    | ClassKind::BodyAngularVelocity
                    | ClassKind::AlignOrientation
                    | ClassKind::BodyGyro
                    | ClassKind::Torque
                    | ClassKind::RodConstraint
                    | ClassKind::NoCollisionConstraint
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
                if matches!(class, ClassKind::WeldConstraint | ClassKind::Weld | ClassKind::Motor6D)
                    && let Some(part) = near
                {
                    // Weld the selected part to whatever it touches.
                    let _ = tree.set(id, Property::Part0, PropertyValue::Ref(Some(part)));
                    let other = touching_part(&tree, part);
                    let _ = tree.set(id, Property::Part1, PropertyValue::Ref(other));
                    libroxel_engine::weld::keep_in_place(&mut tree, id);
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
        Action::ImportMesh(path) => {
            if playing {
                log(engine, LogLevel::Warn, "Stop the game to add a model".to_string());
                return;
            }
            let tree_rc = engine.tree.clone();
            let mut tree = tree_rc.borrow_mut();
            editor.history.begin_edit(&tree, now);
            let at = editor
                .selection
                .and_then(|s| {
                    Some((tree.get_vec3(s, Property::Position)?, tree.get_vec3(s, Property::Size)?))
                })
                .map(|(p, s)| p + libroxel_instance::Vec3::new(0.0, s.y / 2.0, 0.0))
                .unwrap_or_default();
            let done = crate::mesh_import::import(&mut tree, &path, &crate::mesh_import::meshes_dir(), at);
            drop(tree);
            match done {
                Ok(id) => {
                    editor.dirty = true;
                    editor.selection = Some(id);
                    editor.focus_selection = true;
                    log(engine, LogLevel::Info, format!("Added the model {}", path.display()));
                }
                Err(e) => log(engine, LogLevel::Error, e),
            }
        }
        Action::OpenMarket => editor.market.open = !editor.market.open,
        Action::InsertMarketModel(name, text) => {
            if playing {
                log(engine, LogLevel::Warn, "Stop the game to add a model".to_string());
                return;
            }
            let tree_rc = engine.tree.clone();
            let mut tree = tree_rc.borrow_mut();
            editor.history.begin_edit(&tree, now);
            let ws = tree.workspace();
            let done = place::insert_model(&mut tree, &text, ws);
            drop(tree);
            match done {
                Ok((made, _)) => {
                    editor.dirty = true;
                    editor.selection = made.first().copied();
                    editor.focus_selection = !made.is_empty();
                    log(engine, LogLevel::Info, format!("Added {name}"));
                }
                Err(e) => log(engine, LogLevel::Error, format!("Could not add {name}: {e}")),
            }
        }
        Action::RunPlugin(plugin) => {
            if playing {
                log(engine, LogLevel::Warn, "Stop the game to run a plugin".to_string());
                return;
            }
            let tree_rc = engine.tree.clone();
            let ran = crate::plugins::run(&tree_rc.borrow(), editor.selection.as_slice(), &plugin);
            match ran {
                Ok((after, picked, printed)) => {
                    let mut tree = tree_rc.borrow_mut();
                    editor.history.begin_edit(&tree, now);
                    *tree = after;
                    drop(tree);
                    editor.dirty = true;
                    editor.selection = picked.first().copied();
                    for line in printed {
                        log(engine, LogLevel::Info, line);
                    }
                    log(engine, LogLevel::Info, format!("Ran {}", plugin.name));
                }
                Err(e) => log(engine, LogLevel::Error, e),
            }
        }
        Action::PublishAsset(form) => {
            if playing {
                return;
            }
            let tree = engine.tree.borrow();
            let data = match form.kind {
                libroxel_hub::AssetKind::Model => {
                    if tree.class(form.target).is_ok_and(|c| c.is_service()) {
                        Err("Pick a model or part, not a service".to_owned())
                    } else {
                        place::save_model(&tree, form.target).map_err(|e| e.to_string())
                    }
                }
                libroxel_hub::AssetKind::Plugin => match tree.class(form.target) {
                    Ok(ClassKind::Script | ClassKind::LocalScript | ClassKind::ModuleScript) => {
                        Ok(tree.get_string(form.target, Property::Source).unwrap_or_default())
                    }
                    _ => Err("Pick the script with the plugin's code".to_owned()),
                },
            };
            drop(tree);
            match data {
                Ok(data) => editor.market.publish(editor.platform.client(), &form, data),
                Err(e) => editor.market.fail(e),
            }
        }
        Action::InsertModelFile(path) => {
            if playing {
                log(engine, LogLevel::Warn, "Stop the game to add a model".to_string());
                return;
            }
            let tree_rc = engine.tree.clone();
            let mut tree = tree_rc.borrow_mut();
            editor.history.begin_edit(&tree, now);
            let ws = tree.workspace();
            let done = crate::open::insert_model(&mut tree, &path, ws);
            drop(tree);
            match done {
                Ok((made, notes)) => {
                    editor.dirty = true;
                    editor.selection = made.first().copied();
                    editor.focus_selection = !made.is_empty();
                    log(engine, LogLevel::Info, format!("Added {}", path.display()));
                    for note in notes {
                        log(engine, LogLevel::Info, note);
                    }
                }
                Err(e) => log(engine, LogLevel::Error, format!("Could not add {}: {e}", path.display())),
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
        Action::Group(id) => {
            if playing {
                return;
            }
            let tree_rc = engine.tree.clone();
            let mut tree = tree_rc.borrow_mut();
            let Some(parent) = tree.parent(id).ok().flatten() else { return };
            if tree.class(id).is_ok_and(|c| c.is_service()) {
                return;
            }
            editor.history.begin_edit(&tree, now);
            editor.dirty = true;
            if let Ok(model) = tree.create(ClassKind::Model, "Model") {
                let _ = tree.set_parent(model, Some(parent));
                let _ = tree.set_parent(id, Some(model));
                editor.selection = Some(model);
                editor.reveal_selection = true;
            }
        }
        Action::Ungroup(id) => {
            let tree_rc = engine.tree.clone();
            let mut tree = tree_rc.borrow_mut();
            if playing || !tree.class(id).is_ok_and(|c| matches!(c, ClassKind::Model | ClassKind::Folder)) {
                return;
            }
            let Some(parent) = tree.parent(id).ok().flatten() else { return };
            editor.history.begin_edit(&tree, now);
            editor.dirty = true;
            let children = tree.children(id).map(<[InstanceId]>::to_vec).unwrap_or_default();
            for child in &children {
                let _ = tree.set_parent(*child, Some(parent));
            }
            let _ = tree.destroy(id);
            editor.selection = children.first().copied();
            editor.reveal_selection = true;
        }
        Action::ToggleAnchor(id) => {
            let tree_rc = engine.tree.clone();
            let mut tree = tree_rc.borrow_mut();
            let Some(anchored) = tree.get_bool(id, Property::Anchored) else { return };
            if !playing {
                editor.history.begin_edit(&tree, now);
                editor.dirty = true;
            }
            let _ = tree.set(id, Property::Anchored, PropertyValue::Bool(!anchored));
        }
        Action::Select(id) => editor.selection = id,
        Action::GuiHover(id) => {
            if let Some(session) = engine.session.as_mut() {
                session.gui_hover(id);
            }
        }
        Action::Scroll(frame, delta, max) => {
            let mut tree = engine.tree.borrow_mut();
            // Only the ways the frame scrolls (its ScrollingDirection) move;
            // the wheel moves a frame that only scrolls sideways sideways.
            let (sideways, upright) = crate::gui::scroll_axes(&tree, frame);
            let delta = match (sideways, upright) {
                (true, false) => egui::vec2(delta.x + delta.y, 0.0),
                _ => egui::vec2(if sideways { delta.x } else { 0.0 }, if upright { delta.y } else { 0.0 }),
            };
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
        Action::PressButton(id, down) => {
            if let Some(session) = engine.session.as_mut() {
                session.press_button(id, down);
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
        Action::ResetCharacter => {
            if let Some(session) = engine.session.as_mut() {
                session.reset_character();
            }
        }
        Action::TriggerPrompt(id) => {
            if let Some(session) = engine.session.as_mut() {
                session.trigger_prompt(id);
            }
        }
        Action::Mouse(input, down, processed) => {
            if let Some(session) = engine.session.as_mut() {
                session.gui_input(input, down);
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
            if let Some(session) = engine.session.as_mut() {
                session.click_part(id);
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

/// The ribbon's tabs, in the order Studio users know them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RibbonTab {
    Home,
    Model,
    Test,
    View,
}

impl RibbonTab {
    const ALL: [RibbonTab; 4] = [RibbonTab::Home, RibbonTab::Model, RibbonTab::Test, RibbonTab::View];

    fn name(self) -> &'static str {
        match self {
            RibbonTab::Home => "Home",
            RibbonTab::Model => "Model",
            RibbonTab::Test => "Test",
            RibbonTab::View => "View",
        }
    }
}

/// The ribbon along the top of the editor: a File menu and tabs, and the
/// open tab's buttons in named groups under them.
fn ribbon(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    session: Option<&PlaySession>,
    tree: &InstanceTree,
    actions: &mut Vec<Action>,
) {
    let playing = session.is_some();
    ui.add_space(3.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        // A glossy blue File button at the start of the tabs.
        let response = theme::block_button(ui, theme::Kind::Primary, "", "File", 24.0);
        egui::Popup::menu(&response).show(|ui| {
            ui.set_min_width(220.0);
            file_menu(ui, editor, playing, actions);
            ui.separator();
            ui.menu_button(t("Keyboard shortcuts"), help_items);
        });
        ui.add_space(8.0);
        for tab in RibbonTab::ALL {
            if theme::ribbon_tab(ui, tab.name(), editor.ribbon == tab).clicked() {
                editor.ribbon = tab;
            }
        }
        ui.add_space(12.0);
        session_status(ui, editor, session, actions);
        if let Some(coedit) = &mut editor.coedit {
            together_status(ui, coedit, editor.edit_tunnel.as_ref());
        }
        if playing {
            theme::status(ui, theme::AMBER, "Playing: edits are undone when you stop");
        }
    });
    let backdrop = ui.painter().add(egui::Shape::Noop);
    let body = egui::Frame::new().inner_margin(egui::Margin { left: 6, right: 6, top: 4, bottom: 2 }).show(
        ui,
        |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::horizontal().id_salt("ribbon").show(ui, |ui| {
                ui.horizontal(|ui| match editor.ribbon {
                    RibbonTab::Home => home_tab(ui, editor, playing, tree, actions),
                    RibbonTab::Model => model_tab(ui, editor, playing, actions),
                    RibbonTab::Test => test_tab(ui, editor, playing, actions),
                    RibbonTab::View => view_tab(ui, editor, playing),
                });
            });
        },
    );
    ui.painter().set(backdrop, theme::ribbon_back(body.response.rect));
    if editor.tool == Tool::Terrain && !playing {
        terrain_bar(ui, editor, actions);
    }
    ui.add_space(2.0);
}

/// The Select, Move, Scale and Rotate tools, in Studio's order.
fn tools_group(ui: &mut egui::Ui, editor: &mut Editor) {
    theme::ribbon_group(ui, "Tools", |ui| {
        for (tool, glyph, label, keys) in [
            (Tool::Select, icon::CURSOR, "Select", "Ctrl+1"),
            (Tool::Move, icon::ARROWS_OUT_CARDINAL, "Move", "Ctrl+2"),
            (Tool::Scale, icon::ARROWS_OUT, "Scale", "Ctrl+3"),
            (Tool::Rotate, icon::ARROW_CLOCKWISE, "Rotate", "Ctrl+4"),
        ] {
            if theme::ribbon_button(ui, glyph, label, editor.tool == tool).on_hover_text(keys).clicked() {
                editor.tool = tool;
            }
        }
    });
}

/// Play and Stop.
fn play_group(ui: &mut egui::Ui, editor: &mut Editor, playing: bool, actions: &mut Vec<Action>) {
    theme::ribbon_group(ui, "Test", |ui| {
        if !playing {
            if theme::ribbon_button(ui, icon::PLAY, "Play", false).on_hover_text("F5").clicked() {
                actions.push(Action::Play);
            }
        } else {
            let (glyph, stop) = if editor.player_mode { (icon::X, "Leave") } else { (icon::STOP, "Stop") };
            if theme::ribbon_button(ui, glyph, stop, true).on_hover_text("Shift+F5").clicked() {
                actions.push(Action::Stop);
            }
        }
    });
}

fn home_tab(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    playing: bool,
    tree: &InstanceTree,
    actions: &mut Vec<Action>,
) {
    let selected = editor.selection;
    theme::ribbon_group(ui, "Edit", |ui| {
        let undo = !playing && editor.history.can_undo();
        if ui
            .add_enabled_ui(undo, |ui| theme::ribbon_button(ui, icon::ARROW_COUNTER_CLOCKWISE, "Undo", false))
            .inner
            .on_hover_text("Ctrl+Z")
            .clicked()
        {
            actions.push(Action::Undo);
        }
        let redo = !playing && editor.history.can_redo();
        if ui
            .add_enabled_ui(redo, |ui| theme::ribbon_button(ui, icon::ARROW_CLOCKWISE, "Redo", false))
            .inner
            .on_hover_text("Ctrl+Y")
            .clicked()
        {
            actions.push(Action::Redo);
        }
        if let Some(id) = selected {
            if theme::ribbon_button(ui, icon::COPY, "Duplicate", false).on_hover_text("Ctrl+D").clicked() {
                actions.push(Action::Duplicate(id));
            }
            if theme::ribbon_button(ui, icon::TRASH, "Delete", false).on_hover_text("Del").clicked() {
                actions.push(Action::Delete(id));
            }
        } else {
            ui.add_enabled_ui(false, |ui| {
                theme::ribbon_button(ui, icon::COPY, "Duplicate", false);
                theme::ribbon_button(ui, icon::TRASH, "Delete", false);
            });
        }
    });
    tools_group(ui, editor);
    theme::ribbon_group(ui, "Terrain", |ui| {
        ui.add_enabled_ui(!playing, |ui| {
            if theme::ribbon_button(ui, icon::MOUNTAINS, "Editor", editor.tool == Tool::Terrain)
                .on_hover_text("Ctrl+5")
                .clicked()
            {
                editor.tool = if editor.tool == Tool::Terrain { Tool::Move } else { Tool::Terrain };
            }
        });
    });
    theme::ribbon_group(ui, "Insert", |ui| {
        ui.add_enabled_ui(!playing, |ui| {
            let part = theme::ribbon_button(ui, icon::CUBE, "Part", false);
            if part.clicked() {
                actions.push(Action::InsertShape(PartShape::Block));
            }
            egui::Popup::context_menu(&part).show(|ui| {
                for &(shape, label) in SHAPES {
                    if ui.button(label).clicked() {
                        actions.push(Action::InsertShape(shape));
                    }
                }
            });
            if theme::ribbon_button(ui, icon::APP_WINDOW, "UI", false)
                .on_hover_text(t("A screen GUI in StarterGui"))
                .clicked()
            {
                actions.push(Action::Insert(ClassKind::ScreenGui));
            }
            if theme::ribbon_button(ui, icon::CODE, "Script", false).clicked() {
                actions.push(Action::Insert(ClassKind::Script));
            }
            let object = theme::ribbon_button(ui, icon::PLUS_SQUARE, "Object", false);
            egui::Popup::menu(&object).show(|ui| insert_menu(ui, actions));
        });
    });
    let part = selected.filter(|s| tree.get_bool(*s, Property::Anchored).is_some());
    theme::ribbon_group(ui, "Arrange", |ui| {
        ui.add_enabled_ui(!playing && selected.is_some(), |ui| {
            if theme::ribbon_button(ui, icon::SELECTION_PLUS, "Group", false)
                .on_hover_text("Ctrl+G")
                .clicked()
                && let Some(id) = selected
            {
                actions.push(Action::Group(id));
            }
            if theme::ribbon_button(ui, icon::SELECTION_SLASH, "Ungroup", false)
                .on_hover_text("Ctrl+U")
                .clicked()
                && let Some(id) = selected
            {
                actions.push(Action::Ungroup(id));
            }
        });
        ui.add_enabled_ui(part.is_some(), |ui| {
            let anchored = part.and_then(|p| tree.get_bool(p, Property::Anchored)) == Some(true);
            if theme::ribbon_button(ui, icon::ANCHOR, "Anchor", anchored).clicked()
                && let Some(id) = part
            {
                actions.push(Action::ToggleAnchor(id));
            }
        });
    });
    play_group(ui, editor, playing, actions);
}

fn model_tab(ui: &mut egui::Ui, editor: &mut Editor, playing: bool, actions: &mut Vec<Action>) {
    tools_group(ui, editor);
    theme::ribbon_group(ui, "Snap to grid", |ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.spacing_mut().interact_size.y = 22.0;
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(t("Rotate")).color(theme::MUTED));
                egui::ComboBox::from_id_salt("angle-snap")
                    .width(76.0)
                    .selected_text(if editor.angle_snap > 0.0 {
                        format!("{}°", editor.angle_snap)
                    } else {
                        "Free".into()
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut editor.angle_snap, 0.0, t("Free"));
                        for a in [5.0, 15.0, 45.0, 90.0] {
                            ui.selectable_value(&mut editor.angle_snap, a, format!("{a}°"));
                        }
                    });
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{}  ", t("Move"))).color(theme::MUTED));
                egui::ComboBox::from_id_salt("snap")
                    .width(76.0)
                    .selected_text(if editor.snap > 0.0 {
                        format!("{} studs", editor.snap)
                    } else {
                        "Off".into()
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut editor.snap, 0.0, t("Off"));
                        for s in [0.25, 0.5, 1.0, 2.0, 4.0] {
                            ui.selectable_value(&mut editor.snap, s, format!("{s} studs"));
                        }
                    });
            });
        });
    });
    ui.add_enabled_ui(!playing, |ui| {
        theme::ribbon_group(ui, "Parts", |ui| {
            for &(shape, label) in SHAPES {
                let glyph = match shape {
                    PartShape::Ball => icon::CIRCLE,
                    PartShape::Cylinder => icon::CYLINDER,
                    PartShape::Wedge => icon::TRIANGLE,
                    _ => icon::CUBE,
                };
                if theme::ribbon_button(ui, glyph, label, false).clicked() {
                    actions.push(Action::InsertShape(shape));
                }
            }
        });
        theme::ribbon_group(ui, "Advanced", |ui| {
            for (class, label) in [
                (ClassKind::Script, "Script"),
                (ClassKind::LocalScript, "LocalScript"),
                (ClassKind::ModuleScript, "Module"),
            ] {
                if theme::ribbon_button(ui, icon(class), label, false).clicked() {
                    actions.push(Action::Insert(class));
                }
            }
        });
        theme::ribbon_group(ui, "Physics", |ui| {
            for (class, label) in [
                (ClassKind::WeldConstraint, "Weld"),
                (ClassKind::HingeConstraint, "Hinge"),
                (ClassKind::SpringConstraint, "Spring"),
                (ClassKind::RopeConstraint, "Rope"),
            ] {
                if theme::ribbon_button(ui, icon(class), label, false).clicked() {
                    actions.push(Action::Insert(class));
                }
            }
        });
        theme::ribbon_group(ui, "Marketplace", |ui| {
            if theme::ribbon_button(ui, icon::STOREFRONT, "Marketplace", editor.market.open)
                .on_hover_text("Models and plugins people share")
                .clicked()
            {
                actions.push(Action::OpenMarket);
            }
        });
        theme::ribbon_group(ui, "Avatar", |ui| {
            let selected = editor.selection;
            ui.add_enabled_ui(selected.is_some(), |ui| {
                if theme::ribbon_button(ui, icon::T_SHIRT, "Share as hat", false)
                    .on_hover_text(
                        "Put the selected model's parts in the catalog as a hat anyone can wear. It sits with its \
                         bottom on the head, so build it the way up it goes.",
                    )
                    .clicked()
                    && let Some(id) = selected
                {
                    actions.push(Action::ShareHat(id));
                }
            });
        });
        theme::ribbon_group(ui, "Effects", |ui| {
            for (class, label) in [
                (ClassKind::PointLight, "Light"),
                (ClassKind::ParticleEmitter, "Particles"),
                (ClassKind::Fire, "Fire"),
                (ClassKind::Sound, "Sound"),
            ] {
                if theme::ribbon_button(ui, icon(class), label, false).clicked() {
                    actions.push(Action::Insert(class));
                }
            }
        });
    });
    play_group(ui, editor, playing, actions);
}

fn test_tab(ui: &mut egui::Ui, editor: &mut Editor, playing: bool, actions: &mut Vec<Action>) {
    play_group(ui, editor, playing, actions);
    ui.add_enabled_ui(!playing, |ui| {
        theme::ribbon_group(ui, "Clients and servers", |ui| {
            if theme::ribbon_button(ui, icon::BROADCAST, "Host", false)
                .on_hover_text(t("Play, and let other computers join"))
                .clicked()
            {
                actions.push(Action::Host);
            }
            if theme::ribbon_button(ui, icon::SIGN_IN, "Join", false)
                .on_hover_text(t("Join a game another computer hosts"))
                .clicked()
            {
                editor.dialog =
                    Some(Dialog::Join { address: String::new(), name: editor.platform.player_name() });
            }
            if theme::ribbon_button(ui, icon::GLOBE_HEMISPHERE_WEST, "Browse", false)
                .on_hover_text(t("Find games being hosted right now"))
                .clicked()
            {
                editor.platform.open = true;
            }
        });
        theme::ribbon_group(ui, "Collaborate", |ui| {
            if editor.coedit.is_none() {
                if theme::ribbon_button(ui, icon::USERS_THREE, "Edit together", false)
                    .on_hover_text(t("Share this game so others can edit it with you, or join someone's"))
                    .clicked()
                {
                    let name = editor.platform.player_name();
                    editor.dialog = Some(Dialog::EditTogether { address: String::new(), name });
                }
            } else if theme::ribbon_button(ui, icon::USERS_THREE, "Stop sharing", true).clicked() {
                actions.push(Action::StopEdits);
            }
        });
        theme::ribbon_group(ui, "Data", |ui| {
            if theme::ribbon_button(ui, icon::DATABASE, "Clear saved", false)
                .on_hover_text(t("Forget what this game's data stores saved in earlier play tests"))
                .clicked()
            {
                actions.push(Action::ClearData);
            }
        });
    });
}

fn view_tab(ui: &mut egui::Ui, editor: &mut Editor, playing: bool) {
    theme::ribbon_group(ui, "Show", |ui| {
        let side = !playing || editor.panels_in_play;
        let mut opened_side = false;
        for (glyph, label, on, is_side) in [
            (icon::TREE_STRUCTURE, "Explorer", &mut editor.show_explorer, true),
            (icon::SLIDERS_HORIZONTAL, "Properties", &mut editor.show_properties, true),
            (icon::TOOLBOX, "Toolbox", &mut editor.show_toolbox, true),
            (icon::TERMINAL_WINDOW, "Output", &mut editor.show_output, false),
        ] {
            let shown = *on && (side || !is_side);
            if theme::ribbon_button(ui, glyph, label, shown).clicked() {
                if is_side && !side {
                    // Asking for a side panel while playing brings the panels back.
                    *on = true;
                    opened_side = true;
                } else {
                    *on = !*on;
                }
            }
        }
        if opened_side {
            editor.panels_in_play = true;
        }
        if theme::ribbon_button(ui, icon::SIDEBAR, "In play", editor.panels_in_play)
            .on_hover_text(t("Keep the side panels open while playing (off: the game gets the whole width)"))
            .clicked()
        {
            editor.panels_in_play = !editor.panels_in_play;
        }
    });
    theme::ribbon_group(ui, "Settings", |ui| {
        if theme::ribbon_button(ui, icon::GRID_FOUR, "Grid", editor.show_grid).clicked() {
            editor.show_grid = !editor.show_grid;
        }
        if theme::ribbon_button(ui, icon::GAUGE, "Stats", editor.stats.shown)
            .on_hover_text(t("Frame rate and where each frame's time goes, while playing (Shift+F1)"))
            .clicked()
        {
            editor.stats.shown = !editor.stats.shown;
        }
        if theme::ribbon_button(ui, icon::HAND_TAP, "Touch", editor.touch.seen)
            .on_hover_text(t("Show the on-screen stick and jump button while playing"))
            .clicked()
        {
            editor.touch.seen = !editor.touch.seen;
        }
        let sharing = crate::stats::enabled();
        if theme::ribbon_button(ui, icon::CHART_LINE_UP, "Share", sharing)
            .on_hover_text(t("Send crash reports and usage stats to the Hub"))
            .clicked()
        {
            crate::stats::set_enabled(!sharing);
        }
    });
    theme::ribbon_group(ui, "Help", |ui| {
        let keys = theme::ribbon_button(ui, icon::KEYBOARD, "Shortcuts", false);
        egui::Popup::menu(&keys).show(help_items);
    });
}

/// The keyboard shortcuts, as a menu's contents.
fn help_items(ui: &mut egui::Ui) {
    for line in [
        "Right mouse + WASD/QE: fly the camera (Shift = faster)",
        "Scroll: move forward/back · F: look at the selection",
        "Ctrl+1..4: Select, Move, Scale, Rotate · Ctrl+5: terrain",
        "Ctrl+D: duplicate · Del: delete · Ctrl+R: turn 90°",
        "Ctrl+G: group · Ctrl+U: ungroup",
        "Ctrl+Z / Ctrl+Y: undo / redo · Ctrl+S: save · Ctrl+O: open",
        "F5: play · Shift+F5 or Esc: stop · Shift+F1: frame stats",
        "While playing: WASD walk, Space jump, right mouse turns the camera",
    ] {
        ui.label(line);
    }
}

/// A toolbox tile: its icon and colour, its name, and what clicking it does.
type ToolboxItem = (&'static str, Color32, &'static str, Action);

/// The toolbox: ready-made things to drop into the game.
fn toolbox(ui: &mut egui::Ui, editor: &mut Editor, playing: bool, actions: &mut Vec<Action>) {
    theme::panel_title(ui, icon::TOOLBOX, "Toolbox");
    ui.add(
        egui::TextEdit::singleline(&mut editor.toolbox_search)
            .hint_text(t("Search"))
            .desired_width(f32::INFINITY),
    );
    ui.add_space(6.0);
    let search = editor.toolbox_search.trim().to_lowercase();
    let shown = |name: &str| search.is_empty() || name.to_lowercase().contains(&search);
    egui::ScrollArea::vertical().id_salt("toolbox").auto_shrink([false, false]).show(ui, |ui| {
        ui.add_enabled_ui(!playing, |ui| {
            let sections: [(&str, Vec<ToolboxItem>); 4] = [
                (
                    "Parts",
                    SHAPES
                        .iter()
                        .map(|&(shape, label)| {
                            let glyph = match shape {
                                PartShape::Ball => icon::CIRCLE,
                                PartShape::Cylinder => icon::CYLINDER,
                                PartShape::Wedge => icon::TRIANGLE,
                                _ => icon::CUBE,
                            };
                            (glyph, theme::ACCENT, label, Action::InsertShape(shape))
                        })
                        .collect(),
                ),
                (
                    "Models",
                    vec![
                        (icon::PERSON_SIMPLE, theme::ACCENT, "Character", Action::InsertNpc),
                        (icon::STEERING_WHEEL, theme::ACCENT, "Go-kart", Action::InsertKart),
                        (
                            icon(ClassKind::SpawnLocation),
                            theme::ACCENT,
                            "Spawn",
                            Action::Insert(ClassKind::SpawnLocation),
                        ),
                        (icon(ClassKind::Tool), theme::MUTED, "Tool", Action::Insert(ClassKind::Tool)),
                        (icon::STOREFRONT, theme::LINK, "Marketplace", Action::OpenMarket),
                    ],
                ),
                (
                    "Gameplay",
                    vec![
                        (icon(ClassKind::Script), theme::GREEN, "Script", Action::Insert(ClassKind::Script)),
                        (
                            icon(ClassKind::ClickDetector),
                            theme::GREEN,
                            "Click detector",
                            Action::Insert(ClassKind::ClickDetector),
                        ),
                        (
                            icon(ClassKind::ProximityPrompt),
                            theme::GREEN,
                            "Prompt",
                            Action::Insert(ClassKind::ProximityPrompt),
                        ),
                        (
                            icon(ClassKind::RemoteEvent),
                            theme::GREEN,
                            "Remote event",
                            Action::Insert(ClassKind::RemoteEvent),
                        ),
                        (icon(ClassKind::Team), theme::MUTED, "Team", Action::Insert(ClassKind::Team)),
                    ],
                ),
                (
                    "Look",
                    vec![
                        (
                            icon(ClassKind::PointLight),
                            theme::AMBER,
                            "Light",
                            Action::Insert(ClassKind::PointLight),
                        ),
                        (
                            icon(ClassKind::ParticleEmitter),
                            theme::AMBER,
                            "Particles",
                            Action::Insert(ClassKind::ParticleEmitter),
                        ),
                        (icon(ClassKind::Fire), theme::AMBER, "Fire", Action::Insert(ClassKind::Fire)),
                        (icon(ClassKind::Smoke), theme::AMBER, "Smoke", Action::Insert(ClassKind::Smoke)),
                        (
                            icon(ClassKind::ScreenGui),
                            theme::LINK,
                            "Screen GUI",
                            Action::Insert(ClassKind::ScreenGui),
                        ),
                        (
                            icon(ClassKind::TextLabel),
                            theme::LINK,
                            "Text label",
                            Action::Insert(ClassKind::TextLabel),
                        ),
                    ],
                ),
            ];
            for (title, items) in sections {
                let items: Vec<_> = items.into_iter().filter(|(_, _, name, _)| shown(name)).collect();
                if items.is_empty() {
                    continue;
                }
                theme::label_caps(ui, title);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                    for (glyph, colour, name, action) in items {
                        if theme::toolbox_tile(ui, glyph, colour, name).clicked() {
                            actions.push(action);
                        }
                    }
                });
                ui.add_space(8.0);
            }
            theme::label_caps(ui, "From place files");
            ui.label(
                RichText::new(t("Drop a .rbxm model on the window to add it, or a .rbxl place to open it."))
                    .color(theme::MUTED),
            );
        });
    });
}

/// Frame rate and where each frame's time goes, shown with Shift+F1 while
/// playing (like the reference platform's own stats). While playing, a fuller report is also
/// written every few seconds to `speed.txt` in the logs folder, to send when
/// a game runs slowly.
#[derive(Default)]
pub struct Stats {
    pub shown: bool,
    frames: u32,
    seconds: f32,
    text: String,
    report: SpeedReport,
    /// Whether the game had its 3D view off (a loading screen) last frame.
    loading: bool,
}

/// What `speed.txt` sums up: the last few seconds of play.
#[derive(Default)]
struct SpeedReport {
    frames: u32,
    seconds: f32,
    worst: f32,
    slow: u32,
    /// Seconds spent building the UI (the editor and the game's screens).
    ui_seconds: f64,
    step: libroxel_engine::FrameTimes,
    scripts_before: std::collections::HashMap<String, (f64, u64)>,
}

/// How often `speed.txt` is rewritten while playing, in seconds.
const REPORT_EVERY: f32 = 5.0;

/// Where the speed report goes.
pub fn speed_report_path() -> std::path::PathBuf {
    crate::debugging::logs_dir().join("speed.txt")
}

impl Stats {
    fn update(
        &mut self,
        session: &mut libroxel_engine::PlaySession,
        dt: f32,
        about: impl FnOnce() -> String,
    ) {
        // A loading screen (3D rendering off) gets a report of its own, from
        // start to end, put in the Output when it goes: what took the time.
        let loading = !libroxel_script::RENDERING_3D.load(std::sync::atomic::Ordering::Relaxed);
        if loading != self.loading {
            self.loading = loading;
            if loading {
                let before = std::mem::take(&mut self.report.scripts_before);
                self.report = SpeedReport { scripts_before: before, ..Default::default() };
                let _ = session.take_frame_times();
                let _ = session.physics_mut().take_timing();
                let _ = libroxel_engine::view::take_sync_seconds();
                self.frames = 0;
                self.seconds = 0.0;
            } else {
                let text = self.write_report(session, about(), true);
                session.note(text);
                return;
            }
        }
        self.frames += 1;
        self.seconds += dt;
        let r = &mut self.report;
        r.frames += 1;
        r.seconds += dt;
        r.worst = r.worst.max(dt);
        r.slow += u32::from(dt > 1.0 / 30.0);
        if self.seconds < 0.5 {
            return;
        }
        let t = session.take_frame_times();
        r.step.add(&t);
        let per = |s: f64| if t.frames > 0 { s * 1000.0 / f64::from(t.frames) } else { 0.0 };
        let fps = self.frames as f32 / self.seconds;
        let step = per(t.step_seconds());
        self.text = format!(
            "{fps:.0} FPS ({:.1} ms a frame)\ngame step {step:.1} ms: scripts {:.1}, events {:.1}, physics {:.1}, other {:.1}\ndrawing and the rest {:.1} ms",
            1000.0 / fps.max(0.01),
            per(t.scripts),
            per(t.change_events),
            per(t.physics),
            per(t.other),
            (1000.0 / fps.max(0.01)) as f64 - step,
        );
        if t.receive + t.apply > 0.0 {
            self.text.push_str(&format!(
                "\nfrom the host {:.1} ms, into the world {:.1} ms",
                per(t.receive),
                per(t.apply)
            ));
        }
        self.frames = 0;
        self.seconds = 0.0;
        if r.seconds >= REPORT_EVERY && !self.loading {
            self.write_report(session, about(), false);
        }
    }

    /// Writes `speed.txt` (or, for a loading screen, `loading-speed.txt`)
    /// and gives back its text.
    fn write_report(
        &mut self,
        session: &mut libroxel_engine::PlaySession,
        about: String,
        loading: bool,
    ) -> String {
        use std::fmt::Write as _;
        let r = std::mem::take(&mut self.report);
        let t = &r.step;
        let per = |s: f64| if t.frames > 0 { s * 1000.0 / f64::from(t.frames) } else { 0.0 };
        let frame_ms = r.seconds * 1000.0 / r.frames.max(1) as f32;
        let step = per(t.step_seconds());
        let mut out = String::new();
        if loading {
            let _ =
                writeln!(out, "Libroxel speed report for the loading screen ({:.0} seconds)\n", r.seconds);
        } else {
            let _ = writeln!(out, "Libroxel speed report (the last {:.0} seconds of play)\n", r.seconds);
        }
        let _ = writeln!(out, "{about}");
        let _ = writeln!(
            out,
            "{:.0} FPS, {frame_ms:.1} ms a frame on average, slowest frame {:.0} ms, {} of {} frames slower than 30 FPS",
            r.frames as f32 / r.seconds.max(0.01),
            r.worst * 1000.0,
            r.slow,
            r.frames,
        );
        let _ = writeln!(
            out,
            "game step {step:.1} ms a frame: scripts {:.1}, change events {:.1}, physics {:.1}, other {:.1}",
            per(t.scripts),
            per(t.change_events),
            per(t.physics),
            per(t.other),
        );
        if t.receive + t.apply + t.explosion_scan > 0.0 {
            let _ = writeln!(
                out,
                "  joined game: taking in the host's messages {:.1}, putting them in the world {:.1} (slowest message {:.0} ms), looking for explosions {:.1}; {:.1} MB in, {} operations, up to {} messages waiting",
                per(t.receive),
                per(t.apply),
                t.slowest_apply * 1000.0,
                per(t.explosion_scan),
                t.bytes_in as f64 / 1_000_000.0,
                t.ops_applied,
                t.most_waiting,
            );
        }
        let _ = writeln!(out, "drawing and the rest {:.1} ms a frame", frame_ms as f64 - step);
        let frames = f64::from(r.frames.max(1));
        let _ = writeln!(
            out,
            "  of which: copying parts to the 3D view {:.1} ms, building the screen's UI {:.1} ms",
            libroxel_engine::view::take_sync_seconds() * 1000.0 / frames,
            r.ui_seconds * 1000.0 / frames,
        );
        let (shown, merged, clusters) = libroxel_engine::view::cluster_counts();
        let _ = writeln!(
            out,
            "  {shown} parts in the 3D view, {merged} of them drawn merged into {clusters} clusters"
        );
        let p = session.physics_mut().take_timing();
        if p.steps > 0 {
            let ms = |s: f64| s * 1000.0 / f64::from(p.steps);
            let _ = writeln!(
                out,
                "physics {:.1} ms a step: gathering parts {:.1}, broadphase {:.1}, characters {:.1}, loose parts {:.1}, touches {:.1}; {} parts looked at, {} moving",
                ms(p.gather + p.broadphase + p.characters + p.loose + p.touches),
                ms(p.gather),
                ms(p.broadphase),
                ms(p.characters),
                ms(p.loose),
                ms(p.touches),
                p.bodies / u64::from(p.steps),
                p.movers / u64::from(p.steps),
            );
        }
        let _ = writeln!(out);
        let _ = writeln!(out, "Scripts that took the most time:");
        let now: std::collections::HashMap<String, (f64, u64)> =
            session.runtime().script_times().into_iter().map(|(name, secs, n)| (name, (secs, n))).collect();
        let mut spent: Vec<(&String, f64, u64)> = now
            .iter()
            .map(|(name, (secs, n))| {
                // A script that started again (a new play, a respawn) counts from nothing.
                let (s0, n0) = r
                    .scripts_before
                    .get(name)
                    .copied()
                    .filter(|&(s0, n0)| n0 <= *n && s0 <= *secs)
                    .unwrap_or_default();
                (name, secs - s0, n - n0)
            })
            .filter(|(_, secs, _)| *secs > 0.0)
            .collect();
        spent.sort_by(|a, b| b.1.total_cmp(&a.1));
        for (name, secs, n) in spent.into_iter().take(15) {
            let _ = writeln!(
                out,
                "  {name}: {:.2} ms a frame ({n} runs)",
                secs * 1000.0 / f64::from(r.frames.max(1))
            );
        }
        self.report.scripts_before = now;
        let path = if loading {
            speed_report_path().with_file_name("loading-speed.txt")
        } else {
            speed_report_path()
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, &out);
        out
    }
}
