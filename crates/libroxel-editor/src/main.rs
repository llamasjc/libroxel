//! The Libroxel editor.
//!
//! Build a world out of parts, attach scripts, and press Play to test it
//! with a character. Stop puts everything back the way it was.
//!
//! ```text
//! cargo run -p libroxel-editor                 # start with a blank world
//! cargo run -p libroxel-editor -- my_game.lrx  # open a saved place
//! cargo run -p libroxel-editor -- --sample     # open the sample obby
//! ```

mod gui;
mod history;
mod images;
mod templates;
mod ui;
mod viewport;

use std::path::PathBuf;

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{CameraOutputMode, ClearColorConfig};
use bevy::prelude::*;
use bevy::render::render_resource::BlendState;
use bevy_egui::{EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass, PrimaryEguiContext};
use libroxel_engine::view::{EnginePlugin, EngineSet, EngineState, RenderPlugin};
use libroxel_instance::{InstanceId, InstanceTree, place};

use crate::history::History;

/// The active viewport tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Move,
    Scale,
    Rotate,
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
}

/// Everything the editor remembers between frames.
pub struct Editor {
    pub selection: Option<InstanceId>,
    pub tool: Tool,
    /// Grid snap in studs; 0 turns snapping off.
    pub snap: f32,
    /// Rotation snap in degrees; 0 turns it off.
    pub angle_snap: f32,
    pub history: History,
    /// The world as it was when Play was pressed.
    pub edit_snapshot: Option<InstanceTree>,
    pub file: Option<PathBuf>,
    pub dirty: bool,
    pub open_script: Option<InstanceId>,
    pub renaming: Option<(InstanceId, String)>,
    pub dialog: Option<Dialog>,
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
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            selection: None,
            tool: Tool::Move,
            snap: 1.0,
            angle_snap: 15.0,
            history: History::default(),
            edit_snapshot: None,
            file: None,
            dirty: false,
            open_script: None,
            renaming: None,
            dialog: None,
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
        }
    }
}

/// Where the Open and Save dialogs start.
pub fn games_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Projects/libroxel-games")
}

fn main() {
    let mut editor = Editor::default();
    let tree = match std::env::args().nth(1) {
        Some(flag) if flag == "--sample" => templates::sample_obby(),
        Some(path) => match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| place::load(&text).map_err(|e| e.to_string()))
        {
            Ok((tree, _)) => {
                editor.file = Some(PathBuf::from(path));
                tree
            }
            Err(err) => {
                eprintln!("could not open {path}: {err}");
                std::process::exit(1);
            }
        },
        None => templates::blank(),
    };

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Libroxel Editor".into(),
                resolution: (1440, 900).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        .insert_non_send(EngineState::new(tree))
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
        "{}{}{} - Libroxel Editor",
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
