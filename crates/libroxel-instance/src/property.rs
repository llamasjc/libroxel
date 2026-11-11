//! Typed properties.

use crate::{Color3, EnumKind, EnumValue, InstanceId, UDim2, Vec2, Vec3};

/// Every property an instance can have.
///
/// Like [`ClassKind`](crate::ClassKind), this is internal vocabulary: the
/// scripting layer decides what each one is called.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Property {
    Name,
    Position,
    Size,
    Color,
    Anchored,
    CanCollide,
    Transparency,
    Disabled,
    Source,
    Health,
    MaxHealth,
    WalkSpeed,
    JumpPower,
    /// Rotation in degrees: applied around Y, then X, then Z.
    Orientation,
    Shape,
    Material,
    /// The value held by a number value object (whole numbers on an integer one).
    NumberValue,
    StringValue,
    BoolValue,
    /// On a player: the model they control, if any.
    Character,
    UserId,
    /// A GUI object's place inside its parent.
    GuiPosition,
    GuiSize,
    /// Which point of a GUI object sits at its position (0,0 top-left, 1,1 bottom-right).
    AnchorPoint,
    BackgroundColor,
    BackgroundTransparency,
    Visible,
    Text,
    TextColor,
    TextSize,
    /// Whether a screen GUI is shown.
    Enabled,
    /// Whether a screen GUI is put back fresh each time the character respawns.
    ResetOnSpawn,
    /// How far away (in studs) a player can be to click a part.
    MaxActivationDistance,
    /// Hours since midnight, 0 to 24.
    ClockTime,
    Brightness,
    /// On a prompt: what it does ("Open") and what it's on ("Door").
    ActionText,
    ObjectText,
    /// On a prompt: the key that triggers it.
    KeyboardKeyCode,
    /// On a prompt: how long the key must be held, in seconds.
    HoldDuration,
    /// On a billboard: how far from its part it floats, in studs.
    StudsOffset,
    /// On a billboard: drawn even when something is in front of it.
    AlwaysOnTop,
    /// On a billboard: hidden when the camera is further away than this, in studs.
    MaxDistance,
}

/// The value of a property.
#[derive(Debug, Clone, PartialEq)]
pub enum PropertyValue {
    Bool(bool),
    Number(f64),
    String(String),
    Vec3(Vec3),
    Color3(Color3),
    Enum(EnumValue),
    /// A link to another instance. Links are runtime-only and are not saved.
    Ref(Option<InstanceId>),
    Vec2(Vec2),
    UDim2(UDim2),
}

impl PropertyValue {
    /// A short name for the kind of value, used in error messages.
    pub fn kind_name(&self) -> &'static str {
        match self {
            PropertyValue::Bool(_) => "bool",
            PropertyValue::Number(_) => "number",
            PropertyValue::String(_) => "string",
            PropertyValue::Vec3(_) => "Vec3",
            PropertyValue::Color3(_) => "Color3",
            PropertyValue::Enum(v) => v.kind().kind_name(),
            PropertyValue::Ref(_) => "ref",
            PropertyValue::Vec2(_) => "Vec2",
            PropertyValue::UDim2(_) => "UDim2",
        }
    }
}

impl Property {
    /// The kind of value this property holds, as [`PropertyValue::kind_name`] spells it.
    pub fn expected_kind(self) -> &'static str {
        match self {
            Property::Name
            | Property::Source
            | Property::StringValue
            | Property::Text
            | Property::ActionText
            | Property::ObjectText => "string",
            Property::Position | Property::Size | Property::Orientation | Property::StudsOffset => "Vec3",
            Property::Color | Property::BackgroundColor | Property::TextColor => "Color3",
            Property::Anchored
            | Property::CanCollide
            | Property::Disabled
            | Property::BoolValue
            | Property::Visible
            | Property::Enabled
            | Property::ResetOnSpawn
            | Property::AlwaysOnTop => "bool",
            Property::GuiPosition | Property::GuiSize => "UDim2",
            Property::AnchorPoint => "Vec2",
            Property::BackgroundTransparency
            | Property::TextSize
            | Property::MaxActivationDistance
            | Property::ClockTime
            | Property::Brightness
            | Property::HoldDuration
            | Property::MaxDistance => "number",
            Property::KeyboardKeyCode => EnumKind::KeyCode.kind_name(),
            Property::Transparency
            | Property::Health
            | Property::MaxHealth
            | Property::WalkSpeed
            | Property::JumpPower
            | Property::NumberValue
            | Property::UserId => "number",
            Property::Shape => EnumKind::Shape.kind_name(),
            Property::Material => EnumKind::Material.kind_name(),
            Property::Character => "ref",
        }
    }

    /// The enumeration this property takes values from, if it is an enum property.
    pub fn enum_kind(self) -> Option<EnumKind> {
        match self {
            Property::Shape => Some(EnumKind::Shape),
            Property::Material => Some(EnumKind::Material),
            Property::KeyboardKeyCode => Some(EnumKind::KeyCode),
            _ => None,
        }
    }

    /// Every property, in a stable order.
    pub const ALL: &[Property] = &[
        Property::Name,
        Property::Position,
        Property::Size,
        Property::Color,
        Property::Anchored,
        Property::CanCollide,
        Property::Transparency,
        Property::Disabled,
        Property::Source,
        Property::Health,
        Property::MaxHealth,
        Property::WalkSpeed,
        Property::JumpPower,
        Property::Orientation,
        Property::Shape,
        Property::Material,
        Property::NumberValue,
        Property::StringValue,
        Property::BoolValue,
        Property::Character,
        Property::UserId,
        Property::GuiPosition,
        Property::GuiSize,
        Property::AnchorPoint,
        Property::BackgroundColor,
        Property::BackgroundTransparency,
        Property::Visible,
        Property::Text,
        Property::TextColor,
        Property::TextSize,
        Property::Enabled,
        Property::ResetOnSpawn,
        Property::MaxActivationDistance,
        Property::ClockTime,
        Property::Brightness,
        Property::ActionText,
        Property::ObjectText,
        Property::KeyboardKeyCode,
        Property::HoldDuration,
        Property::StudsOffset,
        Property::AlwaysOnTop,
        Property::MaxDistance,
    ];

    /// A stable name for files. Never shown to scripts; never change these.
    pub fn file_name(self) -> &'static str {
        match self {
            Property::Name => "name",
            Property::Position => "position",
            Property::Size => "size",
            Property::Color => "color",
            Property::Anchored => "anchored",
            Property::CanCollide => "can_collide",
            Property::Transparency => "transparency",
            Property::Disabled => "disabled",
            Property::Source => "source",
            Property::Health => "health",
            Property::MaxHealth => "max_health",
            Property::WalkSpeed => "walk_speed",
            Property::JumpPower => "jump_power",
            Property::Orientation => "orientation",
            Property::Shape => "shape",
            Property::Material => "material",
            Property::NumberValue => "number_value",
            Property::StringValue => "string_value",
            Property::BoolValue => "bool_value",
            Property::Character => "character",
            Property::UserId => "user_id",
            Property::GuiPosition => "gui_position",
            Property::GuiSize => "gui_size",
            Property::AnchorPoint => "anchor_point",
            Property::BackgroundColor => "background_color",
            Property::BackgroundTransparency => "background_transparency",
            Property::Visible => "visible",
            Property::Text => "text",
            Property::TextColor => "text_color",
            Property::TextSize => "text_size",
            Property::Enabled => "enabled",
            Property::ResetOnSpawn => "reset_on_spawn",
            Property::MaxActivationDistance => "max_activation_distance",
            Property::ClockTime => "clock_time",
            Property::Brightness => "brightness",
            Property::ActionText => "action_text",
            Property::ObjectText => "object_text",
            Property::KeyboardKeyCode => "keyboard_key_code",
            Property::HoldDuration => "hold_duration",
            Property::StudsOffset => "studs_offset",
            Property::AlwaysOnTop => "always_on_top",
            Property::MaxDistance => "max_distance",
        }
    }

    pub fn from_file_name(name: &str) -> Option<Property> {
        Property::ALL.iter().copied().find(|p| p.file_name() == name)
    }
}
