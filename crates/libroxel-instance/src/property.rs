//! Typed properties.

use crate::{Color3, EnumKind, EnumValue, InstanceId, UDim, UDim2, Vec2, Vec3};

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
    /// On a GUI object: where it goes in a list layout (lowest first).
    LayoutOrder,
    /// On a text box: the grey hint shown while it is empty.
    PlaceholderText,
    /// On a text box: whether clicking into it empties it.
    ClearTextOnFocus,
    /// On a camera: how tall the view is, in degrees.
    FieldOfView,
    /// On a camera: whether it follows the character or scripts move it.
    CameraType,
    /// On a light: how far it reaches, in studs.
    Range,
    /// On a spot light: how wide its cone is, in degrees.
    Angle,
    /// On a UI corner: how round the corners are.
    CornerRadius,
    /// On a list layout: rows or columns.
    FillDirection,
    /// On a list layout: the space between items.
    Padding,
    /// On an explosion: how far it reaches, in studs.
    BlastRadius,
    /// On an explosion: how hard it pushes.
    BlastPressure,
    /// On a humanoid: set to make it jump once.
    Jump,
    /// On a player: the team they're on.
    Team,
    /// On a player: on no team. On a spawn location: anyone may spawn there.
    Neutral,
    /// On a team: its colour. On a spawn location: which team spawns there.
    TeamColor,
    /// On a team: whether players joining are put on it.
    AutoAssignable,
    /// On the workspace: how hard things fall, in studs per second squared.
    Gravity,
    /// On the players service: seconds between dying and coming back.
    RespawnTime,
    /// On the players service: whether characters spawn by themselves.
    CharacterAutoLoads,
    /// On a weld: the first part it holds.
    Part0,
    /// On a weld: the second part it holds.
    Part1,
    /// On a seat: the humanoid sitting in it. Set by the engine.
    Occupant,
    /// On a humanoid: whether it is sitting.
    Sit,
    /// On a humanoid: the seat it sits in.
    SeatPart,
    /// On a vehicle seat: top speed, in studs per second.
    MaxSpeed,
    /// On a vehicle seat: how quickly it gets up to speed.
    Torque,
    /// On a vehicle seat: how fast it turns, in radians per second.
    TurnSpeed,
    /// On a vehicle seat: -1 (back), 0 or 1 (forward), from the driver.
    Throttle,
    /// On a vehicle seat: -1 (left), 0 or 1 (right), from the driver.
    Steer,
    /// The instance an object value links to.
    ObjectValue,
    Vector3Value,
    Color3Value,
    /// On the starter player: the walk speed new characters get.
    CharacterWalkSpeed,
    /// On the starter player: the jump power new characters get.
    CharacterJumpPower,
    /// On lighting: the colour shadows are filled with.
    Ambient,
    /// On lighting: the colour far-away things fade into.
    FogColor,
    /// On lighting: how far away (in studs) the fog starts.
    FogStart,
    /// On lighting: how far away (in studs) everything is fully fogged.
    FogEnd,
    /// On the workspace: loose parts below this height are destroyed.
    FallenPartsDestroyHeight,
    /// On a constraint or mover: the first attachment it holds.
    Attachment0,
    /// On a constraint: the second attachment it holds.
    Attachment1,
    /// On a linear velocity: the velocity it pushes towards, in studs per second.
    VectorVelocity,
    /// On a mover: the strongest push it can give.
    MaxForce,
    /// On a force: the push it gives every moment.
    Force,
    /// On an align position: the fastest it moves its part.
    MaxVelocity,
    /// On an align position: how quickly it gets there.
    Responsiveness,
    /// On an align position: snap straight there.
    RigidityEnabled,
    /// On a rope: how long it is, in studs.
    Length,
    /// On a spring: how long it is when nothing pulls on it.
    FreeLength,
    /// On a spring: how hard it pulls back.
    Stiffness,
    /// On a spring: how quickly its bouncing dies down.
    Damping,
    /// On a rope or spring: how thick it is drawn, in studs.
    Thickness,
    /// On a body velocity: the velocity it pushes towards.
    Velocity,
    /// On an older body mover: the strongest push on each axis.
    BodyMaxForce,
    /// On an older body mover: how hard it pushes.
    BodyPower,
    /// On a body position: how much it slows down to stop overshooting.
    BodyDamping,
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
    /// A link to another instance. Places save links between saved instances.
    Ref(Option<InstanceId>),
    Vec2(Vec2),
    UDim2(UDim2),
    UDim(UDim),
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
            PropertyValue::UDim(_) => "UDim",
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
            | Property::ObjectText
            | Property::PlaceholderText => "string",
            Property::Position | Property::Size | Property::Orientation | Property::StudsOffset => "Vec3",
            Property::Color | Property::BackgroundColor | Property::TextColor => "Color3",
            Property::Color3Value | Property::Ambient | Property::FogColor => "Color3",
            Property::Vector3Value => "Vec3",
            Property::VectorVelocity | Property::Force | Property::Velocity | Property::BodyMaxForce => {
                "Vec3"
            }
            Property::RigidityEnabled => "bool",
            Property::MaxForce
            | Property::MaxVelocity
            | Property::Responsiveness
            | Property::Length
            | Property::FreeLength
            | Property::Stiffness
            | Property::Damping
            | Property::Thickness
            | Property::BodyPower
            | Property::BodyDamping => "number",
            Property::CharacterWalkSpeed
            | Property::CharacterJumpPower
            | Property::FogStart
            | Property::FogEnd
            | Property::FallenPartsDestroyHeight => "number",
            Property::Anchored
            | Property::CanCollide
            | Property::Disabled
            | Property::BoolValue
            | Property::Visible
            | Property::Enabled
            | Property::ResetOnSpawn
            | Property::AlwaysOnTop
            | Property::ClearTextOnFocus
            | Property::Jump
            | Property::Neutral
            | Property::AutoAssignable
            | Property::CharacterAutoLoads
            | Property::Sit => "bool",
            Property::TeamColor => "Color3",
            Property::CornerRadius | Property::Padding => "UDim",
            Property::CameraType => EnumKind::CameraType.kind_name(),
            Property::FillDirection => EnumKind::FillDirection.kind_name(),
            Property::GuiPosition | Property::GuiSize => "UDim2",
            Property::AnchorPoint => "Vec2",
            Property::BackgroundTransparency
            | Property::TextSize
            | Property::MaxActivationDistance
            | Property::ClockTime
            | Property::Brightness
            | Property::HoldDuration
            | Property::MaxDistance
            | Property::LayoutOrder
            | Property::FieldOfView
            | Property::Range
            | Property::Angle
            | Property::BlastRadius
            | Property::BlastPressure
            | Property::Gravity
            | Property::RespawnTime
            | Property::MaxSpeed
            | Property::Torque
            | Property::TurnSpeed
            | Property::Throttle
            | Property::Steer => "number",
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
            Property::Character
            | Property::Team
            | Property::Part0
            | Property::Part1
            | Property::Occupant
            | Property::SeatPart
            | Property::ObjectValue
            | Property::Attachment0
            | Property::Attachment1 => "ref",
        }
    }

    /// The enumeration this property takes values from, if it is an enum property.
    pub fn enum_kind(self) -> Option<EnumKind> {
        match self {
            Property::Shape => Some(EnumKind::Shape),
            Property::Material => Some(EnumKind::Material),
            Property::KeyboardKeyCode => Some(EnumKind::KeyCode),
            Property::CameraType => Some(EnumKind::CameraType),
            Property::FillDirection => Some(EnumKind::FillDirection),
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
        Property::LayoutOrder,
        Property::PlaceholderText,
        Property::ClearTextOnFocus,
        Property::FieldOfView,
        Property::CameraType,
        Property::Range,
        Property::Angle,
        Property::CornerRadius,
        Property::FillDirection,
        Property::Padding,
        Property::BlastRadius,
        Property::BlastPressure,
        Property::Jump,
        Property::Team,
        Property::Neutral,
        Property::TeamColor,
        Property::AutoAssignable,
        Property::Gravity,
        Property::RespawnTime,
        Property::CharacterAutoLoads,
        Property::Part0,
        Property::Part1,
        Property::Occupant,
        Property::Sit,
        Property::SeatPart,
        Property::MaxSpeed,
        Property::Torque,
        Property::TurnSpeed,
        Property::Throttle,
        Property::Steer,
        Property::ObjectValue,
        Property::Vector3Value,
        Property::Color3Value,
        Property::CharacterWalkSpeed,
        Property::CharacterJumpPower,
        Property::Ambient,
        Property::FogColor,
        Property::FogStart,
        Property::FogEnd,
        Property::FallenPartsDestroyHeight,
        Property::Attachment0,
        Property::Attachment1,
        Property::VectorVelocity,
        Property::MaxForce,
        Property::Force,
        Property::MaxVelocity,
        Property::Responsiveness,
        Property::RigidityEnabled,
        Property::Length,
        Property::FreeLength,
        Property::Stiffness,
        Property::Damping,
        Property::Thickness,
        Property::Velocity,
        Property::BodyMaxForce,
        Property::BodyPower,
        Property::BodyDamping,
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
            Property::LayoutOrder => "layout_order",
            Property::PlaceholderText => "placeholder_text",
            Property::ClearTextOnFocus => "clear_text_on_focus",
            Property::FieldOfView => "field_of_view",
            Property::CameraType => "camera_type",
            Property::Range => "range",
            Property::Angle => "angle",
            Property::CornerRadius => "corner_radius",
            Property::FillDirection => "fill_direction",
            Property::Padding => "padding",
            Property::BlastRadius => "blast_radius",
            Property::BlastPressure => "blast_pressure",
            Property::Jump => "jump",
            Property::Team => "team",
            Property::Neutral => "neutral",
            Property::TeamColor => "team_color",
            Property::AutoAssignable => "auto_assignable",
            Property::Gravity => "gravity",
            Property::RespawnTime => "respawn_time",
            Property::CharacterAutoLoads => "character_auto_loads",
            Property::Part0 => "part0",
            Property::Part1 => "part1",
            Property::Occupant => "occupant",
            Property::Sit => "sit",
            Property::SeatPart => "seat_part",
            Property::MaxSpeed => "max_speed",
            Property::Torque => "torque",
            Property::TurnSpeed => "turn_speed",
            Property::Throttle => "throttle",
            Property::Steer => "steer",
            Property::ObjectValue => "object_value",
            Property::Vector3Value => "vector3_value",
            Property::Color3Value => "color3_value",
            Property::CharacterWalkSpeed => "character_walk_speed",
            Property::CharacterJumpPower => "character_jump_power",
            Property::Ambient => "ambient",
            Property::FogColor => "fog_color",
            Property::FogStart => "fog_start",
            Property::FogEnd => "fog_end",
            Property::FallenPartsDestroyHeight => "fallen_parts_destroy_height",
            Property::Attachment0 => "attachment0",
            Property::Attachment1 => "attachment1",
            Property::VectorVelocity => "vector_velocity",
            Property::MaxForce => "max_force",
            Property::Force => "force",
            Property::MaxVelocity => "max_velocity",
            Property::Responsiveness => "responsiveness",
            Property::RigidityEnabled => "rigidity_enabled",
            Property::Length => "length",
            Property::FreeLength => "free_length",
            Property::Stiffness => "stiffness",
            Property::Damping => "damping",
            Property::Thickness => "thickness",
            Property::Velocity => "velocity",
            Property::BodyMaxForce => "body_max_force",
            Property::BodyPower => "body_power",
            Property::BodyDamping => "body_damping",
        }
    }

    pub fn from_file_name(name: &str) -> Option<Property> {
        Property::ALL.iter().copied().find(|p| p.file_name() == name)
    }
}
