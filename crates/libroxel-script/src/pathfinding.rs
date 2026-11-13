//! The pathfinding service: paths around walls and up ledges, for
//! characters that scripts walk with `MoveTo`.

use libroxel_instance::pathfind::{Agent, Waypoint, find_path};
use libroxel_instance::{ClassKind, EnumValue, InstanceId, PathStatus, PathWaypointAction};
use mlua::{Lua, MetaMethod, Table, UserData, UserDataFields, UserDataMethods, Value};

use crate::bindings::{LuaEnumItem, LuaVector3, as_vector3, host};
use crate::names::pathfinding as names;

fn runtime_err(message: impl Into<String>) -> mlua::Error {
    mlua::Error::runtime(message.into())
}

pub(crate) struct PathfindingService;

impl UserData for PathfindingService {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method(names::CREATE_PATH, |_, _, params: Option<Table>| {
            let mut agent = Agent::default();
            if let Some(params) = params {
                if let Some(r) = params.get::<Option<f32>>(names::AGENT_RADIUS)? {
                    agent.radius = r.clamp(0.25, 20.0);
                }
                if let Some(h) = params.get::<Option<f32>>(names::AGENT_HEIGHT)? {
                    agent.height = h.clamp(0.5, 50.0);
                }
                if params.get::<Option<bool>>(names::AGENT_CAN_JUMP)? == Some(false) {
                    agent.jump = 0.0;
                }
            }
            Ok(Path { agent, status: PathStatus::NoPath, waypoints: Vec::new() })
        });
    }
}

struct Path {
    agent: Agent,
    status: PathStatus,
    waypoints: Vec<Waypoint>,
}

impl UserData for Path {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get(names::STATUS, |_, this| {
            Ok(LuaEnumItem(EnumValue::PathStatus(this.status)))
        });
        fields.add_meta_field(MetaMethod::Type, names::PATH_TYPE_NAME);
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        crate::lenient::add_service_members(methods, names::PATH_TYPE_NAME);
        methods.add_method_mut(names::COMPUTE_ASYNC, |lua, this, (start, finish): (Value, Value)| {
            let start =
                as_vector3(&start).ok_or_else(|| runtime_err("ComputeAsync expects a Vector3 start"))?;
            let finish =
                as_vector3(&finish).ok_or_else(|| runtime_err("ComputeAsync expects a Vector3 finish"))?;
            let found = {
                let host = host(lua);
                let tree = host.tree.borrow();
                // Characters (anything with a humanoid) are walked round by
                // themselves, not avoided by the path.
                let characters: Vec<InstanceId> = tree
                    .descendants_of_class(tree.workspace(), ClassKind::Humanoid)
                    .into_iter()
                    .filter_map(|h| tree.parent(h).ok().flatten())
                    .collect();
                find_path(&tree, start, finish, this.agent, |part| {
                    !characters.iter().any(|c| tree.is_descendant_of(part, *c).unwrap_or(false))
                })
            };
            match found {
                Some(waypoints) => {
                    this.status = PathStatus::Success;
                    this.waypoints = waypoints;
                }
                None => {
                    this.status = PathStatus::NoPath;
                    this.waypoints.clear();
                }
            }
            Ok(())
        });
        methods.add_method(names::GET_WAYPOINTS, |lua, this, ()| {
            let list = lua.create_table()?;
            for (i, w) in this.waypoints.iter().enumerate() {
                list.set(i + 1, PathWaypoint(*w))?;
            }
            Ok(list)
        });
    }
}

struct PathWaypoint(Waypoint);

impl UserData for PathWaypoint {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get(names::POSITION, |_, this| Ok(LuaVector3(this.0.position)));
        fields.add_field_method_get(names::ACTION, |_, this| {
            let action = if this.0.jump { PathWaypointAction::Jump } else { PathWaypointAction::Walk };
            Ok(LuaEnumItem(EnumValue::PathWaypointAction(action)))
        });
        fields.add_field_method_get(names::LABEL, |_, _| Ok(""));
        fields.add_meta_field(MetaMethod::Type, names::WAYPOINT_TYPE_NAME);
    }
}

/// The service, for `game:GetService`.
pub(crate) fn get_service(lua: &Lua, name: &str) -> mlua::Result<Option<Value>> {
    if name == crate::names::services::PATHFINDING_SERVICE {
        return Ok(Some(Value::UserData(lua.create_userdata(PathfindingService)?)));
    }
    Ok(None)
}
