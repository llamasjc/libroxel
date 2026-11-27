//! What scripts need to play over a network: remote events and remote
//! functions that cross between the server and the players' computers.
//!
//! The runtime never touches a socket. Scripts' sends pile up as
//! [`NetMessage`]s for the engine to take, and the engine hands back what
//! arrived from the other side. Values cross as JSON (see [`encode`]), with
//! instances written as `{"$i": id}` in this runtime's ids; the engine
//! changes those to the other side's ids with [`map_instances`].

use std::collections::HashMap;

use mlua::{Function, Lua, MultiValue, Value};
use playlibre_instance::serde_json::{self, Value as Json, json};
use playlibre_instance::{CFrame, Color3, EnumKind, InstanceId, Rotation, UDim, UDim2, Vec2, Vec3};

use crate::ScriptError;
use crate::bindings::{LuaColor3, LuaEnumItem, SignalKind, as_instance, host, push_instance, take_errors};
use crate::compat::LuaBrickColor;
use crate::effects::{LuaColorSequence, LuaNumberRange, LuaNumberSequence};
use crate::types::{LuaCFrame, LuaUDim, LuaUDim2, LuaVector2};

/// Which side of a game this runtime is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NetRole {
    /// One computer: the server and the only player at once.
    #[default]
    Solo,
    /// The game's server, which other computers join.
    Server,
    /// A player's computer, joined to a server.
    Client,
}

/// Something a script sent to the other side.
#[derive(Debug, Clone, PartialEq)]
pub enum NetMessage {
    /// `FireServer`, on a client.
    ServerEvent { remote: InstanceId, args: Json },
    /// `FireClient` (to `player`) or `FireAllClients` (`player` is `None`), on the server.
    ClientEvent { remote: InstanceId, player: Option<InstanceId>, args: Json },
    /// `InvokeServer`, on a client. The answer comes back numbered `call`.
    Invoke { call: u64, remote: InstanceId, args: Json },
    /// The server's answer to `player`'s call numbered `call`: the values
    /// returned, or the error message when not `ok`.
    InvokeResult { call: u64, player: InstanceId, ok: bool, values: Json },
}

#[derive(Debug, Default)]
pub(crate) struct NetState {
    pub(crate) role: NetRole,
    /// The player on this computer, once the engine has said who it is.
    pub(crate) local_player: Option<InstanceId>,
    pub(crate) outbox: Vec<NetMessage>,
    next_call: u64,
    /// Answers to this client's calls that have arrived.
    results: HashMap<u64, (bool, Json)>,
}

pub(crate) fn role(lua: &Lua) -> NetRole {
    host(lua).net.borrow().role
}

pub(crate) fn send(lua: &Lua, message: NetMessage) {
    host(lua).net.borrow_mut().outbox.push(message);
}

fn runtime_err(message: impl Into<String>) -> mlua::Error {
    mlua::Error::runtime(message.into())
}

// ---------------------------------------------------------------------------
// Values on the wire
// ---------------------------------------------------------------------------

fn number(n: f64) -> Json {
    match serde_json::Number::from_f64(n) {
        Some(n) => Json::Number(n),
        None if n.is_nan() => json!({ "$n": "nan" }),
        None => json!({ "$n": if n > 0.0 { "inf" } else { "-inf" } }),
    }
}

fn bytes(b: &[u8]) -> Json {
    Json::String(data_encoding::BASE64.encode(b))
}

fn floats(values: &[f32]) -> Json {
    Json::Array(values.iter().map(|v| number(f64::from(*v))).collect())
}

/// A script value as JSON for sending. Tables become lists or objects;
/// instances, vectors, colours, CFrames, UDims and enum items are tagged
/// objects like `{"$v3": [x, y, z]}`.
pub fn encode(value: &Value) -> mlua::Result<Json> {
    encode_at(value, 0)
}

fn encode_at(value: &Value, depth: usize) -> mlua::Result<Json> {
    if depth > 32 {
        return Err(runtime_err("tables sent over the network can't be nested that deeply"));
    }
    Ok(match value {
        Value::Nil => Json::Null,
        Value::Boolean(b) => Json::Bool(*b),
        Value::Integer(n) => number(*n as f64),
        Value::Number(n) => number(*n),
        Value::String(s) => match s.to_str() {
            Ok(text) => Json::String(text.to_owned()),
            Err(_) => json!({ "$s": bytes(&s.as_bytes()) }),
        },
        Value::Buffer(b) => json!({ "$b": bytes(&b.to_vec()) }),
        Value::Table(table) => {
            let len = table.raw_len();
            let pairs: Vec<(Value, Value)> = table.pairs::<Value, Value>().collect::<mlua::Result<_>>()?;
            if pairs.len() == len {
                let items: mlua::Result<Vec<Json>> =
                    (1..=len).map(|i| encode_at(&table.raw_get::<Value>(i)?, depth + 1)).collect();
                Json::Array(items?)
            } else {
                let mut map = serde_json::Map::new();
                for (k, v) in pairs {
                    let key = match k {
                        Value::String(s) => s.to_str()?.to_owned(),
                        Value::Integer(n) => n.to_string(),
                        Value::Number(n) => n.to_string(),
                        other => {
                            return Err(runtime_err(format!(
                                "tables sent over the network can't have {} keys",
                                other.type_name()
                            )));
                        }
                    };
                    map.insert(key, encode_at(&v, depth + 1)?);
                }
                Json::Object(map)
            }
        }
        Value::Vector(v) => json!({ "$v3": floats(&[v.x(), v.y(), v.z()]) }),
        Value::UserData(ud) => {
            if let Some(id) = as_instance(value) {
                json!({ "$i": id.to_bits() })
            } else if let Ok(c) = ud.borrow::<LuaColor3>() {
                json!({ "$c3": floats(&[c.0.r, c.0.g, c.0.b]) })
            } else if let Ok(v) = ud.borrow::<LuaVector2>() {
                json!({ "$v2": floats(&[v.0.x, v.0.y]) })
            } else if let Ok(u) = ud.borrow::<LuaUDim2>() {
                let u = u.0;
                json!({ "$u2": floats(&[u.x_scale, u.x_offset, u.y_scale, u.y_offset]) })
            } else if let Ok(u) = ud.borrow::<LuaUDim>() {
                json!({ "$u": floats(&[u.0.scale, u.0.offset]) })
            } else if let Ok(c) = ud.borrow::<LuaCFrame>() {
                let CFrame { pos, rot } = c.0;
                let mut all = vec![pos.x, pos.y, pos.z];
                for axis in [rot.x, rot.y, rot.z] {
                    all.extend([axis.x, axis.y, axis.z]);
                }
                json!({ "$cf": floats(&all) })
            } else if let Ok(e) = ud.borrow::<LuaEnumItem>() {
                json!({ "$e": [e.0.kind().kind_name(), e.0.file_name()] })
            } else if let Ok(r) = ud.borrow::<LuaNumberRange>() {
                json!({ "$nr": floats(&[r.0.x, r.0.y]) })
            } else if let Ok(seq) = ud.borrow::<LuaNumberSequence>() {
                let points: Vec<f32> = seq.0.iter().flat_map(|&(t, v)| [t, v]).collect();
                json!({ "$ns": floats(&points) })
            } else if let Ok(seq) = ud.borrow::<LuaColorSequence>() {
                let points: Vec<f32> = seq.0.iter().flat_map(|&(t, c)| [t, c.r, c.g, c.b]).collect();
                json!({ "$cs": floats(&points) })
            } else if let Ok(b) = ud.borrow::<LuaBrickColor>() {
                json!({ "$bc": b.0 })
            } else {
                return Err(runtime_err("that value can't be sent over the network"));
            }
        }
        other => return Err(runtime_err(format!("a {} can't be sent over the network", other.type_name()))),
    })
}

/// Script arguments as a JSON list.
pub(crate) fn encode_args(args: &MultiValue) -> mlua::Result<Json> {
    Ok(Json::Array(args.iter().map(encode).collect::<mlua::Result<_>>()?))
}

fn tagged<'a>(map: &'a serde_json::Map<String, Json>, tag: &str) -> Option<&'a Json> {
    if map.len() == 1 { map.get(tag) } else { None }
}

fn float_list(json: &Json, len: usize) -> Option<Vec<f32>> {
    let items = json.as_array().filter(|a| a.len() == len)?;
    items.iter().map(|v| v.as_f64().map(|f| f as f32)).collect()
}

/// A list of floats whose length is a multiple of `each`, at most `most` long.
fn float_groups(json: &Json, each: usize, most: usize) -> Option<Vec<f32>> {
    let items = json.as_array().filter(|a| a.len() % each == 0 && a.len() <= most)?;
    items.iter().map(|v| v.as_f64().map(|f| f as f32)).collect()
}

fn unbytes(json: &Json) -> Option<Vec<u8>> {
    data_encoding::BASE64.decode(json.as_str()?.as_bytes()).ok()
}

/// Turns [`encode`]'s JSON back into a script value. Instances that don't
/// exist here become `nil`.
pub fn decode(lua: &Lua, json: &Json) -> mlua::Result<Value> {
    Ok(match json {
        Json::Null => Value::Nil,
        Json::Bool(b) => Value::Boolean(*b),
        Json::Number(n) => Value::Number(n.as_f64().unwrap_or(0.0)),
        Json::String(s) => Value::String(lua.create_string(s)?),
        Json::Array(items) => {
            let table = lua.create_table()?;
            for (i, item) in items.iter().enumerate() {
                table.raw_set(i + 1, decode(lua, item)?)?;
            }
            Value::Table(table)
        }
        Json::Object(map) => {
            if let Some(bits) = tagged(map, "$i").and_then(Json::as_u64) {
                let id = InstanceId::from_bits(bits);
                let alive = {
                    let host = host(lua);
                    let tree = host.tree.borrow();
                    tree.exists(id) && !tree.is_destroyed(id).unwrap_or(true)
                };
                return Ok(if alive { Value::UserData(push_instance(lua, id)?) } else { Value::Nil });
            }
            if let Some(n) = tagged(map, "$n").and_then(Json::as_str) {
                return Ok(Value::Number(match n {
                    "inf" => f64::INFINITY,
                    "-inf" => f64::NEG_INFINITY,
                    _ => f64::NAN,
                }));
            }
            if let Some(b) = tagged(map, "$s").and_then(unbytes) {
                return Ok(Value::String(lua.create_string(b)?));
            }
            if let Some(b) = tagged(map, "$b").and_then(unbytes) {
                return Ok(Value::Buffer(lua.create_buffer(b)?));
            }
            if let Some(v) = tagged(map, "$nr").and_then(|j| float_list(j, 2)) {
                return Ok(Value::UserData(lua.create_userdata(LuaNumberRange(Vec2::new(v[0], v[1])))?));
            }
            if let Some(v) = tagged(map, "$ns").and_then(|j| float_groups(j, 2, 40)) {
                let points = v.chunks(2).map(|p| (p[0], p[1])).collect();
                return Ok(Value::UserData(lua.create_userdata(LuaNumberSequence(points))?));
            }
            if let Some(v) = tagged(map, "$cs").and_then(|j| float_groups(j, 4, 80)) {
                let points = v.chunks(4).map(|p| (p[0], Color3::new(p[1], p[2], p[3]))).collect();
                return Ok(Value::UserData(lua.create_userdata(LuaColorSequence(points))?));
            }
            if let Some(i) = tagged(map, "$bc").and_then(Json::as_u64) {
                return Ok(match crate::compat::brick_color(i as usize) {
                    Some(b) => Value::UserData(lua.create_userdata(b)?),
                    None => Value::Nil,
                });
            }
            if let Some(v) = tagged(map, "$v3").and_then(|j| float_list(j, 3)) {
                return Ok(crate::bindings::vector_value(Vec3::new(v[0], v[1], v[2])));
            }
            if let Some(v) = tagged(map, "$c3").and_then(|j| float_list(j, 3)) {
                return Ok(Value::UserData(lua.create_userdata(LuaColor3(Color3::new(v[0], v[1], v[2])))?));
            }
            if let Some(v) = tagged(map, "$v2").and_then(|j| float_list(j, 2)) {
                return Ok(Value::UserData(lua.create_userdata(LuaVector2(Vec2::new(v[0], v[1])))?));
            }
            if let Some(v) = tagged(map, "$u2").and_then(|j| float_list(j, 4)) {
                let u = UDim2::new(v[0], v[1], v[2], v[3]);
                return Ok(Value::UserData(lua.create_userdata(LuaUDim2(u))?));
            }
            if let Some(v) = tagged(map, "$u").and_then(|j| float_list(j, 2)) {
                return Ok(Value::UserData(lua.create_userdata(LuaUDim(UDim::new(v[0], v[1])))?));
            }
            if let Some(v) = tagged(map, "$cf").and_then(|j| float_list(j, 12)) {
                let axis = |i: usize| Vec3::new(v[i], v[i + 1], v[i + 2]);
                let rot = Rotation { x: axis(3), y: axis(6), z: axis(9) };
                return Ok(Value::UserData(lua.create_userdata(LuaCFrame(CFrame::new(axis(0), rot)))?));
            }
            if let Some(pair) = tagged(map, "$e").and_then(Json::as_array).filter(|a| a.len() == 2) {
                let (kind, name) = (pair[0].as_str().unwrap_or(""), pair[1].as_str().unwrap_or(""));
                let item =
                    EnumKind::ALL.iter().find(|k| k.kind_name() == kind).and_then(|k| k.from_file_name(name));
                return Ok(match item {
                    Some(item) => Value::UserData(lua.create_userdata(LuaEnumItem(item))?),
                    None => Value::Nil,
                });
            }
            let table = lua.create_table()?;
            for (k, v) in map {
                table.raw_set(k.as_str(), decode(lua, v)?)?;
            }
            Value::Table(table)
        }
    })
}

fn decode_args(lua: &Lua, json: &Json) -> mlua::Result<MultiValue> {
    let mut out = MultiValue::new();
    for item in json.as_array().map(Vec::as_slice).unwrap_or_default() {
        out.push_back(decode(lua, item)?);
    }
    Ok(out)
}

/// Rewrites every instance in [`encode`]'s JSON with `map`, for sending it
/// to a computer whose ids differ. Instances `map` doesn't know become `null`.
pub fn map_instances(json: &mut Json, map: &mut impl FnMut(InstanceId) -> Option<InstanceId>) {
    match json {
        Json::Array(items) => items.iter_mut().for_each(|item| map_instances(item, map)),
        Json::Object(object) => {
            if let Some(bits) = tagged(object, "$i").and_then(Json::as_u64) {
                *json = match map(InstanceId::from_bits(bits)) {
                    Some(id) => json!({ "$i": id.to_bits() }),
                    None => Json::Null,
                };
                return;
            }
            object.values_mut().for_each(|v| map_instances(v, map));
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// What arrives from the other side
// ---------------------------------------------------------------------------

/// A client fired `remote` with `args`: fires `OnServerEvent(player, ...)`.
pub(crate) fn server_event(
    lua: &Lua,
    remote: InstanceId,
    player: InstanceId,
    args: &Json,
) -> Vec<ScriptError> {
    match decode_args(lua, args) {
        Ok(args) => crate::services::fire_args(lua, SignalKind::ServerEvent(remote), Some(player), args),
        Err(err) => report(lua, err),
    }
    take_errors(lua)
}

/// The server fired `remote` at this computer: fires `OnClientEvent(...)`.
pub(crate) fn client_event(lua: &Lua, remote: InstanceId, args: &Json) -> Vec<ScriptError> {
    match decode_args(lua, args) {
        Ok(args) => crate::services::fire_args(lua, SignalKind::ClientEvent(remote), None, args),
        Err(err) => report(lua, err),
    }
    take_errors(lua)
}

fn report(lua: &Lua, err: mlua::Error) {
    host(lua).errors.borrow_mut().push(ScriptError { script: "network".into(), message: err.to_string() });
}

/// A client called `remote:InvokeServer(...)`: runs `OnServerInvoke` in its
/// own thread (so it may wait) and sends the answer when it returns.
pub(crate) fn serve_invoke(
    lua: &Lua,
    call: u64,
    remote: InstanceId,
    player: InstanceId,
    args: &Json,
) -> Vec<ScriptError> {
    let result = (|| -> mlua::Result<()> {
        let serve: Function = lua.named_registry_value(crate::calls::INVOKE_SERVE_FN)?;
        let mut all = MultiValue::new();
        all.push_back(Value::UserData(push_instance(lua, remote)?));
        all.push_back(Value::Number(call as f64));
        all.push_back(Value::UserData(push_instance(lua, player)?));
        all.extend(decode_args(lua, args)?);
        crate::bindings::spawn(lua, serve, all, "network", None)
    })();
    if let Err(err) = result {
        report(lua, err);
    }
    take_errors(lua)
}

/// The server answered this computer's call numbered `call`.
pub(crate) fn invoke_result(lua: &Lua, call: u64, ok: bool, values: Json) {
    host(lua).net.borrow_mut().results.insert(call, (ok, values));
}

// ---------------------------------------------------------------------------
// Host functions for the invoke prelude
// ---------------------------------------------------------------------------

/// `send(remote, ...)`: on a client, sends the call and returns its number;
/// elsewhere returns nothing, and the call is answered here.
pub(crate) fn send_invoke_fn(lua: &Lua) -> mlua::Result<Function> {
    lua.create_function(|lua, args: MultiValue| {
        if role(lua) != NetRole::Client {
            return Ok(None);
        }
        let mut args = args.into_iter();
        let remote = args
            .next()
            .as_ref()
            .and_then(as_instance)
            .ok_or_else(|| runtime_err("call it with a colon, like remote:InvokeServer()"))?;
        let rest: MultiValue = args.collect();
        let args = encode_args(&rest)?;
        let host = host(lua);
        let mut net = host.net.borrow_mut();
        net.next_call += 1;
        let call = net.next_call;
        net.outbox.push(NetMessage::Invoke { call, remote, args });
        Ok(Some(call))
    })
}

/// `poll(call)`: whether the answer has arrived, whether it worked, and the
/// values (a packed table) or the error message.
pub(crate) fn poll_invoke_fn(lua: &Lua) -> mlua::Result<Function> {
    lua.create_function(|lua, call: u64| {
        let Some((ok, values)) = host(lua).net.borrow_mut().results.remove(&call) else {
            return Ok((false, false, Value::Nil));
        };
        if !ok {
            let message = values.as_str().unwrap_or("the server's OnServerInvoke failed").to_owned();
            return Ok((true, false, Value::String(lua.create_string(&message)?)));
        }
        let table = lua.create_table()?;
        let items = values.as_array().cloned().unwrap_or_default();
        for (i, item) in items.iter().enumerate() {
            table.raw_set(i + 1, decode(lua, item)?)?;
        }
        table.raw_set("n", items.len())?;
        Ok((true, true, Value::Table(table)))
    })
}

/// `reply(call, player, ok, ...)`: sends the answer to a client's call.
pub(crate) fn reply_fn(lua: &Lua) -> mlua::Result<Function> {
    lua.create_function(|lua, args: MultiValue| {
        let mut args = args.into_iter();
        let call = match args.next() {
            Some(Value::Number(n)) => n as u64,
            Some(Value::Integer(n)) => n as u64,
            _ => return Err(runtime_err("reply needs a call number")),
        };
        let player =
            args.next().as_ref().and_then(as_instance).ok_or_else(|| runtime_err("reply needs a player"))?;
        let ok = matches!(args.next(), Some(Value::Boolean(true)));
        let rest: MultiValue = args.collect();
        let (ok, values) = if ok {
            match encode_args(&rest) {
                Ok(values) => (true, values),
                Err(err) => (false, Json::String(err.to_string())),
            }
        } else {
            // Only what went wrong goes to the player: never the server's
            // stack traceback (its scripts' names and lines).
            let message = match rest.front() {
                Some(Value::String(s)) => s.to_str().map(|s| s.to_owned()).unwrap_or_default(),
                Some(Value::Error(err)) => match err.as_ref() {
                    mlua::Error::CallbackError { cause, .. } => cause.to_string(),
                    other => other.to_string(),
                },
                _ => "OnServerInvoke failed".to_owned(),
            };
            let message = message.split("\nstack traceback").next().unwrap_or_default().to_owned();
            (false, Json::String(message))
        };
        send(lua, NetMessage::InvokeResult { call, player, ok, values });
        Ok(())
    })
}
