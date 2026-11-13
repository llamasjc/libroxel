//! Services and instance members beyond the core tree: player input, saved
//! data, the clean-up service, raycasts, and remote and bindable events.

use std::collections::BTreeMap;

use libroxel_instance::{
    ClassKind, EnumKind, EnumValue, InstanceId, KeyCode, Property, RaycastFilterType, UserInputType, geometry,
};
use mlua::{
    AnyUserData, Lua, MetaMethod, MultiValue, Table, UserData, UserDataFields, UserDataMethods, Value,
    Variadic,
};

use crate::bindings::{
    LuaEnumItem, LuaInstance, LuaVector3, Signal, SignalKind, as_enum, as_instance, as_vector3, fire, host,
    push_instance, require_class, take_errors,
};
use crate::names::{self, data_store, debris, methods, raycast, services, user_input};
use crate::{ScriptError, UserInput};

/// Saved data: store name, then key, then value.
pub(crate) type DataStores = BTreeMap<String, BTreeMap<String, serde_json::Value>>;

fn runtime_err(message: impl Into<String>) -> mlua::Error {
    mlua::Error::runtime(message.into())
}

/// The service userdata for `name`, if it is one of the services in this module.
pub(crate) fn get_service(lua: &Lua, name: &str) -> mlua::Result<Option<Value>> {
    Ok(Some(Value::UserData(match name {
        services::USER_INPUT_SERVICE => lua.create_userdata(UserInputService)?,
        services::DATA_STORE_SERVICE => lua.create_userdata(DataStoreService)?,
        services::DEBRIS => lua.create_userdata(Debris)?,
        services::COLLECTION_SERVICE => lua.create_userdata(crate::items::CollectionService)?,
        _ => return Ok(None),
    })))
}

pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    let lib = lua.create_table()?;
    lib.set(
        names::constructors::NEW,
        lua.create_function(|_, ()| {
            Ok(RaycastParams { filter: Vec::new(), filter_type: RaycastFilterType::Exclude })
        })?,
    )?;
    lib.set_readonly(true);
    lua.globals().set(names::globals::RAYCAST_PARAMS, lib)?;
    let lib = lua.create_table()?;
    lib.set(names::constructors::NEW, lua.create_function(|_, ()| Ok(OverlapParams::default()))?)?;
    lib.set_readonly(true);
    lua.globals().set(names::globals::OVERLAP_PARAMS, lib)
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

struct UserInputService;

impl UserData for UserInputService {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        use names::events::*;
        fields.add_field_method_get(INPUT_BEGAN, |_, _| Ok(Signal(SignalKind::InputBegan)));
        fields.add_field_method_get(INPUT_ENDED, |_, _| Ok(Signal(SignalKind::InputEnded)));
        fields.add_field_method_get(user_input::TOUCH_ENABLED, |lua, _| {
            Ok(host(lua).world.borrow().devices.touch)
        });
        fields.add_field_method_get(user_input::GAMEPAD_ENABLED, |lua, _| {
            Ok(host(lua).world.borrow().devices.gamepad)
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method(user_input::IS_KEY_DOWN, |lua, _, key: Value| {
            let Some(EnumValue::KeyCode(key)) = as_enum(EnumKind::KeyCode, &key) else {
                return Err(runtime_err("IsKeyDown expects a KeyCode"));
            };
            Ok(host(lua).inputs_down.borrow().contains(&UserInput::Key(key)))
        });
        methods.add_method(user_input::IS_MOUSE_BUTTON_PRESSED, |lua, _, button: Value| {
            let input = match as_enum(EnumKind::UserInputType, &button) {
                Some(EnumValue::UserInputType(UserInputType::MouseButton1)) => UserInput::MouseButton1,
                Some(EnumValue::UserInputType(UserInputType::MouseButton2)) => UserInput::MouseButton2,
                _ => return Ok(false),
            };
            Ok(host(lua).inputs_down.borrow().contains(&input))
        });
        methods.add_method(user_input::GET_LAST_INPUT_TYPE, |lua, _, ()| {
            Ok(LuaEnumItem(EnumValue::UserInputType(host(lua).world.borrow().devices.last)))
        });
        crate::lenient::add_service_members(methods, services::USER_INPUT_SERVICE);
    }
}

/// What an input event passes to scripts.
/// A key or button, and whether it just went down.
struct InputObject(UserInput, bool);

impl UserData for InputObject {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get(user_input::KEY_CODE, |_, this| {
            let key = match this.0 {
                UserInput::Key(k) => k,
                _ => KeyCode::Unknown,
            };
            Ok(LuaEnumItem(EnumValue::KeyCode(key)))
        });
        fields.add_field_method_get(names::compat::USER_INPUT_STATE, |_, this| {
            Ok(crate::compat::input_state(this.1))
        });
        fields.add_field_method_get(user_input::USER_INPUT_TYPE, |_, this| {
            let kind = match this.0 {
                UserInput::Key(_) => UserInputType::Keyboard,
                UserInput::MouseButton1 => UserInputType::MouseButton1,
                UserInput::MouseButton2 => UserInputType::MouseButton2,
            };
            Ok(LuaEnumItem(EnumValue::UserInputType(kind)))
        });
        fields.add_meta_field(MetaMethod::Type, user_input::INPUT_OBJECT_TYPE_NAME);
    }
}

/// A key or button went down (`began`) or came up. `processed` is true when
/// the editor's or game's own UI used it, such as a click on a GUI button.
pub(crate) fn input(lua: &Lua, input: UserInput, began: bool, processed: bool) -> Vec<ScriptError> {
    {
        let host = host(lua);
        let mut down = host.inputs_down.borrow_mut();
        if began {
            down.insert(input);
        } else {
            down.remove(&input);
        }
    }
    let signal = if began { SignalKind::InputBegan } else { SignalKind::InputEnded };
    fire(lua, signal, || {
        let object = Value::UserData(lua.create_userdata(InputObject(input, began))?);
        Ok([object, Value::Boolean(processed)].into_iter().collect())
    });
    if !processed {
        let object = lua.create_userdata(InputObject(input, began)).map(Value::UserData);
        if let Err(err) = object.and_then(|o| crate::compat::action_input(lua, input, began, &o)) {
            host(lua).errors.borrow_mut().push(ScriptError {
                script: names::compat::CONTEXT_ACTION_SERVICE.to_owned(),
                message: err.to_string(),
            });
        }
    }
    if input == UserInput::MouseButton1 && !processed {
        crate::items::mouse_button(lua, began);
    }
    take_errors(lua)
}

// ---------------------------------------------------------------------------
// Saved data
// ---------------------------------------------------------------------------

struct DataStoreService;

impl UserData for DataStoreService {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method(data_store::GET_DATA_STORE, |_, _, name: String| {
            check_name("data store name", &name)?;
            Ok(DataStore(name))
        });
        crate::lenient::add_service_members(methods, services::DATA_STORE_SERVICE);
    }
}

struct DataStore(String);

fn check_name(what: &str, name: &str) -> mlua::Result<()> {
    if name.is_empty() || name.len() > data_store::MAX_NAME_LEN {
        return Err(runtime_err(format!("a {what} must be 1 to {} characters", data_store::MAX_NAME_LEN)));
    }
    Ok(())
}

impl DataStore {
    fn get(&self, lua: &Lua, key: &str) -> Option<serde_json::Value> {
        host(lua).data.borrow().get(&self.0).and_then(|s| s.get(key)).cloned()
    }

    fn set(&self, lua: &Lua, key: &str, value: Option<serde_json::Value>) -> Option<serde_json::Value> {
        let host = host(lua);
        host.data_changed.set(true);
        let mut data = host.data.borrow_mut();
        let store = data.entry(self.0.clone()).or_default();
        let old = match value {
            Some(v) => store.insert(key.to_owned(), v),
            None => store.remove(key),
        };
        if store.is_empty() {
            data.remove(&self.0);
        }
        old
    }
}

impl UserData for DataStore {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, data_store::TYPE_NAME);
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method(data_store::GET_ASYNC, |lua, this, key: String| {
            check_name("key", &key)?;
            this.get(lua, &key).map(|v| from_json(lua, &v)).transpose().map(|v| v.unwrap_or(Value::Nil))
        });
        methods.add_method(data_store::SET_ASYNC, |lua, this, (key, value): (String, Value)| {
            check_name("key", &key)?;
            let json = to_json(&value, 0)?;
            this.set(lua, &key, (!json.is_null()).then_some(json));
            Ok(())
        });
        methods.add_method(
            data_store::UPDATE_ASYNC,
            |lua, this, (key, transform): (String, mlua::Function)| {
                check_name("key", &key)?;
                let old = match this.get(lua, &key) {
                    Some(v) => from_json(lua, &v)?,
                    None => Value::Nil,
                };
                let new: Value = transform.call(old)?;
                // Returning nil leaves the value as it was.
                if new.is_nil() {
                    return this
                        .get(lua, &key)
                        .map(|v| from_json(lua, &v))
                        .transpose()
                        .map(|v| v.unwrap_or(Value::Nil));
                }
                let json = to_json(&new, 0)?;
                this.set(lua, &key, Some(json));
                Ok(new)
            },
        );
        methods.add_method(data_store::INCREMENT_ASYNC, |lua, this, (key, delta): (String, Option<f64>)| {
            check_name("key", &key)?;
            let current = match this.get(lua, &key) {
                None => 0.0,
                Some(serde_json::Value::Number(n)) => n.as_f64().unwrap_or(0.0),
                Some(_) => return Err(runtime_err(format!("{key} does not hold a number"))),
            };
            let new = (current + delta.unwrap_or(1.0)).round();
            this.set(lua, &key, Some(json_number(new)?));
            Ok(new)
        });
        methods.add_method(data_store::REMOVE_ASYNC, |lua, this, key: String| {
            check_name("key", &key)?;
            match this.set(lua, &key, None) {
                Some(old) => from_json(lua, &old),
                None => Ok(Value::Nil),
            }
        });
    }
}

fn json_number(n: f64) -> mlua::Result<serde_json::Value> {
    // Whole numbers are saved without a fraction, so files stay readable.
    if n.fract() == 0.0 && n.abs() < 9.0e15 {
        return Ok(serde_json::Value::from(n as i64));
    }
    serde_json::Number::from_f64(n)
        .map(serde_json::Value::Number)
        .ok_or_else(|| runtime_err("saved numbers must be finite"))
}

/// Converts a script value to saved data. Only nil, booleans, numbers,
/// strings and tables of those can be saved.
pub(crate) fn to_json(value: &Value, depth: usize) -> mlua::Result<serde_json::Value> {
    if depth > 32 {
        return Err(runtime_err("saved tables can't be nested that deeply (or contain themselves)"));
    }
    Ok(match value {
        Value::Nil => serde_json::Value::Null,
        Value::Boolean(b) => serde_json::Value::Bool(*b),
        Value::Integer(n) => json_number(*n as f64)?,
        Value::Number(n) => json_number(*n)?,
        Value::String(s) => serde_json::Value::String(s.to_str()?.to_owned()),
        Value::Table(t) => table_to_json(t, depth)?,
        other => return Err(runtime_err(format!("a {} can't be saved", other.type_name()))),
    })
}

fn table_to_json(table: &Table, depth: usize) -> mlua::Result<serde_json::Value> {
    let len = table.raw_len();
    let pairs: Vec<(Value, Value)> = table.pairs::<Value, Value>().collect::<mlua::Result<_>>()?;
    if pairs.len() == len {
        // An array: keys 1..len.
        let items: mlua::Result<Vec<_>> =
            (1..=len).map(|i| to_json(&table.raw_get::<Value>(i)?, depth + 1)).collect();
        return Ok(serde_json::Value::Array(items?));
    }
    let mut map = serde_json::Map::new();
    for (k, v) in pairs {
        let Value::String(k) = k else {
            return Err(runtime_err("saved tables must be lists or have only string keys"));
        };
        map.insert(k.to_str()?.to_owned(), to_json(&v, depth + 1)?);
    }
    Ok(serde_json::Value::Object(map))
}

pub(crate) fn from_json(lua: &Lua, value: &serde_json::Value) -> mlua::Result<Value> {
    Ok(match value {
        serde_json::Value::Null => Value::Nil,
        serde_json::Value::Bool(b) => Value::Boolean(*b),
        serde_json::Value::Number(n) => Value::Number(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::String(s) => Value::String(lua.create_string(s)?),
        serde_json::Value::Array(items) => {
            let t = lua.create_table()?;
            for (i, item) in items.iter().enumerate() {
                t.raw_set(i + 1, from_json(lua, item)?)?;
            }
            Value::Table(t)
        }
        serde_json::Value::Object(map) => {
            let t = lua.create_table()?;
            for (k, v) in map {
                t.raw_set(k.as_str(), from_json(lua, v)?)?;
            }
            Value::Table(t)
        }
    })
}

/// All saved data as JSON.
pub(crate) fn save_data(lua: &Lua) -> String {
    let host = host(lua);
    host.data_changed.set(false);
    serde_json::to_string_pretty(&*host.data.borrow()).unwrap_or_else(|_| "{}".into())
}

/// Replaces all saved data with `json` (as written by [`save_data`]).
pub(crate) fn load_data(lua: &Lua, json: &str) -> Result<(), String> {
    let data: DataStores = serde_json::from_str(json).map_err(|e| e.to_string())?;
    *host(lua).data.borrow_mut() = data;
    Ok(())
}

// ---------------------------------------------------------------------------
// Debris
// ---------------------------------------------------------------------------

struct Debris;

impl UserData for Debris {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method(debris::ADD_ITEM, |lua, _, (item, lifetime): (Value, Option<f64>)| {
            let id = as_instance(&item).ok_or_else(|| runtime_err("AddItem expects an Instance"))?;
            let host = host(lua);
            let at = host.clock.get() + lifetime.unwrap_or(10.0).max(0.0);
            host.debris.borrow_mut().push((id, at));
            Ok(())
        });
    }
}

/// Destroys every debris item whose time is up.
pub(crate) fn step_debris(lua: &Lua, now: f64) {
    let host = host(lua);
    let due: Vec<InstanceId> = {
        let mut list = host.debris.borrow_mut();
        let (due, later): (Vec<_>, Vec<_>) =
            std::mem::take(&mut *list).into_iter().partition(|(_, at)| *at <= now);
        *list = later;
        due.into_iter().map(|(id, _)| id).collect()
    };
    let mut tree = host.tree.borrow_mut();
    for id in due {
        if tree.exists(id) && !tree.is_destroyed(id).unwrap_or(true) {
            let _ = tree.destroy(id);
        }
    }
}

// ---------------------------------------------------------------------------
// Raycasts
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct RaycastParams {
    filter: Vec<InstanceId>,
    filter_type: RaycastFilterType,
}

impl UserData for RaycastParams {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get(raycast::FILTER_DESCENDANTS_INSTANCES, |lua, this| {
            this.filter.iter().map(|id| push_instance(lua, *id)).collect::<mlua::Result<Vec<_>>>()
        });
        fields.add_field_method_set(raycast::FILTER_DESCENDANTS_INSTANCES, |_, this, list: Table| {
            this.filter = list
                .sequence_values::<Value>()
                .map(|v| v.map(|v| as_instance(&v)))
                .collect::<mlua::Result<Vec<_>>>()?
                .into_iter()
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| runtime_err("FilterDescendantsInstances must be a list of Instances"))?;
            Ok(())
        });
        fields.add_field_method_get(raycast::FILTER_TYPE, |_, this| {
            Ok(LuaEnumItem(EnumValue::RaycastFilterType(this.filter_type)))
        });
        fields.add_field_method_set(raycast::FILTER_TYPE, |_, this, value: Value| {
            match as_enum(EnumKind::RaycastFilterType, &value) {
                Some(EnumValue::RaycastFilterType(t)) => {
                    this.filter_type = t;
                    Ok(())
                }
                _ => Err(runtime_err("FilterType expects a RaycastFilterType")),
            }
        });
        fields.add_meta_field(MetaMethod::Type, raycast::PARAMS_TYPE_NAME);
    }
}

/// What to look for in an overlap query: like raycast parameters, plus
/// the most parts to return (0 for no limit).
#[derive(Clone)]
struct OverlapParams {
    filter: Vec<InstanceId>,
    filter_type: RaycastFilterType,
    max_parts: usize,
}

impl Default for OverlapParams {
    fn default() -> Self {
        Self { filter: Vec::new(), filter_type: RaycastFilterType::Exclude, max_parts: 0 }
    }
}

impl UserData for OverlapParams {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get(raycast::FILTER_DESCENDANTS_INSTANCES, |lua, this| {
            this.filter.iter().map(|id| push_instance(lua, *id)).collect::<mlua::Result<Vec<_>>>()
        });
        fields.add_field_method_set(raycast::FILTER_DESCENDANTS_INSTANCES, |_, this, list: Table| {
            this.filter = instance_list(list)?;
            Ok(())
        });
        fields.add_field_method_get(raycast::FILTER_TYPE, |_, this| {
            Ok(LuaEnumItem(EnumValue::RaycastFilterType(this.filter_type)))
        });
        fields.add_field_method_set(raycast::FILTER_TYPE, |_, this, value: Value| {
            match as_enum(EnumKind::RaycastFilterType, &value) {
                Some(EnumValue::RaycastFilterType(t)) => {
                    this.filter_type = t;
                    Ok(())
                }
                _ => Err(runtime_err("FilterType expects a RaycastFilterType")),
            }
        });
        fields.add_field_method_get(names::overlap::MAX_PARTS, |_, this| Ok(this.max_parts));
        fields.add_field_method_set(names::overlap::MAX_PARTS, |_, this, n: f64| {
            this.max_parts = n.max(0.0) as usize;
            Ok(())
        });
        fields.add_meta_field(MetaMethod::Type, names::overlap::PARAMS_TYPE_NAME);
    }
}

fn instance_list(list: Table) -> mlua::Result<Vec<InstanceId>> {
    list.sequence_values::<Value>()
        .map(|v| v.map(|v| as_instance(&v)))
        .collect::<mlua::Result<Vec<_>>>()?
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| runtime_err("FilterDescendantsInstances must be a list of Instances"))
}

/// Reads overlap parameters (raycast ones work too).
fn overlap_params(params: Option<Value>, member: &str) -> mlua::Result<OverlapParams> {
    match &params {
        None | Some(Value::Nil) => Ok(OverlapParams::default()),
        Some(Value::UserData(ud)) => {
            if let Ok(p) = ud.borrow::<OverlapParams>() {
                return Ok(p.clone());
            }
            if let Ok(p) = ud.borrow::<RaycastParams>() {
                return Ok(OverlapParams {
                    filter: p.filter.clone(),
                    filter_type: p.filter_type,
                    max_parts: 0,
                });
            }
            Err(runtime_err(format!("{member} expects OverlapParams")))
        }
        Some(_) => Err(runtime_err(format!("{member} expects OverlapParams"))),
    }
}

/// The parts overlapping `region` that `params` lets through, leaving out `skip`.
fn query(
    lua: &Lua,
    region: geometry::Region,
    params: &OverlapParams,
    skip: Option<InstanceId>,
) -> mlua::Result<Vec<AnyUserData>> {
    let found = {
        let host = host(lua);
        let tree = host.tree.borrow();
        let listed = |part: InstanceId| {
            params.filter.iter().any(|f| *f == part || tree.is_descendant_of(part, *f).unwrap_or(false))
        };
        let limit = if params.max_parts == 0 { usize::MAX } else { params.max_parts };
        geometry::parts_in(&tree, region, limit, |part| {
            Some(part) != skip
                && tree.get_bool(part, Property::CanQuery) != Some(false)
                && match params.filter_type {
                    RaycastFilterType::Exclude => !listed(part),
                    RaycastFilterType::Include => listed(part),
                }
        })
    };
    found.into_iter().map(|id| push_instance(lua, id)).collect()
}

/// How far apart two parts can be and still count as touching.
const TOUCH_MARGIN: f32 = 0.05;

struct RaycastResult {
    hit: geometry::RayHit,
    material: Option<EnumValue>,
}

impl UserData for RaycastResult {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get(raycast::INSTANCE, |lua, this| push_instance(lua, this.hit.part));
        fields.add_field_method_get(raycast::POSITION, |_, this| Ok(LuaVector3(this.hit.position)));
        fields.add_field_method_get(raycast::NORMAL, |_, this| Ok(LuaVector3(this.hit.normal)));
        fields.add_field_method_get(raycast::DISTANCE, |_, this| Ok(this.hit.distance));
        fields.add_field_method_get(raycast::MATERIAL, |_, this| Ok(this.material.map(LuaEnumItem)));
        fields.add_meta_field(MetaMethod::Type, raycast::RESULT_TYPE_NAME);
    }
}

fn workspace_raycast(
    lua: &Lua,
    origin: Value,
    direction: Value,
    params: Option<Value>,
) -> mlua::Result<Value> {
    let origin = as_vector3(&origin).ok_or_else(|| runtime_err("Raycast expects a Vector3 origin"))?;
    let direction =
        as_vector3(&direction).ok_or_else(|| runtime_err("Raycast expects a Vector3 direction"))?;
    let params = match &params {
        None | Some(Value::Nil) => {
            RaycastParams { filter: Vec::new(), filter_type: RaycastFilterType::Exclude }
        }
        Some(Value::UserData(ud)) => {
            ud.borrow::<RaycastParams>().map_err(|_| runtime_err("Raycast expects RaycastParams"))?.clone()
        }
        Some(_) => return Err(runtime_err("Raycast expects RaycastParams")),
    };
    let hit = {
        let host = host(lua);
        let tree = host.tree.borrow();
        let listed = |part: InstanceId| {
            params.filter.iter().any(|f| *f == part || tree.is_descendant_of(part, *f).unwrap_or(false))
        };
        let hit = geometry::raycast(&tree, origin, direction, |part| {
            tree.get_bool(part, Property::CanQuery) != Some(false)
                && match params.filter_type {
                    RaycastFilterType::Exclude => !listed(part),
                    RaycastFilterType::Include => listed(part),
                }
        });
        hit.map(|hit| {
            let material = match tree.voxels(hit.part) {
                // Terrain: the material of the cell the ray went into.
                Ok(v) if !v.is_empty() => {
                    let (x, y, z) = libroxel_instance::voxels::cell_at(hit.position - hit.normal * 0.5);
                    v.get(x, y, z).map(libroxel_instance::EnumValue::Material)
                }
                _ => tree.get_enum(hit.part, Property::Material),
            };
            RaycastResult { hit, material }
        })
    };
    match hit {
        Some(result) => Ok(Value::UserData(lua.create_userdata(result)?)),
        None => Ok(Value::Nil),
    }
}

// ---------------------------------------------------------------------------
// Remote and bindable events
// ---------------------------------------------------------------------------

/// The player on this computer: the one the engine named, else (in a game
/// on one computer) the first one in the players service.
pub(crate) fn local_player(lua: &Lua) -> Option<InstanceId> {
    let host = host(lua);
    {
        let net = host.net.borrow();
        if net.local_player.is_some() || net.role == crate::net::NetRole::Server {
            return net.local_player;
        }
    }
    let tree = host.tree.borrow();
    let players = tree.service(ClassKind::Players)?;
    tree.children(players).ok()?.iter().copied().find(|c| tree.class(*c) == Ok(ClassKind::Player))
}

pub(crate) fn fire_args(lua: &Lua, signal: SignalKind, first: Option<InstanceId>, args: MultiValue) {
    fire(lua, signal, || {
        let mut all = MultiValue::new();
        if let Some(first) = first {
            all.push_back(Value::UserData(push_instance(lua, first)?));
        }
        all.extend(args.iter().cloned());
        Ok(all)
    });
}

/// Members that only some instance classes have, added to the instance userdata.
pub(crate) fn add_instance_methods<M: UserDataMethods<LuaInstance>>(m: &mut M) {
    m.add_method(
        methods::RAYCAST,
        |lua, this, (origin, direction, params): (Value, Value, Option<Value>)| {
            require_class(lua, this.0, ClassKind::Workspace, methods::RAYCAST)?;
            workspace_raycast(lua, origin, direction, params)
        },
    );
    m.add_method(
        methods::GET_PART_BOUNDS_IN_BOX,
        |lua, this, (cframe, size, params): (Value, Value, Option<Value>)| {
            let member = methods::GET_PART_BOUNDS_IN_BOX;
            require_class(lua, this.0, ClassKind::Workspace, member)?;
            let cframe = crate::types::as_cframe(&cframe)
                .ok_or_else(|| runtime_err(format!("{member} expects a CFrame")))?;
            let size =
                as_vector3(&size).ok_or_else(|| runtime_err(format!("{member} expects a Vector3 size")))?;
            let region = geometry::Region::Box { center: cframe.pos, rot: cframe.rot, half: size / 2.0 };
            query(lua, region, &overlap_params(params, member)?, None)
        },
    );
    m.add_method(
        methods::GET_PART_BOUNDS_IN_RADIUS,
        |lua, this, (center, radius, params): (Value, f32, Option<Value>)| {
            let member = methods::GET_PART_BOUNDS_IN_RADIUS;
            require_class(lua, this.0, ClassKind::Workspace, member)?;
            let center = as_vector3(&center)
                .ok_or_else(|| runtime_err(format!("{member} expects a Vector3 position")))?;
            let region = geometry::Region::Ball { center, radius: radius.max(0.0) };
            query(lua, region, &overlap_params(params, member)?, None)
        },
    );
    m.add_method(methods::GET_PARTS_IN_PART, |lua, this, (part, params): (Value, Option<Value>)| {
        let member = methods::GET_PARTS_IN_PART;
        require_class(lua, this.0, ClassKind::Workspace, member)?;
        let part = as_instance(&part).ok_or_else(|| runtime_err(format!("{member} expects a part")))?;
        let region = geometry::Region::of_part(&host(lua).tree.borrow(), part)
            .ok_or_else(|| runtime_err(format!("{member} expects a part")))?;
        query(lua, region, &overlap_params(params, member)?, Some(part))
    });
    m.add_method(methods::GET_TOUCHING_PARTS, |lua, this, ()| {
        require_class(lua, this.0, ClassKind::Part, methods::GET_TOUCHING_PARTS)?;
        let region = geometry::Region::of_part(&host(lua).tree.borrow(), this.0)
            .ok_or_else(|| runtime_err("GetTouchingParts needs a part with a size"))?;
        query(lua, region.grown(TOUCH_MARGIN), &OverlapParams::default(), Some(this.0))
    });
    m.add_method(methods::FIRE_SERVER, |lua, this, args: MultiValue| {
        require_class(lua, this.0, ClassKind::RemoteEvent, methods::FIRE_SERVER)?;
        if crate::net::role(lua) == crate::net::NetRole::Client {
            let args = crate::net::encode_args(&args)?;
            crate::net::send(lua, crate::net::NetMessage::ServerEvent { remote: this.0, args });
            return Ok(());
        }
        let player = local_player(lua).ok_or_else(|| runtime_err("FireServer needs a player in the game"))?;
        fire_args(lua, SignalKind::ServerEvent(this.0), Some(player), args);
        Ok(())
    });
    m.add_method(methods::FIRE_CLIENT, |lua, this, (player, args): (Value, Variadic<Value>)| {
        require_class(lua, this.0, ClassKind::RemoteEvent, methods::FIRE_CLIENT)?;
        let player = as_instance(&player)
            .filter(|p| host(lua).tree.borrow().class(*p) == Ok(ClassKind::Player))
            .ok_or_else(|| runtime_err("FireClient expects a Player first"))?;
        use crate::net::{NetMessage, NetRole, role};
        if role(lua) == NetRole::Client {
            return Err(runtime_err("FireClient can only be used by the server"));
        }
        let args: MultiValue = args.into_iter().collect();
        // Local scripts here belong to the local player; other players are elsewhere.
        if Some(player) == local_player(lua) {
            fire_args(lua, SignalKind::ClientEvent(this.0), None, args);
        } else if role(lua) == NetRole::Server {
            let args = crate::net::encode_args(&args)?;
            crate::net::send(lua, NetMessage::ClientEvent { remote: this.0, player: Some(player), args });
        }
        Ok(())
    });
    m.add_method(methods::FIRE_ALL_CLIENTS, |lua, this, args: MultiValue| {
        require_class(lua, this.0, ClassKind::RemoteEvent, methods::FIRE_ALL_CLIENTS)?;
        use crate::net::{NetMessage, NetRole, role};
        match role(lua) {
            NetRole::Client => return Err(runtime_err("FireAllClients can only be used by the server")),
            NetRole::Server => {
                let json = crate::net::encode_args(&args)?;
                crate::net::send(lua, NetMessage::ClientEvent { remote: this.0, player: None, args: json });
                if local_player(lua).is_some() {
                    fire_args(lua, SignalKind::ClientEvent(this.0), None, args);
                }
            }
            NetRole::Solo => fire_args(lua, SignalKind::ClientEvent(this.0), None, args),
        }
        Ok(())
    });
    m.add_method(methods::FIRE, |lua, this, args: MultiValue| {
        require_class(lua, this.0, ClassKind::BindableEvent, methods::FIRE)?;
        fire_args(lua, SignalKind::BindableEvent(this.0), None, args);
        Ok(())
    });
}

/// Fires a proximity prompt's `Triggered` (or, when `ended`, `TriggerEnded`) with the player.
pub(crate) fn prompt(lua: &Lua, prompt: InstanceId, player: InstanceId, ended: bool) -> Vec<ScriptError> {
    let signal =
        if ended { SignalKind::PromptTriggerEnded(prompt) } else { SignalKind::PromptTriggered(prompt) };
    fire_args(lua, signal, Some(player), MultiValue::new());
    take_errors(lua)
}
