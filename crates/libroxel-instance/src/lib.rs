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

mod math;
pub mod place;
mod property;
mod tree;

pub use math::{Color3, Vec3};
pub use property::{Property, PropertyValue};
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
    ];

    /// Whether scripts may create this class with a constructor.
    /// Singletons like the root and workspace may not.
    pub fn is_creatable(self) -> bool {
        !matches!(self, ClassKind::DataModel | ClassKind::Workspace)
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
            ClassKind::SpawnLocation => Some(ClassKind::Part),
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
            Property::Size,
            Property::Color,
            Property::Anchored,
            Property::CanCollide,
            Property::Transparency,
        ];
        const SCRIPT: &[Property] = &[Property::Name, Property::Disabled, Property::Source];
        const HUMANOID: &[Property] = &[
            Property::Name,
            Property::Health,
            Property::MaxHealth,
            Property::WalkSpeed,
            Property::JumpPower,
        ];
        match self {
            ClassKind::Part | ClassKind::SpawnLocation => PART,
            ClassKind::Script => SCRIPT,
            ClassKind::Humanoid => HUMANOID,
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
            Property::Name | Property::Source => PropertyValue::String(String::new()),
            Property::Position => PropertyValue::Vec3(Vec3::ZERO),
            Property::Size => PropertyValue::Vec3(Vec3::new(4.0, 1.0, 2.0)),
            Property::Color => PropertyValue::Color3(match self {
                ClassKind::SpawnLocation => Color3::new(0.3, 0.55, 0.95),
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
        }
    }

    pub fn from_file_name(name: &str) -> Option<ClassKind> {
        ClassKind::ALL.iter().copied().find(|c| c.file_name() == name)
    }
}
