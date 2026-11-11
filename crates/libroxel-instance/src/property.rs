//! Typed properties.

use crate::{Color3, EnumKind, EnumValue, InstanceId, Vec3};

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
        }
    }
}

impl Property {
    /// The kind of value this property holds, as [`PropertyValue::kind_name`] spells it.
    pub fn expected_kind(self) -> &'static str {
        match self {
            Property::Name | Property::Source | Property::StringValue => "string",
            Property::Position | Property::Size | Property::Orientation => "Vec3",
            Property::Color => "Color3",
            Property::Anchored | Property::CanCollide | Property::Disabled | Property::BoolValue => "bool",
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
        }
    }

    pub fn from_file_name(name: &str) -> Option<Property> {
        Property::ALL.iter().copied().find(|p| p.file_name() == name)
    }
}
