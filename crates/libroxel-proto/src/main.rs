//! Phase 0 prototype: open a window where a Luau script moves a part.
//!
//! ```text
//! cargo run -p libroxel-proto                 # runs scripts/move_part.luau
//! cargo run -p libroxel-proto -- my_script.luau
//! ```

use bevy::prelude::*;
use libroxel_proto::{LibroxelPlugin, PartAppearance, PartLink, StartupScript};

const DEFAULT_SCRIPT: &str = include_str!("../scripts/move_part.luau");

fn main() {
    let script = match std::env::args().nth(1) {
        Some(path) => {
            let source = std::fs::read_to_string(&path).unwrap_or_else(|err| {
                eprintln!("could not read {path}: {err}");
                std::process::exit(1);
            });
            StartupScript { name: path, source }
        }
        None => StartupScript { name: "move_part.luau".into(), source: DEFAULT_SCRIPT.into() },
    };

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window { title: "Libroxel prototype".into(), ..default() }),
            ..default()
        }))
        .add_plugins(LibroxelPlugin { scripts: vec![script] })
        .add_systems(Startup, setup_scene)
        .add_systems(PostUpdate, (attach_visuals, update_materials))
        .run();
}

#[derive(Resource)]
struct UnitCube(Handle<Mesh>);

fn setup_scene(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    // Parts are unit cubes scaled by their Size.
    commands.insert_resource(UnitCube(meshes.add(Cuboid::new(1.0, 1.0, 1.0))));
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(-14.0, 12.0, 18.0).looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight { illuminance: 8_000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_xyz(8.0, 16.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

fn material_for(appearance: &PartAppearance) -> StandardMaterial {
    let alpha = 1.0 - appearance.transparency;
    StandardMaterial {
        base_color: appearance.color.with_alpha(alpha),
        alpha_mode: if alpha < 1.0 { AlphaMode::Blend } else { AlphaMode::Opaque },
        perceptual_roughness: 0.8,
        ..default()
    }
}

fn attach_visuals(
    mut commands: Commands,
    cube: Res<UnitCube>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_parts: Query<(Entity, &PartAppearance), Added<PartLink>>,
) {
    for (entity, appearance) in &new_parts {
        commands
            .entity(entity)
            .insert((Mesh3d(cube.0.clone()), MeshMaterial3d(materials.add(material_for(appearance)))));
    }
}

fn update_materials(
    mut materials: ResMut<Assets<StandardMaterial>>,
    changed: Query<(&PartAppearance, &MeshMaterial3d<StandardMaterial>), Changed<PartAppearance>>,
) {
    for (appearance, handle) in &changed {
        if let Some(mut material) = materials.get_mut(&handle.0) {
            *material = material_for(appearance);
        }
    }
}
