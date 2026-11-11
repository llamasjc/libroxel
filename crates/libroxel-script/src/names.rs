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

use libroxel_instance::{ClassKind, EnumKind, EnumValue, Material, PartShape, Property};

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
}

/// Methods that only some classes have.
pub mod methods {
    /// Players service: a list of everyone in the game.
    pub const GET_PLAYERS: &str = "GetPlayers";
    /// Players service: the player whose character is the given model, or nil.
    pub const GET_PLAYER_FROM_CHARACTER: &str = "GetPlayerFromCharacter";
    /// Humanoid: lowers health by an amount, never below zero.
    pub const TAKE_DAMAGE: &str = "TakeDamage";
}

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
    /// Fires once per frame with the frame's delta time in seconds.
    pub const HEARTBEAT: &str = "Heartbeat";
}

/// Members of signals and connections.
pub mod signal {
    pub const CONNECT: &str = "Connect";
    pub const DISCONNECT: &str = "Disconnect";
    pub const CONNECTED: &str = "Connected";
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
    }
}

/// The property of `class` that a script-facing name refers to.
pub fn property_from_name(class: ClassKind, name: &str) -> Option<Property> {
    class.properties().iter().copied().find(|p| property_name(*p) == name)
}

/// The name scripts see for a kind of property value.
pub fn value_kind_name(kind: &str) -> &str {
    match kind {
        "Vec3" => vector3::TYPE_NAME,
        "Color3" => color3::TYPE_NAME,
        "bool" => "boolean",
        "ref" => INSTANCE_TYPE_NAME,
        k if k == EnumKind::Shape.kind_name() => enum_name(EnumKind::Shape),
        k if k == EnumKind::Material.kind_name() => enum_name(EnumKind::Material),
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
