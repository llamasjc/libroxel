//! Attributes, tags, tools and the player's mouse.

use mlua::{Lua, MetaMethod, UserData, UserDataFields, UserDataMethods, Value};
use playlibre_instance::{CFrame, ClassKind, InstanceId, Property, PropertyValue, Vec3, geometry};

use crate::bindings::{
    LuaColor3, LuaInstance, Signal, SignalKind, as_instance, fire, host, intern, is_character,
    property_to_lua, push_instance, require_class, tree_err,
};
use crate::names::{self, methods, mouse};
use crate::services::local_player;
use crate::types::{LuaCFrame, as_udim2, as_vector2};

fn runtime_err(message: impl Into<String>) -> mlua::Error {
    mlua::Error::runtime(message.into())
}

// ---------------------------------------------------------------------------
// Attributes
// ---------------------------------------------------------------------------

fn check_attribute_name(name: &str) -> mlua::Result<()> {
    let ok = !name.is_empty()
        && name.len() <= names::attributes::MAX_NAME_LEN
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !ok {
        return Err(runtime_err(format!(
            "attribute names must be 1 to {} letters, digits or underscores",
            names::attributes::MAX_NAME_LEN
        )));
    }
    // Names starting with RBX are reserved.
    if name.starts_with("RBX") {
        return Err(runtime_err(format!(
            "{name} is a reserved attribute name (names starting with RBX are)"
        )));
    }
    Ok(())
}

/// The attribute value for a script value; nil means "remove".
fn attribute_from_lua(value: &Value) -> mlua::Result<Option<PropertyValue>> {
    Ok(Some(match value {
        Value::Nil => return Ok(None),
        Value::Boolean(b) => PropertyValue::Bool(*b),
        Value::Integer(n) => PropertyValue::Number(*n as f64),
        Value::Number(n) => PropertyValue::Number(*n),
        Value::String(s) => PropertyValue::String(s.to_str()?.to_owned()),
        Value::Vector(v) => PropertyValue::Vec3(playlibre_instance::Vec3::new(v.x(), v.y(), v.z())),
        Value::UserData(ud) => {
            if let Ok(c) = ud.borrow::<LuaColor3>() {
                PropertyValue::Color3(c.0)
            } else if let Some(v) = as_vector2(value) {
                PropertyValue::Vec2(v)
            } else if let Some(u) = as_udim2(value) {
                PropertyValue::UDim2(u)
            } else if ud.is::<LuaInstance>() {
                return Err(runtime_err("attributes can't hold an Instance"));
            } else {
                // CFrames, BrickColors, NumberRanges, enum items, Rects,
                // sequences and fonts: see `rich_attribute`.
                return Ok(None);
            }
        }
        // Rects and fonts are made in Lua.
        Value::Table(t) if t.metatable().is_some() => return Ok(None),
        other => return Err(runtime_err(format!("attributes can't hold a {}", other.type_name()))),
    }))
}

/// Marks an attribute whose value is kept with the scripts rather than in
/// the tree (kinds the tree has no value for). The text after it numbers
/// the value, by instance and name; copies of the instance share it.
const RICH: &str = "\u{1}libroxel-attribute:";
const RICH_KEY: &str = "libroxel.rich_attributes";

fn rich_values(lua: &Lua) -> mlua::Result<mlua::Table> {
    if let Ok(t) = lua.named_registry_value::<mlua::Table>(RICH_KEY) {
        return Ok(t);
    }
    let t = lua.create_table()?;
    lua.set_named_registry_value(RICH_KEY, &t)?;
    Ok(t)
}

/// Turns attribute values with no kind of their own in the tree into text
/// and back (see [`playlibre_instance::RICH_ATTRIBUTE`]).
const CODEC: &str = r#"
local function n(x) return string.format("%.9g", x) end
local codec = {}
function codec.encode(v)
    local kind = typeof(v)
    local out = { kind }
    if kind == "NumberRange" then
        table.insert(out, n(v.Min)); table.insert(out, n(v.Max))
    elseif kind == "NumberSequence" then
        for _, k in v.Keypoints do table.insert(out, n(k.Time)); table.insert(out, n(k.Value)) end
    elseif kind == "ColorSequence" then
        for _, k in v.Keypoints do
            table.insert(out, n(k.Time))
            table.insert(out, n(k.Value.R)); table.insert(out, n(k.Value.G)); table.insert(out, n(k.Value.B))
        end
    elseif kind == "Rect" then
        for _, x in { v.Min.X, v.Min.Y, v.Max.X, v.Max.Y } do table.insert(out, n(x)) end
    elseif kind == "CFrame" then
        for _, x in { v:GetComponents() } do table.insert(out, n(x)) end
    elseif kind == "Ray" then
        for _, x in { v.Origin.X, v.Origin.Y, v.Origin.Z, v.Direction.X, v.Direction.Y, v.Direction.Z } do
            table.insert(out, n(x))
        end
    else
        return nil
    end
    return table.concat(out, " ")
end
function codec.decode(text)
    local parts = string.split(text, " ")
    local kind = table.remove(parts, 1)
    local x = {}
    for i, p in parts do x[i] = tonumber(p) or 0 end
    if kind == "NumberRange" then
        return NumberRange.new(x[1] or 0, x[2] or x[1] or 0)
    elseif kind == "NumberSequence" then
        local points = {}
        for i = 1, #x - 1, 2 do table.insert(points, NumberSequenceKeypoint.new(x[i], x[i + 1])) end
        return NumberSequence.new(points)
    elseif kind == "ColorSequence" then
        local points = {}
        for i = 1, #x - 3, 4 do
            table.insert(points, ColorSequenceKeypoint.new(x[i], Color3.new(x[i + 1], x[i + 2], x[i + 3])))
        end
        return ColorSequence.new(points)
    elseif kind == "Rect" then
        return Rect.new(x[1] or 0, x[2] or 0, x[3] or 0, x[4] or 0)
    elseif kind == "CFrame" then
        return CFrame.new(table.unpack(x, 1, 12))
    elseif kind == "Ray" then
        return Ray.new(Vector3.new(x[1] or 0, x[2] or 0, x[3] or 0), Vector3.new(x[4] or 0, x[5] or 0, x[6] or 0))
    elseif kind == "Faces" then
        return Faces.new(table.unpack(parts))
    elseif kind == "Axes" then
        return Axes.new(table.unpack(parts))
    elseif kind == "Font" then
        -- Weight, style, then the family, which may hold spaces.
        local weight = Enum.FontWeight[parts[1] or "Regular"] or Enum.FontWeight.Regular
        local style = Enum.FontStyle[parts[2] or "Normal"] or Enum.FontStyle.Normal
        return Font.new(table.concat(parts, " ", 3), weight, style)
    elseif kind == "EnumItem" then
        local ok, item = pcall(function() return Enum[parts[1]][parts[2]] end)
        return ok and item or nil
    end
    return nil
end
return codec
"#;
const CODEC_KEY: &str = "libroxel.rich_attribute_codec";

fn codec(lua: &Lua) -> mlua::Result<mlua::Table> {
    if let Ok(t) = lua.named_registry_value::<mlua::Table>(CODEC_KEY) {
        return Ok(t);
    }
    let t: mlua::Table = lua.load(CODEC).set_name("=libroxel.attributes").eval()?;
    lua.set_named_registry_value(CODEC_KEY, &t)?;
    Ok(t)
}

/// The tree's stand-in for a value with no kind of its own there: as text,
/// when it has a text form (so it saves and reaches joined players), or
/// else kept by scripts alone.
fn rich_attribute(lua: &Lua, id: InstanceId, name: &str, value: Value) -> mlua::Result<PropertyValue> {
    let text: Option<String> = codec(lua)?.get::<mlua::Function>("encode")?.call(value.clone())?;
    if let Some(text) = text {
        return Ok(PropertyValue::String(format!("{}{text}", playlibre_instance::RICH_ATTRIBUTE)));
    }
    let key = format!("{}:{name}", id.to_bits());
    rich_values(lua)?.raw_set(key.as_str(), value)?;
    Ok(PropertyValue::String(format!("{RICH}{key}")))
}

/// A kept value's text form (see [`playlibre_instance::RICH_ATTRIBUTE`]) as
/// the value it stands for; None when `text` isn't one.
pub(crate) fn rich_to_lua(lua: &Lua, text: &str) -> mlua::Result<Option<Value>> {
    match text.strip_prefix(playlibre_instance::RICH_ATTRIBUTE) {
        Some(encoded) => Ok(Some(codec(lua)?.get::<mlua::Function>("decode")?.call(encoded)?)),
        None => Ok(None),
    }
}

/// An attribute's value for scripts.
fn attribute_to_lua(lua: &Lua, value: PropertyValue) -> mlua::Result<Value> {
    if let PropertyValue::String(text) = &value
        && let Some(encoded) = text.strip_prefix(playlibre_instance::RICH_ATTRIBUTE)
    {
        return codec(lua)?.get::<mlua::Function>("decode")?.call(encoded);
    }
    if let PropertyValue::String(text) = &value
        && let Some(n) = text.strip_prefix(RICH)
    {
        return rich_values(lua)?.raw_get(n);
    }
    property_to_lua(lua, value)
}

// ---------------------------------------------------------------------------
// Tags
// ---------------------------------------------------------------------------

pub(crate) struct CollectionService;

fn instance_arg(value: &Value, method: &str) -> mlua::Result<InstanceId> {
    as_instance(value).ok_or_else(|| runtime_err(format!("{method} expects an Instance")))
}

fn add_tag(lua: &Lua, id: InstanceId, tag: &str) -> mlua::Result<()> {
    host(lua).tree.borrow_mut().add_tag(id, tag).map_err(tree_err)
}

fn remove_tag(lua: &Lua, id: InstanceId, tag: &str) -> mlua::Result<()> {
    host(lua).tree.borrow_mut().remove_tag(id, tag).map_err(tree_err)
}

fn has_tag(lua: &Lua, id: InstanceId, tag: &str) -> mlua::Result<bool> {
    host(lua).tree.borrow().has_tag(id, tag).map_err(tree_err)
}

fn get_tags(lua: &Lua, id: InstanceId) -> mlua::Result<Vec<String>> {
    Ok(host(lua).tree.borrow().tags(id).map_err(tree_err)?.to_vec())
}

impl UserData for CollectionService {
    fn add_fields<F: mlua::UserDataFields<Self>>(fields: &mut F) {
        for event in ["TagAdded", "TagRemoved"] {
            fields.add_field_function_get(event, move |lua, obj: mlua::AnyUserData| {
                host(lua).tag_sets_heard.set(true);
                crate::lenient::service_index(lua, "CollectionService", obj, event)
            });
        }
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        crate::lenient::add_service_members(m, "CollectionService");
        m.add_method(methods::ADD_TAG, |lua, _, (inst, tag): (Value, String)| {
            add_tag(lua, instance_arg(&inst, methods::ADD_TAG)?, &tag)
        });
        m.add_method(methods::REMOVE_TAG, |lua, _, (inst, tag): (Value, String)| {
            remove_tag(lua, instance_arg(&inst, methods::REMOVE_TAG)?, &tag)
        });
        m.add_method(methods::HAS_TAG, |lua, _, (inst, tag): (Value, String)| {
            has_tag(lua, instance_arg(&inst, methods::HAS_TAG)?, &tag)
        });
        m.add_method(methods::GET_TAGS, |lua, _, inst: Value| {
            get_tags(lua, instance_arg(&inst, methods::GET_TAGS)?)
        });
        m.add_method(methods::GET_TAGGED, |lua, _, tag: String| {
            let ids = host(lua).tree.borrow().tagged(&tag);
            ids.into_iter().map(|id| push_instance(lua, id)).collect::<mlua::Result<Vec<_>>>()
        });
        m.add_method(methods::GET_INSTANCE_ADDED_SIGNAL, |lua, _, tag: String| {
            Ok(Signal(SignalKind::TagAdded(intern(lua, &tag))))
        });
        m.add_method(methods::GET_INSTANCE_REMOVED_SIGNAL, |lua, _, tag: String| {
            Ok(Signal(SignalKind::TagRemoved(intern(lua, &tag))))
        });
    }
}

// ---------------------------------------------------------------------------
// Tools and the mouse
// ---------------------------------------------------------------------------

/// The local player's character model, if they have one.
fn local_character(lua: &Lua) -> Option<InstanceId> {
    let player = local_player(lua)?;
    host(lua).tree.borrow().get_ref(player, Property::Character)
}

/// The tool the local character is holding, if any.
fn held_tool(lua: &Lua) -> Option<InstanceId> {
    let character = local_character(lua)?;
    let host = host(lua);
    let tree = host.tree.borrow();
    tree.find_first_child_of_class(character, ClassKind::Tool).ok().flatten()
}

/// The left mouse button went down or up (and no GUI used the click).
/// What the mouse's older key events pass for `key`: the letter it types,
/// in lower case (a space, a return, a tab, a backspace), if it types one.
pub(crate) fn typed(key: playlibre_instance::KeyCode) -> Option<&'static str> {
    use playlibre_instance::KeyCode as K;
    Some(match key {
        K::A => "a",
        K::B => "b",
        K::C => "c",
        K::D => "d",
        K::E => "e",
        K::F => "f",
        K::G => "g",
        K::H => "h",
        K::I => "i",
        K::J => "j",
        K::K => "k",
        K::L => "l",
        K::M => "m",
        K::N => "n",
        K::O => "o",
        K::P => "p",
        K::Q => "q",
        K::R => "r",
        K::S => "s",
        K::T => "t",
        K::U => "u",
        K::V => "v",
        K::W => "w",
        K::X => "x",
        K::Y => "y",
        K::Z => "z",
        K::Zero => "0",
        K::One => "1",
        K::Two => "2",
        K::Three => "3",
        K::Four => "4",
        K::Five => "5",
        K::Six => "6",
        K::Seven => "7",
        K::Eight => "8",
        K::Nine => "9",
        K::Space => " ",
        K::Return => "\r",
        K::Tab => "\t",
        K::Backspace => "\u{8}",
        K::Slash => "/",
        _ => return None,
    })
}

/// A key went down or up: the mouse's older key events fire with it.
pub(crate) fn mouse_key(lua: &Lua, key: playlibre_instance::KeyCode, down: bool) {
    let Some(letter) = typed(key) else { return };
    let signal = if down { SignalKind::MouseKeyDown } else { SignalKind::MouseKeyUp };
    fire(lua, signal, || Ok([Value::String(lua.create_string(letter)?)].into_iter().collect()));
}

pub(crate) fn mouse_button(lua: &Lua, down: bool) {
    let signal = if down { SignalKind::MouseButton1Down } else { SignalKind::MouseButton1Up };
    fire(lua, signal, || Ok(mlua::MultiValue::new()));
    let Some(tool) = held_tool(lua) else { return };
    activate(lua, tool, down);
}

/// Fires `tool`'s `Activated` or `Deactivated`, unless it is turned off.
pub(crate) fn activate(lua: &Lua, tool: InstanceId, down: bool) {
    if host(lua).tree.borrow().get_bool(tool, Property::Enabled) == Some(false) {
        return;
    }
    if down {
        host(lua).world.borrow_mut().swings.push(tool);
    }
    let signal = if down { SignalKind::ToolActivated(tool) } else { SignalKind::ToolDeactivated(tool) };
    fire(lua, signal, || Ok(mlua::MultiValue::new()));
}

pub(crate) fn set_mouse(lua: &Lua, ray: Option<crate::MouseRay>) {
    host(lua).mouse.set(ray);
}

/// What the mouse ray hits, ignoring the local character: the point and the part.
fn mouse_hit(lua: &Lua) -> Option<(crate::MouseRay, Vec3, Option<InstanceId>)> {
    let ray = host(lua).mouse.get()?;
    let character = local_character(lua);
    let host = host(lua);
    let tree = host.tree.borrow();
    let hit = geometry::raycast_indexed(
        &tree,
        &crate::bindings::part_index(&host, &tree),
        ray.origin,
        ray.direction * mouse::REACH,
        |part| character.is_none_or(|c| !tree.is_descendant_of(part, c).unwrap_or(false)),
    );
    Some(match hit {
        Some(hit) => (ray, hit.position, Some(hit.part)),
        None => (ray, ray.origin + ray.direction * mouse::REACH, None),
    })
}

/// The local player's mouse.
pub(crate) struct Mouse;

impl UserData for Mouse {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get(mouse::HIT, |lua, _| {
            Ok(LuaCFrame(match mouse_hit(lua) {
                Some((ray, at, _)) => CFrame::look_at(at, at + ray.direction),
                None => CFrame::IDENTITY,
            }))
        });
        fields.add_field_method_get(mouse::TARGET, |lua, _| {
            match mouse_hit(lua).and_then(|(_, _, part)| part) {
                Some(part) => Ok(Value::UserData(push_instance(lua, part)?)),
                None => Ok(Value::Nil),
            }
        });
        fields.add_field_method_get(mouse::X, |lua, _| Ok(host(lua).mouse.get().map_or(0.0, |m| m.x)));
        fields.add_field_method_get(mouse::Y, |lua, _| Ok(host(lua).mouse.get().map_or(0.0, |m| m.y)));
        fields.add_field_method_get(names::events::BUTTON1_DOWN, |_, _| {
            Ok(Signal(SignalKind::MouseButton1Down))
        });
        fields.add_field_method_get(names::events::BUTTON1_UP, |_, _| Ok(Signal(SignalKind::MouseButton1Up)));
        for name in [mouse::KEY_DOWN, mouse::OLD_KEY_DOWN] {
            fields.add_field_method_get(name, |_, _| Ok(Signal(SignalKind::MouseKeyDown)));
        }
        fields.add_field_method_get(mouse::KEY_UP, |_, _| Ok(Signal(SignalKind::MouseKeyUp)));
        fields.add_meta_field(MetaMethod::Type, mouse::TYPE_NAME);
    }

    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        // The rest of a mouse's members (TargetFilter, Icon, UnitRay…).
        crate::lenient::add_service_members(methods, mouse::TYPE_NAME);
    }
}

/// Puts every tool `character` holds into its player's backpack.
fn unequip_all(lua: &Lua, character: InstanceId) -> mlua::Result<()> {
    let host = host(lua);
    let mut tree = host.tree.borrow_mut();
    let players = tree.service(ClassKind::Players);
    let player = players.and_then(|p| {
        let children = tree.children(p).ok()?;
        children.iter().copied().find(|c| tree.get_ref(*c, Property::Character) == Some(character))
    });
    let backpack = player.and_then(|p| tree.find_first_child_of_class(p, ClassKind::Backpack).ok().flatten());
    let held: Vec<InstanceId> = tree
        .children(character)
        .map_err(tree_err)?
        .iter()
        .copied()
        .filter(|c| tree.class(*c) == Ok(ClassKind::Tool))
        .collect();
    for tool in held {
        tree.set_parent(tool, backpack).map_err(tree_err)?;
    }
    Ok(())
}

fn humanoid_character(lua: &Lua, humanoid: InstanceId) -> mlua::Result<InstanceId> {
    let parent = host(lua).tree.borrow().parent(humanoid).map_err(tree_err)?;
    parent
        .filter(|p| is_character(&host(lua).tree.borrow(), *p))
        .ok_or_else(|| runtime_err("this humanoid isn't in a character"))
}

/// Attribute, tag, tool and player members, added to the instance userdata.
pub(crate) fn add_instance_methods<M: UserDataMethods<LuaInstance>>(m: &mut M) {
    m.add_method("Activate", |lua, this, ()| {
        crate::bindings::require_class(lua, this.0, ClassKind::Tool, "Activate")?;
        activate(lua, this.0, true);
        Ok(())
    });
    m.add_method("Deactivate", |lua, this, ()| {
        crate::bindings::require_class(lua, this.0, ClassKind::Tool, "Deactivate")?;
        activate(lua, this.0, false);
        Ok(())
    });
    m.add_method(methods::SET_ATTRIBUTE, |lua, this, (name, value): (String, Value)| {
        check_attribute_name(&name)?;
        let value = match attribute_from_lua(&value)? {
            None if !value.is_nil() => Some(rich_attribute(lua, this.0, &name, value)?),
            other => other,
        };
        host(lua).tree.borrow_mut().set_attribute(this.0, &name, value).map_err(tree_err)
    });
    m.add_method(methods::GET_ATTRIBUTE, |lua, this, name: String| {
        let value = host(lua).tree.borrow().get_attribute(this.0, &name).map_err(tree_err)?;
        value.map(|v| attribute_to_lua(lua, v)).transpose().map(|v| v.unwrap_or(Value::Nil))
    });
    m.add_method(methods::GET_ATTRIBUTES, |lua, this, ()| {
        let all = host(lua).tree.borrow().attributes(this.0).map_err(tree_err)?.clone();
        let table = lua.create_table()?;
        for (name, value) in all {
            table.raw_set(name, attribute_to_lua(lua, value)?)?;
        }
        Ok(table)
    });
    m.add_method(methods::GET_ATTRIBUTE_CHANGED_SIGNAL, |lua, this, name: String| {
        Ok(Signal(SignalKind::AttributeNamedChanged(this.0, intern(lua, &name))))
    });
    m.add_method(methods::ADD_TAG, |lua, this, tag: String| add_tag(lua, this.0, &tag));
    m.add_method(methods::REMOVE_TAG, |lua, this, tag: String| remove_tag(lua, this.0, &tag));
    m.add_method(methods::HAS_TAG, |lua, this, tag: String| has_tag(lua, this.0, &tag));
    m.add_method(methods::GET_TAGS, |lua, this, ()| get_tags(lua, this.0));
    m.add_method(methods::EQUIP_TOOL, |lua, this, tool: Value| {
        require_class(lua, this.0, ClassKind::Humanoid, methods::EQUIP_TOOL)?;
        let tool = as_instance(&tool)
            .filter(|t| host(lua).tree.borrow().class(*t) == Ok(ClassKind::Tool))
            .ok_or_else(|| runtime_err("EquipTool expects a Tool"))?;
        let character = humanoid_character(lua, this.0)?;
        unequip_all(lua, character)?;
        host(lua).tree.borrow_mut().set_parent(tool, Some(character)).map_err(tree_err)
    });
    m.add_method(methods::UNEQUIP_TOOLS, |lua, this, ()| {
        require_class(lua, this.0, ClassKind::Humanoid, methods::UNEQUIP_TOOLS)?;
        let character = humanoid_character(lua, this.0)?;
        unequip_all(lua, character)
    });
    m.add_method(methods::GET_MOUSE, |lua, this, ()| {
        require_class(lua, this.0, ClassKind::Player, methods::GET_MOUSE)?;
        Ok(Mouse)
    });
}
