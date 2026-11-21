//! The Libroxel app.
//!
//! It opens on a home screen: **Play** your games, find and join games
//! **Online**, **Create** one in the editor, or **Host** one for friends.
//! In the editor you build a world out of parts, attach scripts, and press
//! Play to test it with a character. Stop puts everything back.
//!
//! A game file (or `--sample`) on the command line skips the home screen
//! and opens it in the editor. A `libroxel://` link (from Play on the
//! website) opens the home screen and joins or starts that game.
//!
//! ```text
//! cargo run                             # the home screen
//! cargo run -p libroxel -- my_game.lrx  # edit a saved place
//! ```

mod browse;
mod debugging;
mod file_picker;
mod gui;
mod history;
mod hub_home;
mod images;
mod menu;
mod mesh_import;
mod open;
mod rich_text;
mod server;
mod social;
mod starter_games;
mod templates;
mod terrain_tools;
mod theme;
mod touch;
mod ui;
mod url_handler;
mod viewport;
mod viewport_frame;

use std::path::PathBuf;

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{CameraOutputMode, ClearColorConfig};
use bevy::prelude::*;
use bevy::render::render_resource::BlendState;
use bevy_egui::{EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass, PrimaryEguiContext};
use libroxel_engine::view::{EnginePlugin, EngineSet, EngineState, RenderPlugin};
use libroxel_instance::{InstanceId, InstanceTree};

use crate::history::History;

/// The active viewport tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Move,
    Scale,
    Rotate,
    /// Add, dig or paint terrain where you click.
    Terrain,
}

/// Which file dialog is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dialog {
    Open {
        path: String,
    },
    SaveAs {
        path: String,
    },
    /// Joining a game someone hosts: where, and the name to play as.
    Join {
        address: String,
        name: String,
    },
    /// Publishing the open game to the Hub.
    Publish {
        title: String,
        description: String,
    },
    /// Editing a game together: share this one, or join someone's.
    EditTogether {
        address: String,
        name: String,
    },
}

/// Everything the editor remembers between frames.
pub struct Editor {
    /// The graphics card and driver drawing the game, for the speed report.
    pub graphics_adapter: String,
    pub selection: Option<InstanceId>,
    pub tool: Tool,
    /// The terrain brush, and when it may next be used while held down.
    pub brush: terrain_tools::Brush,
    pub brush_ready: f64,
    /// Grid snap in studs; 0 turns snapping off.
    pub snap: f32,
    /// Rotation snap in degrees; 0 turns it off.
    pub angle_snap: f32,
    pub history: History,
    /// The world as it was when Play was pressed.
    pub edit_snapshot: Option<InstanceTree>,
    pub file: Option<PathBuf>,
    /// The file from another engine the game was opened from, if it was:
    /// its folder is searched for pictures, models and sounds.
    pub imported_from: Option<PathBuf>,
    pub dirty: bool,
    pub open_script: Option<InstanceId>,
    pub renaming: Option<(InstanceId, String)>,
    pub dialog: Option<Dialog>,
    /// The computer's "open a file" window, when it's showing.
    pub picker: file_picker::Picker,
    pub command: String,
    pub gizmo: transform_gizmo_egui::Gizmo,
    /// Whether the gizmo was being dragged last frame.
    pub gizmo_active: bool,
    /// Ask the viewport to frame the selection this frame.
    pub focus_selection: bool,
    pub show_grid: bool,
    /// Expand the explorer so the selection is visible (after picking in the view).
    pub reveal_selection: bool,
    /// The fly camera from before Play, restored on Stop.
    pub saved_camera: Option<(Transform, libroxel_engine::view::CameraRig)>,
    /// The "add attribute" row: its name and which kind of value.
    pub new_attribute: (String, usize),
    /// The "add tag" box.
    pub new_tag: String,
    /// While playing: the text box being typed into, and whether its field
    /// still needs to take the keyboard.
    pub typing: Option<InstanceId>,
    pub focus_typing: bool,
    /// While hosting or joining by code: the connection over the internet.
    pub tunnel: Option<Tunnel>,
    /// Editing this game together with other computers.
    pub coedit: Option<libroxel_engine::coedit::CoEdit>,
    /// While sharing or joining an edit by code: its internet connection.
    pub edit_tunnel: Option<Tunnel>,
    /// The Hub: Browse, the built-in Hub, and listing a hosted game.
    pub platform: browse::Platform,
    /// The main menu, shown when the app starts.
    pub menu: menu::Menu,
    /// Playing a game started from the main menu: only the game shows, and
    /// leaving it goes back to the menu.
    pub player_mode: bool,
    /// Fingers on a touch screen, for the on-screen controls.
    pub touch: touch::TouchPad,
    /// Which tab of the ribbon along the top is open.
    pub ribbon: ui::RibbonTab,
    /// Which of the editor's panels are showing.
    pub show_explorer: bool,
    pub show_properties: bool,
    pub show_output: bool,
    pub show_toolbox: bool,
    /// Keep the toolbox, explorer and properties open while playing (the
    /// default, as in other editors); off, the game gets the whole width.
    pub panels_in_play: bool,
    /// What's typed in the toolbox's search box.
    pub toolbox_search: String,
    /// The Output, saved to a file as it grows.
    pub log_file: debugging::LogFile,
    /// What the Output panel shows, and how it's filtered.
    pub output_view: ui::OutputView,
    pub output_search: String,
    /// What the Output shows, worked out again only for new lines.
    pub output_cache: ui::OutputCache,
    /// Shift+F1's frame stats.
    pub stats: ui::Stats,
    /// While playing: the GUI button the left mouse button went down on.
    pub pressed_button: Option<InstanceId>,
    /// The Explorer's filter: only what matches (and what leads to it) shows.
    pub explorer_filter: String,
    /// The Properties panel's filter.
    pub properties_filter: String,
    /// A filter box to give the keyboard to next frame.
    pub focus_filter: Option<bevy_egui::egui::Id>,
    pub show_prints: bool,
    pub show_warnings: bool,
    pub show_errors: bool,
    /// Show an error or warning that repeats once, with how many times.
    pub group_repeats: bool,
}

/// A game carried over the internet with a join code.
pub enum Tunnel {
    /// This computer hosts and hands out a join code.
    Host(libroxel_net::HostTunnel),
    /// This computer joined someone else's game with their code.
    Join(libroxel_net::JoinTunnel),
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            selection: None,
            tool: Tool::Move,
            brush: Default::default(),
            brush_ready: 0.0,
            snap: 1.0,
            angle_snap: 15.0,
            history: History::default(),
            edit_snapshot: None,
            file: None,
            imported_from: None,
            dirty: false,
            open_script: None,
            renaming: None,
            dialog: None,
            picker: file_picker::Picker::default(),
            command: String::new(),
            gizmo: transform_gizmo_egui::Gizmo::default(),
            gizmo_active: false,
            focus_selection: false,
            show_grid: true,
            reveal_selection: false,
            saved_camera: None,
            new_attribute: (String::new(), 0),
            new_tag: String::new(),
            typing: None,
            focus_typing: false,
            tunnel: None,
            coedit: None,
            edit_tunnel: None,
            platform: browse::Platform::default(),
            menu: menu::Menu::default(),
            player_mode: false,
            touch: Default::default(),
            ribbon: ui::RibbonTab::Home,
            show_explorer: true,
            show_properties: true,
            show_output: true,
            show_toolbox: true,
            panels_in_play: true,
            graphics_adapter: String::new(),
            toolbox_search: String::new(),
            log_file: Default::default(),
            output_view: ui::OutputView::Log,
            output_search: String::new(),
            output_cache: Default::default(),
            stats: Default::default(),
            pressed_button: None,
            explorer_filter: String::new(),
            properties_filter: String::new(),
            focus_filter: None,
            show_prints: true,
            show_warnings: true,
            show_errors: true,
            group_repeats: true,
        }
    }
}

/// Where the Open and Save dialogs start.
pub fn games_dir() -> PathBuf {
    // Windows has no HOME; use the user's profile folder there.
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Projects/libroxel-games")
}

/// Where this player's identity key is kept, in the user's settings folder.
pub fn identity_path() -> PathBuf {
    let dir = std::env::var_os("APPDATA")
        .map(|d| PathBuf::from(d).join("Libroxel"))
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(|d| PathBuf::from(d).join("libroxel")))
        .or_else(|| std::env::var_os("HOME").map(|d| PathBuf::from(d).join(".config/libroxel")))
        .unwrap_or_else(|| PathBuf::from(".libroxel"));
    dir.join("identity.key")
}

/// This player's identity, made the first time it is needed and kept in
/// the user's settings folder.
pub fn identity() -> Result<libroxel_net::Identity, String> {
    libroxel_net::Identity::load_or_create(&identity_path())
        .map_err(|e| format!("Could not load your identity: {e}"))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if server::dispatch(&args) {
        return;
    }
    // Play buttons on the website open this copy of the app.
    url_handler::register();
    let mut editor = Editor { platform: browse::Platform::start(), ..Editor::default() };
    let mut notes = Vec::new();
    let tree = match args.first().cloned() {
        Some(flag) if flag == "--sample" => templates::sample_obby(),
        Some(link) if url_handler::is_link(&link) => {
            editor.menu.open = true;
            match url_handler::parse(&link) {
                Some(launch) => editor.menu.home.launch = Some(url_handler::PendingLaunch::new(launch)),
                None => eprintln!("note: this copy of Libroxel doesn't understand the link {link}"),
            }
            templates::blank()
        }
        Some(path) => match open::open(std::path::Path::new(&path)) {
            Ok(opened) => {
                if opened.imported {
                    editor.imported_from = Some(PathBuf::from(&path));
                } else {
                    editor.file = Some(PathBuf::from(path));
                }
                notes = opened.notes;
                opened.tree
            }
            Err(err) => {
                eprintln!("could not open {path}: {err}");
                std::process::exit(1);
            }
        },
        None => {
            // Started plainly: begin at the main menu.
            editor.menu.open = true;
            templates::blank()
        }
    };

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Libroxel".into(),
                resolution: (1440, 900).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        .insert_non_send({
            let mut engine = EngineState::new(tree);
            engine.log.extend(notes.into_iter().map(|text| libroxel_engine::LogEntry {
                level: libroxel_engine::LogLevel::Info,
                time: 0.0,
                text,
            }));
            engine
        })
        .insert_non_send(editor)
        .add_plugins((EnginePlugin, RenderPlugin))
        .add_systems(Startup, setup_ui_camera)
        .add_systems(EguiPrimaryContextPass, ui::editor_ui)
        .add_systems(Update, (viewport::draw_selection, viewport::draw_grid).after(EngineSet::Sync))
        .add_systems(Update, update_title)
        .run();
}

/// egui draws with its own camera so the 3D camera can be limited to the
/// space between the panels.
fn setup_ui_camera(mut commands: Commands, mut settings: ResMut<EguiGlobalSettings>) {
    settings.auto_create_primary_context = false;
    commands.spawn((
        PrimaryEguiContext,
        Camera2d,
        RenderLayers::none(),
        Camera {
            order: 1,
            output_mode: CameraOutputMode::Write {
                blend_state: Some(BlendState::ALPHA_BLENDING),
                clear_color: ClearColorConfig::None,
            },
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..default()
        },
    ));
}

fn update_title(editor: NonSend<Editor>, engine: NonSend<EngineState>, mut windows: Query<&mut Window>) {
    let name = editor
        .file
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".into());
    let title = format!(
        "{}{}{} - Libroxel",
        name,
        if editor.dirty { " *" } else { "" },
        if engine.is_playing() { " (playing)" } else { "" }
    );
    for mut window in &mut windows {
        if window.title != title {
            window.title = title.clone();
        }
    }
}
