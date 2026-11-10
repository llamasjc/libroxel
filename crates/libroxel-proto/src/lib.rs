//! The Bevy side of the Phase 0 prototype.
//!
//! [`LibroxelPlugin`] owns a [`ScriptRuntime`] and mirrors every part in the
//! workspace onto a Bevy entity with a [`Transform`] and a
//! [`PartAppearance`]. It needs no renderer, so it runs headless in tests;
//! the binary adds meshes and a camera on top.

use std::collections::HashMap;

use bevy::prelude::*;
use libroxel_instance::{ClassKind, InstanceId, Property, PropertyValue};
use libroxel_script::{OutputLevel, ScriptRuntime};

/// A script to run when the app starts.
#[derive(Debug, Clone)]
pub struct StartupScript {
    pub name: String,
    pub source: String,
}

/// Runs Luau scripts against an instance tree and mirrors parts into the ECS.
#[derive(Default)]
pub struct LibroxelPlugin {
    pub scripts: Vec<StartupScript>,
}

/// The scripting runtime. Lua is single-threaded, so it lives in a
/// non-send resource and its systems run on the main thread.
pub struct ScriptHost {
    pub runtime: ScriptRuntime,
}

/// Links an entity to the part it mirrors.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartLink(pub InstanceId);

/// The part's name, for debugging and lookups.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct PartName(pub String);

/// How the part should look. The renderer turns this into a material.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct PartAppearance {
    pub color: Color,
    pub transparency: f32,
}

#[derive(Resource, Default)]
struct PartEntities(HashMap<InstanceId, Entity>);

#[derive(Resource)]
struct PendingScripts(Vec<StartupScript>);

impl Plugin for LibroxelPlugin {
    fn build(&self, app: &mut App) {
        let runtime = ScriptRuntime::new_with_tree(Default::default()).expect("Luau runtime starts");
        app.insert_non_send(ScriptHost { runtime })
            .init_resource::<PartEntities>()
            .insert_resource(PendingScripts(self.scripts.clone()))
            .add_systems(Startup, (run_startup_scripts, sync_parts).chain())
            .add_systems(Update, (step_scripts, sync_parts).chain());
    }
}

fn report_output(host: &ScriptHost) {
    for line in host.runtime.take_output() {
        match line.level {
            OutputLevel::Print => info!(target: "script", "{}", line.text),
            OutputLevel::Warn => warn!(target: "script", "{}", line.text),
        }
    }
}

fn run_startup_scripts(host: NonSend<ScriptHost>, mut pending: ResMut<PendingScripts>) {
    for script in pending.0.drain(..) {
        if let Err(err) = host.runtime.run(&script.name, &script.source) {
            error!(target: "script", "{err}");
        }
    }
    report_output(&host);
}

fn step_scripts(host: NonSend<ScriptHost>, time: Res<Time>) {
    for err in host.runtime.step(time.delta_secs_f64()) {
        error!(target: "script", "{err}");
    }
    report_output(&host);
}

fn to_bevy(v: libroxel_instance::Vec3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

/// Mirrors workspace parts onto entities.
///
/// Phase 0 does a full pass each frame, which is fine for a handful of
/// parts. Later phases will drive this from the tree's change journal.
fn sync_parts(
    host: NonSend<ScriptHost>,
    mut entities: ResMut<PartEntities>,
    mut commands: Commands,
    mut parts: Query<(&mut Transform, &mut PartAppearance, &mut PartName), With<PartLink>>,
) {
    let mut tree = host.runtime.tree().borrow_mut();
    tree.drain_changes();

    let live: Vec<InstanceId> = tree
        .descendants(tree.workspace())
        .unwrap_or_default()
        .into_iter()
        .filter(|id| tree.class(*id).is_ok_and(|c| c == ClassKind::Part))
        .collect();

    // Despawn entities whose part left the workspace.
    entities.0.retain(|id, entity| {
        let keep = live.contains(id);
        if !keep {
            commands.entity(*entity).despawn();
        }
        keep
    });

    for id in live {
        let get = |p| tree.get(id, p).ok();
        let (Some(PropertyValue::Vec3(pos)), Some(PropertyValue::Vec3(size))) =
            (get(Property::Position), get(Property::Size))
        else {
            continue;
        };
        let color = match get(Property::Color) {
            Some(PropertyValue::Color3(c)) => Color::srgb(c.r, c.g, c.b),
            _ => Color::WHITE,
        };
        let transparency = match get(Property::Transparency) {
            Some(PropertyValue::Number(t)) => t as f32,
            _ => 0.0,
        };
        let transform = Transform::from_translation(to_bevy(pos)).with_scale(to_bevy(size));
        let appearance = PartAppearance { color, transparency };
        let name = PartName(tree.name(id).unwrap_or_default().to_owned());

        match entities.0.get(&id).and_then(|e| parts.get_mut(*e).ok()) {
            Some((mut t, mut a, mut n)) => {
                // Only write on change so Bevy's change detection stays meaningful.
                t.set_if_neq(transform);
                a.set_if_neq(appearance);
                n.set_if_neq(name);
            }
            None if !entities.0.contains_key(&id) => {
                let entity = commands.spawn((PartLink(id), name, transform, appearance)).id();
                entities.0.insert(id, entity);
            }
            None => {} // spawned this frame; commands not applied yet
        }
    }
}
