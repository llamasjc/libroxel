//! Calls that wait for an answer (remote and bindable functions), the web
//! helper service, and small helpers scripts expect to find: older global
//! spellings, `task.defer`, colour conversions and a few instance lookups.

use libroxel_instance::{ClassKind, InstanceId, Property};
use mlua::{Function, Lua, MultiValue, Table, UserData, UserDataMethods, Value, Variadic};

use crate::bindings::{LuaColor3, as_instance, host, push_instance, tree_err};
use crate::names::{self, functions, game_info, http};

const CALLBACKS: &str = "libroxel.callbacks";
const INVOKE_SERVER_FN: &str = "libroxel.invoke_server";
const INVOKE_CLIENT_FN: &str = "libroxel.invoke_client";
/// Answers a call from another computer: see [`crate::net::serve_invoke`].
pub(crate) const INVOKE_SERVE_FN: &str = "libroxel.invoke_serve";
const INVOKE_FN: &str = "libroxel.invoke";

fn runtime_err(message: impl Into<String>) -> mlua::Error {
    mlua::Error::runtime(message.into())
}

/// The class a callback belongs on.
fn callback_class(name: &str) -> Option<ClassKind> {
    match name {
        functions::ON_SERVER_INVOKE | functions::ON_CLIENT_INVOKE => Some(ClassKind::RemoteFunction),
        functions::ON_INVOKE => Some(ClassKind::BindableFunction),
        _ => None,
    }
}

fn callback_key(id: InstanceId, name: &str) -> String {
    format!("{id}:{name}")
}

fn callbacks(lua: &Lua) -> mlua::Result<Table> {
    lua.named_registry_value(CALLBACKS)
}

/// The callback `name` on `id`, or nil while none is set. Fails if `id`
/// isn't the right class for it.
fn callback(lua: &Lua, id: InstanceId, name: &str) -> mlua::Result<Value> {
    let class = host(lua).tree.borrow().class(id).map_err(tree_err)?;
    if Some(class) != callback_class(name) {
        return Err(crate::bindings::not_a_member(lua, id, name));
    }
    callbacks(lua)?.raw_get(callback_key(id, name))
}

/// Members beyond the saved properties: `Some` when `key` is one of them.
pub(crate) fn index(lua: &Lua, id: InstanceId, class: ClassKind, key: &str) -> mlua::Result<Option<Value>> {
    let registry = match key {
        functions::INVOKE_SERVER if class == ClassKind::RemoteFunction => INVOKE_SERVER_FN,
        functions::INVOKE_CLIENT if class == ClassKind::RemoteFunction => INVOKE_CLIENT_FN,
        functions::INVOKE if class == ClassKind::BindableFunction => INVOKE_FN,
        _ if callback_class(key) == Some(class) => {
            return Err(runtime_err(format!("{key} is a callback: it can be set but not read")));
        }
        names::attachment::WORLD_POSITION if class == ClassKind::Attachment => {
            let part = host(lua).tree.borrow().parent(id).map_err(tree_err)?;
            let offset = host(lua).tree.borrow().get_vec3(id, Property::Position).unwrap_or_default();
            let world =
                match part.filter(|p| host(lua).tree.borrow().class(*p).is_ok_and(|c| c.is_physical())) {
                    Some(part) => crate::bindings::part_cframe(lua, part)?.point_to_world(offset),
                    None => offset,
                };
            return Ok(Some(crate::bindings::vector_value(world)));
        }
        game_info::JOB_ID if class == ClassKind::DataModel => {
            return Ok(Some(Value::String(lua.create_string("")?)));
        }
        game_info::PLACE_ID | game_info::GAME_ID | game_info::CREATOR_ID if class == ClassKind::DataModel => {
            return Ok(Some(Value::Integer(0)));
        }
        _ => return Ok(None),
    };
    let _ = id;
    Ok(Some(lua.named_registry_value(registry)?))
}

/// Sets a member beyond the saved properties. Returns whether `key` was one.
pub(crate) fn new_index(
    lua: &Lua,
    id: InstanceId,
    class: ClassKind,
    key: &str,
    value: &Value,
) -> mlua::Result<bool> {
    if callback_class(key) != Some(class) {
        return Ok(false);
    }
    match value {
        Value::Nil | Value::Function(_) => {
            callbacks(lua)?.raw_set(callback_key(id, key), value.clone())?;
            Ok(true)
        }
        other => Err(runtime_err(format!("{key} expects a function or nil, got a {}", other.type_name()))),
    }
}

/// Luau halves of the invoke methods: the caller waits until a callback is
/// set, then calls it in its own thread, so the callback may wait too.
const INVOKE_PRELUDE: &str = r#"
    local get, local_player, check_player, send, poll, reply, mark = ...
    local yield = coroutine.yield
    local function await(self, name)
        while true do
            local cb = get(self, name)
            if cb then return cb end
            yield(mark, 0)
        end
    end
    local invoke_server = function(self, ...)
        -- On a player's computer the call goes to the server; wait for the answer.
        local call = send(self, ...)
        if call then
            while true do
                local done, ok, result = poll(call)
                if done then
                    if not ok then error(result, 2) end
                    return table.unpack(result, 1, result.n)
                end
                yield(mark, 0)
            end
        end
        local cb = await(self, "OnServerInvoke")
        return cb(local_player(), ...)
    end
    local serve = function(self, call, player, ...)
        local cb = await(self, "OnServerInvoke")
        reply(call, player, pcall(cb, player, ...))
    end
    local invoke_client = function(self, player, ...)
        check_player(player)
        local cb = await(self, "OnClientInvoke")
        return cb(...)
    end
    local invoke = function(self, ...)
        local cb = await(self, "OnInvoke")
        return cb(...)
    end
    return invoke_server, invoke_client, invoke, serve
"#;

// ---------------------------------------------------------------------------
// The web helper service
// ---------------------------------------------------------------------------

struct HttpService;

/// A fresh random 64-bit number from the system's secure random source:
/// games use GUIDs as hard-to-guess keys (trade offers, codes, sessions),
/// so the next one mustn't be workable out from the clock or the last one.
fn random_u64() -> u64 {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("the system has a random number source");
    u64::from_le_bytes(bytes)
}

/// A random version-4 GUID in upper case, like `1B4E28BA-2FA1-41D2-883F-0016D3CCA427`.
fn guid() -> String {
    let (a, b) = (random_u64(), random_u64());
    let a = (a & !0xF000) | 0x4000; // version 4
    let b = (b & !(0xC << 60)) | (0x8 << 60); // variant 1
    format!(
        "{:08X}-{:04X}-{:04X}-{:04X}-{:012X}",
        a >> 32,
        (a >> 16) & 0xFFFF,
        a & 0xFFFF,
        b >> 48,
        b & 0xFFFF_FFFF_FFFF
    )
}

fn url_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

impl UserData for HttpService {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        crate::lenient::add_service_members(m, "HttpService");
        m.add_method(http::JSON_ENCODE, |_, _, value: Value| {
            let json = crate::services::to_json(&value, 0)?;
            serde_json::to_string(&json).map_err(|e| runtime_err(e.to_string()))
        });
        m.add_method(http::JSON_DECODE, |lua, _, text: String| {
            let json: serde_json::Value =
                serde_json::from_str(&text).map_err(|e| runtime_err(format!("can't decode JSON: {e}")))?;
            crate::services::from_json(lua, &json)
        });
        m.add_method(http::GENERATE_GUID, |_, _, braces: Option<bool>| {
            let id = guid();
            Ok(if braces.unwrap_or(true) { format!("{{{id}}}") } else { id })
        });
        m.add_method(http::URL_ENCODE, |_, _, text: String| Ok(url_encode(&text)));
    }
}

/// The service userdata for `name`, if it is one of the services in this module.
pub(crate) fn get_service(lua: &Lua, name: &str) -> mlua::Result<Option<Value>> {
    Ok(match name {
        names::services::HTTP_SERVICE => Some(Value::UserData(lua.create_userdata(HttpService)?)),
        _ => None,
    })
}

// ---------------------------------------------------------------------------
// Colours
// ---------------------------------------------------------------------------

/// Hue, saturation and value, each 0 to 1.
pub(crate) fn to_hsv(c: libroxel_instance::Color3) -> (f32, f32, f32) {
    let (r, g, b) = (c.r, c.g, c.b);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d <= 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    let s = if max <= 0.0 { 0.0 } else { d / max };
    (h, s, max)
}

pub(crate) fn from_hsv(h: f32, s: f32, v: f32) -> libroxel_instance::Color3 {
    let h = h.rem_euclid(1.0) * 6.0;
    let s = s.clamp(0.0, 1.0);
    let v = v.clamp(0.0, 1.0);
    let c = v * s;
    let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    libroxel_instance::Color3::new(r + m, g + m, b + m)
}

pub(crate) fn to_hex(c: libroxel_instance::Color3) -> String {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("{:02x}{:02x}{:02x}", byte(c.r), byte(c.g), byte(c.b))
}

fn from_hex(text: &str) -> Option<libroxel_instance::Color3> {
    let hex = text.trim().trim_start_matches('#');
    let digits: Vec<u8> = match hex.len() {
        3 => hex.chars().map(|c| c.to_digit(16).map(|d| (d * 17) as u8)).collect::<Option<_>>()?,
        6 => (0..3).map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()).collect::<Option<_>>()?,
        _ => return None,
    };
    Some(libroxel_instance::Color3::from_rgb8(digits[0], digits[1], digits[2]))
}

// ---------------------------------------------------------------------------
// Instance lookups
// ---------------------------------------------------------------------------

/// The first ancestor of `id` that passes `test`, as a script value.
fn find_ancestor_by(
    lua: &Lua,
    id: InstanceId,
    test: impl Fn(&Lua, InstanceId) -> mlua::Result<bool>,
) -> mlua::Result<Option<mlua::AnyUserData>> {
    let mut current = host(lua).tree.borrow().parent(id).map_err(tree_err)?;
    while let Some(p) = current {
        if test(lua, p)? {
            return push_instance(lua, p).map(Some);
        }
        current = host(lua).tree.borrow().parent(p).map_err(tree_err)?;
    }
    Ok(None)
}

pub(crate) fn add_instance_methods<M: UserDataMethods<crate::bindings::LuaInstance>>(m: &mut M) {
    use names::instance::*;
    m.add_method(FIND_FIRST_CHILD_WHICH_IS_A, |lua, this, class: String| {
        let children = host(lua).tree.borrow().children(this.0).map_err(tree_err)?.to_vec();
        for child in children {
            if crate::bindings::is_a(lua, child, &class)? {
                return push_instance(lua, child).map(Some);
            }
        }
        Ok(None)
    });
    m.add_method(FIND_FIRST_ANCESTOR_OF_CLASS, |lua, this, class: String| {
        find_ancestor_by(lua, this.0, |lua, id| crate::bindings::is_class(lua, id, &class))
    });
    m.add_method(FIND_FIRST_ANCESTOR_WHICH_IS_A, |lua, this, class: String| {
        find_ancestor_by(lua, this.0, |lua, id| crate::bindings::is_a(lua, id, &class))
    });
    m.add_method(IS_ANCESTOR_OF, |lua, this, other: Value| {
        let other = as_instance(&other).ok_or_else(|| runtime_err("IsAncestorOf expects an Instance"))?;
        if other == this.0 {
            return Ok(false);
        }
        host(lua).tree.borrow().is_descendant_of(other, this.0).map_err(tree_err)
    });
    m.add_method(names::methods::GET_PLAYER_BY_USER_ID, |lua, this, user_id: f64| {
        crate::bindings::require_class(
            lua,
            this.0,
            ClassKind::Players,
            names::methods::GET_PLAYER_BY_USER_ID,
        )?;
        let found = {
            let host = host(lua);
            let tree = host.tree.borrow();
            let children = tree.children(this.0).map_err(tree_err)?;
            children.iter().copied().find(|p| {
                tree.class(*p) == Ok(ClassKind::Player)
                    && tree.get_number(*p, Property::UserId) == Some(user_id)
            })
        };
        found.map(|id| push_instance(lua, id)).transpose()
    });
}

// ---------------------------------------------------------------------------
// Installation
// ---------------------------------------------------------------------------

pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    lua.set_named_registry_value(CALLBACKS, lua.create_table()?)?;
    let get = lua.create_function(|lua, (this, name): (Value, String)| {
        let id = as_instance(&this)
            .ok_or_else(|| runtime_err("call it with a colon, like remote:InvokeServer()"))?;
        callback(lua, id, &name)
    })?;
    let local_player = lua.create_function(|lua, ()| {
        let player = crate::services::local_player(lua)
            .ok_or_else(|| runtime_err("InvokeServer needs a player in the game"))?;
        push_instance(lua, player)
    })?;
    let check_player = lua.create_function(|lua, player: Value| {
        let player = as_instance(&player)
            .filter(|p| host(lua).tree.borrow().class(*p) == Ok(ClassKind::Player))
            .ok_or_else(|| runtime_err("InvokeClient expects a Player first"))?;
        if Some(player) != crate::services::local_player(lua) {
            return Err(runtime_err("that player isn't on this computer, so it can't answer"));
        }
        Ok(())
    })?;
    let (send, poll, reply) =
        (crate::net::send_invoke_fn(lua)?, crate::net::poll_invoke_fn(lua)?, crate::net::reply_fn(lua)?);
    let (server, client, bindable, serve): (Function, Function, Function, Function) = lua
        .load(INVOKE_PRELUDE)
        .set_name("=libroxel")
        .call((get, local_player, check_player, send, poll, reply, crate::bindings::wait_mark()))?;
    lua.set_named_registry_value(INVOKE_SERVE_FN, serve)?;
    lua.set_named_registry_value(INVOKE_SERVER_FN, server)?;
    lua.set_named_registry_value(INVOKE_CLIENT_FN, client)?;
    lua.set_named_registry_value(INVOKE_FN, bindable)?;

    let g = lua.globals();
    use names::globals;
    g.set(
        globals::TICK,
        lua.create_function(|_, ()| {
            Ok(std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0))
        })?,
    )?;
    g.set(globals::ELAPSED_TIME, g.get::<Function>(globals::TIME)?)?;
    let task: Table = g.get(globals::TASK)?;
    let delay: Function = task.get(names::task::DELAY)?;
    g.set(globals::SPAWN, task.get::<Function>(names::task::SPAWN)?)?;
    g.set(globals::DELAY, delay.clone())?;
    // Deferred: runs at the next step, like a zero-second delay.
    task.set(
        names::task::DEFER,
        lua.create_function(move |_, (func, args): (Function, Variadic<Value>)| {
            let mut all: MultiValue = args.into_iter().collect();
            all.push_front(Value::Function(func));
            all.push_front(Value::Number(0.0));
            delay.call::<Value>(all)
        })?,
    )?;

    let color3: Table = g.get(globals::COLOR3)?;
    color3.set(
        names::color3::FROM_HSV,
        lua.create_function(|_, (h, s, v): (f32, f32, f32)| Ok(LuaColor3(from_hsv(h, s, v))))?,
    )?;
    color3.set(
        names::color3::FROM_HEX,
        lua.create_function(|_, text: String| {
            from_hex(&text).map(LuaColor3).ok_or_else(|| runtime_err(format!("{text:?} is not a hex colour")))
        })?,
    )?;

    let vector3: Table = g.get(globals::VECTOR3)?;
    use crate::bindings::LuaVector3;
    use libroxel_instance::Vec3;
    vector3.set(names::vector3::X_AXIS, LuaVector3(Vec3::new(1.0, 0.0, 0.0)))?;
    vector3.set(names::vector3::Y_AXIS, LuaVector3(Vec3::new(0.0, 1.0, 0.0)))?;
    vector3.set(names::vector3::Z_AXIS, LuaVector3(Vec3::new(0.0, 0.0, 1.0)))?;
    Ok(())
}
