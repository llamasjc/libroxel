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
    /// A group of instances that move together (behaviour arrives in a later phase).
    Model,
    /// A solid box with a position, size and colour.
    Part,
    /// A Luau script attached to the tree.
    Script,
}

impl ClassKind {
    /// Whether scripts may create this class with a constructor.
    /// Singletons like the root and workspace may not.
    pub fn is_creatable(self) -> bool {
        !matches!(self, ClassKind::DataModel | ClassKind::Workspace)
    }

    /// Whether this class has a position, size and colour.
    pub fn is_physical(self) -> bool {
        matches!(self, ClassKind::Part)
    }

    /// The parent class in the class hierarchy, if any.
    pub fn superclass(self) -> Option<ClassKind> {
        match self {
            // Workspace behaves like a Model that can't be removed.
            ClassKind::Workspace => Some(ClassKind::Model),
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
            Property::Transparency,
        ];
        const SCRIPT: &[Property] = &[Property::Name, Property::Source];
        match self {
            ClassKind::Part => PART,
            ClassKind::Script => SCRIPT,
            _ => COMMON,
        }
    }

    /// Whether this class has `property`.
    pub fn has_property(self, property: Property) -> bool {
        self.properties().contains(&property)
    }
}
