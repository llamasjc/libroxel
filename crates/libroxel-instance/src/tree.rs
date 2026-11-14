//! The tree itself: storage, hierarchy and the change journal.

use std::fmt;

use crate::{ClassKind, Color3, EnumValue, Property, PropertyValue, Vec3};

/// A generational handle to an instance.
///
/// Ids are cheap to copy and compare. An id stays valid while its instance
/// exists, including after it is destroyed (so scripts can still read a
/// destroyed instance's properties). Only [`InstanceTree::purge_destroyed`]
/// retires ids; a retired id never resolves again, even after its slot is
/// reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InstanceId {
    index: u32,
    generation: u32,
}

impl InstanceId {
    /// The id as one number, for sending to another computer.
    pub fn to_bits(self) -> u64 {
        (u64::from(self.generation) << 32) | u64::from(self.index)
    }

    /// The id [`to_bits`](Self::to_bits) made.
    pub fn from_bits(bits: u64) -> Self {
        Self { index: bits as u32, generation: (bits >> 32) as u32 }
    }
}

impl fmt::Display for InstanceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}v{}", self.index, self.generation)
    }
}

/// Something that happened to the tree, recorded so other systems (the
/// renderer, replication) can catch up without diffing the whole tree.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Created(InstanceId),
    Reparented {
        id: InstanceId,
        old_parent: Option<InstanceId>,
        new_parent: Option<InstanceId>,
    },
    PropertyChanged {
        id: InstanceId,
        property: Property,
    },
    Destroyed(InstanceId),
    AttributeChanged {
        id: InstanceId,
        name: String,
    },
    TagAdded {
        id: InstanceId,
        tag: String,
    },
    TagRemoved {
        id: InstanceId,
        tag: String,
    },
    /// A terrain's cells changed.
    VoxelsChanged(InstanceId),
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum TreeError {
    #[error("instance {0} no longer exists")]
    Dead(InstanceId),
    #[error("the parent of {0} is locked and cannot be changed")]
    ParentLocked(InstanceId),
    #[error("cannot parent {child} to {parent}: it would create a cycle")]
    WouldCycle { child: InstanceId, parent: InstanceId },
    #[error("{0:?} cannot be created directly")]
    NotCreatable(ClassKind),
    #[error("{class:?} has no property {property:?}")]
    NoSuchProperty { class: ClassKind, property: Property },
    #[error("{property:?} expects a {expected}, got a {got}")]
    WrongType { property: Property, expected: &'static str, got: &'static str },
    #[error("attributes can't hold a {0}")]
    BadAttribute(&'static str),
}

pub type Result<T> = std::result::Result<T, TreeError>;

#[derive(Debug, Clone)]
struct InstanceData {
    class: ClassKind,
    name: String,
    parent: Option<InstanceId>,
    children: Vec<InstanceId>,
    parent_locked: bool,
    destroyed: bool,
    /// Every property except `Name`, in [`ClassKind::properties`] order.
    props: Vec<(Property, PropertyValue)>,
    /// Values scripts and the editor attach by name.
    attributes: std::collections::BTreeMap<String, PropertyValue>,
    /// Properties of the Roblox-style class that libroxel has no setting
    /// for, kept by their script name so files and scripts don't lose them.
    extras: std::collections::BTreeMap<String, PropertyValue>,
    /// Labels for grouping instances, kept sorted.
    tags: Vec<String>,
    /// A terrain's cells (empty for everything else).
    voxels: crate::voxels::Voxels,
    /// Changes whenever a property does (see [`InstanceTree::revision`]).
    revision: u64,
}

/// Revisions come from one counter for every tree, so a revision number
/// always means the same property values, even across undo snapshots.
static NEXT_REVISION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn next_revision() -> u64 {
    NEXT_REVISION.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl InstanceData {
    fn new(class: ClassKind, name: String) -> Self {
        let props = class
            .properties()
            .iter()
            .filter(|p| **p != Property::Name)
            .filter_map(|p| Some((*p, class.default_value(*p)?)))
            .collect();
        Self {
            class,
            name,
            parent: None,
            children: Vec::new(),
            parent_locked: false,
            destroyed: false,
            props,
            attributes: Default::default(),
            extras: Default::default(),
            tags: Vec::new(),
            voxels: Default::default(),
            revision: next_revision(),
        }
    }

    fn prop_mut(&mut self, property: Property) -> Option<&mut PropertyValue> {
        let at = self.slot_of(property)?;
        self.props.get_mut(at).map(|(_, v)| v)
    }

    fn prop(&self, property: Property) -> Option<&PropertyValue> {
        let at = self.slot_of(property)?;
        self.props.get(at).map(|(_, v)| v)
    }

    /// Where `property` is in `props`: worked out once per class (the list
    /// is always built in the same order), checked, else looked for.
    fn slot_of(&self, property: Property) -> Option<usize> {
        if let Some(at) = prop_slot(self.class, property)
            && self.props.get(at).is_some_and(|(p, _)| *p == property)
        {
            return Some(at);
        }
        self.props.iter().position(|(p, _)| *p == property)
    }
}

/// Each class's property positions in [`InstanceData::props`].
fn prop_slot(class: ClassKind, property: Property) -> Option<usize> {
    use std::sync::OnceLock;
    static SLOTS: OnceLock<Vec<Vec<u16>>> = OnceLock::new();
    let slots = SLOTS.get_or_init(|| {
        let mut by_class: Vec<Vec<u16>> = Vec::new();
        for (class, table) in ClassKind::ALL.iter().chain(std::iter::once(&ClassKind::Other)).map(|&class| {
            let mut table = Vec::new();
            let present = class
                .properties()
                .iter()
                .filter(|p| **p != Property::Name)
                .filter(|p| class.default_value(**p).is_some());
            for (i, p) in present.enumerate() {
                let n = *p as usize;
                if table.len() <= n {
                    table.resize(n + 1, u16::MAX);
                }
                table[n] = i as u16;
            }
            (class, table)
        }) {
            let n = class as usize;
            if by_class.len() <= n {
                by_class.resize(n + 1, Vec::new());
            }
            by_class[n] = table;
        }
        by_class
    });
    let at = *slots.get(class as usize)?.get(property as usize)?;
    (at != u16::MAX).then_some(usize::from(at))
}

#[derive(Debug, Clone)]
struct Slot {
    generation: u32,
    data: Option<InstanceData>,
}

/// A world's instance tree.
#[derive(Debug, Clone)]
pub struct InstanceTree {
    slots: Vec<Slot>,
    free: Vec<u32>,
    root: InstanceId,
    workspace: InstanceId,
    /// One per [`ClassKind::SERVICES`], in the same order.
    services: Vec<InstanceId>,
    changes: Vec<Change>,
    /// Instances whose place in the world may have changed (moved, resized,
    /// reparented, destroyed), for spatial indexes to catch up on; and how
    /// many times the log was restarted.
    moved: Vec<InstanceId>,
    moved_epoch: u64,
    /// Each tag and the instances that carry it (some may have been
    /// destroyed since), so finding tagged instances doesn't walk the game.
    tag_index: std::collections::HashMap<String, std::collections::HashSet<InstanceId>>,
    /// Bumped whenever something is reparented or destroyed.
    structure: u64,
    /// The workspace's descendants of each class, kept while nothing is
    /// reparented: many systems ask for them every frame.
    class_cache: std::cell::RefCell<ClassCache>,
    /// Where the workspace's parts are, for raycasts.
    part_index: std::cell::RefCell<crate::geometry::PartIndex>,
}

#[derive(Debug, Clone, Default)]
struct ClassCache {
    structure: Option<u64>,
    all: Vec<InstanceId>,
    /// The class of each of `all`, by its number.
    classes: Vec<u16>,
    by_class: std::collections::HashMap<ClassKind, Vec<InstanceId>>,
}

impl Default for InstanceTree {
    fn default() -> Self {
        Self::new()
    }
}

impl InstanceTree {
    /// Creates a tree with a root and every service ([`ClassKind::SERVICES`]).
    /// None of them can be reparented or destroyed.
    pub fn new() -> Self {
        let mut tree = Self {
            slots: Vec::new(),
            free: Vec::new(),
            root: InstanceId { index: 0, generation: 0 },
            workspace: InstanceId { index: 0, generation: 0 },
            services: Vec::new(),
            moved: Vec::new(),
            moved_epoch: 0,
            changes: Vec::new(),
            tag_index: Default::default(),
            structure: 0,
            class_cache: Default::default(),
            part_index: Default::default(),
        };
        tree.root = tree.alloc(InstanceData::new(ClassKind::DataModel, "Game".into()));
        tree.data_mut(tree.root).expect("root exists").parent_locked = true;
        for &class in ClassKind::SERVICES {
            let name = match class {
                ClassKind::Workspace => "Workspace",
                ClassKind::Players => "Players",
                ClassKind::ReplicatedStorage => "ReplicatedStorage",
                ClassKind::StarterGui => "StarterGui",
                ClassKind::Lighting => "Lighting",
                ClassKind::ServerScriptService => "ServerScriptService",
                ClassKind::StarterPack => "StarterPack",
                ClassKind::Teams => "Teams",
                ClassKind::ServerStorage => "ServerStorage",
                ClassKind::StarterPlayer => "StarterPlayer",
                _ => unreachable!("every service has a name"),
            };
            let id = tree.alloc(InstanceData::new(class, name.into()));
            tree.set_parent(id, Some(tree.root)).expect("fresh services can be parented");
            tree.data_mut(id).expect("service exists").parent_locked = true;
            tree.services.push(id);
        }
        tree.workspace = tree.services[0];
        // The starter player's two script folders come with it.
        let starter = tree.service(ClassKind::StarterPlayer).expect("the starter player is a service");
        for (class, name) in [
            (ClassKind::StarterPlayerScripts, "StarterPlayerScripts"),
            (ClassKind::StarterCharacterScripts, "StarterCharacterScripts"),
        ] {
            let id = tree.alloc(InstanceData::new(class, name.into()));
            tree.set_parent(id, Some(starter)).expect("fresh folders can be parented");
            tree.data_mut(id).expect("folder exists").parent_locked = true;
        }
        tree.changes.clear();
        tree
    }

    /// The service of class `class`, if it is a service.
    pub fn service(&self, class: ClassKind) -> Option<InstanceId> {
        ClassKind::SERVICES.iter().position(|c| *c == class).map(|i| self.services[i])
    }

    /// One of the starter player's script folders
    /// ([`StarterPlayerScripts`](ClassKind::StarterPlayerScripts) or
    /// [`StarterCharacterScripts`](ClassKind::StarterCharacterScripts)).
    pub fn starter_scripts(&self, class: ClassKind) -> Option<InstanceId> {
        let starter = self.service(ClassKind::StarterPlayer)?;
        self.find_first_child_of_class(starter, class).ok().flatten()
    }

    pub fn root(&self) -> InstanceId {
        self.root
    }

    pub fn workspace(&self) -> InstanceId {
        self.workspace
    }

    fn alloc(&mut self, data: InstanceData) -> InstanceId {
        let tags = (!data.tags.is_empty()).then(|| data.tags.clone());
        let id = if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            slot.data = Some(data);
            InstanceId { index, generation: slot.generation }
        } else {
            let index = u32::try_from(self.slots.len()).expect("fewer than 2^32 instances");
            self.slots.push(Slot { generation: 0, data: Some(data) });
            InstanceId { index, generation: 0 }
        };
        for tag in tags.into_iter().flatten() {
            self.tag_index.entry(tag).or_default().insert(id);
        }
        id
    }

    fn data(&self, id: InstanceId) -> Result<&InstanceData> {
        self.slots
            .get(id.index as usize)
            .filter(|slot| slot.generation == id.generation)
            .and_then(|slot| slot.data.as_ref())
            .ok_or(TreeError::Dead(id))
    }

    fn data_mut(&mut self, id: InstanceId) -> Result<&mut InstanceData> {
        self.slots
            .get_mut(id.index as usize)
            .filter(|slot| slot.generation == id.generation)
            .and_then(|slot| slot.data.as_mut())
            .ok_or(TreeError::Dead(id))
    }

    /// Creates a new, unparented instance.
    pub fn create(&mut self, class: ClassKind, name: impl Into<String>) -> Result<InstanceId> {
        if !class.is_creatable() {
            return Err(TreeError::NotCreatable(class));
        }
        let id = self.alloc(InstanceData::new(class, name.into()));
        self.changes.push(Change::Created(id));
        Ok(id)
    }

    /// Whether `id` still refers to an instance (destroyed or not).
    pub fn exists(&self, id: InstanceId) -> bool {
        self.data(id).is_ok()
    }

    /// Whether `id` has been destroyed.
    pub fn is_destroyed(&self, id: InstanceId) -> Result<bool> {
        Ok(self.data(id)?.destroyed)
    }

    pub fn class(&self, id: InstanceId) -> Result<ClassKind> {
        Ok(self.data(id)?.class)
    }

    pub fn name(&self, id: InstanceId) -> Result<&str> {
        Ok(&self.data(id)?.name)
    }

    pub fn parent(&self, id: InstanceId) -> Result<Option<InstanceId>> {
        Ok(self.data(id)?.parent)
    }

    pub fn children(&self, id: InstanceId) -> Result<&[InstanceId]> {
        Ok(&self.data(id)?.children)
    }

    /// The first direct child called `name`, in child order.
    pub fn find_first_child(&self, id: InstanceId, name: &str) -> Result<Option<InstanceId>> {
        let data = self.data(id)?;
        Ok(data.children.iter().copied().find(|child| self.data(*child).is_ok_and(|c| c.name == name)))
    }

    /// The first direct child of class `class` (or a subclass of it).
    pub fn find_first_child_of_class(&self, id: InstanceId, class: ClassKind) -> Result<Option<InstanceId>> {
        let data = self.data(id)?;
        Ok(data.children.iter().copied().find(|child| self.data(*child).is_ok_and(|c| c.class.is_a(class))))
    }

    /// All descendants of `id` in depth-first pre-order, not including `id`.
    pub fn descendants(&self, id: InstanceId) -> Result<Vec<InstanceId>> {
        let mut out = Vec::new();
        let mut stack: Vec<InstanceId> = self.data(id)?.children.iter().rev().copied().collect();
        while let Some(next) = stack.pop() {
            out.push(next);
            if let Ok(data) = self.data(next) {
                stack.extend(data.children.iter().rev().copied());
            }
        }
        Ok(out)
    }

    /// Whether `id` is a strict descendant of `ancestor`.
    pub fn is_descendant_of(&self, id: InstanceId, ancestor: InstanceId) -> Result<bool> {
        let mut current = self.data(id)?.parent;
        while let Some(parent) = current {
            if parent == ancestor {
                return Ok(true);
            }
            current = self.data(parent)?.parent;
        }
        Ok(false)
    }

    /// Moves `id` under `new_parent`, or detaches it with `None`.
    pub fn set_parent(&mut self, id: InstanceId, new_parent: Option<InstanceId>) -> Result<()> {
        let data = self.data(id)?;
        if data.parent_locked {
            return Err(TreeError::ParentLocked(id));
        }
        let old_parent = data.parent;
        if old_parent == new_parent {
            return Ok(());
        }
        if let Some(parent) = new_parent {
            if self.data(parent)?.destroyed {
                return Err(TreeError::ParentLocked(id));
            }
            if parent == id || self.is_descendant_of(parent, id)? {
                return Err(TreeError::WouldCycle { child: id, parent });
            }
        }

        if let Some(old) = old_parent {
            self.data_mut(old)?.children.retain(|c| *c != id);
        }
        if let Some(parent) = new_parent {
            self.data_mut(parent)?.children.push(id);
        }
        self.data_mut(id)?.parent = new_parent;
        self.structure += 1;
        self.changes.push(Change::Reparented { id, old_parent, new_parent });
        self.note_moved(id);
        Ok(())
    }

    /// Destroys `id` and all its descendants: each is detached, its parent
    /// is locked, and a [`Change::Destroyed`] is recorded. Their data stays
    /// readable until [`purge_destroyed`](Self::purge_destroyed).
    pub fn destroy(&mut self, id: InstanceId) -> Result<()> {
        let data = self.data(id)?;
        if data.destroyed {
            return Ok(());
        }
        if data.parent_locked {
            return Err(TreeError::ParentLocked(id));
        }
        self.set_parent(id, None)?;
        self.structure += 1;
        let mut doomed = self.descendants(id)?;
        doomed.insert(0, id);
        // Destroy leaves first so observers never see a destroyed parent with live children.
        for victim in doomed.into_iter().rev() {
            let children = std::mem::take(&mut self.data_mut(victim)?.children);
            for child in children {
                let child_data = self.data_mut(child)?;
                child_data.parent = None;
            }
            let data = self.data_mut(victim)?;
            data.parent = None;
            data.parent_locked = true;
            data.destroyed = true;
            self.changes.push(Change::Destroyed(victim));
            self.note_moved(victim);
        }
        Ok(())
    }

    /// Frees every destroyed instance. Their ids stop resolving.
    ///
    /// Call this only when nothing (such as a script) can still hold one of
    /// those ids, or accept that such holders will see [`TreeError::Dead`].
    pub fn purge_destroyed(&mut self) -> usize {
        let mut purged = 0;
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if slot.data.as_ref().is_some_and(|d| d.destroyed) {
                let id = InstanceId { index: index as u32, generation: slot.generation };
                for tag in slot.data.as_ref().map(|d| d.tags.as_slice()).unwrap_or_default() {
                    if let Some(set) = self.tag_index.get_mut(tag) {
                        set.remove(&id);
                    }
                }
                slot.data = None;
                slot.generation = slot.generation.wrapping_add(1);
                self.free.push(index as u32);
                purged += 1;
            }
        }
        purged
    }

    /// Reads a property.
    pub fn get(&self, id: InstanceId, property: Property) -> Result<PropertyValue> {
        let data = self.data(id)?;
        if property == Property::Name {
            return Ok(PropertyValue::String(data.name.clone()));
        }
        data.prop(property).cloned().ok_or(TreeError::NoSuchProperty { class: data.class, property })
    }

    /// Writes a property, recording a change only if the value differs.
    pub fn set(&mut self, id: InstanceId, property: Property, value: PropertyValue) -> Result<()> {
        let data = self.data_mut(id)?;
        let class = data.class;
        let wrong_type = |value: &PropertyValue| TreeError::WrongType {
            property,
            expected: property.expected_kind(),
            got: value.kind_name(),
        };
        let changed = if property == Property::Name {
            match value {
                PropertyValue::String(v) => replace(&mut data.name, v),
                other => return Err(wrong_type(&other)),
            }
        } else {
            let slot = data.prop_mut(property).ok_or(TreeError::NoSuchProperty { class, property })?;
            if slot.kind_name() != value.kind_name() {
                return Err(wrong_type(&value));
            }
            let value = match (property, value) {
                (
                    Property::Transparency
                    | Property::Reflectance
                    | Property::BackgroundTransparency
                    | Property::LightEmission
                    | Property::Opacity
                    | Property::FillTransparency
                    | Property::OutlineTransparency
                    | Property::TextTransparency,
                    PropertyValue::Number(t),
                ) => PropertyValue::Number(t.clamp(0.0, 1.0)),
                (Property::NumberValue, PropertyValue::Number(n)) if class == ClassKind::IntValue => {
                    PropertyValue::Number(n.round())
                }
                (Property::ClockTime, PropertyValue::Number(n)) => PropertyValue::Number(n.rem_euclid(24.0)),
                (
                    Property::HoldDuration
                    | Property::MaxActivationDistance
                    | Property::MaxDistance
                    | Property::Range
                    | Property::BlastRadius
                    | Property::BlastPressure
                    | Property::RespawnTime
                    | Property::Gravity
                    | Property::MaxSpeed
                    | Property::Torque
                    | Property::TurnSpeed
                    | Property::CharacterWalkSpeed
                    | Property::CharacterJumpPower
                    | Property::FogStart
                    | Property::FogEnd
                    | Property::MaxForce
                    | Property::MaxVelocity
                    | Property::Responsiveness
                    | Property::Length
                    | Property::FreeLength
                    | Property::Stiffness
                    | Property::Damping
                    | Property::Thickness
                    | Property::BodyPower
                    | Property::BodyDamping
                    | Property::MotorMaxTorque
                    | Property::AngularSpeed
                    | Property::ServoMaxTorque
                    | Property::MaxTorque
                    | Property::StudsPerTileU
                    | Property::StudsPerTileV
                    | Property::PlaybackSpeed
                    | Property::Rate
                    | Property::EffectSize
                    | Property::Drag
                    | Property::Heat
                    | Property::BorderSizePixel
                    | Property::ScrollBarThickness
                    | Property::AspectRatio
                    | Property::Scale,
                    PropertyValue::Number(n),
                ) => PropertyValue::Number(n.max(0.0)),
                (Property::LayoutOrder | Property::ZIndex, PropertyValue::Number(n)) => {
                    PropertyValue::Number(n.round())
                }
                (Property::Throttle | Property::Steer, PropertyValue::Number(n)) => {
                    PropertyValue::Number(n.round().clamp(-1.0, 1.0))
                }
                (Property::FieldOfView, PropertyValue::Number(n)) => {
                    PropertyValue::Number(n.clamp(1.0, 120.0))
                }
                (Property::Angle, PropertyValue::Number(n)) => PropertyValue::Number(n.clamp(0.0, 180.0)),
                (_, v) => v,
            };
            replace(slot, value)
        };
        if changed {
            self.data_mut(id)?.revision = next_revision();
            self.changes.push(Change::PropertyChanged { id, property });
            if matches!(
                property,
                Property::Position
                    | Property::Size
                    | Property::Orientation
                    | Property::Shape
                    | Property::Anchored
            ) {
                self.note_moved(id);
            }
        }
        Ok(())
    }

    /// A number that changes every time one of `id`'s properties, attributes
    /// or tags does, so
    /// a mirror of the tree (like the renderer) can skip instances that
    /// haven't changed since it last looked.
    pub fn revision(&self, id: InstanceId) -> Result<u64> {
        Ok(self.data(id)?.revision)
    }

    /// Every attribute on `id`, sorted by name.
    pub fn attributes(&self, id: InstanceId) -> Result<&std::collections::BTreeMap<String, PropertyValue>> {
        Ok(&self.data(id)?.attributes)
    }

    pub fn get_attribute(&self, id: InstanceId, name: &str) -> Result<Option<PropertyValue>> {
        Ok(self.data(id)?.attributes.get(name).cloned())
    }

    /// Sets an attribute, or removes it with `None`. Links and enum values
    /// can't be attributes.
    pub fn set_attribute(&mut self, id: InstanceId, name: &str, value: Option<PropertyValue>) -> Result<()> {
        if let Some(v @ (PropertyValue::Ref(_) | PropertyValue::Enum(_))) = &value {
            return Err(TreeError::BadAttribute(v.kind_name()));
        }
        let attributes = &mut self.data_mut(id)?.attributes;
        let changed = match value {
            Some(v) => attributes.insert(name.to_owned(), v.clone()).as_ref() != Some(&v),
            None => attributes.remove(name).is_some(),
        };
        if changed {
            self.data_mut(id)?.revision = next_revision();
            self.changes.push(Change::AttributeChanged { id, name: name.to_owned() });
        }
        Ok(())
    }

    /// Every kept property libroxel has no setting for (see
    /// [`set_extra`](Self::set_extra)), sorted by name.
    pub fn extras(&self, id: InstanceId) -> Result<&std::collections::BTreeMap<String, PropertyValue>> {
        Ok(&self.data(id)?.extras)
    }

    pub fn get_extra(&self, id: InstanceId, name: &str) -> Result<Option<PropertyValue>> {
        Ok(self.data(id)?.extras.get(name).cloned())
    }

    /// Keeps a property libroxel has no setting for, such as a dialog's
    /// prompt, by its script name; `None` forgets it. Enumeration items and
    /// brick colours are kept as their numbers.
    pub fn set_extra(&mut self, id: InstanceId, name: &str, value: Option<PropertyValue>) -> Result<()> {
        let data = self.data_mut(id)?;
        let changed = match value {
            Some(v) => data.extras.insert(name.to_owned(), v.clone()).as_ref() != Some(&v),
            None => data.extras.remove(name).is_some(),
        };
        if changed {
            data.revision = next_revision();
        }
        Ok(())
    }

    /// A terrain's cells.
    pub fn voxels(&self, id: InstanceId) -> Result<&crate::voxels::Voxels> {
        Ok(&self.data(id)?.voxels)
    }

    /// Changes a terrain's cells. Anything but a terrain is left alone.
    pub fn edit_voxels<R>(
        &mut self,
        id: InstanceId,
        edit: impl FnOnce(&mut crate::voxels::Voxels) -> R,
    ) -> Result<Option<R>> {
        let data = self.data_mut(id)?;
        if data.class != ClassKind::Terrain {
            return Ok(None);
        }
        let before = data.voxels.version();
        let out = edit(&mut data.voxels);
        if data.voxels.version() != before {
            data.revision = next_revision();
            self.changes.push(Change::VoxelsChanged(id));
        }
        Ok(Some(out))
    }

    /// The workspace's terrain, if it has one.
    pub fn terrain(&self) -> Option<InstanceId> {
        self.find_first_child_of_class(self.workspace, ClassKind::Terrain).ok().flatten()
    }

    /// The workspace's terrain, made if there isn't one yet.
    pub fn terrain_or_make(&mut self) -> Result<InstanceId> {
        if let Some(t) = self.terrain() {
            return Ok(t);
        }
        let t = self.create(ClassKind::Terrain, "Terrain")?;
        self.set_parent(t, Some(self.workspace))?;
        Ok(t)
    }

    /// The tags on `id`, sorted.
    pub fn tags(&self, id: InstanceId) -> Result<&[String]> {
        Ok(&self.data(id)?.tags)
    }

    pub fn has_tag(&self, id: InstanceId, tag: &str) -> Result<bool> {
        Ok(self.data(id)?.tags.binary_search_by(|t| t.as_str().cmp(tag)).is_ok())
    }

    pub fn add_tag(&mut self, id: InstanceId, tag: &str) -> Result<()> {
        let tags = &mut self.data_mut(id)?.tags;
        if let Err(at) = tags.binary_search_by(|t| t.as_str().cmp(tag)) {
            tags.insert(at, tag.to_owned());
            self.tag_index.entry(tag.to_owned()).or_default().insert(id);
            self.data_mut(id)?.revision = next_revision();
            self.changes.push(Change::TagAdded { id, tag: tag.to_owned() });
        }
        Ok(())
    }

    pub fn remove_tag(&mut self, id: InstanceId, tag: &str) -> Result<()> {
        let tags = &mut self.data_mut(id)?.tags;
        if let Ok(at) = tags.binary_search_by(|t| t.as_str().cmp(tag)) {
            tags.remove(at);
            if let Some(set) = self.tag_index.get_mut(tag) {
                set.remove(&id);
            }
            self.data_mut(id)?.revision = next_revision();
            self.changes.push(Change::TagRemoved { id, tag: tag.to_owned() });
        }
        Ok(())
    }

    /// Every live instance in the game (under the root) with `tag`, in tree order.
    pub fn tagged(&self, tag: &str) -> Vec<InstanceId> {
        let Some(set) = self.tag_index.get(tag) else { return Vec::new() };
        let mut found: Vec<(Vec<usize>, InstanceId)> = Vec::new();
        let mut places: std::collections::HashMap<InstanceId, usize> = Default::default();
        for &id in set {
            if !self.has_tag(id, tag).unwrap_or(false) || self.data(id).is_ok_and(|d| d.destroyed) {
                continue;
            }
            // Where it is in the tree: its place among its siblings, all the way up.
            let mut path = Vec::new();
            let mut at = id;
            let mut under_root = false;
            while let Ok(Some(parent)) = self.parent(at) {
                let place = match places.get(&at) {
                    Some(&p) => p,
                    None => {
                        let siblings = self.children(parent).unwrap_or_default();
                        for (i, s) in siblings.iter().enumerate() {
                            places.insert(*s, i);
                        }
                        places.get(&at).copied().unwrap_or(usize::MAX)
                    }
                };
                path.push(place);
                if parent == self.root {
                    under_root = true;
                    break;
                }
                at = parent;
            }
            if under_root {
                path.reverse();
                found.push((path, id));
            }
        }
        found.sort();
        found.into_iter().map(|(_, id)| id).collect()
    }

    /// Typed read of a `Vec3` property.
    pub fn get_vec3(&self, id: InstanceId, property: Property) -> Option<Vec3> {
        match self.get(id, property).ok()? {
            PropertyValue::Vec3(v) => Some(v),
            _ => None,
        }
    }

    /// Typed read of a 2D vector property.
    pub fn get_vec2(&self, id: InstanceId, property: Property) -> Option<crate::Vec2> {
        match self.get(id, property).ok()? {
            PropertyValue::Vec2(v) => Some(v),
            _ => None,
        }
    }

    /// Typed read of a number property.
    pub fn get_number(&self, id: InstanceId, property: Property) -> Option<f64> {
        match self.get(id, property).ok()? {
            PropertyValue::Number(v) => Some(v),
            _ => None,
        }
    }

    /// Typed read of a bool property.
    pub fn get_bool(&self, id: InstanceId, property: Property) -> Option<bool> {
        match self.get(id, property).ok()? {
            PropertyValue::Bool(v) => Some(v),
            _ => None,
        }
    }

    /// Typed read of a colour property.
    pub fn get_color(&self, id: InstanceId, property: Property) -> Option<Color3> {
        match self.get(id, property).ok()? {
            PropertyValue::Color3(v) => Some(v),
            _ => None,
        }
    }

    /// Typed read of an enum property.
    pub fn get_enum(&self, id: InstanceId, property: Property) -> Option<EnumValue> {
        match self.get(id, property).ok()? {
            PropertyValue::Enum(v) => Some(v),
            _ => None,
        }
    }

    /// Typed read of a link property.
    pub fn get_ref(&self, id: InstanceId, property: Property) -> Option<InstanceId> {
        match self.get(id, property).ok()? {
            PropertyValue::Ref(v) => v,
            _ => None,
        }
    }

    /// Typed read of a string property.
    pub fn get_string(&self, id: InstanceId, property: Property) -> Option<String> {
        match self.get(id, property).ok()? {
            PropertyValue::String(v) => Some(v),
            _ => None,
        }
    }

    /// Deep-copies `id` and its descendants. The copy is unparented. Links
    /// between instances inside the copy point at their copies, so a cloned
    /// model's welds hold the cloned parts.
    pub fn clone_subtree(&mut self, id: InstanceId) -> Result<InstanceId> {
        let mut copies = std::collections::HashMap::new();
        let new_id = self.clone_inner(id, &mut copies)?;
        for &copy in copies.values() {
            let data = self.data_mut(copy)?;
            for (_, value) in data.props.iter_mut() {
                if let PropertyValue::Ref(Some(target)) = value
                    && let Some(mapped) = copies.get(target)
                {
                    *target = *mapped;
                }
            }
        }
        Ok(new_id)
    }

    fn clone_inner(
        &mut self,
        id: InstanceId,
        copies: &mut std::collections::HashMap<InstanceId, InstanceId>,
    ) -> Result<InstanceId> {
        let source = self.data(id)?;
        if !source.class.is_creatable() {
            return Err(TreeError::NotCreatable(source.class));
        }
        let mut copy = source.clone();
        let children = std::mem::take(&mut copy.children);
        copy.parent = None;
        copy.parent_locked = false;
        copy.destroyed = false;
        let new_id = self.alloc(copy);
        copies.insert(id, new_id);
        self.changes.push(Change::Created(new_id));
        for child in children {
            if self.data(child)?.destroyed {
                continue;
            }
            let child_copy = self.clone_inner(child, copies)?;
            self.set_parent(child_copy, Some(new_id))?;
        }
        Ok(new_id)
    }

    /// All live instances that are, or inherit from, `class` under `ancestor`.
    /// Everything under the workspace, in tree order (cached while nothing
    /// is reparented or destroyed).
    pub fn workspace_descendants(&self) -> Vec<InstanceId> {
        let mut cache = self.class_cache.borrow_mut();
        self.refresh_class_cache(&mut cache);
        cache.all.clone()
    }

    fn refresh_class_cache(&self, cache: &mut ClassCache) {
        if cache.structure != Some(self.structure) {
            cache.all = self.descendants(self.workspace).unwrap_or_default();
            cache.classes =
                cache.all.iter().map(|id| self.data(*id).map_or(u16::MAX, |d| d.class as u16)).collect();
            cache.by_class.clear();
            cache.structure = Some(self.structure);
        }
    }

    pub fn descendants_of_class(&self, ancestor: InstanceId, class: ClassKind) -> Vec<InstanceId> {
        if ancestor == self.workspace {
            let mut cache = self.class_cache.borrow_mut();
            self.refresh_class_cache(&mut cache);
            let ClassCache { all, classes, by_class, .. } = &mut *cache;
            return by_class
                .entry(class)
                .or_insert_with(|| {
                    // Which classes count, worked out once, then one pass.
                    let fits: Vec<bool> = ClassKind::ALL.iter().map(|c| c.is_a(class)).collect();
                    let by_number: std::collections::HashMap<u16, bool> =
                        ClassKind::ALL.iter().zip(&fits).map(|(c, f)| (*c as u16, *f)).collect();
                    let size = by_number.keys().map(|n| usize::from(*n) + 1).max().unwrap_or(0);
                    let mut table = vec![false; size];
                    for (n, f) in by_number {
                        table[usize::from(n)] = f;
                    }
                    let fits = |c: &u16| table.get(usize::from(*c)).copied().unwrap_or(false);
                    all.iter().zip(classes.iter()).filter(|(_, c)| fits(c)).map(|(id, _)| *id).collect()
                })
                .clone();
        }
        self.descendants(ancestor)
            .unwrap_or_default()
            .into_iter()
            .filter(|id| self.class(*id).is_ok_and(|c| c.is_a(class)))
            .collect()
    }

    fn note_moved(&mut self, id: InstanceId) {
        // A long log is cheaper to start over (indexes rebuild) than to keep.
        if self.moved.len() >= 1 << 18 {
            self.moved.clear();
            self.moved_epoch += 1;
        }
        self.moved.push(id);
    }

    /// The parts of the workspace that might be along a ray (see
    /// [`PartIndex::along_ray`](crate::geometry::PartIndex::along_ray)).
    pub fn parts_along_ray(&self, origin: Vec3, dir: Vec3, reach: f32) -> Vec<InstanceId> {
        let mut index = self.part_index.borrow_mut();
        index.refresh(self);
        index.along_ray(origin, dir, reach)
    }

    /// The parts of the workspace that might be in the box from `lo` to `hi`.
    pub fn parts_near_box(&self, lo: Vec3, hi: Vec3) -> Vec<InstanceId> {
        let mut index = self.part_index.borrow_mut();
        index.refresh(self);
        index.near_box(lo, hi)
    }

    /// The log of instances that may have moved: its epoch (which changes
    /// when the log starts over, so everything must be looked at again) and
    /// the entries from `from` on.
    pub fn moved_since(&self, from: usize) -> (u64, &[InstanceId]) {
        (self.moved_epoch, self.moved.get(from..).unwrap_or(&[]))
    }

    /// How long the moved log is now (where to read from next time).
    pub fn moved_len(&self) -> usize {
        self.moved.len()
    }

    /// Convenience for renaming.
    pub fn set_name(&mut self, id: InstanceId, name: impl Into<String>) -> Result<()> {
        self.set(id, Property::Name, PropertyValue::String(name.into()))
    }

    /// Takes every change recorded since the last call.
    pub fn drain_changes(&mut self) -> Vec<Change> {
        std::mem::take(&mut self.changes)
    }

    /// Number of live slots (destroyed but unpurged instances included).
    pub fn len(&self) -> usize {
        self.slots.iter().filter(|s| s.data.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

fn replace<T: PartialEq>(slot: &mut T, value: T) -> bool {
    if *slot == value {
        false
    } else {
        *slot = value;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(tree: &mut InstanceTree, name: &str) -> InstanceId {
        tree.create(ClassKind::Part, name).unwrap()
    }

    #[test]
    fn new_tree_has_locked_root_and_workspace() {
        let mut tree = InstanceTree::new();
        let (root, ws) = (tree.root(), tree.workspace());
        assert_eq!(tree.class(root).unwrap(), ClassKind::DataModel);
        assert_eq!(tree.parent(ws).unwrap(), Some(root));
        assert_eq!(tree.children(root).unwrap()[0], ws);
        let players = tree.service(ClassKind::Players).unwrap();
        assert_eq!(tree.class(players).unwrap(), ClassKind::Players);
        assert_eq!(tree.destroy(players), Err(TreeError::ParentLocked(players)));
        assert_eq!(tree.set_parent(ws, None), Err(TreeError::ParentLocked(ws)));
        assert_eq!(tree.destroy(root), Err(TreeError::ParentLocked(root)));
        assert!(tree.drain_changes().is_empty());
    }

    #[test]
    fn singletons_are_not_creatable() {
        let mut tree = InstanceTree::new();
        assert_eq!(
            tree.create(ClassKind::Workspace, "w"),
            Err(TreeError::NotCreatable(ClassKind::Workspace))
        );
    }

    #[test]
    fn parenting_and_lookup() {
        let mut tree = InstanceTree::new();
        let ws = tree.workspace();
        let folder = tree.create(ClassKind::Folder, "Stuff").unwrap();
        let a = part(&mut tree, "A");
        let b = part(&mut tree, "B");
        tree.set_parent(folder, Some(ws)).unwrap();
        tree.set_parent(a, Some(folder)).unwrap();
        tree.set_parent(b, Some(folder)).unwrap();

        assert_eq!(tree.children(folder).unwrap(), &[a, b]);
        assert_eq!(tree.find_first_child(folder, "B").unwrap(), Some(b));
        assert_eq!(tree.find_first_child(folder, "C").unwrap(), None);
        assert_eq!(tree.find_first_child_of_class(ws, ClassKind::Folder).unwrap(), Some(folder));
        assert_eq!(tree.descendants(ws).unwrap(), vec![folder, a, b]);
        assert!(tree.is_descendant_of(a, ws).unwrap());
        assert!(!tree.is_descendant_of(ws, a).unwrap());

        // Moving keeps child lists consistent.
        tree.set_parent(a, Some(ws)).unwrap();
        assert_eq!(tree.children(folder).unwrap(), &[b]);
        assert_eq!(tree.children(ws).unwrap(), &[folder, a]);
    }

    #[test]
    fn cycles_are_rejected() {
        let mut tree = InstanceTree::new();
        let outer = tree.create(ClassKind::Folder, "Outer").unwrap();
        let inner = tree.create(ClassKind::Folder, "Inner").unwrap();
        tree.set_parent(inner, Some(outer)).unwrap();
        assert_eq!(
            tree.set_parent(outer, Some(inner)),
            Err(TreeError::WouldCycle { child: outer, parent: inner })
        );
        assert_eq!(
            tree.set_parent(outer, Some(outer)),
            Err(TreeError::WouldCycle { child: outer, parent: outer })
        );
    }

    #[test]
    fn destroy_detaches_locks_and_keeps_data_readable() {
        let mut tree = InstanceTree::new();
        let ws = tree.workspace();
        let model = tree.create(ClassKind::Model, "Car").unwrap();
        let wheel = part(&mut tree, "Wheel");
        tree.set_parent(model, Some(ws)).unwrap();
        tree.set_parent(wheel, Some(model)).unwrap();
        tree.drain_changes();

        tree.destroy(model).unwrap();
        assert!(tree.children(ws).unwrap().is_empty());
        assert_eq!(tree.parent(wheel).unwrap(), None);
        assert!(tree.is_destroyed(wheel).unwrap());
        assert_eq!(tree.name(wheel).unwrap(), "Wheel");
        assert_eq!(tree.set_parent(wheel, Some(ws)), Err(TreeError::ParentLocked(wheel)));

        let changes = tree.drain_changes();
        assert!(changes.contains(&Change::Destroyed(model)));
        assert!(changes.contains(&Change::Destroyed(wheel)));
        let wheel_pos = changes.iter().position(|c| *c == Change::Destroyed(wheel)).unwrap();
        let model_pos = changes.iter().position(|c| *c == Change::Destroyed(model)).unwrap();
        assert!(wheel_pos < model_pos, "children are destroyed before parents");

        // Destroying twice is harmless.
        tree.destroy(model).unwrap();
    }

    #[test]
    fn purged_ids_never_resolve_again() {
        let mut tree = InstanceTree::new();
        let old = part(&mut tree, "Old");
        tree.destroy(old).unwrap();
        assert_eq!(tree.purge_destroyed(), 1);
        assert_eq!(tree.name(old), Err(TreeError::Dead(old)));

        let new = part(&mut tree, "New");
        assert_ne!(old, new, "the reused slot gets a new generation");
        assert_eq!(tree.name(old), Err(TreeError::Dead(old)));
        assert_eq!(tree.name(new).unwrap(), "New");
    }

    #[test]
    fn properties_are_typed_and_class_checked() {
        let mut tree = InstanceTree::new();
        let p = part(&mut tree, "P");
        let folder = tree.create(ClassKind::Folder, "F").unwrap();
        tree.drain_changes();

        tree.set(p, Property::Position, PropertyValue::Vec3(Vec3::new(1.0, 2.0, 3.0))).unwrap();
        assert_eq!(tree.get(p, Property::Position).unwrap(), PropertyValue::Vec3(Vec3::new(1.0, 2.0, 3.0)));
        assert_eq!(
            tree.set(p, Property::Position, PropertyValue::Bool(true)),
            Err(TreeError::WrongType { property: Property::Position, expected: "Vec3", got: "bool" })
        );
        assert_eq!(
            tree.get(folder, Property::Position),
            Err(TreeError::NoSuchProperty { class: ClassKind::Folder, property: Property::Position })
        );

        // Setting the same value again records nothing.
        tree.set(p, Property::Position, PropertyValue::Vec3(Vec3::new(1.0, 2.0, 3.0))).unwrap();
        assert_eq!(
            tree.drain_changes(),
            vec![Change::PropertyChanged { id: p, property: Property::Position }]
        );
    }

    #[test]
    fn clone_subtree_copies_everything_but_the_parent() {
        let mut tree = InstanceTree::new();
        let ws = tree.workspace();
        let model = tree.create(ClassKind::Model, "Car").unwrap();
        let wheel = part(&mut tree, "Wheel");
        tree.set_parent(model, Some(ws)).unwrap();
        tree.set_parent(wheel, Some(model)).unwrap();
        tree.set(wheel, Property::Size, PropertyValue::Vec3(Vec3::ONE)).unwrap();

        let copy = tree.clone_subtree(model).unwrap();
        assert_ne!(copy, model);
        assert_eq!(tree.parent(copy).unwrap(), None);
        let wheel_copy = tree.find_first_child(copy, "Wheel").unwrap().unwrap();
        assert_ne!(wheel_copy, wheel);
        assert_eq!(tree.get_vec3(wheel_copy, Property::Size), Some(Vec3::ONE));
        assert_eq!(tree.children(model).unwrap(), &[wheel], "original untouched");
        assert_eq!(tree.clone_subtree(ws), Err(TreeError::NotCreatable(ClassKind::Workspace)));

        // Links inside the copy point at the copied instances; links out stay.
        let weld = tree.create(ClassKind::WeldConstraint, "Weld").unwrap();
        tree.set_parent(weld, Some(model)).unwrap();
        tree.set(weld, Property::Part0, PropertyValue::Ref(Some(wheel))).unwrap();
        tree.set(weld, Property::Part1, PropertyValue::Ref(Some(ws))).unwrap();
        let copy = tree.clone_subtree(model).unwrap();
        let weld_copy = tree.find_first_child(copy, "Weld").unwrap().unwrap();
        let wheel_copy = tree.find_first_child(copy, "Wheel").unwrap().unwrap();
        assert_eq!(tree.get_ref(weld_copy, Property::Part0), Some(wheel_copy));
        assert_eq!(tree.get_ref(weld_copy, Property::Part1), Some(ws));
    }

    #[test]
    fn defaults_follow_the_class() {
        let mut tree = InstanceTree::new();
        let spawn = tree.create(ClassKind::SpawnLocation, "Spawn").unwrap();
        let hum = tree.create(ClassKind::Humanoid, "Humanoid").unwrap();
        assert_eq!(tree.get_bool(spawn, Property::Anchored), Some(true));
        assert_eq!(tree.get_number(hum, Property::WalkSpeed), Some(16.0));
        assert!(ClassKind::SpawnLocation.is_physical());
    }

    #[test]
    fn int_values_round_and_enums_are_kind_checked() {
        use crate::{Material, PartShape};
        let mut tree = InstanceTree::new();
        let coins = tree.create(ClassKind::IntValue, "Coins").unwrap();
        tree.set(coins, Property::NumberValue, PropertyValue::Number(2.6)).unwrap();
        assert_eq!(tree.get_number(coins, Property::NumberValue), Some(3.0));
        let p = part(&mut tree, "P");
        tree.set(p, Property::Shape, PropertyValue::Enum(EnumValue::Shape(PartShape::Ball))).unwrap();
        assert!(matches!(
            tree.set(p, Property::Shape, PropertyValue::Enum(EnumValue::Material(Material::Neon))),
            Err(TreeError::WrongType { .. })
        ));
    }

    #[test]
    fn class_hierarchy() {
        assert!(ClassKind::Workspace.is_a(ClassKind::Model));
        assert!(ClassKind::Part.is_a(ClassKind::Part));
        assert!(!ClassKind::Part.is_a(ClassKind::Model));
    }
}
