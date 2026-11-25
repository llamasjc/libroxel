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
    lib.set(names::constructors::NEW, lua.create_function(|_, ()| Ok(RaycastParams::default()))?)?;
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
        fields.add_field_method_get(user_input::KEYBOARD_ENABLED, |lua, _| {
            Ok(host(lua).world.borrow().devices.keyboard)
        });
        fields.add_field_method_get(user_input::MOUSE_ENABLED, |lua, _| {
            Ok(host(lua).world.borrow().devices.mouse)
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

/// Keeps an error the lenient layer raised, for the input service.
pub(crate) fn report(lua: &Lua, result: mlua::Result<()>) {
    if let Err(err) = result {
        host(lua)
            .errors
            .borrow_mut()
            .push(ScriptError { script: services::USER_INPUT_SERVICE.to_owned(), message: err.to_string() });
    }
}

/// What an input event passes to scripts.
/// A key or button, and whether it just went down.
pub(crate) struct InputObject(pub UserInput, pub bool);

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
        fields.add_field_method_get(user_input::POSITION, |lua, this| {
            let at = match this.0 {
                UserInput::Key(_) => None,
                _ => host(lua).mouse.get(),
            };
            Ok(LuaVector3(
                at.map_or(libroxel_instance::Vec3::ZERO, |m| libroxel_instance::Vec3::new(m.x, m.y, 0.0)),
            ))
        });
        fields.add_field_method_get(user_input::DELTA, |_, _| Ok(LuaVector3(libroxel_instance::Vec3::ZERO)));
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
        methods.add_method(
            data_store::GET_DATA_STORE,
            |_, _, (name, scope, _options): (String, Option<String>, Value)| {
                check_name("data store name", &name)?;
                let scope = scope.unwrap_or_else(|| data_store::GLOBAL_SCOPE.to_owned());
                check_name("scope", &scope)?;
                Ok(DataStore(store_key(&name, &scope)))
            },
        );
        crate::lenient::add_service_members(methods, services::DATA_STORE_SERVICE);
    }
}

/// Where a store's keys are saved. The global scope keeps the plain name, so
/// data saved before scopes were told apart still loads.
pub(crate) fn store_key(name: &str, scope: &str) -> String {
    if scope == data_store::GLOBAL_SCOPE { name.to_owned() } else { format!("{name}\u{1f}{scope}") }
}

struct DataStore(String);

fn check_name(what: &str, name: &str) -> mlua::Result<()> {
    if name.is_empty() || name.len() > data_store::MAX_NAME_LEN {
        return Err(runtime_err(format!("a {what} must be 1 to {} characters", data_store::MAX_NAME_LEN)));
    }
    Ok(())
}

/// One saved version of a key.
#[derive(Debug, Clone)]
pub(crate) struct KeyVersion {
    id: String,
    /// When it was written, in milliseconds since 1970.
    time: f64,
    /// `None` for the version that removed the key.
    value: Option<serde_json::Value>,
}

/// What is kept about a key besides its value.
#[derive(Debug, Clone, Default)]
pub(crate) struct KeyInfo {
    created: f64,
    user_ids: Vec<f64>,
    metadata: serde_json::Map<String, serde_json::Value>,
    /// Oldest first, at most [`data_store::KEPT_VERSIONS`].
    versions: Vec<KeyVersion>,
}

/// Key information by store and key. It lasts for the session: values are
/// saved, but their history isn't.
pub(crate) type KeyInfos = std::collections::HashMap<(String, String), KeyInfo>;

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

/// A new version name, always later than the last one handed out.
fn next_version() -> String {
    thread_local! {
        static LAST: std::cell::Cell<u128> = const { std::cell::Cell::new(0) };
    }
    let micros = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros())
        .unwrap_or(0);
    let n = LAST.with(|last| {
        let n = micros.max(last.get() + 1);
        last.set(n);
        n
    });
    format!("{n:016X}.0000000001.0001")
}

/// `DataStoreKeyInfo`: a key's version, times, user ids and metadata.
#[derive(Clone)]
struct LuaKeyInfo {
    version: String,
    created: f64,
    updated: f64,
    user_ids: Vec<f64>,
    metadata: serde_json::Map<String, serde_json::Value>,
}

impl LuaKeyInfo {
    fn of(info: &KeyInfo, version: &KeyVersion) -> LuaKeyInfo {
        LuaKeyInfo {
            version: version.id.clone(),
            created: info.created,
            updated: version.time,
            user_ids: info.user_ids.clone(),
            metadata: info.metadata.clone(),
        }
    }
}

impl UserData for LuaKeyInfo {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("Version", |_, this| Ok(this.version.clone()));
        fields.add_field_method_get("CreatedTime", |_, this| Ok(this.created));
        fields.add_field_method_get("UpdatedTime", |_, this| Ok(this.updated));
        fields.add_field_method_get("ClassName", |_, _| Ok(data_store::KEY_INFO));
        fields.add_meta_field(MetaMethod::Type, "Instance");
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("GetUserIds", |_, this, ()| Ok(this.user_ids.clone()));
        methods.add_method("GetMetadata", |lua, this, ()| {
            from_json(lua, &serde_json::Value::Object(this.metadata.clone()))
        });
        methods.add_method("IsA", |_, _, class: String| {
            Ok(class == data_store::KEY_INFO || class == "Instance")
        });
        methods.add_meta_method(MetaMethod::ToString, |_, _, ()| Ok(data_store::KEY_INFO));
    }
}

/// The user ids a script passed: a list of numbers, or nothing.
fn user_ids(value: &Value) -> mlua::Result<Option<Vec<f64>>> {
    match value {
        Value::Nil => Ok(None),
        Value::Table(t) => t
            .sequence_values::<f64>()
            .collect::<mlua::Result<Vec<_>>>()
            .map(Some)
            .map_err(|_| runtime_err("user ids must be a list of numbers")),
        _ => Err(runtime_err("user ids must be a list of numbers")),
    }
}

/// The metadata a `DataStoreSetOptions` (or a plain table) carries.
fn metadata(value: &Value) -> mlua::Result<Option<serde_json::Map<String, serde_json::Value>>> {
    let table = match value {
        Value::Nil => return Ok(None),
        Value::Table(t) => Value::Table(t.clone()),
        Value::UserData(ud) => mlua::ObjectLike::call_method::<Value>(ud, "GetMetadata", ())?,
        _ => return Ok(None),
    };
    match to_json(&table, 0)? {
        serde_json::Value::Object(map) => Ok(Some(map)),
        // An empty table looks like an empty list.
        serde_json::Value::Array(a) if a.is_empty() => Ok(Some(Default::default())),
        serde_json::Value::Null => Ok(None),
        _ => Err(runtime_err("metadata must be a table with string keys")),
    }
}

impl DataStore {
    fn get(&self, lua: &Lua, key: &str) -> Option<serde_json::Value> {
        host(lua).data.borrow().get(&self.0).and_then(|s| s.get(key)).cloned()
    }

    /// The key information for `key`'s current value, if it has one.
    fn info(&self, lua: &Lua, key: &str) -> Option<LuaKeyInfo> {
        self.get(lua, key)?;
        let host = host(lua);
        let mut infos = host.data_info.borrow_mut();
        let info = infos.entry((self.0.clone(), key.to_owned())).or_default();
        if info.versions.is_empty() {
            // Loaded from an earlier session, so its history is unknown.
            let time = now_ms();
            info.created = time;
            info.versions.push(KeyVersion { id: next_version(), time, value: self.get(lua, key) });
        }
        info.versions.last().map(|v| LuaKeyInfo::of(info, v))
    }

    /// Writes (or with `None`, removes) a key, keeping its history. Gives the
    /// old value and the new version's key information.
    fn set(
        &self,
        lua: &Lua,
        key: &str,
        value: Option<serde_json::Value>,
        ids: Option<Vec<f64>>,
        meta: Option<serde_json::Map<String, serde_json::Value>>,
    ) -> mlua::Result<(Option<serde_json::Value>, LuaKeyInfo)> {
        if let Some(v) = &value {
            let size = serde_json::to_string(v).map(|s| s.len()).unwrap_or(0);
            if size > data_store::MAX_VALUE_LEN {
                return Err(runtime_err(format!(
                    "a saved value can be at most {} bytes once encoded (this is {size})",
                    data_store::MAX_VALUE_LEN
                )));
            }
        }
        let host = host(lua);
        host.data_changed.set(true);
        let old = {
            let mut data = host.data.borrow_mut();
            let store = data.entry(self.0.clone()).or_default();
            let old = match value.clone() {
                Some(v) => store.insert(key.to_owned(), v),
                None => store.remove(key),
            };
            if store.is_empty() {
                data.remove(&self.0);
            }
            old
        };
        let mut infos = host.data_info.borrow_mut();
        let info = infos.entry((self.0.clone(), key.to_owned())).or_default();
        let time = now_ms();
        if old.is_none() || info.versions.is_empty() {
            info.created = time;
        }
        if let Some(ids) = ids {
            info.user_ids = ids;
        }
        if let Some(meta) = meta {
            info.metadata = meta;
        }
        info.versions.push(KeyVersion { id: next_version(), time, value });
        if info.versions.len() > data_store::KEPT_VERSIONS {
            info.versions.remove(0);
        }
        let shown = LuaKeyInfo::of(info, info.versions.last().expect("just pushed"));
        Ok((old, shown))
    }

    fn value_and_info(&self, lua: &Lua, key: &str) -> mlua::Result<(Value, Option<LuaKeyInfo>)> {
        match self.get(lua, key) {
            Some(v) => Ok((from_json(lua, &v)?, self.info(lua, key))),
            None => Ok((Value::Nil, None)),
        }
    }

    /// A saved version, picked from the key's history.
    fn version(
        &self,
        lua: &Lua,
        key: &str,
        pick: impl Fn(&[KeyVersion]) -> Option<usize>,
    ) -> mlua::Result<(Value, Option<LuaKeyInfo>)> {
        self.info(lua, key);
        let host = host(lua);
        let infos = host.data_info.borrow();
        let Some(info) = infos.get(&(self.0.clone(), key.to_owned())) else {
            return Ok((Value::Nil, None));
        };
        let Some(v) = pick(&info.versions).map(|i| &info.versions[i]) else {
            return Ok((Value::Nil, None));
        };
        match &v.value {
            Some(value) => Ok((from_json(lua, value)?, Some(LuaKeyInfo::of(info, v)))),
            None => Ok((Value::Nil, None)),
        }
    }
}

impl UserData for DataStore {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, data_store::TYPE_NAME);
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method(data_store::GET_ASYNC, |lua, this, (key, _options): (String, Value)| {
            check_name("key", &key)?;
            this.value_and_info(lua, &key)
        });
        methods.add_method(
            data_store::SET_ASYNC,
            |lua, this, (key, value, ids, options): (String, Value, Value, Value)| {
                check_name("key", &key)?;
                let json = to_json(&value, 0)?;
                let (_, info) = this.set(
                    lua,
                    &key,
                    (!json.is_null()).then_some(json),
                    user_ids(&ids)?,
                    metadata(&options)?,
                )?;
                Ok(info.version)
            },
        );
        methods.add_method(
            data_store::UPDATE_ASYNC,
            |lua, this, (key, transform): (String, mlua::Function)| {
                check_name("key", &key)?;
                let (old, info) = this.value_and_info(lua, &key)?;
                let (new, ids, meta): (Value, Value, Value) = transform.call((old, info))?;
                // Returning nil leaves the value as it was.
                if new.is_nil() {
                    return this.value_and_info(lua, &key);
                }
                let json = to_json(&new, 0)?;
                let (_, info) = this.set(lua, &key, Some(json.clone()), user_ids(&ids)?, metadata(&meta)?)?;
                Ok((from_json(lua, &json)?, Some(info)))
            },
        );
        methods.add_method(
            data_store::INCREMENT_ASYNC,
            |lua, this, (key, delta, ids, _options): (String, Option<f64>, Value, Value)| {
                check_name("key", &key)?;
                let current = match this.get(lua, &key) {
                    None => 0.0,
                    Some(serde_json::Value::Number(n)) => n.as_f64().unwrap_or(0.0),
                    Some(_) => return Err(runtime_err(format!("{key} does not hold a number"))),
                };
                let new = (current + delta.unwrap_or(1.0)).round();
                this.set(lua, &key, Some(json_number(new)?), user_ids(&ids)?, None)?;
                Ok(new)
            },
        );
        methods.add_method(data_store::REMOVE_ASYNC, |lua, this, key: String| {
            check_name("key", &key)?;
            if this.get(lua, &key).is_none() {
                return Ok((Value::Nil, None));
            }
            this.info(lua, &key);
            let (old, info) = this.set(lua, &key, None, None, None)?;
            Ok((old.map(|v| from_json(lua, &v)).transpose()?.unwrap_or(Value::Nil), Some(info)))
        });
        methods.add_method(
            data_store::LIST_KEYS_ASYNC,
            |lua, this, (prefix, page_size): (Option<String>, Option<usize>)| {
                let prefix = prefix.unwrap_or_default();
                let keys: Vec<String> = host(lua)
                    .data
                    .borrow()
                    .get(&this.0)
                    .map(|s| s.keys().filter(|k| k.starts_with(&prefix)).cloned().collect())
                    .unwrap_or_default();
                let rows = lua.create_table()?;
                for key in keys {
                    let row = lua.create_table()?;
                    row.set("KeyName", key)?;
                    rows.push(row)?;
                }
                crate::lenient::list_pages(lua, rows, page_size.unwrap_or(50))
            },
        );
        methods.add_method(
            data_store::LIST_VERSIONS_ASYNC,
            |lua,
             this,
             (key, direction, min, max, page_size): (
                String,
                Value,
                Option<f64>,
                Option<f64>,
                Option<usize>,
            )| {
                check_name("key", &key)?;
                this.info(lua, &key);
                let descending =
                    !direction.is_nil() && direction.to_string().is_ok_and(|s| s.contains("Descending"));
                let rows = lua.create_table()?;
                let mut versions = host(lua)
                    .data_info
                    .borrow()
                    .get(&(this.0.clone(), key))
                    .map(|i| i.versions.clone())
                    .unwrap_or_default();
                if descending {
                    versions.reverse();
                }
                for v in versions {
                    if min.is_some_and(|m| v.time < m) || max.is_some_and(|m| m > 0.0 && v.time > m) {
                        continue;
                    }
                    let row = lua.create_table()?;
                    row.set("Version", v.id)?;
                    row.set("CreatedTime", v.time)?;
                    row.set("IsDeleted", v.value.is_none())?;
                    rows.push(row)?;
                }
                crate::lenient::list_pages(lua, rows, page_size.unwrap_or(50))
            },
        );
        methods.add_method(data_store::GET_VERSION_ASYNC, |lua, this, (key, version): (String, String)| {
            check_name("key", &key)?;
            this.version(lua, &key, |list| list.iter().position(|v| v.id == version))
        });
        methods.add_method(data_store::GET_VERSION_AT_TIME_ASYNC, |lua, this, (key, time): (String, f64)| {
            check_name("key", &key)?;
            this.version(lua, &key, |list| list.iter().rposition(|v| v.time <= time))
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
        for name in [debris::ADD_ITEM, debris::OLD_ADD_ITEM] {
            methods.add_method(name, |lua, _, (item, lifetime): (Value, Option<f64>)| {
                let id = as_instance(&item).ok_or_else(|| runtime_err("AddItem expects an Instance"))?;
                let host = host(lua);
                let at = host.clock.get() + lifetime.unwrap_or(10.0).max(0.0);
                host.debris.borrow_mut().push((id, at));
                Ok(())
            });
        }
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

#[derive(Clone, Default)]
struct RaycastParams {
    filter: Vec<InstanceId>,
    filter_type: RaycastFilterType,
    more: QueryOptions,
}

/// The rest of a query's settings. Water is never hit (terrain water
/// isn't solid), so `IgnoreWater` changes nothing.
#[derive(Clone, Default)]
struct QueryOptions {
    ignore_water: bool,
    respect_can_collide: bool,
    brute_force: bool,
    collision_group: Option<String>,
}

/// Adds the settings in [`QueryOptions`] to a query type's fields.
macro_rules! query_options {
    ($fields:expr) => {
        $fields.add_field_method_get("IgnoreWater", |_, this| Ok(this.more.ignore_water));
        $fields.add_field_method_set("IgnoreWater", |_, this, v: bool| {
            this.more.ignore_water = v;
            Ok(())
        });
        $fields.add_field_method_get("RespectCanCollide", |_, this| Ok(this.more.respect_can_collide));
        $fields.add_field_method_set("RespectCanCollide", |_, this, v: bool| {
            this.more.respect_can_collide = v;
            Ok(())
        });
        $fields.add_field_method_get("BruteForceAllSlow", |_, this| Ok(this.more.brute_force));
        $fields.add_field_method_set("BruteForceAllSlow", |_, this, v: bool| {
            this.more.brute_force = v;
            Ok(())
        });
        $fields.add_field_method_get("CollisionGroup", |_, this| {
            Ok(this.more.collision_group.clone().unwrap_or_else(|| "Default".to_owned()))
        });
        $fields.add_field_method_set("CollisionGroup", |_, this, v: String| {
            this.more.collision_group = Some(v);
            Ok(())
        });
    };
}

/// Adds `AddToFilter` to a query type.
macro_rules! add_to_filter {
    ($methods:expr) => {
        $methods.add_method_mut("AddToFilter", |_, this, value: Value| {
            match &value {
                Value::Table(list) => this.filter.extend(instance_list(list.clone())?),
                other => this
                    .filter
                    .push(as_instance(other).ok_or_else(|| runtime_err("AddToFilter expects Instances"))?),
            }
            Ok(())
        });
    };
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
        query_options!(fields);
        fields.add_meta_field(MetaMethod::Type, raycast::PARAMS_TYPE_NAME);
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        add_to_filter!(methods);
    }
}

/// What to look for in an overlap query: like raycast parameters, plus
/// the most parts to return (0 for no limit).
#[derive(Clone, Default)]
struct OverlapParams {
    filter: Vec<InstanceId>,
    filter_type: RaycastFilterType,
    max_parts: usize,
    more: QueryOptions,
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
        query_options!(fields);
        fields.add_meta_field(MetaMethod::Type, names::overlap::PARAMS_TYPE_NAME);
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        add_to_filter!(methods);
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
                    more: p.more.clone(),
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
    scope: Option<InstanceId>,
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
        let keep = |part: InstanceId| {
            Some(part) != skip
                && tree.get_bool(part, Property::CanQuery) != Some(false)
                && match params.filter_type {
                    RaycastFilterType::Exclude => !listed(part),
                    RaycastFilterType::Include => listed(part),
                }
        };
        match scope {
            None => geometry::parts_in_indexed(
                &tree,
                &crate::bindings::part_index(&host, &tree),
                region,
                limit,
                keep,
            ),
            Some(scope) => tree
                .descendants_of_class(scope, ClassKind::Part)
                .into_iter()
                .filter(|p| {
                    keep(*p) && geometry::Region::of_part(&tree, *p).is_some_and(|r| r.overlaps(region))
                })
                .take(limit)
                .collect(),
        }
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

fn query_scope(lua: &Lua, id: InstanceId, member: &str) -> mlua::Result<Option<InstanceId>> {
    let class = crate::bindings::class_of(lua, id)?;
    if class.is_a(ClassKind::Workspace) {
        return Ok(None);
    }
    if class == ClassKind::Model
        && crate::lenient::stand_in_class(&host(lua).tree.borrow(), id).as_deref() == Some("WorldModel")
    {
        return Ok(Some(id));
    }
    Err(crate::bindings::not_a_member(lua, id, member))
}

fn workspace_raycast(
    lua: &Lua,
    scope: Option<InstanceId>,
    origin: Value,
    direction: Value,
    params: Option<Value>,
) -> mlua::Result<Value> {
    let origin = as_vector3(&origin).ok_or_else(|| runtime_err("Raycast expects a Vector3 origin"))?;
    let direction =
        as_vector3(&direction).ok_or_else(|| runtime_err("Raycast expects a Vector3 direction"))?;
    let params = match &params {
        None | Some(Value::Nil) => RaycastParams::default(),
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
        let keep = |part: InstanceId| {
            tree.get_bool(part, Property::CanQuery) != Some(false)
                && (!params.more.respect_can_collide
                    || tree.get_bool(part, Property::CanCollide) != Some(false))
                && match params.filter_type {
                    RaycastFilterType::Exclude => !listed(part),
                    RaycastFilterType::Include => listed(part),
                }
        };
        let hit = match scope {
            None => geometry::raycast_indexed(
                &tree,
                &crate::bindings::part_index(&host, &tree),
                origin,
                direction,
                keep,
            ),
            Some(scope) => {
                let reach = direction.length();
                let dir = if reach > 0.0 { direction / reach } else { direction };
                tree.descendants_of_class(scope, ClassKind::Part)
                    .into_iter()
                    .filter(|p| reach > 0.0 && keep(*p))
                    .filter_map(|p| {
                        let (t, normal) = geometry::ray_part(&tree, p, origin, dir)?;
                        (t >= 0.0 && t <= reach).then_some(geometry::RayHit {
                            part: p,
                            position: origin + dir * t,
                            normal,
                            distance: t,
                        })
                    })
                    .min_by(|a, b| a.distance.total_cmp(&b.distance))
            }
        };
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
            let scope = query_scope(lua, this.0, methods::RAYCAST)?;
            workspace_raycast(lua, scope, origin, direction, params)
        },
    );
    m.add_method(
        methods::GET_PART_BOUNDS_IN_BOX,
        |lua, this, (cframe, size, params): (Value, Value, Option<Value>)| {
            let member = methods::GET_PART_BOUNDS_IN_BOX;
            let scope = query_scope(lua, this.0, member)?;
            let cframe = crate::types::as_cframe(&cframe)
                .ok_or_else(|| runtime_err(format!("{member} expects a CFrame")))?;
            let size =
                as_vector3(&size).ok_or_else(|| runtime_err(format!("{member} expects a Vector3 size")))?;
            let region = geometry::Region::Box { center: cframe.pos, rot: cframe.rot, half: size / 2.0 };
            query(lua, scope, region, &overlap_params(params, member)?, None)
        },
    );
    m.add_method(
        methods::GET_PART_BOUNDS_IN_RADIUS,
        |lua, this, (center, radius, params): (Value, f32, Option<Value>)| {
            let member = methods::GET_PART_BOUNDS_IN_RADIUS;
            let scope = query_scope(lua, this.0, member)?;
            let center = as_vector3(&center)
                .ok_or_else(|| runtime_err(format!("{member} expects a Vector3 position")))?;
            let region = geometry::Region::Ball { center, radius: radius.max(0.0) };
            query(lua, scope, region, &overlap_params(params, member)?, None)
        },
    );
    m.add_method(methods::GET_PARTS_IN_PART, |lua, this, (part, params): (Value, Option<Value>)| {
        let member = methods::GET_PARTS_IN_PART;
        let scope = query_scope(lua, this.0, member)?;
        let part = as_instance(&part).ok_or_else(|| runtime_err(format!("{member} expects a part")))?;
        let region = geometry::Region::of_part(&host(lua).tree.borrow(), part)
            .ok_or_else(|| runtime_err(format!("{member} expects a part")))?;
        query(lua, scope, region, &overlap_params(params, member)?, Some(part))
    });
    m.add_method(methods::GET_TOUCHING_PARTS, |lua, this, ()| {
        require_class(lua, this.0, ClassKind::Part, methods::GET_TOUCHING_PARTS)?;
        let region = geometry::Region::of_part(&host(lua).tree.borrow(), this.0)
            .ok_or_else(|| runtime_err("GetTouchingParts needs a part with a size"))?;
        query(lua, None, region.grown(TOUCH_MARGIN), &OverlapParams::default(), Some(this.0))
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
    m.add_function(methods::FIRE, |lua, (obj, args): (AnyUserData, MultiValue)| {
        let id = obj.borrow::<crate::bindings::LuaInstance>()?.0;
        let class = crate::bindings::class_of(lua, id)?;
        if class != ClassKind::BindableEvent {
            let Some(Value::Function(f)) = crate::lenient::index(lua, id, class, obj.clone(), methods::FIRE)?
            else {
                return Err(crate::bindings::not_a_member(lua, id, methods::FIRE));
            };
            let mut all = args;
            all.push_front(Value::UserData(obj));
            return f.call::<()>(all);
        }
        fire_args(lua, SignalKind::BindableEvent(id), None, args);
        Ok(())
    });
}

/// Fires a proximity prompt's `Triggered` (or, when `ended`, `TriggerEnded`) with the player.
pub(crate) fn prompt(lua: &Lua, prompt: InstanceId, player: InstanceId, ended: bool) -> Vec<ScriptError> {
    let signal =
        if ended { SignalKind::PromptTriggerEnded(prompt) } else { SignalKind::PromptTriggered(prompt) };
    fire_args(lua, signal, Some(player), MultiValue::new());
    // ProximityPromptService hears every prompt: PromptTriggered(prompt, player).
    let name = if ended { "PromptTriggerEnded" } else { "PromptTriggered" };
    let fired = (|| -> mlua::Result<()> {
        let args =
            (crate::bindings::push_instance(lua, prompt)?, crate::bindings::push_instance(lua, player)?);
        crate::lenient::fire_service_signal(lua, "ProximityPromptService", name, args)
    })();
    if let Err(err) = fired {
        crate::bindings::host(lua)
            .errors
            .borrow_mut()
            .push(ScriptError { script: "ProximityPromptService".to_owned(), message: err.to_string() });
    }
    take_errors(lua)
}
