//! The instance tree: the hierarchical object model behind every Libroxel world.
//!
//! A world is a tree of *instances*. Every instance has a class, a name, an
//! optional parent, ordered children, and a set of typed properties. The root
//! is a [`ClassKind::DataModel`] that always owns a [`ClassKind::Workspace`],
//! which holds everything that exists physically in the world.
//!
//! This crate is engine-agnostic: it has no Bevy or Lua dependency, so the
//! same tree can drive the renderer, the scripting runtime, the network
//! replicator and the file format. The names that scripts see (class names,
//! property names) live in the scripting layer, not here.
//!
//! Instances are addressed by [`InstanceId`], a generational handle: once an
//! instance is destroyed its id never resolves again, even if the slot is
//! reused.

mod enums;
pub mod geometry;
mod math;
pub mod place;
mod property;
mod tree;
pub mod voxels;

pub use enums::{
    ActuatorType, CameraType, EasingDirection, EasingStyle, EnumKind, EnumValue, FillDirection, KeyCode,
    Material, NormalId, PartShape, RaycastFilterType, ScaleType, TextXAlignment, TextYAlignment,
    UserInputType,
};
pub use math::{CFrame, Color3, Rotation, UDim, UDim2, Vec2, Vec3};
pub use property::{Property, PropertyValue};
pub use serde_json;
pub use tree::{Change, InstanceId, InstanceTree, TreeError};

/// Every kind of instance the engine knows about.
///
/// This is the engine's internal vocabulary. The scripting layer maps each
/// variant to the class name scripts use, so these can be renamed freely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClassKind {
    /// The root of a world. There is exactly one per tree.
    DataModel,
    /// Holds everything that exists physically in the world.
    Workspace,
    /// A plain container with no behaviour of its own.
    Folder,
    /// A group of instances, such as a character.
    Model,
    /// A solid box with a position, size and colour.
    Part,
    /// A part that marks where players appear.
    SpawnLocation,
    /// A Luau script attached to the tree. It runs when the game starts.
    Script,
    /// Gives a character health and movement settings.
    Humanoid,
    /// Luau code that other scripts load with `require`. It never runs on its own.
    ModuleScript,
    /// The service that holds one [`ClassKind::Player`] per connected player.
    Players,
    /// Someone playing the game. Created by the engine, never by scripts.
    Player,
    /// A service for things that are kept but not in the world, such as modules and templates.
    ReplicatedStorage,
    /// Holds a whole number.
    IntValue,
    /// Holds a number.
    NumberValue,
    /// Holds a string.
    StringValue,
    /// Holds true or false.
    BoolValue,
    /// A service whose GUIs are copied to each player when they join.
    StarterGui,
    /// Lighting and time of day.
    Lighting,
    /// Inside a player: the GUIs on their screen. Made by the engine.
    PlayerGui,
    /// A layer of GUI drawn over the 3D view.
    ScreenGui,
    /// The base of every on-screen rectangle. Not creatable on its own.
    GuiObject,
    /// A plain rectangle.
    Frame,
    /// A rectangle with text.
    TextLabel,
    /// A rectangle with text that can be clicked.
    TextButton,
    /// Inside a part: lets players click the part.
    ClickDetector,
    /// A service for scripts that aren't attached to anything in the world.
    ServerScriptService,
    /// A script that runs for one player: inside their GUI or character.
    LocalScript,
    /// Sends messages between the game's scripts and a player's local scripts.
    RemoteEvent,
    /// Sends messages from one script to others.
    BindableEvent,
    /// Inside a part: shows a "press a key" prompt when a player is near.
    ProximityPrompt,
    /// Something a player carries and uses, with a part called `Handle` to hold.
    Tool,
    /// A service whose tools are given to each player when they spawn.
    StarterPack,
    /// Inside a player: the tools they carry but aren't holding. Made by the engine.
    Backpack,
    /// Inside a part: GUI drawn in the world, floating at the part.
    BillboardGui,
    /// In the workspace: where the player's view is and how wide it is.
    Camera,
    /// Inside a part: light shining out in every direction.
    PointLight,
    /// Inside a part: a cone of light out of the part's front.
    SpotLight,
    /// A rectangle the player can type text into.
    TextBox,
    /// Inside a GUI object: rounds its corners.
    UICorner,
    /// Inside a GUI object: lines its other children up in a row or column.
    UIListLayout,
    /// In the workspace: a blast that hits everything nearby, once.
    Explosion,
    /// A service holding the game's teams.
    Teams,
    /// A side players can be on, with its own colour.
    Team,
    /// Holds two parts together so they move as one.
    WeldConstraint,
    /// A part characters sit in when they touch it.
    Seat,
    /// A seat that drives the parts welded to it.
    VehicleSeat,
    /// Like a remote event, but the caller waits for the other side's answer.
    RemoteFunction,
    /// Like a bindable event, but the caller waits for the answer.
    BindableFunction,
    /// A value object holding a link to another instance.
    ObjectValue,
    /// A value object holding a 3D vector.
    Vector3Value,
    /// A value object holding a colour.
    Color3Value,
    /// A service for things only game scripts use, kept out of the world.
    ServerStorage,
    /// A service with the settings each player's character starts with.
    StarterPlayer,
    /// Inside a part: a point on it (Position is relative to the part) that constraints and movers hold.
    Attachment,
    /// Pushes Attachment0's part to move at VectorVelocity, with up to MaxForce.
    LinearVelocity,
    /// Pushes Attachment0's part with a steady Force.
    VectorForce,
    /// Pulls Attachment0's part to Attachment1 (or to Position).
    AlignPosition,
    /// Keeps two attachments no further apart than Length.
    RopeConstraint,
    /// Pulls two attachments towards FreeLength apart, like a spring.
    SpringConstraint,
    /// Inside a part: the older way to make it move at Velocity.
    BodyVelocity,
    /// Inside a part: the older way to pull it to Position.
    BodyPosition,
    /// Inside a part: the older way to push it with a steady Force.
    BodyForce,
    /// Lets Attachment1's part turn about Attachment0's X axis, freely,
    /// driven by a motor, or held at an angle by a servo.
    HingeConstraint,
    /// Spins Attachment0's part at AngularVelocity, with up to MaxTorque.
    AngularVelocity,
    /// Inside a part: the older way to make it spin at AngularVelocity.
    BodyAngularVelocity,
    /// In the workspace: ground made of one-stud cubes, built by scripts
    /// (FillBlock, FillBall) or the editor's terrain tools.
    Terrain,
    /// Plays a WAV or OGG file (SoundId) while playing: Play, Stop, Pause,
    /// Resume. Inside a part it sounds quieter further away.
    Sound,
    /// Inside a part: a picture stretched over one Face.
    Decal,
    /// Inside a part: a picture tiled over one Face, StudsPerTileU by StudsPerTileV.
    Texture,
    /// Inside a part: sends out little particles.
    ParticleEmitter,
    /// Inside a part: flames.
    Fire,
    /// Inside a part: rising smoke.
    Smoke,
    /// Inside a part: glittering sparkles.
    Sparkles,
    /// Draws an outline and a tint over its Adornee (or its parent).
    Highlight,
    /// A frame whose inside (its canvas) can be bigger than it and scrolled.
    ScrollingFrame,
    /// A box on a GUI showing a picture.
    ImageLabel,
    /// A picture on a GUI that can be clicked.
    ImageButton,
    /// Inside a GUI object: outlines it, or its text.
    UIStroke,
    /// Inside a GUI object: keeps what's inside it away from its edges.
    UIPadding,
    /// Inside a GUI object: lines its other children up in a grid of cells.
    UIGridLayout,
    /// Inside a GUI object: keeps its width / height at AspectRatio.
    UIAspectRatioConstraint,
    /// Inside a GUI object: makes it bigger or smaller by Scale.
    UIScale,
}

impl ClassKind {
    /// Every class, in a stable order.
    pub const ALL: &[ClassKind] = &[
        ClassKind::DataModel,
        ClassKind::Workspace,
        ClassKind::Folder,
        ClassKind::Model,
        ClassKind::Part,
        ClassKind::SpawnLocation,
        ClassKind::Script,
        ClassKind::Humanoid,
        ClassKind::ModuleScript,
        ClassKind::Players,
        ClassKind::Player,
        ClassKind::ReplicatedStorage,
        ClassKind::IntValue,
        ClassKind::NumberValue,
        ClassKind::StringValue,
        ClassKind::BoolValue,
        ClassKind::StarterGui,
        ClassKind::Lighting,
        ClassKind::PlayerGui,
        ClassKind::ScreenGui,
        ClassKind::GuiObject,
        ClassKind::Frame,
        ClassKind::TextLabel,
        ClassKind::TextButton,
        ClassKind::ClickDetector,
        ClassKind::ServerScriptService,
        ClassKind::LocalScript,
        ClassKind::RemoteEvent,
        ClassKind::BindableEvent,
        ClassKind::ProximityPrompt,
        ClassKind::Tool,
        ClassKind::StarterPack,
        ClassKind::Backpack,
        ClassKind::BillboardGui,
        ClassKind::Camera,
        ClassKind::PointLight,
        ClassKind::SpotLight,
        ClassKind::TextBox,
        ClassKind::UICorner,
        ClassKind::UIListLayout,
        ClassKind::Explosion,
        ClassKind::Teams,
        ClassKind::Team,
        ClassKind::WeldConstraint,
        ClassKind::Seat,
        ClassKind::VehicleSeat,
        ClassKind::RemoteFunction,
        ClassKind::BindableFunction,
        ClassKind::ObjectValue,
        ClassKind::Vector3Value,
        ClassKind::Color3Value,
        ClassKind::ServerStorage,
        ClassKind::StarterPlayer,
        ClassKind::Attachment,
        ClassKind::LinearVelocity,
        ClassKind::VectorForce,
        ClassKind::AlignPosition,
        ClassKind::RopeConstraint,
        ClassKind::SpringConstraint,
        ClassKind::BodyVelocity,
        ClassKind::BodyPosition,
        ClassKind::BodyForce,
        ClassKind::HingeConstraint,
        ClassKind::AngularVelocity,
        ClassKind::BodyAngularVelocity,
        ClassKind::Terrain,
        ClassKind::Sound,
        ClassKind::Decal,
        ClassKind::Texture,
        ClassKind::ParticleEmitter,
        ClassKind::Fire,
        ClassKind::Smoke,
        ClassKind::Sparkles,
        ClassKind::Highlight,
        ClassKind::ScrollingFrame,
        ClassKind::ImageLabel,
        ClassKind::ImageButton,
        ClassKind::UIStroke,
        ClassKind::UIPadding,
        ClassKind::UIGridLayout,
        ClassKind::UIAspectRatioConstraint,
        ClassKind::UIScale,
    ];

    /// The services every tree has under its root, in order. They can't be
    /// created, moved or destroyed.
    pub const SERVICES: &[ClassKind] = &[
        ClassKind::Workspace,
        ClassKind::Players,
        ClassKind::ReplicatedStorage,
        ClassKind::StarterGui,
        ClassKind::Lighting,
        ClassKind::ServerScriptService,
        ClassKind::StarterPack,
        ClassKind::Teams,
        ClassKind::ServerStorage,
        ClassKind::StarterPlayer,
    ];

    /// Whether this class is one of the [`SERVICES`](Self::SERVICES).
    pub fn is_service(self) -> bool {
        Self::SERVICES.contains(&self)
    }

    /// Whether the tree may create this class. The root and services may not.
    pub fn is_creatable(self) -> bool {
        !matches!(self, ClassKind::DataModel | ClassKind::GuiObject) && !self.is_service()
    }

    /// Whether scripts and the editor may create this class. Players and
    /// their GUI containers are made by the engine when someone joins.
    pub fn is_user_creatable(self) -> bool {
        self.is_creatable() && !matches!(self, ClassKind::Player | ClassKind::PlayerGui | ClassKind::Backpack)
    }

    /// Whether this class is drawn on screen as a rectangle.
    pub fn is_gui_object(self) -> bool {
        self.is_a(ClassKind::GuiObject)
    }

    /// Whether this class is a GUI object that can be clicked.
    pub fn is_button(self) -> bool {
        matches!(self, ClassKind::TextButton | ClassKind::ImageButton)
    }

    /// Whether this class is something characters sit in.
    pub fn is_seat(self) -> bool {
        matches!(self, ClassKind::Seat | ClassKind::VehicleSeat)
    }

    /// Whether this class is a box in the world (it has a position, size and colour).
    pub fn is_physical(self) -> bool {
        self.is_a(ClassKind::Part)
    }

    /// The parent class in the class hierarchy, if any.
    pub fn superclass(self) -> Option<ClassKind> {
        match self {
            // Workspace behaves like a Model that can't be removed.
            ClassKind::Workspace => Some(ClassKind::Model),
            ClassKind::SpawnLocation | ClassKind::Seat | ClassKind::VehicleSeat => Some(ClassKind::Part),
            ClassKind::Frame
            | ClassKind::TextLabel
            | ClassKind::TextButton
            | ClassKind::TextBox
            | ClassKind::ScrollingFrame
            | ClassKind::ImageLabel
            | ClassKind::ImageButton => Some(ClassKind::GuiObject),
            _ => None,
        }
    }

    /// Whether this class is `other` or inherits from it.
    pub fn is_a(self, other: ClassKind) -> bool {
        let mut current = Some(self);
        while let Some(class) = current {
            if class == other {
                return true;
            }
            current = class.superclass();
        }
        false
    }

    /// The properties this class exposes, in a stable order.
    pub fn properties(self) -> &'static [Property] {
        const COMMON: &[Property] = &[Property::Name];
        const PART: &[Property] = &[
            Property::Name,
            Property::Position,
            Property::Orientation,
            Property::Size,
            Property::Shape,
            Property::Color,
            Property::Material,
            Property::Anchored,
            Property::CanCollide,
            Property::Transparency,
        ];
        const SCRIPT: &[Property] = &[Property::Name, Property::Disabled, Property::Source];
        const MODULE: &[Property] = &[Property::Name, Property::Source];
        const PLAYER: &[Property] =
            &[Property::Name, Property::UserId, Property::Character, Property::Team, Property::Neutral];
        const SPAWN: &[Property] = &[
            Property::Name,
            Property::Position,
            Property::Orientation,
            Property::Size,
            Property::Shape,
            Property::Color,
            Property::Material,
            Property::Anchored,
            Property::CanCollide,
            Property::Transparency,
            Property::TeamColor,
            Property::Neutral,
        ];
        const WORKSPACE: &[Property] =
            &[Property::Name, Property::Gravity, Property::FallenPartsDestroyHeight];
        const OBJECT: &[Property] = &[Property::Name, Property::ObjectValue];
        const VECTOR3: &[Property] = &[Property::Name, Property::Vector3Value];
        const COLOR3: &[Property] = &[Property::Name, Property::Color3Value];
        const ATTACHMENT: &[Property] =
            &[Property::Name, Property::Position, Property::Orientation, Property::Visible];
        const LINEAR_VELOCITY: &[Property] = &[
            Property::Name,
            Property::Enabled,
            Property::Attachment0,
            Property::VectorVelocity,
            Property::MaxForce,
        ];
        const VECTOR_FORCE: &[Property] =
            &[Property::Name, Property::Enabled, Property::Attachment0, Property::Force];
        const ALIGN_POSITION: &[Property] = &[
            Property::Name,
            Property::Enabled,
            Property::Attachment0,
            Property::Attachment1,
            Property::Position,
            Property::MaxForce,
            Property::MaxVelocity,
            Property::Responsiveness,
            Property::RigidityEnabled,
        ];
        const ROPE: &[Property] = &[
            Property::Name,
            Property::Enabled,
            Property::Attachment0,
            Property::Attachment1,
            Property::Length,
            Property::Visible,
            Property::Color,
            Property::Thickness,
        ];
        const SPRING: &[Property] = &[
            Property::Name,
            Property::Enabled,
            Property::Attachment0,
            Property::Attachment1,
            Property::FreeLength,
            Property::Stiffness,
            Property::Damping,
            Property::Visible,
            Property::Color,
            Property::Thickness,
        ];
        const BODY_VELOCITY: &[Property] =
            &[Property::Name, Property::Velocity, Property::BodyMaxForce, Property::BodyPower];
        const BODY_POSITION: &[Property] = &[
            Property::Name,
            Property::Position,
            Property::BodyMaxForce,
            Property::BodyPower,
            Property::BodyDamping,
        ];
        const BODY_FORCE: &[Property] = &[Property::Name, Property::Force];
        const HINGE: &[Property] = &[
            Property::Name,
            Property::Enabled,
            Property::Attachment0,
            Property::Attachment1,
            Property::ActuatorType,
            Property::MotorVelocity,
            Property::MotorMaxTorque,
            Property::TargetAngle,
            Property::AngularSpeed,
            Property::ServoMaxTorque,
            Property::CurrentAngle,
        ];
        const ANGULAR_VELOCITY: &[Property] = &[
            Property::Name,
            Property::Enabled,
            Property::Attachment0,
            Property::AngularVelocity,
            Property::MaxTorque,
        ];
        const TERRAIN: &[Property] = &[Property::Name];
        const SOUND: &[Property] = &[
            Property::Name,
            Property::SoundId,
            Property::Volume,
            Property::PlaybackSpeed,
            Property::Looped,
            Property::Playing,
        ];
        const DECAL: &[Property] = &[
            Property::Name,
            Property::Texture,
            Property::Face,
            Property::DecalColor,
            Property::Transparency,
        ];
        const TEXTURE: &[Property] = &[
            Property::Name,
            Property::Texture,
            Property::Face,
            Property::DecalColor,
            Property::Transparency,
            Property::StudsPerTileU,
            Property::StudsPerTileV,
        ];
        const BODY_ANGULAR_VELOCITY: &[Property] =
            &[Property::Name, Property::AngularVelocity, Property::BodyMaxTorque, Property::BodyPower];
        const PARTICLES: &[Property] = &[
            Property::Name,
            Property::Enabled,
            Property::Rate,
            Property::Lifetime,
            Property::Speed,
            Property::SpreadAngle,
            Property::EffectSize,
            Property::Color,
            Property::Transparency,
            Property::Acceleration,
            Property::Drag,
            Property::LightEmission,
        ];
        const FIRE: &[Property] = &[
            Property::Name,
            Property::Enabled,
            Property::EffectSize,
            Property::Heat,
            Property::Color,
            Property::SecondaryColor,
        ];
        const SMOKE: &[Property] = &[
            Property::Name,
            Property::Enabled,
            Property::EffectSize,
            Property::Color,
            Property::Opacity,
            Property::RiseVelocity,
        ];
        const SPARKLES: &[Property] = &[Property::Name, Property::Enabled, Property::SparkleColor];
        const HIGHLIGHT: &[Property] = &[
            Property::Name,
            Property::Enabled,
            Property::Adornee,
            Property::FillColor,
            Property::FillTransparency,
            Property::OutlineColor,
            Property::OutlineTransparency,
        ];
        const STARTER_PLAYER: &[Property] =
            &[Property::Name, Property::CharacterWalkSpeed, Property::CharacterJumpPower];
        const PLAYERS: &[Property] = &[Property::Name, Property::RespawnTime, Property::CharacterAutoLoads];
        const WELD: &[Property] = &[Property::Name, Property::Part0, Property::Part1, Property::Enabled];
        const SEAT: &[Property] = &[
            Property::Name,
            Property::Position,
            Property::Orientation,
            Property::Size,
            Property::Shape,
            Property::Color,
            Property::Material,
            Property::Anchored,
            Property::CanCollide,
            Property::Transparency,
            Property::Disabled,
            Property::Occupant,
        ];
        const VEHICLE_SEAT: &[Property] = &[
            Property::Name,
            Property::Position,
            Property::Orientation,
            Property::Size,
            Property::Shape,
            Property::Color,
            Property::Material,
            Property::Anchored,
            Property::CanCollide,
            Property::Transparency,
            Property::Disabled,
            Property::Occupant,
            Property::MaxSpeed,
            Property::Torque,
            Property::TurnSpeed,
            Property::Throttle,
            Property::Steer,
        ];
        const TEAM: &[Property] = &[Property::Name, Property::TeamColor, Property::AutoAssignable];
        const NUMBER: &[Property] = &[Property::Name, Property::NumberValue];
        const STRING: &[Property] = &[Property::Name, Property::StringValue];
        const BOOL: &[Property] = &[Property::Name, Property::BoolValue];
        const SCREEN_GUI: &[Property] = &[Property::Name, Property::Enabled, Property::ResetOnSpawn];
        const FRAME: &[Property] = &[
            Property::Name,
            Property::GuiPosition,
            Property::GuiSize,
            Property::AnchorPoint,
            Property::BackgroundColor,
            Property::BackgroundTransparency,
            Property::Visible,
            Property::LayoutOrder,
            Property::ZIndex,
            Property::BorderSizePixel,
            Property::BorderColor,
            Property::ClipsDescendants,
        ];
        const IMAGE: &[Property] = &[
            Property::Name,
            Property::GuiPosition,
            Property::GuiSize,
            Property::AnchorPoint,
            Property::BackgroundColor,
            Property::BackgroundTransparency,
            Property::Visible,
            Property::LayoutOrder,
            Property::ZIndex,
            Property::BorderSizePixel,
            Property::BorderColor,
            Property::ClipsDescendants,
            Property::Image,
            Property::ImageColor,
            Property::ImageTransparency,
            Property::ScaleType,
        ];
        const SCROLLING: &[Property] = &[
            Property::Name,
            Property::GuiPosition,
            Property::GuiSize,
            Property::AnchorPoint,
            Property::BackgroundColor,
            Property::BackgroundTransparency,
            Property::Visible,
            Property::LayoutOrder,
            Property::ZIndex,
            Property::BorderSizePixel,
            Property::BorderColor,
            Property::ClipsDescendants,
            Property::CanvasSize,
            Property::CanvasPosition,
            Property::ScrollBarThickness,
        ];
        const STROKE: &[Property] = &[
            Property::Name,
            Property::Enabled,
            Property::Color,
            Property::Thickness,
            Property::Transparency,
        ];
        const PADDING: &[Property] = &[
            Property::Name,
            Property::PaddingLeft,
            Property::PaddingRight,
            Property::PaddingTop,
            Property::PaddingBottom,
        ];
        const GRID: &[Property] =
            &[Property::Name, Property::CellSize, Property::CellPadding, Property::FillDirection];
        const ASPECT: &[Property] = &[Property::Name, Property::AspectRatio];
        const SCALE: &[Property] = &[Property::Name, Property::Scale];
        const TEXT: &[Property] = &[
            Property::Name,
            Property::GuiPosition,
            Property::GuiSize,
            Property::AnchorPoint,
            Property::BackgroundColor,
            Property::BackgroundTransparency,
            Property::Visible,
            Property::Text,
            Property::TextColor,
            Property::TextSize,
            Property::LayoutOrder,
            Property::ZIndex,
            Property::BorderSizePixel,
            Property::BorderColor,
            Property::ClipsDescendants,
            Property::TextTransparency,
            Property::TextXAlignment,
            Property::TextYAlignment,
            Property::TextWrapped,
            Property::TextScaled,
        ];
        const TEXT_BOX: &[Property] = &[
            Property::Name,
            Property::GuiPosition,
            Property::GuiSize,
            Property::AnchorPoint,
            Property::BackgroundColor,
            Property::BackgroundTransparency,
            Property::Visible,
            Property::Text,
            Property::TextColor,
            Property::TextSize,
            Property::LayoutOrder,
            Property::PlaceholderText,
            Property::ClearTextOnFocus,
            Property::ZIndex,
            Property::BorderSizePixel,
            Property::BorderColor,
            Property::ClipsDescendants,
            Property::TextTransparency,
            Property::TextXAlignment,
            Property::TextYAlignment,
            Property::TextWrapped,
            Property::TextScaled,
        ];
        const CAMERA: &[Property] = &[
            Property::Name,
            Property::Position,
            Property::Orientation,
            Property::FieldOfView,
            Property::CameraType,
        ];
        const POINT_LIGHT: &[Property] =
            &[Property::Name, Property::Enabled, Property::Brightness, Property::Color, Property::Range];
        const SPOT_LIGHT: &[Property] = &[
            Property::Name,
            Property::Enabled,
            Property::Brightness,
            Property::Color,
            Property::Range,
            Property::Angle,
        ];
        const CORNER: &[Property] = &[Property::Name, Property::CornerRadius];
        const LIST: &[Property] = &[Property::Name, Property::FillDirection, Property::Padding];
        const EXPLOSION: &[Property] =
            &[Property::Name, Property::Position, Property::BlastRadius, Property::BlastPressure];
        const CLICK: &[Property] = &[Property::Name, Property::MaxActivationDistance];
        const PROMPT: &[Property] = &[
            Property::Name,
            Property::ActionText,
            Property::ObjectText,
            Property::KeyboardKeyCode,
            Property::HoldDuration,
            Property::MaxActivationDistance,
            Property::Enabled,
        ];
        const TOOL: &[Property] = &[Property::Name, Property::Enabled];
        const BILLBOARD: &[Property] = &[
            Property::Name,
            Property::Enabled,
            Property::GuiSize,
            Property::StudsOffset,
            Property::AlwaysOnTop,
            Property::MaxDistance,
        ];
        const LIGHTING: &[Property] = &[
            Property::Name,
            Property::ClockTime,
            Property::Brightness,
            Property::Ambient,
            Property::FogColor,
            Property::FogStart,
            Property::FogEnd,
        ];
        const HUMANOID: &[Property] = &[
            Property::Name,
            Property::Health,
            Property::MaxHealth,
            Property::WalkSpeed,
            Property::JumpPower,
            Property::Jump,
            Property::Sit,
            Property::SeatPart,
        ];
        match self {
            ClassKind::Part => PART,
            ClassKind::SpawnLocation => SPAWN,
            ClassKind::Team => TEAM,
            ClassKind::Script | ClassKind::LocalScript => SCRIPT,
            ClassKind::Humanoid => HUMANOID,
            ClassKind::ModuleScript => MODULE,
            ClassKind::Player => PLAYER,
            ClassKind::IntValue | ClassKind::NumberValue => NUMBER,
            ClassKind::StringValue => STRING,
            ClassKind::BoolValue => BOOL,
            ClassKind::ScreenGui => SCREEN_GUI,
            ClassKind::Frame | ClassKind::GuiObject => FRAME,
            ClassKind::TextLabel | ClassKind::TextButton => TEXT,
            ClassKind::TextBox => TEXT_BOX,
            ClassKind::Camera => CAMERA,
            ClassKind::PointLight => POINT_LIGHT,
            ClassKind::SpotLight => SPOT_LIGHT,
            ClassKind::UICorner => CORNER,
            ClassKind::UIListLayout => LIST,
            ClassKind::Explosion => EXPLOSION,
            ClassKind::ClickDetector => CLICK,
            ClassKind::ProximityPrompt => PROMPT,
            ClassKind::Tool => TOOL,
            ClassKind::BillboardGui => BILLBOARD,
            ClassKind::Lighting => LIGHTING,
            ClassKind::Workspace => WORKSPACE,
            ClassKind::Players => PLAYERS,
            ClassKind::WeldConstraint => WELD,
            ClassKind::Seat => SEAT,
            ClassKind::VehicleSeat => VEHICLE_SEAT,
            ClassKind::ObjectValue => OBJECT,
            ClassKind::Vector3Value => VECTOR3,
            ClassKind::Color3Value => COLOR3,
            ClassKind::StarterPlayer => STARTER_PLAYER,
            ClassKind::Attachment => ATTACHMENT,
            ClassKind::LinearVelocity => LINEAR_VELOCITY,
            ClassKind::VectorForce => VECTOR_FORCE,
            ClassKind::AlignPosition => ALIGN_POSITION,
            ClassKind::RopeConstraint => ROPE,
            ClassKind::SpringConstraint => SPRING,
            ClassKind::BodyVelocity => BODY_VELOCITY,
            ClassKind::BodyPosition => BODY_POSITION,
            ClassKind::BodyForce => BODY_FORCE,
            ClassKind::HingeConstraint => HINGE,
            ClassKind::AngularVelocity => ANGULAR_VELOCITY,
            ClassKind::BodyAngularVelocity => BODY_ANGULAR_VELOCITY,
            ClassKind::Terrain => TERRAIN,
            ClassKind::Sound => SOUND,
            ClassKind::Decal => DECAL,
            ClassKind::Texture => TEXTURE,
            ClassKind::ParticleEmitter => PARTICLES,
            ClassKind::Fire => FIRE,
            ClassKind::Smoke => SMOKE,
            ClassKind::Sparkles => SPARKLES,
            ClassKind::Highlight => HIGHLIGHT,
            ClassKind::ScrollingFrame => SCROLLING,
            ClassKind::ImageLabel | ClassKind::ImageButton => IMAGE,
            ClassKind::UIStroke => STROKE,
            ClassKind::UIPadding => PADDING,
            ClassKind::UIGridLayout => GRID,
            ClassKind::UIAspectRatioConstraint => ASPECT,
            ClassKind::UIScale => SCALE,
            _ => COMMON,
        }
    }

    /// Whether this class has `property`.
    pub fn has_property(self, property: Property) -> bool {
        self.properties().contains(&property)
    }

    /// The value `property` starts with on a new instance of this class.
    pub fn default_value(self, property: Property) -> Option<PropertyValue> {
        if !self.has_property(property) {
            return None;
        }
        Some(match property {
            Property::Name | Property::Source | Property::StringValue => PropertyValue::String(String::new()),
            Property::Position => PropertyValue::Vec3(match self {
                ClassKind::BodyPosition => Vec3::new(0.0, 50.0, 0.0),
                _ => Vec3::ZERO,
            }),
            Property::Orientation => PropertyValue::Vec3(Vec3::ZERO),
            Property::Shape => PropertyValue::Enum(EnumValue::Shape(PartShape::Block)),
            Property::Material => PropertyValue::Enum(EnumValue::Material(Material::Plastic)),
            Property::NumberValue | Property::UserId => PropertyValue::Number(0.0),
            Property::BoolValue => PropertyValue::Bool(false),
            Property::Character => PropertyValue::Ref(None),
            Property::GuiPosition => PropertyValue::UDim2(UDim2::default()),
            Property::GuiSize => PropertyValue::UDim2(match self {
                ClassKind::Frame => UDim2::new(0.0, 100.0, 0.0, 100.0),
                ClassKind::BillboardGui => UDim2::new(0.0, 200.0, 0.0, 50.0),
                _ => UDim2::new(0.0, 200.0, 0.0, 50.0),
            }),
            Property::AnchorPoint => PropertyValue::Vec2(Vec2::ZERO),
            Property::BackgroundColor => PropertyValue::Color3(Color3::WHITE),
            Property::BackgroundTransparency => PropertyValue::Number(0.0),
            Property::Visible => PropertyValue::Bool(!matches!(self, ClassKind::Attachment)),
            Property::Enabled | Property::ResetOnSpawn => PropertyValue::Bool(true),
            Property::Text => PropertyValue::String(match self {
                ClassKind::TextButton => "Button".into(),
                ClassKind::TextBox => String::new(),
                _ => "Label".into(),
            }),
            Property::TextColor => PropertyValue::Color3(Color3::from_rgb8(27, 42, 53)),
            Property::TextSize => PropertyValue::Number(14.0),
            Property::MaxActivationDistance => PropertyValue::Number(match self {
                ClassKind::ProximityPrompt => 10.0,
                _ => 32.0,
            }),
            Property::ActionText => PropertyValue::String("Interact".into()),
            Property::ObjectText => PropertyValue::String(String::new()),
            Property::KeyboardKeyCode => PropertyValue::Enum(EnumValue::KeyCode(KeyCode::E)),
            Property::HoldDuration => PropertyValue::Number(0.0),
            Property::StudsOffset => PropertyValue::Vec3(Vec3::new(0.0, 2.0, 0.0)),
            Property::AlwaysOnTop => PropertyValue::Bool(false),
            Property::MaxDistance => PropertyValue::Number(100.0),
            Property::ClockTime => PropertyValue::Number(14.0),
            Property::Brightness => PropertyValue::Number(match self {
                ClassKind::Lighting => 2.0,
                _ => 1.0,
            }),
            Property::LayoutOrder => PropertyValue::Number(0.0),
            Property::PlaceholderText => PropertyValue::String("Type here".into()),
            Property::ClearTextOnFocus => PropertyValue::Bool(true),
            Property::FieldOfView => PropertyValue::Number(70.0),
            Property::CameraType => PropertyValue::Enum(EnumValue::CameraType(CameraType::Custom)),
            Property::Range => PropertyValue::Number(match self {
                ClassKind::SpotLight => 16.0,
                _ => 8.0,
            }),
            Property::Angle => PropertyValue::Number(90.0),
            Property::CornerRadius => PropertyValue::UDim(UDim::new(0.0, 8.0)),
            Property::FillDirection => PropertyValue::Enum(EnumValue::FillDirection(match self {
                ClassKind::UIGridLayout => FillDirection::Horizontal,
                _ => FillDirection::Vertical,
            })),
            Property::ZIndex => PropertyValue::Number(1.0),
            Property::BorderSizePixel => PropertyValue::Number(0.0),
            Property::BorderColor => PropertyValue::Color3(Color3::from_rgb8(27, 42, 53)),
            Property::ClipsDescendants | Property::TextWrapped | Property::TextScaled => {
                PropertyValue::Bool(false)
            }
            Property::TextTransparency => PropertyValue::Number(0.0),
            Property::TextXAlignment => {
                PropertyValue::Enum(EnumValue::TextXAlignment(TextXAlignment::Center))
            }
            Property::TextYAlignment => {
                PropertyValue::Enum(EnumValue::TextYAlignment(TextYAlignment::Center))
            }
            Property::CanvasSize => PropertyValue::UDim2(UDim2::new(0.0, 0.0, 2.0, 0.0)),
            Property::CanvasPosition => PropertyValue::Vec2(Vec2::ZERO),
            Property::ScrollBarThickness => PropertyValue::Number(12.0),
            Property::Image => PropertyValue::String(String::new()),
            Property::ImageColor => PropertyValue::Color3(Color3::new(1.0, 1.0, 1.0)),
            Property::ImageTransparency => PropertyValue::Number(0.0),
            Property::ScaleType => PropertyValue::Enum(EnumValue::ScaleType(ScaleType::Stretch)),
            Property::PaddingLeft
            | Property::PaddingRight
            | Property::PaddingTop
            | Property::PaddingBottom => PropertyValue::UDim(UDim::default()),
            Property::CellSize => PropertyValue::UDim2(UDim2::new(0.0, 100.0, 0.0, 100.0)),
            Property::CellPadding => PropertyValue::UDim2(UDim2::new(0.0, 5.0, 0.0, 5.0)),
            Property::AspectRatio | Property::Scale => PropertyValue::Number(1.0),
            Property::Padding => PropertyValue::UDim(UDim::default()),
            Property::BlastRadius => PropertyValue::Number(4.0),
            Property::BlastPressure => PropertyValue::Number(500_000.0),
            Property::Jump => PropertyValue::Bool(false),
            Property::Team => PropertyValue::Ref(None),
            Property::Neutral => PropertyValue::Bool(true),
            Property::TeamColor => PropertyValue::Color3(match self {
                ClassKind::Team => Color3::WHITE,
                _ => Color3::MEDIUM_GREY,
            }),
            Property::AutoAssignable | Property::CharacterAutoLoads => PropertyValue::Bool(true),
            Property::Gravity => PropertyValue::Number(196.2),
            Property::RespawnTime => PropertyValue::Number(5.0),
            Property::Part0 | Property::Part1 | Property::Occupant | Property::SeatPart => {
                PropertyValue::Ref(None)
            }
            Property::Sit => PropertyValue::Bool(false),
            Property::MaxSpeed => PropertyValue::Number(25.0),
            Property::Torque => PropertyValue::Number(10.0),
            Property::TurnSpeed => PropertyValue::Number(1.0),
            Property::Throttle | Property::Steer => PropertyValue::Number(0.0),
            Property::ObjectValue => PropertyValue::Ref(None),
            Property::Vector3Value => PropertyValue::Vec3(Vec3::ZERO),
            Property::Color3Value => PropertyValue::Color3(Color3::new(0.0, 0.0, 0.0)),
            Property::CharacterWalkSpeed => PropertyValue::Number(16.0),
            Property::CharacterJumpPower => PropertyValue::Number(50.0),
            Property::Ambient => PropertyValue::Color3(Color3::new(0.5, 0.5, 0.5)),
            Property::FogColor => PropertyValue::Color3(Color3::new(0.75, 0.75, 0.75)),
            Property::FogStart => PropertyValue::Number(0.0),
            Property::FogEnd => PropertyValue::Number(100_000.0),
            Property::FallenPartsDestroyHeight => PropertyValue::Number(-100.0),
            Property::Attachment0 | Property::Attachment1 => PropertyValue::Ref(None),
            Property::VectorVelocity | Property::Force => PropertyValue::Vec3(Vec3::ZERO),
            Property::MaxForce => PropertyValue::Number(match self {
                ClassKind::AlignPosition => 10_000.0,
                _ => 1_000.0,
            }),
            Property::MaxVelocity => PropertyValue::Number(1_000_000.0),
            Property::Responsiveness => PropertyValue::Number(10.0),
            Property::RigidityEnabled => PropertyValue::Bool(false),
            Property::Length => PropertyValue::Number(5.0),
            Property::FreeLength => PropertyValue::Number(1.0),
            Property::Stiffness => PropertyValue::Number(100.0),
            Property::Damping => PropertyValue::Number(5.0),
            Property::Thickness => PropertyValue::Number(match self {
                ClassKind::UIStroke => 1.0,
                _ => 0.1,
            }),
            Property::Velocity => PropertyValue::Vec3(Vec3::new(0.0, 2.0, 0.0)),
            Property::BodyMaxForce => PropertyValue::Vec3(Vec3::new(4000.0, 4000.0, 4000.0)),
            Property::BodyPower => PropertyValue::Number(match self {
                ClassKind::BodyPosition => 10_000.0,
                _ => 1_250.0,
            }),
            Property::BodyDamping => PropertyValue::Number(1_250.0),
            Property::ActuatorType => PropertyValue::Enum(EnumValue::ActuatorType(ActuatorType::None)),
            Property::MotorVelocity | Property::TargetAngle | Property::CurrentAngle => {
                PropertyValue::Number(0.0)
            }
            Property::MotorMaxTorque | Property::ServoMaxTorque | Property::MaxTorque => {
                PropertyValue::Number(100_000.0)
            }
            Property::AngularSpeed => PropertyValue::Number(1.0),
            Property::Texture => PropertyValue::String(String::new()),
            Property::SoundId => PropertyValue::String(String::new()),
            Property::Volume => PropertyValue::Number(0.5),
            Property::PlaybackSpeed => PropertyValue::Number(1.0),
            Property::Looped | Property::Playing => PropertyValue::Bool(false),
            Property::Face => PropertyValue::Enum(EnumValue::NormalId(NormalId::Front)),
            Property::DecalColor => PropertyValue::Color3(Color3::WHITE),
            Property::StudsPerTileU | Property::StudsPerTileV => PropertyValue::Number(2.0),
            Property::AngularVelocity => PropertyValue::Vec3(Vec3::ZERO),
            Property::BodyMaxTorque => PropertyValue::Vec3(Vec3::new(4000.0, 4000.0, 4000.0)),
            Property::Rate => PropertyValue::Number(20.0),
            Property::Lifetime => PropertyValue::Vec2(Vec2::new(5.0, 10.0)),
            Property::Speed => PropertyValue::Vec2(Vec2::new(5.0, 5.0)),
            Property::SpreadAngle => PropertyValue::Vec2(Vec2::ZERO),
            Property::EffectSize => PropertyValue::Number(match self {
                ClassKind::Fire => 5.0,
                _ => 1.0,
            }),
            Property::Acceleration => PropertyValue::Vec3(Vec3::ZERO),
            Property::Drag | Property::LightEmission => PropertyValue::Number(0.0),
            Property::Heat => PropertyValue::Number(9.0),
            Property::SecondaryColor => PropertyValue::Color3(Color3::from_rgb8(139, 80, 55)),
            Property::Opacity => PropertyValue::Number(0.5),
            Property::RiseVelocity => PropertyValue::Number(1.0),
            Property::SparkleColor => PropertyValue::Color3(Color3::from_rgb8(144, 25, 255)),
            Property::Adornee => PropertyValue::Ref(None),
            Property::FillColor => PropertyValue::Color3(Color3::from_rgb8(255, 0, 0)),
            Property::FillTransparency => PropertyValue::Number(0.5),
            Property::OutlineColor => PropertyValue::Color3(Color3::WHITE),
            Property::OutlineTransparency => PropertyValue::Number(0.0),
            Property::Size => PropertyValue::Vec3(Vec3::new(4.0, 1.0, 2.0)),
            Property::Color => PropertyValue::Color3(match self {
                ClassKind::SpawnLocation => Color3::new(0.3, 0.55, 0.95),
                ClassKind::PointLight | ClassKind::SpotLight => Color3::WHITE,
                ClassKind::RopeConstraint => Color3::from_rgb8(120, 90, 60),
                ClassKind::ParticleEmitter => Color3::WHITE,
                ClassKind::Fire => Color3::from_rgb8(236, 139, 70),
                ClassKind::Smoke => Color3::WHITE,
                ClassKind::UIStroke => Color3::new(0.0, 0.0, 0.0),
                ClassKind::SpringConstraint => Color3::from_rgb8(200, 200, 210),
                _ => Color3::MEDIUM_GREY,
            }),
            Property::Anchored => PropertyValue::Bool(self == ClassKind::SpawnLocation),
            Property::CanCollide => PropertyValue::Bool(true),
            Property::Disabled => PropertyValue::Bool(false),
            Property::Transparency => PropertyValue::Number(0.0),
            Property::Health | Property::MaxHealth => PropertyValue::Number(100.0),
            Property::WalkSpeed => PropertyValue::Number(16.0),
            Property::JumpPower => PropertyValue::Number(50.0),
        })
    }

    /// A stable name for files. Never shown to scripts; never change these.
    pub fn file_name(self) -> &'static str {
        match self {
            ClassKind::DataModel => "data_model",
            ClassKind::Workspace => "workspace",
            ClassKind::Folder => "folder",
            ClassKind::Model => "model",
            ClassKind::Part => "part",
            ClassKind::SpawnLocation => "spawn_location",
            ClassKind::Script => "script",
            ClassKind::Humanoid => "humanoid",
            ClassKind::ModuleScript => "module_script",
            ClassKind::Players => "players",
            ClassKind::Player => "player",
            ClassKind::ReplicatedStorage => "replicated_storage",
            ClassKind::IntValue => "int_value",
            ClassKind::NumberValue => "number_value",
            ClassKind::StringValue => "string_value",
            ClassKind::BoolValue => "bool_value",
            ClassKind::StarterGui => "starter_gui",
            ClassKind::Lighting => "lighting",
            ClassKind::PlayerGui => "player_gui",
            ClassKind::ScreenGui => "screen_gui",
            ClassKind::GuiObject => "gui_object",
            ClassKind::Frame => "frame",
            ClassKind::TextLabel => "text_label",
            ClassKind::TextButton => "text_button",
            ClassKind::ClickDetector => "click_detector",
            ClassKind::ServerScriptService => "server_script_service",
            ClassKind::LocalScript => "local_script",
            ClassKind::RemoteEvent => "remote_event",
            ClassKind::BindableEvent => "bindable_event",
            ClassKind::ProximityPrompt => "proximity_prompt",
            ClassKind::Tool => "tool",
            ClassKind::StarterPack => "starter_pack",
            ClassKind::Backpack => "backpack",
            ClassKind::BillboardGui => "billboard_gui",
            ClassKind::Camera => "camera",
            ClassKind::PointLight => "point_light",
            ClassKind::SpotLight => "spot_light",
            ClassKind::TextBox => "text_box",
            ClassKind::UICorner => "ui_corner",
            ClassKind::UIListLayout => "ui_list_layout",
            ClassKind::Explosion => "explosion",
            ClassKind::Teams => "teams",
            ClassKind::Team => "team",
            ClassKind::WeldConstraint => "weld_constraint",
            ClassKind::Seat => "seat",
            ClassKind::VehicleSeat => "vehicle_seat",
            ClassKind::RemoteFunction => "remote_function",
            ClassKind::BindableFunction => "bindable_function",
            ClassKind::ObjectValue => "object_value",
            ClassKind::Vector3Value => "vector3_value",
            ClassKind::Color3Value => "color3_value",
            ClassKind::ServerStorage => "server_storage",
            ClassKind::StarterPlayer => "starter_player",
            ClassKind::Attachment => "attachment",
            ClassKind::LinearVelocity => "linear_velocity",
            ClassKind::VectorForce => "vector_force",
            ClassKind::AlignPosition => "align_position",
            ClassKind::RopeConstraint => "rope_constraint",
            ClassKind::SpringConstraint => "spring_constraint",
            ClassKind::BodyVelocity => "body_velocity",
            ClassKind::BodyPosition => "body_position",
            ClassKind::BodyForce => "body_force",
            ClassKind::HingeConstraint => "hinge_constraint",
            ClassKind::AngularVelocity => "angular_velocity",
            ClassKind::BodyAngularVelocity => "body_angular_velocity",
            ClassKind::Terrain => "terrain",
            ClassKind::Sound => "sound",
            ClassKind::Decal => "decal",
            ClassKind::Texture => "texture",
            ClassKind::ParticleEmitter => "particle_emitter",
            ClassKind::Fire => "fire",
            ClassKind::Smoke => "smoke",
            ClassKind::Sparkles => "sparkles",
            ClassKind::Highlight => "highlight",
            ClassKind::ScrollingFrame => "scrolling_frame",
            ClassKind::ImageLabel => "image_label",
            ClassKind::ImageButton => "image_button",
            ClassKind::UIStroke => "ui_stroke",
            ClassKind::UIPadding => "ui_padding",
            ClassKind::UIGridLayout => "ui_grid_layout",
            ClassKind::UIAspectRatioConstraint => "ui_aspect_ratio_constraint",
            ClassKind::UIScale => "ui_scale",
        }
    }

    pub fn from_file_name(name: &str) -> Option<ClassKind> {
        ClassKind::ALL.iter().copied().find(|c| c.file_name() == name)
    }
}
