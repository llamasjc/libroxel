//! The camera, part velocities, text box focus and explosions: things that
//! scripts reach through the tree but that the engine owns.

use std::collections::HashMap;

use libroxel_instance::{ClassKind, InstanceId, Property, PropertyValue, Vec2, Vec3};
use mlua::{Lua, MultiValue, UserDataMethods, Value};

use crate::ScriptError;
use crate::bindings::{
    LuaInstance, LuaVector3, SignalKind, as_vector3, fire, host, part_cframe, push_instance, require_class,
    take_errors, tree_err,
};
use crate::names::{extras, methods};
use crate::types::LuaVector2;

/// A change to a part's velocity that a script asked for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VelocityRequest {
    /// Set the velocity outright.
    Set(Vec3),
    /// Add to the velocity (already divided by the part's mass).
    Add(Vec3),
}

/// Engine-owned state that scripts can read and ask to change.
#[derive(Debug, Default)]
pub(crate) struct WorldState {
    /// The velocity of every moving part, as of the last physics step.
    velocities: HashMap<InstanceId, Vec3>,
    requests: Vec<(InstanceId, VelocityRequest)>,
    /// The text box the player is typing into.
    focused: Option<InstanceId>,
    /// The 3D view's size in pixels.
    viewport: Vec2,
}

fn runtime_err(message: impl Into<String>) -> mlua::Error {
    mlua::Error::runtime(message.into())
}

/// A part's mass: its volume times the density.
fn mass(lua: &Lua, id: InstanceId) -> f32 {
    let size = host(lua).tree.borrow().get_vec3(id, Property::Size).unwrap_or_default();
    (size.x * size.y * size.z * extras::DENSITY).max(0.01)
}

fn velocity(lua: &Lua, id: InstanceId) -> Vec3 {
    let host = host(lua);
    let world = host.world.borrow();
    let mut v = world.velocities.get(&id).copied().unwrap_or_default();
    // Requests not yet applied are already visible to scripts.
    for (target, request) in &world.requests {
        if *target == id {
            match request {
                VelocityRequest::Set(set) => v = *set,
                VelocityRequest::Add(add) => v = v + *add,
            }
        }
    }
    v
}

fn request(lua: &Lua, id: InstanceId, request: VelocityRequest) {
    host(lua).world.borrow_mut().requests.push((id, request));
}

/// The workspace's camera, made if there isn't one yet.
fn current_camera(lua: &Lua) -> mlua::Result<InstanceId> {
    let host = host(lua);
    let mut tree = host.tree.borrow_mut();
    let ws = tree.workspace();
    if let Some(cam) = tree.find_first_child_of_class(ws, ClassKind::Camera).map_err(tree_err)? {
        return Ok(cam);
    }
    let cam = tree.create(ClassKind::Camera, extras::CAMERA_NAME).map_err(tree_err)?;
    tree.set_parent(cam, Some(ws)).map_err(tree_err)?;
    Ok(cam)
}

/// Members beyond the saved properties: `Some` when `key` is one of them.
pub(crate) fn index(lua: &Lua, id: InstanceId, class: ClassKind, key: &str) -> mlua::Result<Option<Value>> {
    Ok(Some(match key {
        extras::CURRENT_CAMERA if class == ClassKind::Workspace => {
            Value::UserData(push_instance(lua, current_camera(lua)?)?)
        }
        extras::ASSEMBLY_LINEAR_VELOCITY if class.is_physical() => {
            Value::UserData(lua.create_userdata(LuaVector3(velocity(lua, id)))?)
        }
        extras::VIEWPORT_SIZE if class == ClassKind::Camera => {
            let size = host(lua).world.borrow().viewport;
            Value::UserData(lua.create_userdata(LuaVector2(size))?)
        }
        _ => return Ok(None),
    }))
}

/// Sets a member beyond the saved properties. Returns whether `key` was one.
pub(crate) fn new_index(
    lua: &Lua,
    id: InstanceId,
    class: ClassKind,
    key: &str,
    value: &Value,
) -> mlua::Result<bool> {
    match key {
        extras::ASSEMBLY_LINEAR_VELOCITY if class.is_physical() => {
            let v = as_vector3(value).ok_or_else(|| runtime_err(format!("{key} expects a Vector3")))?;
            request(lua, id, VelocityRequest::Set(v));
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// Where `point` shows on a view of `size` pixels through the camera:
/// pixels from the top-left, then depth in studs, and whether it is in view.
fn project(lua: &Lua, camera: InstanceId, point: Vec3) -> mlua::Result<(Vec3, bool)> {
    let cf = part_cframe(lua, camera)?;
    let fov = host(lua).tree.borrow().get_number(camera, Property::FieldOfView).unwrap_or(70.0) as f32;
    let size = host(lua).world.borrow().viewport;
    let local = cf.inverse().point_to_world(point);
    let depth = -local.z;
    if depth <= 1e-4 {
        return Ok((Vec3::new(0.0, 0.0, depth), false));
    }
    let focal = (size.y / 2.0) / (fov.to_radians() / 2.0).tan();
    let x = size.x / 2.0 + focal * local.x / depth;
    let y = size.y / 2.0 - focal * local.y / depth;
    let on_screen = (0.0..=size.x).contains(&x) && (0.0..=size.y).contains(&y);
    Ok((Vec3::new(x, y, depth), on_screen))
}

pub(crate) fn add_instance_methods<M: UserDataMethods<LuaInstance>>(m: &mut M) {
    m.add_method(methods::GET_MASS, |lua, this, ()| {
        require_class(lua, this.0, ClassKind::Part, methods::GET_MASS)?;
        Ok(mass(lua, this.0))
    });
    m.add_method(methods::APPLY_IMPULSE, |lua, this, impulse: Value| {
        require_class(lua, this.0, ClassKind::Part, methods::APPLY_IMPULSE)?;
        let impulse = as_vector3(&impulse).ok_or_else(|| runtime_err("ApplyImpulse expects a Vector3"))?;
        let m = mass(lua, this.0);
        request(lua, this.0, VelocityRequest::Add(impulse / m));
        Ok(())
    });
    for name in [methods::WORLD_TO_VIEWPORT_POINT, methods::WORLD_TO_SCREEN_POINT] {
        m.add_method(name, move |lua, this, point: Value| {
            require_class(lua, this.0, ClassKind::Camera, name)?;
            let point = as_vector3(&point).ok_or_else(|| runtime_err(format!("{name} expects a Vector3")))?;
            let (screen, visible) = project(lua, this.0, point)?;
            Ok((LuaVector3(screen), visible))
        });
    }
    m.add_method(methods::CAPTURE_FOCUS, |lua, this, ()| {
        require_class(lua, this.0, ClassKind::TextBox, methods::CAPTURE_FOCUS)?;
        focus(lua, this.0);
        Ok(())
    });
    m.add_method(methods::RELEASE_FOCUS, |lua, this, enter: Option<bool>| {
        require_class(lua, this.0, ClassKind::TextBox, methods::RELEASE_FOCUS)?;
        if host(lua).world.borrow().focused == Some(this.0) {
            release(lua, enter.unwrap_or(false));
        }
        Ok(())
    });
    m.add_method(methods::IS_FOCUSED, |lua, this, ()| {
        require_class(lua, this.0, ClassKind::TextBox, methods::IS_FOCUSED)?;
        Ok(host(lua).world.borrow().focused == Some(this.0))
    });
}

/// Starts typing into `text_box`: lets go of any other one, empties it if
/// it clears on focus, and fires `Focused`.
pub(crate) fn focus(lua: &Lua, text_box: InstanceId) {
    let current = host(lua).world.borrow().focused;
    if current == Some(text_box) {
        return;
    }
    if current.is_some() {
        release(lua, false);
    }
    host(lua).world.borrow_mut().focused = Some(text_box);
    {
        let host = host(lua);
        let mut tree = host.tree.borrow_mut();
        if tree.get_bool(text_box, Property::ClearTextOnFocus) == Some(true) {
            let _ = tree.set(text_box, Property::Text, PropertyValue::String(String::new()));
        }
    }
    fire(lua, SignalKind::TextBoxFocused(text_box), || Ok(MultiValue::new()));
}

/// Stops typing, firing `FocusLost(enterPressed)` on the focused text box.
pub(crate) fn release(lua: &Lua, enter: bool) {
    let Some(text_box) = host(lua).world.borrow_mut().focused.take() else { return };
    fire(
        lua,
        SignalKind::TextBoxFocusLost(text_box),
        || Ok(std::iter::once(Value::Boolean(enter)).collect()),
    );
}

// Host entry points.

pub(crate) fn focused_text_box(lua: &Lua) -> Option<InstanceId> {
    let focused = host(lua).world.borrow().focused?;
    let host = host(lua);
    let tree = host.tree.borrow();
    tree.exists(focused).then_some(focused).filter(|id| !tree.is_destroyed(*id).unwrap_or(true))
}

pub(crate) fn set_viewport(lua: &Lua, size: Vec2) {
    host(lua).world.borrow_mut().viewport = size;
}

pub(crate) fn set_velocities(lua: &Lua, velocities: HashMap<InstanceId, Vec3>) {
    host(lua).world.borrow_mut().velocities = velocities;
}

pub(crate) fn take_velocity_requests(lua: &Lua) -> Vec<(InstanceId, VelocityRequest)> {
    std::mem::take(&mut host(lua).world.borrow_mut().requests)
}

pub(crate) fn explosion_hit(
    lua: &Lua,
    explosion: InstanceId,
    part: InstanceId,
    distance: f32,
) -> Vec<ScriptError> {
    fire(lua, SignalKind::ExplosionHit(explosion), || {
        let mut args = MultiValue::new();
        args.push_back(Value::UserData(push_instance(lua, part)?));
        args.push_back(Value::Number(distance as f64));
        Ok(args)
    });
    take_errors(lua)
}

pub(crate) fn text_box_focus(lua: &Lua, text_box: Option<InstanceId>, enter: bool) -> Vec<ScriptError> {
    match text_box {
        Some(id) => focus(lua, id),
        None => release(lua, enter),
    }
    take_errors(lua)
}
