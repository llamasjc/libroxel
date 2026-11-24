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

mod csg;
pub mod meshes;
pub mod terrain;
mod unions;

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
    let mut importer = Importer::new(&dom);
    importer.assets = path.parent().map(|p| p.join("assets"));
    let report = importer.place(&mut tree);
    tree.drain_changes();
    Ok((tree, report))
}

/// Puts a place's services' contents into `tree`'s services (or, for a
/// model file, its instances into the workspace).
pub fn import_place(tree: &mut InstanceTree, dom: &WeakDom) -> Report {
    Importer::new(dom).place(tree)
}

impl Importer<'_> {
    fn place(mut self, tree: &mut InstanceTree) -> Report {
        let importer = &mut self;
        let dom = importer.dom;
        let root = dom.root();
        let is_place =
            root.children().iter().filter_map(|r| dom.get_by_ref(*r)).any(|i| i.class == "Workspace");
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
        self.report
    }
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
    /// Where files for asset numbers are kept, beside the opened file.
    assets: Option<std::path::PathBuf>,
}

impl<'a> Importer<'a> {
    fn new(dom: &'a WeakDom) -> Self {
        Self {
            dom,
            db: rbx_reflection_database::get_bundled(),
            report: Report::default(),
            made: HashMap::new(),
            links: Vec::new(),
            assets: None,
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
        if class == ClassKind::Other {
            let _ = tree.set(id, Property::OtherClass, PropertyValue::String(file_class.to_owned()));
        }
        if names::stand_in(file_class).is_some() {
            let _ = tree.set_extra(id, names::STAND_IN, Some(PropertyValue::String(file_class.to_owned())));
        }
        self.properties(tree, inst, id);
        if matches!(file_class, "SpecialMesh" | "BlockMesh" | "CylinderMesh") {
            self.special_mesh(tree, inst, parent);
        }
        self.shape_from_file(tree, inst, id);
        self.children(tree, inst, id);
        Some(id)
    }

    /// Gives a model to a part whose shape Libroxel has no setting for:
    /// a union or mesh part shape stored in the file, or a corner wedge.
    fn shape_from_file(&mut self, tree: &mut InstanceTree, inst: &Instance, id: InstanceId) {
        let made = match inst.class.as_str() {
            "UnionOperation" | "NegateOperation" | "PartOperation" => {
                let mut mesh = unions::graphics(inst);
                let mut physics = unions::physics_bytes(inst).map(<[u8]>::to_vec);
                let online =
                    match inst.properties.get(&"AssetId".into()).or(inst.properties.get(&"Content".into())) {
                        Some(Variant::ContentId(c)) => Some(c.as_str().to_owned()),
                        Some(Variant::Content(c)) => c.as_uri().map(str::to_owned),
                        _ => None,
                    }
                    .filter(|u| !u.trim().is_empty());
                if mesh.is_none()
                    && let Some(id) = online.as_deref().and_then(asset_number)
                    && let Some((m, p)) = self.union_asset(&id)
                {
                    mesh = meshes::union_graphics(&m);
                    physics = p.or(physics);
                }
                let shape = mesh
                    .or_else(|| unions::from_parts(inst))
                    .or_else(|| physics.as_deref().and_then(meshes::union_physics));
                match shape {
                    Some(shape) => {
                        let text = shape.to_obj();
                        let name = meshes::file_name("union", &text);
                        if let Some(collision) = physics.as_deref().and_then(meshes::union_collision) {
                            let file = format!("{}.collision.obj", name.trim_end_matches(".obj"));
                            if !self.report.files.iter().any(|(n, _)| *n == file) {
                                self.report.files.push((file, collision.into_bytes()));
                            }
                        }
                        Some((name, text))
                    }
                    None => {
                        match online {
                            Some(url) => {
                                if tree.set(id, Property::MeshId, PropertyValue::String(url)).is_ok() {
                                    self.report.properties += 1;
                                }
                            }
                            None => self.report.skip(inst.class.as_str(), "MeshData"),
                        }
                        None
                    }
                }
            }
            "MeshPart" => {
                let missing = tree
                    .get_string(id, Property::MeshId)
                    .and_then(|m| asset_number(&m))
                    .is_some_and(|n| !self.has_asset(&n, &["", ".mesh", ".obj"]));
                let physics = unions::physics_bytes(inst).filter(|_| missing);
                match physics.and_then(meshes::union_physics) {
                    Some(shape) => {
                        let text = shape.to_obj();
                        let name = meshes::file_name("mesh", &text);
                        if let Some(collision) = physics.and_then(meshes::union_collision) {
                            let file = format!("{}.collision.obj", name.trim_end_matches(".obj"));
                            if !self.report.files.iter().any(|(n, _)| *n == file) {
                                self.report.files.push((file, collision.into_bytes()));
                            }
                        }
                        Some((name, text))
                    }
                    None => None,
                }
            }
            "TrussPart" => {
                let size = tree.get_vec3(id, Property::Size).unwrap_or(Vec3::new(2.0, 2.0, 2.0));
                let cells = [size.x, size.y, size.z].map(|s| (s / 2.0).round().max(1.0) as u32);
                let (model, solid) = meshes::truss(cells);
                let name = format!("truss-{}x{}x{}.obj", cells[0], cells[1], cells[2]);
                let collision = format!("truss-{}x{}x{}.collision.obj", cells[0], cells[1], cells[2]);
                if !self.report.files.iter().any(|(n, _)| *n == collision) {
                    self.report.files.push((collision, solid.into_bytes()));
                }
                Some((name, model))
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

    fn has_asset(&self, id: &str, endings: &[&str]) -> bool {
        self.assets.as_ref().is_some_and(|dir| endings.iter().any(|e| dir.join(format!("{id}{e}")).is_file()))
    }

    /// A union's shape saved from its asset number, as a file in `assets`
    /// beside the opened file: its `MeshData` and `PhysicsData`.
    fn union_asset(&self, id: &str) -> Option<(Vec<u8>, Option<Vec<u8>>)> {
        let dir = self.assets.as_ref()?;
        let bytes =
            ["", ".rbxm", ".rbxmx"].iter().find_map(|e| std::fs::read(dir.join(format!("{id}{e}"))).ok())?;
        if bytes.starts_with(b"CSG") {
            return Some((bytes, None));
        }
        let dom = read_dom(&bytes).ok()?;
        dom.descendants().find_map(|i| {
            let get = |n: &str| {
                i.properties.get(&n.into()).and_then(bytes_of).filter(|b| !b.is_empty()).map(<[u8]>::to_vec)
            };
            Some((get("MeshData")?, get("PhysicsData")))
        })
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
                let texture = ["TextureId", "TextureContent"].iter().find_map(|k| {
                    match inst.properties.get(&(*k).into()) {
                        Some(Variant::ContentId(c)) => Some(c.as_str().to_owned()),
                        Some(Variant::Content(c)) => c.as_uri().map(str::to_owned),
                        _ => None,
                    }
                });
                if let Some(texture) = texture.filter(|s| !s.is_empty()) {
                    let _ = tree.set(part, Property::MeshTexture, PropertyValue::String(texture));
                }
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
        if self.db.classes.contains_key(name) {
            // Kept as itself: scripts see its class and properties.
            return (ClassKind::Other, None);
        }
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

    /// Whether scripts can use `class.property`, by the reflection database.
    fn scriptable(&self, class: &str, property: &str) -> bool {
        let mut current = self.db.classes.get(class);
        while let Some(c) = current {
            if let Some(p) = c.properties.get(property) {
                return !matches!(p.scriptability, rbx_reflection::Scriptability::None);
            }
            current = c.superclass.and_then(|s| self.db.classes.get(s));
        }
        false
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
            // A mesh part's picture, newer files' name for it.
            let name = if class == ClassKind::Part && name == "TextureContent" {
                "TextureID"
            } else {
                renamed(name)
            };
            let Some(property) = names::property_from_name(class, name) else {
                if !ignored(name) && !self.is_default(file_class, name, value) {
                    // Kept by name, so scripts read the file's value.
                    let kept = self
                        .scriptable(file_class, name)
                        .then(|| extra_value(value))
                        .flatten()
                        .is_some_and(|v| tree.set_extra(id, name, Some(v)).is_ok());
                    if kept {
                        self.report.properties += 1;
                    } else {
                        self.report.skip(file_class, name);
                    }
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
        if let ("MaterialColors", Variant::MaterialColors(colours)) = (name, value) {
            use std::str::FromStr;
            for m in TERRAIN_COLOURED {
                let Ok(material) = rbx_dom_weak::types::TerrainMaterials::from_str(m) else { continue };
                let c = colours.get_color(material);
                if c != material.default_color() {
                    let colour = PropertyValue::Color3(Color3::from_rgb8(c.r, c.g, c.b));
                    if tree.set_extra(id, &format!("MaterialColor.{m}"), Some(colour)).is_ok() {
                        self.report.properties += 1;
                    }
                }
            }
            return true;
        }
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
                        other => rich_attribute(other).map(PropertyValue::String),
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
                // A particle emitter's Lifetime and Speed: least and most.
                Variant::NumberRange(r) => Vec2::new(r.min, r.max),
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
/// The number in `rbxassetid://123` or `https://www.roblox.com/asset/?id=123`.
fn asset_number(url: &str) -> Option<String> {
    let lower = url.trim().to_ascii_lowercase();
    let rest = match lower.strip_prefix("rbxassetid://") {
        Some(rest) => rest,
        None => &lower[lower.find("id=")? + 3..],
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    (!digits.is_empty()).then_some(digits)
}

fn bytes_of(value: &Variant) -> Option<&[u8]> {
    match value {
        Variant::BinaryString(b) => Some(b.as_ref()),
        Variant::SharedString(s) => Some(s.data()),
        _ => None,
    }
}

/// Terrain materials whose colour a place can change.
const TERRAIN_COLOURED: [&str; 21] = [
    "Grass",
    "Slate",
    "Concrete",
    "Brick",
    "Sand",
    "WoodPlanks",
    "Rock",
    "Glacier",
    "Snow",
    "Sandstone",
    "Mud",
    "Basalt",
    "Ground",
    "CrackedLava",
    "Asphalt",
    "Cobblestone",
    "Ice",
    "LeafyGrass",
    "Salt",
    "Limestone",
    "Pavement",
];

/// A property Libroxel has no setting for, as a value it can keep:
/// enumeration items and brick colours as their numbers.
fn extra_value(value: &Variant) -> Option<PropertyValue> {
    Some(match value {
        Variant::Bool(b) => PropertyValue::Bool(*b),
        Variant::String(s) => PropertyValue::String(s.clone()),
        Variant::ContentId(c) => PropertyValue::String(c.as_str().to_owned()),
        Variant::Content(c) => PropertyValue::String(c.as_uri().unwrap_or_default().to_owned()),
        Variant::Vector3(v) => PropertyValue::Vec3(Vec3::new(v.x, v.y, v.z)),
        Variant::Vector3int16(v) => {
            PropertyValue::Vec3(Vec3::new(f32::from(v.x), f32::from(v.y), f32::from(v.z)))
        }
        Variant::Color3(c) => PropertyValue::Color3(Color3::new(c.r, c.g, c.b)),
        Variant::Color3uint8(c) => PropertyValue::Color3(Color3::from_rgb8(c.r, c.g, c.b)),
        Variant::Vector2(v) => PropertyValue::Vec2(Vec2::new(v.x, v.y)),
        Variant::UDim2(u) => PropertyValue::UDim2(udim2(u)),
        Variant::UDim(u) => PropertyValue::UDim(UDim::new(u.scale, u.offset as f32)),
        Variant::BrickColor(b) => PropertyValue::Number(f64::from(*b as u16)),
        Variant::Enum(e) => PropertyValue::Number(f64::from(e.to_u32())),
        Variant::Float32(_) | Variant::Float64(_) | Variant::Int32(_) | Variant::Int64(_) => {
            PropertyValue::Number(number(value)?)
        }
        _ => return None,
    })
}

/// An attribute value with no kind of its own in Libroxel, as the text it
/// keeps such values as (see [`libroxel_instance::RICH_ATTRIBUTE`]).
fn rich_attribute(value: &Variant) -> Option<String> {
    let mut out: Vec<String> = Vec::new();
    let mut put = |kind: &str, numbers: &[f32]| {
        out.push(kind.to_owned());
        out.extend(numbers.iter().map(|n| format!("{n}")));
    };
    match value {
        Variant::NumberRange(r) => put("NumberRange", &[r.min, r.max]),
        Variant::NumberSequence(s) => {
            let numbers: Vec<f32> = s.keypoints.iter().flat_map(|k| [k.time, k.value]).collect();
            put("NumberSequence", &numbers);
        }
        Variant::ColorSequence(s) => {
            let numbers: Vec<f32> =
                s.keypoints.iter().flat_map(|k| [k.time, k.color.r, k.color.g, k.color.b]).collect();
            put("ColorSequence", &numbers);
        }
        Variant::Rect(r) => put("Rect", &[r.min.x, r.min.y, r.max.x, r.max.y]),
        Variant::CFrame(cf) => {
            let (p, m) = (cf.position, cf.orientation);
            put("CFrame", &[p.x, p.y, p.z, m.x.x, m.x.y, m.x.z, m.y.x, m.y.y, m.y.z, m.z.x, m.z.y, m.z.z]);
        }
        _ => return None,
    }
    Some(format!("{}{}", libroxel_instance::RICH_ATTRIBUTE, out.join(" ")))
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
