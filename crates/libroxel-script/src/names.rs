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

use libroxel_instance::{ClassKind, Property};

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
    }
}

/// The class a script-facing name refers to.
pub fn class_from_name(name: &str) -> Option<ClassKind> {
    Some(match name {
        "DataModel" => ClassKind::DataModel,
        "Workspace" => ClassKind::Workspace,
        "Folder" => ClassKind::Folder,
        "Model" => ClassKind::Model,
        "Part" => ClassKind::Part,
        "SpawnLocation" => ClassKind::SpawnLocation,
        "Script" => ClassKind::Script,
        "Humanoid" => ClassKind::Humanoid,
        _ => return None,
    })
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
    }
}

/// The property a script-facing name refers to.
pub fn property_from_name(name: &str) -> Option<Property> {
    Some(match name {
        "Name" => Property::Name,
        "Position" => Property::Position,
        "Size" => Property::Size,
        "Color" => Property::Color,
        "Anchored" => Property::Anchored,
        "Transparency" => Property::Transparency,
        "Source" => Property::Source,
        "CanCollide" => Property::CanCollide,
        "Disabled" => Property::Disabled,
        "Health" => Property::Health,
        "MaxHealth" => Property::MaxHealth,
        "WalkSpeed" => Property::WalkSpeed,
        "JumpPower" => Property::JumpPower,
        _ => return None,
    })
}

/// The name scripts see for a kind of property value.
pub fn value_kind_name(kind: &str) -> &str {
    match kind {
        "Vec3" => vector3::TYPE_NAME,
        "Color3" => color3::TYPE_NAME,
        "bool" => "boolean",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
                assert_eq!(property_from_name(property_name(property)), Some(property));
            }
        }
    }
}
