//! Opens place and model files saved by Roblox Studio (binary `.rbxl` and
//! `.rbxm`, XML `.rbxlx` and `.rbxmx`) as Libroxel instance trees.
//!
//! The files are read with the open-source rbx-dom crates. Each instance
//! becomes the Libroxel class of the same name where there is one, a
//! close stand-in where there isn't (a mesh part becomes a part, a wedge
//! part a wedge-shaped part), and a folder otherwise, so nothing under it
//! is lost. Properties are matched by name and converted; whatever can't
//! be is counted in the [`Report`].

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use libroxel_instance::{
    ClassKind, Color3, EnumValue, InstanceId, InstanceTree, PartShape, Property, PropertyValue, Rotation,
    UDim, UDim2, Vec2, Vec3,
};
use libroxel_script::names;
use rbx_dom_weak::types::{Ref, Variant};
use rbx_dom_weak::{Instance, WeakDom};

pub mod meshes;
pub mod terrain;

/// Why a file couldn't be opened.
#[derive(Debug)]
pub enum ImportError {
    Read(std::io::Error),
    /// Neither a binary nor an XML file of this kind.
    NotARobloxFile,
    Binary(String),
    Xml(String),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::Read(e) => write!(f, "could not read the file: {e}"),
            ImportError::NotARobloxFile => write!(f, "this isn't a .rbxl, .rbxlx, .rbxm or .rbxmx file"),
            ImportError::Binary(e) => write!(f, "could not read the binary file: {e}"),
            ImportError::Xml(e) => write!(f, "could not read the XML file: {e}"),
        }
    }
}

impl std::error::Error for ImportError {}

/// What an import brought across, and what it had to leave behind.
#[derive(Debug, Default, Clone)]
pub struct Report {
    /// Instances made in the tree.
    pub instances: usize,
    /// Properties set.
    pub properties: usize,
    /// Scripts brought across (scripts, local scripts and module scripts).
    pub scripts: usize,
    /// Classes made as a close stand-in, by the file's class name.
    pub stand_ins: BTreeMap<String, usize>,
    /// Classes Libroxel doesn't have, made as folders, by class name.
    pub unknown_classes: BTreeMap<String, usize>,
    /// Properties that weren't brought across, as `Class.Property`.
    pub skipped_properties: BTreeMap<String, usize>,
    /// Services Libroxel doesn't have, left out with everything in them.
    pub skipped_services: Vec<String>,
    /// One-stud terrain cells filled from the file's terrain.
    pub terrain_cells: usize,
    /// Model files made from shapes stored inside the file (unions), by
    /// file name, for the caller to save where models are looked for.
    pub files: Vec<(String, Vec<u8>)>,
    /// Anything else worth telling the creator.
    pub notes: Vec<String>,
}

impl Report {
    /// A few lines saying what came across and what didn't.
    pub fn summary(&self) -> Vec<String> {
        let mut lines = vec![format!(
            "Brought across {} instances ({} scripts) and {} property values.",
            self.instances, self.scripts, self.properties
        )];
        let list = |map: &BTreeMap<String, usize>, n: usize| {
            let mut v: Vec<(&String, &usize)> = map.iter().collect();
            v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            v.iter().take(n).map(|(k, c)| format!("{k} ×{c}")).collect::<Vec<_>>().join(", ")
        };
        if self.terrain_cells > 0 {
            lines.push(format!("Terrain: {} one-stud cells.", self.terrain_cells));
        }
        lines.extend(self.notes.iter().cloned());
        if !self.stand_ins.is_empty() {
            lines.push(format!("Made as the closest Libroxel class: {}.", list(&self.stand_ins, 12)));
        }
        if !self.unknown_classes.is_empty() {
            lines.push(format!("Not in Libroxel yet, kept as folders: {}.", list(&self.unknown_classes, 12)));
        }
        if !self.skipped_services.is_empty() {
            lines.push(format!("Services left out: {}.", self.skipped_services.join(", ")));
        }
        if !self.skipped_properties.is_empty() {
            lines.push(format!(
                "{} kinds of property weren't brought across, most often {}.",
                self.skipped_properties.len(),
                list(&self.skipped_properties, 8)
            ));
        }
        lines
    }

    fn skip(&mut self, class: &str, property: &str) {
        *self.skipped_properties.entry(format!("{class}.{property}")).or_default() += 1;
    }
}

/// Reads a Roblox file of either kind from its bytes.
pub fn read_dom(bytes: &[u8]) -> Result<WeakDom, ImportError> {
    let start = bytes.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(0);
    let body = &bytes[start..];
    let body = body.strip_prefix(b"\xEF\xBB\xBF".as_slice()).unwrap_or(body);
    if body.starts_with(b"<roblox!") {
        rbx_binary::from_reader(body).map_err(|e| ImportError::Binary(e.to_string()))
    } else if body.starts_with(b"<roblox") || body.starts_with(b"<?xml") {
        rbx_xml::from_reader_default(body).map_err(|e| ImportError::Xml(e.to_string()))
    } else {
        Err(ImportError::NotARobloxFile)
    }
}

/// Whether a file name looks like one this crate opens.
pub fn is_roblox_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "rbxl" | "rbxlx" | "rbxm" | "rbxmx"))
}

/// Opens a place (or a model, which lands in the workspace) as a new tree.
pub fn open(path: &Path) -> Result<(InstanceTree, Report), ImportError> {
    let bytes = std::fs::read(path).map_err(ImportError::Read)?;
    let dom = read_dom(&bytes)?;
    let mut tree = InstanceTree::new();
    let report = import_place(&mut tree, &dom);
    tree.drain_changes();
    Ok((tree, report))
}

/// Puts a place's services' contents into `tree`'s services (or, for a
/// model file, its instances into the workspace).
pub fn import_place(tree: &mut InstanceTree, dom: &WeakDom) -> Report {
    let mut importer = Importer::new(dom);
    let root = dom.root();
    let is_place = root.children().iter().filter_map(|r| dom.get_by_ref(*r)).any(|i| i.class == "Workspace");
    if is_place {
        for &child in root.children() {
            let Some(inst) = dom.get_by_ref(child) else { continue };
            let class = names::service_alias(inst.class.as_str());
            match names::class_from_name(class).and_then(|c| tree.service(c)) {
                Some(service) => {
                    importer.properties(tree, inst, service);
                    importer.children(tree, inst, service);
                }
                // Something that isn't a service, left at the top: it goes in the workspace.
                None if !importer.is_service(inst.class.as_str()) => {
                    let ws = tree.workspace();
                    importer.instance(tree, inst, ws);
                }
                // Studio's own bookkeeping.
                None if inst.class == "Instance" => {}
                // A service Libroxel doesn't have: only worth a mention if it held anything.
                None if !inst.children().is_empty() => {
                    importer.report.skipped_services.push(inst.class.to_string())
                }
                None => {}
            }
        }
    } else {
        let ws = tree.workspace();
        importer.children(tree, root, ws);
    }
    importer.finish(tree);
    importer.report
}

/// Puts a model file's instances under `parent`, returning them and the report.
pub fn import_model(tree: &mut InstanceTree, dom: &WeakDom, parent: InstanceId) -> (Vec<InstanceId>, Report) {
    let mut importer = Importer::new(dom);
    let made = importer.children(tree, dom.root(), parent);
    importer.finish(tree);
    (made, importer.report)
}

struct Importer<'a> {
    dom: &'a WeakDom,
    db: &'static rbx_reflection::ReflectionDatabase<'static>,
    report: Report,
    /// The file's instances to the ones made for them.
    made: HashMap<Ref, InstanceId>,
    /// Links to set once everything is made.
    links: Vec<(InstanceId, Property, Ref)>,
}

impl<'a> Importer<'a> {
    fn new(dom: &'a WeakDom) -> Self {
        Self {
            dom,
            db: rbx_reflection_database::get_bundled(),
            report: Report::default(),
            made: HashMap::new(),
            links: Vec::new(),
        }
    }

    fn children(&mut self, tree: &mut InstanceTree, from: &Instance, into: InstanceId) -> Vec<InstanceId> {
        let mut made = Vec::new();
        for &child in from.children() {
            let Some(inst) = self.dom.get_by_ref(child) else { continue };
            if let Some(id) = self.instance(tree, inst, into) {
                made.push(id);
            }
        }
        made
    }

    /// Makes `inst` (and everything under it) under `parent`.
    fn instance(
        &mut self,
        tree: &mut InstanceTree,
        inst: &Instance,
        parent: InstanceId,
    ) -> Option<InstanceId> {
        let file_class = inst.class.as_str();
        let (class, shape) = self.class_for(inst);
        // The starter player's folders already exist: fill them.
        let id = match tree
            .starter_scripts(class)
            .filter(|_| tree.class(parent) == Ok(ClassKind::StarterPlayer))
        {
            Some(existing) => existing,
            None if class == ClassKind::Terrain => {
                let terrain = tree.terrain_or_make().ok()?;
                self.terrain(tree, inst, terrain);
                self.made.insert(inst.referent(), terrain);
                self.children(tree, inst, terrain);
                return Some(terrain);
            }
            None => {
                let id = tree.create(class, inst.name.as_str()).ok()?;
                if tree.set_parent(id, Some(parent)).is_err() {
                    let _ = tree.destroy(id);
                    return None;
                }
                self.report.instances += 1;
                id
            }
        };
        if matches!(class, ClassKind::Script | ClassKind::LocalScript | ClassKind::ModuleScript) {
            self.report.scripts += 1;
        }
        self.made.insert(inst.referent(), id);
        if let Some(shape) = shape
            && tree.set(id, Property::Shape, PropertyValue::Enum(EnumValue::Shape(shape))).is_ok()
        {
            self.report.properties += 1;
        }
        if class == ClassKind::Folder && !matches!(file_class, "Folder") {
            // An unknown class: its properties have nowhere to go.
        } else {
            self.properties(tree, inst, id);
        }
        if matches!(file_class, "SpecialMesh" | "BlockMesh" | "CylinderMesh") {
            self.special_mesh(tree, inst, parent);
        }
        self.shape_from_file(tree, inst, id);
        self.children(tree, inst, id);
        Some(id)
    }

    /// Gives a model to a part whose shape Libroxel has no setting for:
    /// a union's shape stored in the file, or a corner wedge.
    fn shape_from_file(&mut self, tree: &mut InstanceTree, inst: &Instance, id: InstanceId) {
        let made = match inst.class.as_str() {
            "UnionOperation" | "NegateOperation" | "PartOperation" => {
                let data = |name: &str| inst.properties.get(&name.into()).and_then(bytes_of);
                let shape = data("MeshData")
                    .and_then(meshes::union_graphics)
                    .or_else(|| data("PhysicsData").and_then(meshes::union_physics));
                match shape {
                    Some(shape) => {
                        let text = shape.to_obj();
                        Some((meshes::file_name("union", &text), text))
                    }
                    None => {
                        self.report.skip(inst.class.as_str(), "MeshData");
                        None
                    }
                }
            }
            "CornerWedgePart" => {
                let _ =
                    tree.set(id, Property::Shape, PropertyValue::Enum(EnumValue::Shape(PartShape::Block)));
                Some(("cornerwedge.obj".to_owned(), meshes::corner_wedge()))
            }
            _ => None,
        };
        let Some((name, text)) = made else { return };
        if tree.set(id, Property::MeshId, PropertyValue::String(name.clone())).is_ok() {
            self.report.properties += 1;
        }
        if !self.report.files.iter().any(|(n, _)| *n == name) {
            self.report.files.push((name, text.into_bytes()));
        }
    }

    /// An old-style mesh inside a part: changes the part's shape, or gives
    /// it a model file. (The mesh itself is kept too, for scripts.)
    fn special_mesh(&mut self, tree: &mut InstanceTree, inst: &Instance, part: InstanceId) {
        if tree.class(part) != Ok(ClassKind::Part) {
            return;
        }
        let kind = match inst.class.as_str() {
            "BlockMesh" => "Brick",
            "CylinderMesh" => "Cylinder",
            _ => match inst.properties.get(&"MeshType".into()) {
                Some(Variant::Enum(e)) => self.enum_item_name("MeshType", e.to_u32()).unwrap_or("Head"),
                _ => "Head",
            },
        };
        let shape = |s| PropertyValue::Enum(EnumValue::Shape(s));
        let _ = match kind {
            "Sphere" => tree.set(part, Property::Shape, shape(PartShape::Ball)),
            "Cylinder" | "Head" => tree.set(part, Property::Shape, shape(PartShape::Cylinder)),
            "Wedge" => tree.set(part, Property::Shape, shape(PartShape::Wedge)),
            "FileMesh" => {
                let id =
                    ["MeshId", "MeshContent"].iter().find_map(|k| match inst.properties.get(&(*k).into()) {
                        Some(Variant::ContentId(c)) => Some(c.as_str().to_owned()),
                        Some(Variant::Content(c)) => c.as_uri().map(str::to_owned),
                        Some(Variant::String(s)) => Some(s.clone()),
                        _ => None,
                    });
                match id.filter(|s| !s.is_empty()) {
                    Some(id) => tree.set(part, Property::MeshId, PropertyValue::String(id)),
                    None => Ok(()),
                }
            }
            "CornerWedge" => {
                let text = meshes::corner_wedge();
                if !self.report.files.iter().any(|(n, _)| n == "cornerwedge.obj") {
                    self.report.files.push(("cornerwedge.obj".to_owned(), text.into_bytes()));
                }
                tree.set(part, Property::MeshId, PropertyValue::String("cornerwedge.obj".to_owned()))
            }
            _ => Ok(()),
        };
    }

    /// Fills the terrain from the file's voxels.
    fn terrain(&mut self, tree: &mut InstanceTree, inst: &Instance, terrain: InstanceId) {
        let content_map = inst.properties.iter().any(|(k, v)| {
            k.as_str() == "VoxelGridAssetContentMap"
                && !matches!(v, Variant::BinaryString(b) if AsRef::<[u8]>::as_ref(b).is_empty())
        });
        let Some(Variant::BinaryString(grid)) = inst.properties.get(&"SmoothGrid".into()) else {
            if content_map {
                self.report.notes.push(EXPANDED_TERRAIN.to_owned());
            }
            return;
        };
        match terrain::decode(AsRef::<[u8]>::as_ref(grid)) {
            Ok(voxels) => {
                if voxels.is_empty() && content_map {
                    self.report.notes.push(EXPANDED_TERRAIN.to_owned());
                }
                let (cells, cut) = terrain::cells(&voxels);
                self.report.terrain_cells += cells.len();
                let _ =
                    tree.edit_voxels(terrain, |v| v.set_cells(cells.into_iter().map(|(c, m)| (c, Some(m)))));
                if cut {
                    self.report.notes.push(format!(
                        "The terrain is bigger than Libroxel holds at once; only the first {} cells came across.",
                        terrain::MAX_CELLS
                    ));
                }
                let approximated: std::collections::BTreeSet<&str> = voxels
                    .values()
                    .map(|v| terrain::MATERIALS[v.material as usize])
                    .filter(|m| !matches!(*m, "Air" | "Water" | "Grass" | "Sand" | "Rock" | "Snow" | "Brick"))
                    .collect();
                if !approximated.is_empty() {
                    let list: Vec<&str> = approximated.into_iter().collect();
                    self.report.notes.push(format!(
                        "Terrain materials shown as the nearest Libroxel one: {}.",
                        list.join(", ")
                    ));
                }
            }
            // Newer terrain keeps only a header here; its voxels are elsewhere.
            Err(terrain::GridError::Header(version, _)) if content_map || version > 1 => {
                self.report.notes.push(EXPANDED_TERRAIN.to_owned())
            }
            Err(e) => self.report.notes.push(format!("The terrain couldn't be read: {e}.")),
        }
    }

    /// The Libroxel class for a file's instance, and a part shape it implies.
    fn class_for(&mut self, inst: &Instance) -> (ClassKind, Option<PartShape>) {
        let name = inst.class.as_str();
        // A script that runs on the player's computer is a local script.
        if name == "Script"
            && let Some(Variant::Enum(e)) = inst.properties.get(&"RunContext".into())
            && self.enum_item_name("RunContext", e.to_u32()) == Some("Client")
        {
            return (ClassKind::LocalScript, None);
        }
        if let Some(class) = names::class_from_name(name).filter(|c| c.is_user_creatable() || is_folder(*c)) {
            return (class, None);
        }
        if let Some((class, shape)) = names::stand_in(name) {
            *self.report.stand_ins.entry(name.to_owned()).or_default() += 1;
            return (class, shape);
        }
        *self.report.unknown_classes.entry(name.to_owned()).or_default() += 1;
        (ClassKind::Folder, None)
    }

    fn is_service(&self, class: &str) -> bool {
        self.db.classes.get(class).is_some_and(|c| c.tags.contains(&rbx_reflection::ClassTag::Service))
    }

    /// Whether `value` is what Roblox gives `class.property` anyway, so
    /// leaving it out loses nothing.
    fn is_default(&self, class: &str, property: &str, value: &Variant) -> bool {
        let Some(descriptor) = self.db.classes.get(class) else { return false };
        self.db.find_default_property(descriptor, property).is_some_and(|d| d == value)
    }

    fn enum_item_name(&self, roblox_enum: &str, value: u32) -> Option<&'static str> {
        let e = self.db.enums.get(roblox_enum)?;
        e.items.iter().find(|(_, v)| **v == value).map(|(k, _)| *k)
    }

    /// The Roblox enum a class's property holds, from the reflection database.
    fn enum_of(&self, class: &str, property: &str) -> Option<&'static str> {
        let mut class = self.db.classes.get(class)?;
        loop {
            if let Some(p) = class.properties.get(property) {
                return match p.data_type {
                    rbx_reflection::DataType::Enum(name) => Some(name),
                    _ => None,
                };
            }
            class = self.db.classes.get(class.superclass?)?;
        }
    }

    fn properties(&mut self, tree: &mut InstanceTree, inst: &Instance, id: InstanceId) {
        let Ok(class) = tree.class(id) else { return };
        let file_class = inst.class.as_str();
        let mut entries: Vec<(&str, &Variant)> =
            inst.properties.iter().map(|(k, v)| (k.as_str(), v)).collect();
        entries.sort_by_key(|(k, _)| *k);
        for (name, value) in entries {
            if self.special(tree, class, id, name, value) {
                continue;
            }
            let name = renamed(name);
            let Some(property) = names::property_from_name(class, name) else {
                if !ignored(name) && !self.is_default(file_class, name, value) {
                    self.report.skip(file_class, name);
                }
                continue;
            };
            if property == Property::Name {
                continue;
            }
            if let Variant::Ref(r) = value {
                if r.is_some() {
                    self.links.push((id, property, *r));
                }
                continue;
            }
            let Some(template) = class.default_value(property) else { continue };
            let done = self
                .convert(value, &template, file_class, name)
                .is_some_and(|v| tree.set(id, property, v).is_ok());
            if done {
                self.report.properties += 1;
            } else {
                self.report.skip(file_class, name);
            }
        }
    }

    /// Properties that need more than a straight conversion. Returns
    /// whether `name` was handled.
    fn special(
        &mut self,
        tree: &mut InstanceTree,
        class: ClassKind,
        id: InstanceId,
        name: &str,
        value: &Variant,
    ) -> bool {
        let mut set = |tree: &mut InstanceTree, p: Property, v: PropertyValue| {
            if class.has_property(p) && tree.set(id, p, v).is_ok() {
                self.report.properties += 1;
            }
        };
        match (name, value) {
            // Where a part, attachment or camera is and which way it faces.
            ("CFrame", Variant::CFrame(cf)) if class.has_property(Property::Orientation) => {
                let (pos, turn) = cframe(cf);
                set(tree, Property::Position, PropertyValue::Vec3(pos));
                set(tree, Property::Orientation, PropertyValue::Vec3(turn));
                true
            }
            ("C0" | "C1", Variant::CFrame(cf)) => {
                let (pos, turn) = cframe(cf);
                let (p, o) = if name == "C0" {
                    (Property::C0Position, Property::C0Orientation)
                } else {
                    (Property::C1Position, Property::C1Orientation)
                };
                set(tree, p, PropertyValue::Vec3(pos));
                set(tree, o, PropertyValue::Vec3(turn));
                true
            }
            // Scripts saved by newer Studios say whether they're on, not off.
            ("Enabled", Variant::Bool(on)) if class.has_property(Property::Disabled) => {
                set(tree, Property::Disabled, PropertyValue::Bool(!on));
                true
            }
            ("Attributes", Variant::Attributes(attributes)) => {
                for (key, raw) in attributes.iter() {
                    let v = match raw {
                        Variant::Bool(b) => Some(PropertyValue::Bool(*b)),
                        Variant::Float32(n) => Some(PropertyValue::Number(f64::from(*n))),
                        Variant::Float64(n) => Some(PropertyValue::Number(*n)),
                        Variant::Int32(n) => Some(PropertyValue::Number(f64::from(*n))),
                        Variant::String(s) => Some(PropertyValue::String(s.clone())),
                        Variant::BinaryString(b) => String::from_utf8(AsRef::<[u8]>::as_ref(b).to_vec())
                            .ok()
                            .map(PropertyValue::String),
                        Variant::BrickColor(b) => {
                            let c = b.to_color3uint8();
                            Some(PropertyValue::Color3(Color3::from_rgb8(c.r, c.g, c.b)))
                        }
                        Variant::Vector3(v) => Some(PropertyValue::Vec3(Vec3::new(v.x, v.y, v.z))),
                        Variant::Vector2(v) => Some(PropertyValue::Vec2(Vec2::new(v.x, v.y))),
                        Variant::Color3(c) => Some(PropertyValue::Color3(Color3::new(c.r, c.g, c.b))),
                        Variant::UDim2(u) => Some(PropertyValue::UDim2(udim2(u))),
                        Variant::UDim(u) => Some(PropertyValue::UDim(UDim::new(u.scale, u.offset as f32))),
                        _ => None,
                    };
                    if !v.is_some_and(|v| tree.set_attribute(id, key, Some(v)).is_ok()) {
                        self.report.skip("Attribute", key);
                    }
                }
                true
            }
            ("Tags", Variant::Tags(tags)) => {
                for tag in tags.iter() {
                    let _ = tree.add_tag(id, tag);
                }
                true
            }
            // A shape that came from the class (a wedge part) wins over the file's.
            ("Shape" | "shape", _)
                if class == ClassKind::Part
                    && tree
                        .get(id, Property::Shape)
                        .is_ok_and(|s| s != PropertyValue::Enum(EnumValue::Shape(PartShape::Block))) =>
            {
                true
            }
            _ => false,
        }
    }

    fn convert(
        &self,
        value: &Variant,
        template: &PropertyValue,
        class: &str,
        name: &str,
    ) -> Option<PropertyValue> {
        Some(match template {
            PropertyValue::Bool(_) => PropertyValue::Bool(match value {
                Variant::Bool(b) => *b,
                _ => return None,
            }),
            PropertyValue::Number(_) => PropertyValue::Number(number(value)?),
            PropertyValue::String(_) => PropertyValue::String(match value {
                Variant::String(s) => s.clone(),
                Variant::ContentId(c) => c.as_str().to_owned(),
                Variant::Content(c) => c.as_uri().unwrap_or_default().to_owned(),
                Variant::BinaryString(b) => String::from_utf8(AsRef::<[u8]>::as_ref(b).to_vec()).ok()?,
                Variant::Enum(e) => self.enum_item_name(self.enum_of(class, name)?, e.to_u32())?.to_owned(),
                _ => return None,
            }),
            PropertyValue::Vec3(_) => PropertyValue::Vec3(match value {
                Variant::Vector3(v) => Vec3::new(v.x, v.y, v.z),
                Variant::Vector3int16(v) => Vec3::new(f32::from(v.x), f32::from(v.y), f32::from(v.z)),
                _ => return None,
            }),
            PropertyValue::Color3(_) => PropertyValue::Color3(match value {
                Variant::Color3(c) => Color3::new(c.r, c.g, c.b),
                Variant::Color3uint8(c) => Color3::from_rgb8(c.r, c.g, c.b),
                Variant::ColorSequence(seq) => {
                    let c = seq.keypoints.first()?.color;
                    Color3::new(c.r, c.g, c.b)
                }
                Variant::BrickColor(b) => {
                    let c = b.to_color3uint8();
                    Color3::from_rgb8(c.r, c.g, c.b)
                }
                _ => return None,
            }),
            PropertyValue::Vec2(_) => PropertyValue::Vec2(match value {
                Variant::Vector2(v) => Vec2::new(v.x, v.y),
                Variant::Vector2int16(v) => Vec2::new(f32::from(v.x), f32::from(v.y)),
                _ => return None,
            }),
            PropertyValue::UDim2(_) => PropertyValue::UDim2(match value {
                Variant::UDim2(u) => udim2(u),
                _ => return None,
            }),
            PropertyValue::UDim(_) => PropertyValue::UDim(match value {
                Variant::UDim(u) => UDim::new(u.scale, u.offset as f32),
                _ => return None,
            }),
            PropertyValue::Enum(ours) => {
                let raw = match value {
                    Variant::Enum(e) => e.to_u32(),
                    Variant::EnumItem(e) => e.value,
                    _ => return None,
                };
                let item = self.enum_item_name(self.enum_of(class, name)?, raw)?;
                PropertyValue::Enum(names::enum_item_from_name(ours.kind(), item)?)
            }
            PropertyValue::Ref(_) => return None,
        })
    }

    /// Sets the links (a model's primary part, a weld's parts…) now that
    /// everything they point at exists.
    fn finish(&mut self, tree: &mut InstanceTree) {
        for (id, property, target) in std::mem::take(&mut self.links) {
            match self.made.get(&target) {
                Some(t) if tree.set(id, property, PropertyValue::Ref(Some(*t))).is_ok() => {
                    self.report.properties += 1;
                }
                _ => {
                    let class = tree.class(id).map(names::class_name).unwrap_or("?");
                    self.report.skip(class, names::property_name(property));
                }
            }
        }
    }
}

const EXPANDED_TERRAIN: &str = "This place uses Studio's newer expanded terrain, whose voxel format Libroxel can't read yet, so its terrain was left out.";

fn is_folder(class: ClassKind) -> bool {
    matches!(class, ClassKind::StarterPlayerScripts | ClassKind::StarterCharacterScripts)
}

/// Properties every file has that Libroxel has no use for, left out of the report.
fn ignored(name: &str) -> bool {
    matches!(
        name,
        "Archivable"
            | "SourceAssetId"
            | "UniqueId"
            | "HistoryId"
            | "Capabilities"
            | "DefinesCapabilities"
            | "ScriptGuid"
            | "LinkedSource"
            | "RunContext"
            | "PivotOffset"
            | "WorldPivotData"
            | "ModelMeshCFrame"
            | "ModelMeshData"
            | "ModelMeshSize"
            | "NeedsPivotMigration"
            | "LevelOfDetail"
            | "ModelStreamingMode"
            | "BrickColor"
            | "RootPriority"
            | "AttributesSerialize"
            | "Tags"
            | "FormFactor"
            | "formFactorRaw"
            | "Locked"
            // Read separately (a union's shape, the terrain), or how Studio
            // keeps them, which Libroxel works out for itself.
            | "MeshData"
            | "PhysicsData"
            | "ChildData"
            | "InitialSize"
            | "PhysicalConfigData"
            | "RenderFidelity"
            | "CollisionFidelity"
            | "UsePartColor"
            | "SmoothGrid"
            | "PhysicsGrid"
            | "MaterialColors"
    ) || name.ends_with("Surface")
        || name.ends_with("SurfaceInput")
        || name.ends_with("ParamA")
        || name.ends_with("ParamB")
}

/// Newer files' names for properties Libroxel knows by their older names.
fn renamed(name: &str) -> &str {
    match name {
        "TextureContent" => "Texture",
        "MeshContent" => "MeshId",
        "ImageContent" => "Image",
        "SoundContent" => "SoundId",
        "ConstrainedValue" => "Value",
        other => other,
    }
}

/// The bytes of a binary property, however the file stored them.
fn bytes_of(value: &Variant) -> Option<&[u8]> {
    match value {
        Variant::BinaryString(b) => Some(b.as_ref()),
        Variant::SharedString(s) => Some(s.data()),
        _ => None,
    }
}

fn number(value: &Variant) -> Option<f64> {
    Some(match value {
        // A curve over time (a beam's or particles' transparency): where it starts.
        Variant::NumberSequence(seq) => f64::from(seq.keypoints.first()?.value),
        Variant::NumberRange(range) => f64::from(range.min),
        Variant::Float32(n) => f64::from(*n),
        Variant::Float64(n) => *n,
        Variant::Int32(n) => f64::from(*n),
        Variant::Int64(n) => *n as f64,
        _ => return None,
    })
}

fn udim2(u: &rbx_dom_weak::types::UDim2) -> UDim2 {
    UDim2::new(u.x.scale, u.x.offset as f32, u.y.scale, u.y.offset as f32)
}

/// A file's CFrame as a position and an orientation in degrees.
fn cframe(cf: &rbx_dom_weak::types::CFrame) -> (Vec3, Vec3) {
    let p = cf.position;
    // The file stores the matrix by rows; a rotation here is by columns.
    let m = cf.orientation;
    let turn = Rotation {
        x: Vec3::new(m.x.x, m.y.x, m.z.x),
        y: Vec3::new(m.x.y, m.y.y, m.z.y),
        z: Vec3::new(m.x.z, m.y.z, m.z.z),
    };
    (Vec3::new(p.x, p.y, p.z), turn.to_orientation())
}
