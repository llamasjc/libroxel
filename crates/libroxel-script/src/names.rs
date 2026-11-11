//! The naming layer: every identifier a script can see, in one place.
//!
//! The engine has its own internal vocabulary ([`ClassKind`], [`Property`]).
//! Scripts see the names below instead. Keeping every script-facing string
//! here means the public API can be renamed, aliased, or versioned by editing
//! this one file, without touching the instance tree or the bindings.
//!
//! The names follow a common, widely-known style for scripting a scene
//! graph (an `Instance` tree, `Vector3`, services, signals with `Connect`).
//! They are written from the public descriptions of that style only.
//! See `CONTRIBUTING.md` for the clean-room rules.

use libroxel_instance::{
    ClassKind, EasingDirection, EasingStyle, EnumKind, EnumValue, Material, PartShape, Property,
};

/// Global variables installed into every script.
pub mod globals {
    pub const GAME: &str = "game";
    pub const WORKSPACE: &str = "workspace";
    pub const INSTANCE: &str = "Instance";
    pub const VECTOR3: &str = "Vector3";
    pub const COLOR3: &str = "Color3";
    pub const PRINT: &str = "print";
    pub const WARN: &str = "warn";
    /// The scheduling library.
    pub const TASK: &str = "task";
    /// Shorthand for `task.wait`, kept because many existing scripts use it.
    pub const WAIT: &str = "wait";
    /// Seconds since the game started.
    pub const TIME: &str = "time";
    /// Inside a script: the script instance itself.
    pub const SCRIPT: &str = "script";
    /// Loads a module script and returns what it returned.
    pub const REQUIRE: &str = "require";
    /// The table of enumerations, such as `Enum.Material`.
    pub const ENUM: &str = "Enum";
    pub const CFRAME: &str = "CFrame";
    pub const VECTOR2: &str = "Vector2";
    pub const UDIM2: &str = "UDim2";
    pub const TWEEN_INFO: &str = "TweenInfo";
}

/// Members of the scheduling library.
pub mod task {
    /// Pause the current thread for some seconds (one frame by default).
    pub const WAIT: &str = "wait";
    /// Run a function now in a new thread.
    pub const SPAWN: &str = "spawn";
    /// Run a function in a new thread after some seconds.
    pub const DELAY: &str = "delay";
}

/// Events that instances fire.
pub mod events {
    /// On parts: fires with the other part when something starts touching it.
    pub const TOUCHED: &str = "Touched";
    /// On the players service: fires with the player when someone joins.
    pub const PLAYER_ADDED: &str = "PlayerAdded";
    /// On the players service: fires with the player just before they leave.
    pub const PLAYER_REMOVING: &str = "PlayerRemoving";
    /// On a player: fires with the new character model each time one spawns.
    pub const CHARACTER_ADDED: &str = "CharacterAdded";
    /// On a humanoid: fires once when its health reaches zero.
    pub const DIED: &str = "Died";
    /// On any instance: fires with a property's name after it changes.
    pub const CHANGED: &str = "Changed";
    /// On any instance: fires with the new child when one is added.
    pub const CHILD_ADDED: &str = "ChildAdded";
    /// On a text button: fires when it is clicked.
    pub const MOUSE_BUTTON1_CLICK: &str = "MouseButton1Click";
    /// On a text button: fires when it is clicked (same moment as the click event).
    pub const ACTIVATED: &str = "Activated";
    /// On a click detector: fires with the player who clicked its part.
    pub const MOUSE_CLICK: &str = "MouseClick";
    /// On a tween: fires when it finishes playing.
    pub const COMPLETED: &str = "Completed";
}

/// Methods that only some classes have.
pub mod methods {
    /// Players service: a list of everyone in the game.
    pub const GET_PLAYERS: &str = "GetPlayers";
    /// Players service: the player whose character is the given model, or nil.
    pub const GET_PLAYER_FROM_CHARACTER: &str = "GetPlayerFromCharacter";
    /// Humanoid: lowers health by an amount, never below zero.
    pub const TAKE_DAMAGE: &str = "TakeDamage";
    /// Any instance: a signal that fires when one named property changes.
    pub const GET_PROPERTY_CHANGED_SIGNAL: &str = "GetPropertyChangedSignal";
    /// Any instance: destroys every child.
    pub const CLEAR_ALL_CHILDREN: &str = "ClearAllChildren";
    /// Any instance: the nearest ancestor with a name.
    pub const FIND_FIRST_ANCESTOR: &str = "FindFirstAncestor";
    /// Any instance: the first descendant with a name.
    pub const FIND_FIRST_DESCENDANT: &str = "FindFirstDescendant";
    /// Players service: the player on this computer.
    pub const LOCAL_PLAYER: &str = "LocalPlayer";
    /// Tween service: makes a tween.
    pub const CREATE: &str = "Create";
    /// Model: moves the whole model so its first part is at a CFrame.
    pub const PIVOT_TO: &str = "PivotTo";
    /// Model: the CFrame of its first part.
    pub const GET_PIVOT: &str = "GetPivot";
}

/// Members of tweens.
pub mod tween {
    pub const PLAY: &str = "Play";
    pub const PAUSE: &str = "Pause";
    pub const CANCEL: &str = "Cancel";
    pub const TYPE_NAME: &str = "Tween";
    pub const INFO_TYPE_NAME: &str = "TweenInfo";
    pub const TIME: &str = "Time";
    pub const EASING_STYLE: &str = "EasingStyle";
    pub const EASING_DIRECTION: &str = "EasingDirection";
    pub const REPEAT_COUNT: &str = "RepeatCount";
    pub const REVERSES: &str = "Reverses";
    pub const DELAY_TIME: &str = "DelayTime";
}

/// Members of the position-and-rotation type.
pub mod cframe {
    pub const TYPE_NAME: &str = "CFrame";
    pub const POSITION: &str = "Position";
    pub const X: &str = "X";
    pub const Y: &str = "Y";
    pub const Z: &str = "Z";
    pub const LOOK_VECTOR: &str = "LookVector";
    pub const RIGHT_VECTOR: &str = "RightVector";
    pub const UP_VECTOR: &str = "UpVector";
    pub const ROTATION: &str = "Rotation";
    pub const INVERSE: &str = "Inverse";
    pub const LERP: &str = "Lerp";
    pub const TO_WORLD_SPACE: &str = "ToWorldSpace";
    pub const TO_OBJECT_SPACE: &str = "ToObjectSpace";
    pub const POINT_TO_WORLD_SPACE: &str = "PointToWorldSpace";
    pub const POINT_TO_OBJECT_SPACE: &str = "PointToObjectSpace";
    pub const TO_ORIENTATION: &str = "ToOrientation";
    /// Constructors.
    pub const LOOK_AT: &str = "lookAt";
    pub const ANGLES: &str = "Angles";
    pub const FROM_ORIENTATION: &str = "fromOrientation";
    pub const FROM_AXIS_ANGLE: &str = "fromAxisAngle";
    pub const IDENTITY: &str = "identity";
}

/// Members of the 2D vector and screen-dimension types.
pub mod gui_values {
    pub const VECTOR2_TYPE_NAME: &str = "Vector2";
    pub const UDIM2_TYPE_NAME: &str = "UDim2";
    pub const FROM_SCALE: &str = "fromScale";
    pub const FROM_OFFSET: &str = "fromOffset";
    pub const X: &str = "X";
    pub const Y: &str = "Y";
    /// On a UDim2 axis.
    pub const SCALE: &str = "Scale";
    pub const OFFSET: &str = "Offset";
    pub const WIDTH: &str = "Width";
    pub const HEIGHT: &str = "Height";
}

/// The extra member every part has: its position and rotation together.
pub const PART_CFRAME: &str = "CFrame";

/// Members of enum items.
pub mod enum_item {
    pub const NAME: &str = "Name";
    pub const VALUE: &str = "Value";
    pub const ENUM_TYPE: &str = "EnumType";
    pub const TYPE_NAME: &str = "EnumItem";
}

/// The name scripts use for each enumeration, as in `Enum.<name>`.
pub fn enum_name(kind: EnumKind) -> &'static str {
    match kind {
        EnumKind::Shape => "PartType",
        EnumKind::Material => "Material",
        EnumKind::EasingStyle => "EasingStyle",
        EnumKind::EasingDirection => "EasingDirection",
    }
}

/// The enumeration a script-facing name refers to.
pub fn enum_from_name(name: &str) -> Option<EnumKind> {
    EnumKind::ALL.iter().copied().find(|k| enum_name(*k) == name)
}

/// The name scripts use for each enum value, as in `Enum.Material.<name>`.
pub fn enum_item_name(value: EnumValue) -> &'static str {
    match value {
        EnumValue::Shape(s) => match s {
            PartShape::Block => "Block",
            PartShape::Ball => "Ball",
            PartShape::Cylinder => "Cylinder",
            PartShape::Wedge => "Wedge",
        },
        EnumValue::Material(m) => match m {
            Material::Plastic => "Plastic",
            Material::SmoothPlastic => "SmoothPlastic",
            Material::Neon => "Neon",
            Material::Metal => "Metal",
            Material::Glass => "Glass",
            Material::Wood => "Wood",
            Material::Brick => "Brick",
            Material::Grass => "Grass",
        },
        EnumValue::EasingStyle(e) => match e {
            EasingStyle::Linear => "Linear",
            EasingStyle::Sine => "Sine",
            EasingStyle::Quad => "Quad",
            EasingStyle::Cubic => "Cubic",
            EasingStyle::Quart => "Quart",
            EasingStyle::Quint => "Quint",
            EasingStyle::Exponential => "Exponential",
            EasingStyle::Circular => "Circular",
            EasingStyle::Back => "Back",
            EasingStyle::Bounce => "Bounce",
            EasingStyle::Elastic => "Elastic",
        },
        EnumValue::EasingDirection(d) => match d {
            EasingDirection::In => "In",
            EasingDirection::Out => "Out",
            EasingDirection::InOut => "InOut",
        },
    }
}

/// The value of `kind` that a script-facing item name refers to.
pub fn enum_item_from_name(kind: EnumKind, name: &str) -> Option<EnumValue> {
    kind.items().iter().copied().find(|v| enum_item_name(*v) == name)
}

/// Names of the instances the engine builds for each player.
pub mod character {
    pub const MODEL: &str = "Player";
    pub const ROOT_PART: &str = "HumanoidRootPart";
    pub const HUMANOID: &str = "Humanoid";
}

/// Constructors and constants on the global library tables.
pub mod constructors {
    pub const NEW: &str = "new";
    pub const FROM_RGB: &str = "fromRGB";
    pub const ZERO: &str = "zero";
    pub const ONE: &str = "one";
}

/// Members that every instance has, beyond its class properties.
pub mod instance {
    pub const CLASS_NAME: &str = "ClassName";
    pub const PARENT: &str = "Parent";
    pub const FIND_FIRST_CHILD: &str = "FindFirstChild";
    pub const FIND_FIRST_CHILD_OF_CLASS: &str = "FindFirstChildOfClass";
    pub const GET_CHILDREN: &str = "GetChildren";
    pub const GET_DESCENDANTS: &str = "GetDescendants";
    pub const IS_A: &str = "IsA";
    pub const IS_DESCENDANT_OF: &str = "IsDescendantOf";
    pub const GET_FULL_NAME: &str = "GetFullName";
    pub const DESTROY: &str = "Destroy";
    pub const CLONE: &str = "Clone";
    /// Yields until a child with the given name exists.
    pub const WAIT_FOR_CHILD: &str = "WaitForChild";
    /// Only on the root instance.
    pub const GET_SERVICE: &str = "GetService";
}

/// Members of the 3D vector type.
pub mod vector3 {
    pub const X: &str = "X";
    pub const Y: &str = "Y";
    pub const Z: &str = "Z";
    pub const MAGNITUDE: &str = "Magnitude";
    pub const UNIT: &str = "Unit";
    pub const DOT: &str = "Dot";
    pub const CROSS: &str = "Cross";
    pub const LERP: &str = "Lerp";
    pub const TYPE_NAME: &str = "Vector3";
}

/// Members of the colour type.
pub mod color3 {
    pub const R: &str = "R";
    pub const G: &str = "G";
    pub const B: &str = "B";
    pub const TYPE_NAME: &str = "Color3";
}

/// Services reachable through the root's `GetService`.
pub mod services {
    pub const RUN_SERVICE: &str = "RunService";
    pub const TWEEN_SERVICE: &str = "TweenService";
    /// Fires once per frame with the frame's delta time in seconds.
    pub const HEARTBEAT: &str = "Heartbeat";
}

/// Members of signals and connections.
pub mod signal {
    pub const CONNECT: &str = "Connect";
    pub const DISCONNECT: &str = "Disconnect";
    pub const CONNECTED: &str = "Connected";
    /// Like Connect, but only for the next time the signal fires.
    pub const ONCE: &str = "Once";
    /// Yields until the signal fires, then returns what it fired with.
    pub const WAIT: &str = "Wait";
}

/// What `typeof` reports for an instance.
pub const INSTANCE_TYPE_NAME: &str = "Instance";

/// The script-facing name of each class.
pub fn class_name(class: ClassKind) -> &'static str {
    match class {
        ClassKind::DataModel => "DataModel",
        ClassKind::Workspace => "Workspace",
        ClassKind::Folder => "Folder",
        ClassKind::Model => "Model",
        ClassKind::Part => "Part",
        ClassKind::SpawnLocation => "SpawnLocation",
        ClassKind::Script => "Script",
        ClassKind::Humanoid => "Humanoid",
        ClassKind::ModuleScript => "ModuleScript",
        ClassKind::Players => "Players",
        ClassKind::Player => "Player",
        ClassKind::ReplicatedStorage => "ReplicatedStorage",
        ClassKind::IntValue => "IntValue",
        ClassKind::NumberValue => "NumberValue",
        ClassKind::StringValue => "StringValue",
        ClassKind::BoolValue => "BoolValue",
        ClassKind::StarterGui => "StarterGui",
        ClassKind::Lighting => "Lighting",
        ClassKind::PlayerGui => "PlayerGui",
        ClassKind::ScreenGui => "ScreenGui",
        ClassKind::GuiObject => "GuiObject",
        ClassKind::Frame => "Frame",
        ClassKind::TextLabel => "TextLabel",
        ClassKind::TextButton => "TextButton",
        ClassKind::ClickDetector => "ClickDetector",
        ClassKind::ServerScriptService => "ServerScriptService",
        ClassKind::LocalScript => "LocalScript",
    }
}

/// The class a script-facing name refers to.
pub fn class_from_name(name: &str) -> Option<ClassKind> {
    ClassKind::ALL.iter().copied().find(|c| class_name(*c) == name)
}

/// The script-facing name of each property.
pub fn property_name(property: Property) -> &'static str {
    match property {
        Property::Name => "Name",
        Property::Position => "Position",
        Property::Size => "Size",
        Property::Color => "Color",
        Property::Anchored => "Anchored",
        Property::Transparency => "Transparency",
        Property::Source => "Source",
        Property::CanCollide => "CanCollide",
        Property::Disabled => "Disabled",
        Property::Health => "Health",
        Property::MaxHealth => "MaxHealth",
        Property::WalkSpeed => "WalkSpeed",
        Property::JumpPower => "JumpPower",
        Property::Orientation => "Orientation",
        Property::Shape => "Shape",
        Property::Material => "Material",
        // Every value object calls its value `Value`, whatever its type.
        Property::NumberValue | Property::StringValue | Property::BoolValue => "Value",
        Property::Character => "Character",
        Property::UserId => "UserId",
        Property::GuiPosition => "Position",
        Property::GuiSize => "Size",
        Property::AnchorPoint => "AnchorPoint",
        Property::BackgroundColor => "BackgroundColor3",
        Property::BackgroundTransparency => "BackgroundTransparency",
        Property::Visible => "Visible",
        Property::Text => "Text",
        Property::TextColor => "TextColor3",
        Property::TextSize => "TextSize",
        Property::Enabled => "Enabled",
        Property::ResetOnSpawn => "ResetOnSpawn",
        Property::MaxActivationDistance => "MaxActivationDistance",
        Property::ClockTime => "ClockTime",
        Property::Brightness => "Brightness",
    }
}

/// The property of `class` that a script-facing name refers to.
pub fn property_from_name(class: ClassKind, name: &str) -> Option<Property> {
    class.properties().iter().copied().find(|p| property_name(*p) == name)
}

/// The name scripts see for a kind of property value.
pub fn value_kind_name(kind: &str) -> &str {
    if let Some(e) = EnumKind::ALL.iter().find(|e| e.kind_name() == kind) {
        return enum_name(*e);
    }
    match kind {
        "Vec3" => vector3::TYPE_NAME,
        "Color3" => color3::TYPE_NAME,
        "bool" => "boolean",
        "ref" => INSTANCE_TYPE_NAME,
        "Vec2" => gui_values::VECTOR2_TYPE_NAME,
        "UDim2" => gui_values::UDIM2_TYPE_NAME,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_names_round_trip() {
        for &kind in EnumKind::ALL {
            assert_eq!(enum_from_name(enum_name(kind)), Some(kind));
            for &item in kind.items() {
                assert_eq!(enum_item_from_name(kind, enum_item_name(item)), Some(item));
            }
        }
    }

    #[test]
    fn class_names_round_trip() {
        for &class in ClassKind::ALL {
            assert_eq!(class_from_name(class_name(class)), Some(class));
        }
    }

    #[test]
    fn property_names_round_trip() {
        for &class in ClassKind::ALL {
            for &property in class.properties() {
                assert_eq!(property_from_name(class, property_name(property)), Some(property));
            }
        }
    }
}
