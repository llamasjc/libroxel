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
    /// On a part: how mirror-like it is, from 0 to 1.
    Reflectance,
    /// On a part: whether it casts a shadow.
    CastShadow,
    /// On a part: whether it fires Touched and TouchEnded.
    CanTouch,
    /// On a part: whether ray and region searches find it.
    CanQuery,
    /// On a part welded to others: whether it adds nothing to their weight.
    Massless,
    /// On a part: whether the editor won't pick it in the view.
    Locked,
    /// On a part: the collision group it's in (see PhysicsService).
    CollisionGroup,
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
    /// On a hinge: whether it swings freely, runs as a motor, or holds an angle.
    ActuatorType,
    /// On a hinge motor: how fast it turns, in radians per second.
    MotorVelocity,
    /// On a hinge motor: the most turning force it has.
    MotorMaxTorque,
    /// On a hinge servo: the angle it turns to, in degrees.
    TargetAngle,
    /// On a hinge servo: how fast it turns there, in radians per second.
    AngularSpeed,
    /// On a hinge servo: the most turning force it has.
    ServoMaxTorque,
    /// On a hinge: the angle it's at now, in degrees (set while playing).
    CurrentAngle,
    /// On a slider: whether it stops at LowerLimit and UpperLimit.
    LimitsEnabled,
    /// On a slider: the least it slides, in studs along its axis.
    LowerLimit,
    /// On a slider: the most it slides, in studs along its axis.
    UpperLimit,
    /// On a slider: how far along its axis it is now, in studs (set while playing).
    CurrentPosition,
    /// On a spinner: the spin it aims for, in radians per second about each axis.
    AngularVelocity,
    /// On an angular velocity: the most turning force it has.
    MaxTorque,
    /// On a body angular velocity: the most turning force about each axis.
    BodyMaxTorque,
    /// On a sound: the file to play (or a built-in libroxel:// sound).
    SoundId,
    /// On a sound: how loud, 0 to 10 (1 is the file as it is).
    Volume,
    /// On a sound: how fast (and how high) it plays; 1 is normal.
    PlaybackSpeed,
    /// On a sound: whether it starts again when it ends.
    Looped,
    /// On a sound: whether it's playing now.
    Playing,
    /// On a decal or texture: the picture's file name.
    Texture,
    /// On a decal or texture: which side of its part it's on.
    Face,
    /// On a decal or texture: a tint multiplied with the picture.
    DecalColor,
    /// On an animation: which one (a built-in libroxel:// animation).
    AnimationId,
    /// On body colours: the colour of each body part.
    HeadColor,
    TorsoColor,
    LeftArmColor,
    RightArmColor,
    LeftLegColor,
    RightLegColor,
    /// On a texture: how wide each tile is, in studs.
    StudsPerTileU,
    /// On a texture: how tall each tile is, in studs.
    StudsPerTileV,
    /// On a particle emitter: particles sent out each second.
    Rate,
    /// On a particle emitter: how long each particle lasts, as a range of seconds.
    Lifetime,
    /// On a particle emitter: how fast particles leave, as a range of studs per second.
    Speed,
    /// On a particle emitter: how far (in degrees) particles stray from straight out.
    SpreadAngle,
    /// On a particle emitter, fire or smoke: how big it is, in studs.
    EffectSize,
    /// On a particle emitter: how particles speed up each second.
    Acceleration,
    /// On a particle emitter: how quickly particles slow down.
    Drag,
    /// On a particle emitter: how much particles glow (0 to 1).
    LightEmission,
    /// On fire: how fast the flames rise.
    Heat,
    /// On fire: the colour of the flames' tips.
    SecondaryColor,
    /// On smoke: how thick it is (0 to 1).
    Opacity,
    /// On smoke: how fast it rises.
    RiseVelocity,
    /// On sparkles: their colour.
    SparkleColor,
    /// On a highlight: what it highlights (its parent when empty).
    Adornee,
    /// On a highlight: the tint over the inside.
    FillColor,
    /// On a highlight: how see-through the tint is.
    FillTransparency,
    /// On a highlight: the outline's colour.
    OutlineColor,
    /// On a highlight: how see-through the outline is.
    OutlineTransparency,
    /// On a GUI object: higher ones are drawn over lower ones beside them.
    ZIndex,
    /// On a GUI object: how thick its border is, in pixels.
    BorderSizePixel,
    /// On a GUI object: its border's colour.
    BorderColor,
    /// On a GUI object: hides the parts of what's inside that stick out.
    ClipsDescendants,
    /// On a text object: how see-through the text is.
    TextTransparency,
    /// On a text object: left, centre or right.
    TextXAlignment,
    /// On a text object: top, centre or bottom.
    TextYAlignment,
    /// On a text object: long text goes onto more lines.
    TextWrapped,
    /// On a text object: the text is made as big as fits.
    TextScaled,
    /// On a scrolling frame: how big its inside is.
    CanvasSize,
    /// On a scrolling frame: how far it is scrolled, in pixels.
    CanvasPosition,
    /// On a scrolling frame: how wide its scroll bar is, in pixels.
    ScrollBarThickness,
    /// On an image label or button: the picture's file name.
    Image,
    /// On an image: a tint multiplied with the picture.
    ImageColor,
    /// On an image: how see-through the picture is.
    ImageTransparency,
    /// On an image: how the picture fills the box.
    ScaleType,
    /// On a UI padding: the space on the left.
    PaddingLeft,
    /// On a UI padding: the space on the right.
    PaddingRight,
    /// On a UI padding: the space at the top.
    PaddingTop,
    /// On a UI padding: the space at the bottom.
    PaddingBottom,
    /// On a grid layout: how big each cell is.
    CellSize,
    /// On a grid layout: the space between cells.
    CellPadding,
    /// On an aspect ratio constraint: width divided by height.
    AspectRatio,
    /// On a UI scale: how much bigger (or smaller, below 1).
    Scale,
    /// On a weld or motor: where the joint sits on Part0, as a position and
    /// an orientation (together the script-facing C0).
    C0Position,
    C0Orientation,
    /// On a weld or motor: where the joint sits on Part1 (the script-facing C1).
    C1Position,
    C1Orientation,
    /// On a motor: the angle (in radians) it turns towards about the
    /// joint's Z axis. Its CurrentAngle is in radians too, and its
    /// MaxVelocity is radians each 1/60 of a second.
    DesiredAngle,
    /// On a model: the part its pivot follows (and that PivotTo moves it by).
    PrimaryPart,
    /// On an atmosphere: how thick the haze is, 0 to 1.
    Density,
    /// On an atmosphere: extra haze towards the horizon, 0 to 10.
    Haze,
    /// On a bloom effect: how strongly bright things glow (scripts call it Intensity).
    BloomIntensity,
    /// On a bloom effect or a blur: how far the glow spreads (scripts call it Size).
    BloomSize,
    /// On a bloom effect: how bright something must be to glow.
    Threshold,
    /// On a colour correction: more (or, below 0, less) contrast.
    Contrast,
    /// On a colour correction: more (or, below 0, less) colour; -1 is grey.
    Saturation,
    /// On a colour correction: a colour the whole view is multiplied by.
    TintColor,
    /// On a selection box: the outline's colour (scripts call it Color3).
    SelectionColor,
    /// On a selection box: how thick the outline is, in studs.
    LineThickness,
    /// On a beam: how wide it is at Attachment0, in studs.
    Width0,
    /// On a beam: how wide it is at Attachment1, in studs.
    Width1,
    /// On a size constraint: the smallest its GUI object may be, in pixels.
    MinSize,
    /// On a size constraint: the biggest its GUI object may be, in pixels.
    MaxSize,
    /// On a text size constraint: the smallest its text may be.
    MinTextSize,
    /// On a text size constraint: the biggest its text may be.
    MaxTextSize,
    /// On a sky: how many stars show at night.
    StarCount,
    /// On a sky: the sun and moon are drawn.
    CelestialBodiesShown,
    /// On a sky: how wide the sun looks, in degrees.
    SunAngularSize,
    /// On a sky: how wide the moon looks, in degrees.
    MoonAngularSize,
    /// On a trail: how long each bit of it lasts, in seconds (scripts call it Lifetime).
    TrailLifetime,
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
            | Property::PlaceholderText
            | Property::CollisionGroup => "string",
            Property::Position | Property::Size | Property::Orientation | Property::StudsOffset => "Vec3",
            Property::Color | Property::BackgroundColor | Property::TextColor => "Color3",
            Property::Color3Value | Property::Ambient | Property::FogColor => "Color3",
            Property::Vector3Value => "Vec3",
            Property::VectorVelocity | Property::Force | Property::Velocity | Property::BodyMaxForce => {
                "Vec3"
            }
            Property::RigidityEnabled | Property::LimitsEnabled => "bool",
            Property::Lifetime | Property::Speed | Property::SpreadAngle => "Vec2",
            Property::Acceleration => "Vec3",
            Property::SecondaryColor
            | Property::SparkleColor
            | Property::FillColor
            | Property::OutlineColor => "Color3",
            Property::Adornee => "ref",
            Property::BorderColor => "Color3",
            Property::ClipsDescendants | Property::TextWrapped | Property::TextScaled => "bool",
            Property::TextXAlignment => EnumKind::TextXAlignment.kind_name(),
            Property::TextYAlignment => EnumKind::TextYAlignment.kind_name(),
            Property::CanvasSize | Property::CellSize | Property::CellPadding => "UDim2",
            Property::CanvasPosition => "Vec2",
            Property::Image => "string",
            Property::ImageColor => "Color3",
            Property::ImageTransparency => "number",
            Property::ScaleType => EnumKind::ScaleType.kind_name(),
            Property::PaddingLeft
            | Property::PaddingRight
            | Property::PaddingTop
            | Property::PaddingBottom => "UDim",
            Property::ZIndex
            | Property::BorderSizePixel
            | Property::TextTransparency
            | Property::ScrollBarThickness
            | Property::AspectRatio
            | Property::Scale
            | Property::DesiredAngle
            | Property::Density
            | Property::Haze
            | Property::BloomIntensity
            | Property::BloomSize
            | Property::Threshold
            | Property::Contrast
            | Property::Saturation
            | Property::LineThickness
            | Property::Width0
            | Property::Width1
            | Property::MinTextSize
            | Property::MaxTextSize
            | Property::StarCount
            | Property::SunAngularSize
            | Property::MoonAngularSize
            | Property::TrailLifetime => "number",
            Property::CelestialBodiesShown => "bool",
            Property::TintColor | Property::SelectionColor => "Color3",
            Property::MinSize | Property::MaxSize => "Vec2",
            Property::C0Position
            | Property::C0Orientation
            | Property::C1Position
            | Property::C1Orientation => "Vec3",
            Property::Rate
            | Property::EffectSize
            | Property::Drag
            | Property::LightEmission
            | Property::Heat
            | Property::Opacity
            | Property::RiseVelocity
            | Property::FillTransparency
            | Property::OutlineTransparency => "number",
            Property::MaxForce
            | Property::MaxVelocity
            | Property::Responsiveness
            | Property::Length
            | Property::FreeLength
            | Property::Stiffness
            | Property::Damping
            | Property::Thickness
            | Property::BodyPower
            | Property::BodyDamping
            | Property::MotorVelocity
            | Property::MotorMaxTorque
            | Property::TargetAngle
            | Property::AngularSpeed
            | Property::ServoMaxTorque
            | Property::CurrentAngle
            | Property::LowerLimit
            | Property::UpperLimit
            | Property::CurrentPosition
            | Property::MaxTorque
            | Property::StudsPerTileU
            | Property::StudsPerTileV => "number",
            Property::Texture | Property::SoundId | Property::AnimationId => "string",
            Property::Volume | Property::PlaybackSpeed => "number",
            Property::Looped | Property::Playing => "bool",
            Property::DecalColor
            | Property::HeadColor
            | Property::TorsoColor
            | Property::LeftArmColor
            | Property::RightArmColor
            | Property::LeftLegColor
            | Property::RightLegColor => "Color3",
            Property::Face => EnumKind::NormalId.kind_name(),
            Property::AngularVelocity | Property::BodyMaxTorque => "Vec3",
            Property::ActuatorType => EnumKind::ActuatorType.kind_name(),
            Property::CharacterWalkSpeed
            | Property::CharacterJumpPower
            | Property::FogStart
            | Property::FogEnd
            | Property::FallenPartsDestroyHeight => "number",
            Property::Anchored
            | Property::CanCollide
            | Property::CastShadow
            | Property::CanTouch
            | Property::CanQuery
            | Property::Massless
            | Property::Locked
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
            | Property::Reflectance
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
            | Property::Attachment1
            | Property::PrimaryPart => "ref",
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
            Property::TextXAlignment => Some(EnumKind::TextXAlignment),
            Property::ScaleType => Some(EnumKind::ScaleType),
            Property::ActuatorType => Some(EnumKind::ActuatorType),
            Property::Face => Some(EnumKind::NormalId),
            Property::TextYAlignment => Some(EnumKind::TextYAlignment),
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
        Property::Reflectance,
        Property::CastShadow,
        Property::CanTouch,
        Property::CanQuery,
        Property::Massless,
        Property::Locked,
        Property::CollisionGroup,
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
        Property::ActuatorType,
        Property::MotorVelocity,
        Property::MotorMaxTorque,
        Property::TargetAngle,
        Property::AngularSpeed,
        Property::ServoMaxTorque,
        Property::CurrentAngle,
        Property::LimitsEnabled,
        Property::LowerLimit,
        Property::UpperLimit,
        Property::CurrentPosition,
        Property::AngularVelocity,
        Property::MaxTorque,
        Property::BodyMaxTorque,
        Property::SoundId,
        Property::Volume,
        Property::PlaybackSpeed,
        Property::Looped,
        Property::Playing,
        Property::Texture,
        Property::Face,
        Property::DecalColor,
        Property::AnimationId,
        Property::HeadColor,
        Property::TorsoColor,
        Property::LeftArmColor,
        Property::RightArmColor,
        Property::LeftLegColor,
        Property::RightLegColor,
        Property::StudsPerTileU,
        Property::StudsPerTileV,
        Property::Rate,
        Property::Lifetime,
        Property::Speed,
        Property::SpreadAngle,
        Property::EffectSize,
        Property::Acceleration,
        Property::Drag,
        Property::LightEmission,
        Property::Heat,
        Property::SecondaryColor,
        Property::Opacity,
        Property::RiseVelocity,
        Property::SparkleColor,
        Property::Adornee,
        Property::FillColor,
        Property::FillTransparency,
        Property::OutlineColor,
        Property::OutlineTransparency,
        Property::ZIndex,
        Property::BorderSizePixel,
        Property::BorderColor,
        Property::ClipsDescendants,
        Property::TextTransparency,
        Property::TextXAlignment,
        Property::TextYAlignment,
        Property::TextWrapped,
        Property::TextScaled,
        Property::CanvasSize,
        Property::CanvasPosition,
        Property::ScrollBarThickness,
        Property::Image,
        Property::ImageColor,
        Property::ImageTransparency,
        Property::ScaleType,
        Property::PaddingLeft,
        Property::PaddingRight,
        Property::PaddingTop,
        Property::PaddingBottom,
        Property::CellSize,
        Property::CellPadding,
        Property::AspectRatio,
        Property::Scale,
        Property::C0Position,
        Property::C0Orientation,
        Property::C1Position,
        Property::C1Orientation,
        Property::DesiredAngle,
        Property::PrimaryPart,
        Property::Density,
        Property::Haze,
        Property::BloomIntensity,
        Property::BloomSize,
        Property::Threshold,
        Property::Contrast,
        Property::Saturation,
        Property::TintColor,
        Property::SelectionColor,
        Property::LineThickness,
        Property::Width0,
        Property::Width1,
        Property::MinSize,
        Property::MaxSize,
        Property::MinTextSize,
        Property::MaxTextSize,
        Property::StarCount,
        Property::CelestialBodiesShown,
        Property::SunAngularSize,
        Property::MoonAngularSize,
        Property::TrailLifetime,
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
            Property::Reflectance => "reflectance",
            Property::CastShadow => "cast_shadow",
            Property::CanTouch => "can_touch",
            Property::CanQuery => "can_query",
            Property::Massless => "massless",
            Property::Locked => "locked",
            Property::CollisionGroup => "collision_group",
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
            Property::ActuatorType => "actuator_type",
            Property::MotorVelocity => "motor_velocity",
            Property::MotorMaxTorque => "motor_max_torque",
            Property::TargetAngle => "target_angle",
            Property::AngularSpeed => "angular_speed",
            Property::ServoMaxTorque => "servo_max_torque",
            Property::CurrentAngle => "current_angle",
            Property::LimitsEnabled => "limits_enabled",
            Property::LowerLimit => "lower_limit",
            Property::UpperLimit => "upper_limit",
            Property::CurrentPosition => "current_position",
            Property::AngularVelocity => "angular_velocity",
            Property::MaxTorque => "max_torque",
            Property::BodyMaxTorque => "body_max_torque",
            Property::SoundId => "sound_id",
            Property::Volume => "volume",
            Property::PlaybackSpeed => "playback_speed",
            Property::Looped => "looped",
            Property::Playing => "playing",
            Property::Texture => "texture",
            Property::Face => "face",
            Property::DecalColor => "decal_color",
            Property::AnimationId => "animation_id",
            Property::HeadColor => "head_color",
            Property::TorsoColor => "torso_color",
            Property::LeftArmColor => "left_arm_color",
            Property::RightArmColor => "right_arm_color",
            Property::LeftLegColor => "left_leg_color",
            Property::RightLegColor => "right_leg_color",
            Property::StudsPerTileU => "studs_per_tile_u",
            Property::StudsPerTileV => "studs_per_tile_v",
            Property::Rate => "rate",
            Property::Lifetime => "lifetime",
            Property::Speed => "speed",
            Property::SpreadAngle => "spread_angle",
            Property::EffectSize => "effect_size",
            Property::Acceleration => "acceleration",
            Property::Drag => "drag",
            Property::LightEmission => "light_emission",
            Property::Heat => "heat",
            Property::SecondaryColor => "secondary_color",
            Property::Opacity => "opacity",
            Property::RiseVelocity => "rise_velocity",
            Property::SparkleColor => "sparkle_color",
            Property::Adornee => "adornee",
            Property::FillColor => "fill_color",
            Property::FillTransparency => "fill_transparency",
            Property::OutlineColor => "outline_color",
            Property::OutlineTransparency => "outline_transparency",
            Property::ZIndex => "z_index",
            Property::BorderSizePixel => "border_size_pixel",
            Property::BorderColor => "border_color",
            Property::ClipsDescendants => "clips_descendants",
            Property::TextTransparency => "text_transparency",
            Property::TextXAlignment => "text_x_alignment",
            Property::TextYAlignment => "text_y_alignment",
            Property::TextWrapped => "text_wrapped",
            Property::TextScaled => "text_scaled",
            Property::CanvasSize => "canvas_size",
            Property::CanvasPosition => "canvas_position",
            Property::ScrollBarThickness => "scroll_bar_thickness",
            Property::Image => "image",
            Property::ImageColor => "image_color",
            Property::ImageTransparency => "image_transparency",
            Property::ScaleType => "scale_type",
            Property::PaddingLeft => "padding_left",
            Property::PaddingRight => "padding_right",
            Property::PaddingTop => "padding_top",
            Property::PaddingBottom => "padding_bottom",
            Property::CellSize => "cell_size",
            Property::CellPadding => "cell_padding",
            Property::AspectRatio => "aspect_ratio",
            Property::Scale => "scale",
            Property::C0Position => "c0_position",
            Property::C0Orientation => "c0_orientation",
            Property::C1Position => "c1_position",
            Property::C1Orientation => "c1_orientation",
            Property::DesiredAngle => "desired_angle",
            Property::PrimaryPart => "primary_part",
            Property::Density => "density",
            Property::Haze => "haze",
            Property::BloomIntensity => "bloom_intensity",
            Property::BloomSize => "bloom_size",
            Property::Threshold => "threshold",
            Property::Contrast => "contrast",
            Property::Saturation => "saturation",
            Property::TintColor => "tint_color",
            Property::SelectionColor => "selection_color",
            Property::LineThickness => "line_thickness",
            Property::Width0 => "width0",
            Property::Width1 => "width1",
            Property::MinSize => "min_size",
            Property::MaxSize => "max_size",
            Property::MinTextSize => "min_text_size",
            Property::MaxTextSize => "max_text_size",
            Property::StarCount => "star_count",
            Property::CelestialBodiesShown => "celestial_bodies_shown",
            Property::SunAngularSize => "sun_angular_size",
            Property::MoonAngularSize => "moon_angular_size",
            Property::TrailLifetime => "trail_lifetime",
        }
    }

    pub fn from_file_name(name: &str) -> Option<Property> {
        Property::ALL.iter().copied().find(|p| p.file_name() == name)
    }
}
