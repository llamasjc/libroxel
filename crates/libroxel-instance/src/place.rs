//! Saving and loading a whole tree ("a place") as JSON.
//!
//! The format uses the engine's stable file names
//! ([`ClassKind::file_name`], [`Property::file_name`]), not script-facing
//! names, so renaming the scripting API never breaks saved games.
//!
//! ```json
//! { "format": "libroxel-place", "version": 1,
//!   "children": [ { "class": "workspace", "name": "Workspace", "children": [
//!       { "class": "part", "name": "Floor", "properties": { "size": [32, 1, 32] } } ] } ] }
//! ```

use serde_json::{Map, Value, json};

use crate::{ClassKind, Color3, InstanceId, InstanceTree, Property, PropertyValue, TreeError, Vec3};

pub const FORMAT: &str = "libroxel-place";
pub const VERSION: u64 = 1;

#[derive(Debug, thiserror::Error)]
pub enum PlaceError {
    #[error("not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("this file is not a Libroxel place")]
    WrongFormat,
    #[error("this place was saved by a newer version of Libroxel (format version {0})")]
    TooNew(u64),
    #[error("unknown class `{0}`")]
    UnknownClass(String),
    #[error("malformed instance: {0}")]
    Malformed(String),
    #[error(transparent)]
    Tree(#[from] TreeError),
}

/// A property value as JSON. Links become `null`: they are saved separately.
pub fn value_to_json(value: &PropertyValue) -> Value {
    match value {
        PropertyValue::Bool(b) => json!(b),
        PropertyValue::Number(n) => json!(n),
        PropertyValue::String(s) => json!(s),
        PropertyValue::Vec3(v) => json!([v.x, v.y, v.z]),
        PropertyValue::Color3(c) => json!([c.r, c.g, c.b]),
        PropertyValue::Enum(v) => json!(v.file_name()),
        PropertyValue::Ref(_) => Value::Null,
        PropertyValue::Vec2(v) => json!([v.x, v.y]),
        PropertyValue::UDim2(u) => json!([u.x_scale, u.x_offset, u.y_scale, u.y_offset]),
        PropertyValue::UDim(u) => json!([u.scale, u.offset]),
    }
}

fn triple(value: &Value) -> Option<[f32; 3]> {
    let arr = value.as_array()?;
    if arr.len() != 3 {
        return None;
    }
    Some([arr[0].as_f64()? as f32, arr[1].as_f64()? as f32, arr[2].as_f64()? as f32])
}

/// Reads a value of `property`'s kind back from [`value_to_json`]'s JSON.
pub fn value_from_json(property: Property, value: &Value) -> Option<PropertyValue> {
    if let Some(kind) = property.enum_kind() {
        return kind.from_file_name(value.as_str()?).map(PropertyValue::Enum);
    }
    Some(match property.expected_kind() {
        "bool" => PropertyValue::Bool(value.as_bool()?),
        "number" => PropertyValue::Number(value.as_f64()?),
        "string" => PropertyValue::String(value.as_str()?.to_owned()),
        "Vec3" => {
            let [x, y, z] = triple(value)?;
            PropertyValue::Vec3(Vec3::new(x, y, z))
        }
        "Color3" => {
            let [r, g, b] = triple(value)?;
            PropertyValue::Color3(Color3::new(r, g, b))
        }
        "Vec2" => {
            let arr = value.as_array().filter(|a| a.len() == 2)?;
            PropertyValue::Vec2(crate::Vec2::new(arr[0].as_f64()? as f32, arr[1].as_f64()? as f32))
        }
        "UDim2" => {
            let arr = value.as_array().filter(|a| a.len() == 4)?;
            let n = |i: usize| arr[i].as_f64().map(|v| v as f32);
            PropertyValue::UDim2(crate::UDim2::new(n(0)?, n(1)?, n(2)?, n(3)?))
        }
        "UDim" => {
            let arr = value.as_array().filter(|a| a.len() == 2)?;
            PropertyValue::UDim(crate::UDim::new(arr[0].as_f64()? as f32, arr[1].as_f64()? as f32))
        }
        _ => return None,
    })
}

/// Attributes are saved with their kind, since the same JSON (a list of
/// numbers) could be several kinds: `{"kind": "Vec3", "value": [1, 2, 3]}`.
/// Booleans, numbers and strings are saved as they are.
pub fn attribute_to_json(value: &PropertyValue) -> Value {
    match value {
        PropertyValue::Bool(_) | PropertyValue::Number(_) | PropertyValue::String(_) => value_to_json(value),
        other => json!({ "kind": other.kind_name(), "value": value_to_json(other) }),
    }
}

pub fn attribute_from_json(value: &Value) -> Option<PropertyValue> {
    Some(match value {
        Value::Bool(b) => PropertyValue::Bool(*b),
        Value::Number(n) => PropertyValue::Number(n.as_f64()?),
        Value::String(s) => PropertyValue::String(s.clone()),
        Value::Object(obj) => {
            let raw = obj.get("value")?;
            let as_kind = |property: Property| value_from_json(property, raw);
            match obj.get("kind")?.as_str()? {
                "Vec3" => as_kind(Property::Position)?,
                "Color3" => as_kind(Property::Color)?,
                "Vec2" => as_kind(Property::AnchorPoint)?,
                "UDim2" => as_kind(Property::GuiSize)?,
                "UDim" => as_kind(Property::Padding)?,
                _ => return None,
            }
        }
        _ => return None,
    })
}

/// Saved instances that something links to, numbered in the order they are saved.
type RefIds = std::collections::HashMap<InstanceId, u64>;

fn ref_targets(tree: &InstanceTree) -> RefIds {
    let all = tree.descendants(tree.root()).unwrap_or_default();
    let saved: std::collections::HashSet<InstanceId> = all.iter().copied().collect();
    let mut wanted = std::collections::HashSet::new();
    for &id in &all {
        let Ok(class) = tree.class(id) else { continue };
        for &property in class.properties() {
            if let Some(target) = tree.get_ref(id, property).filter(|t| saved.contains(t)) {
                wanted.insert(target);
            }
        }
    }
    // Number them in save order (depth first), so saving is repeatable.
    let mut ids = RefIds::new();
    let mut order = Vec::new();
    fn walk(tree: &InstanceTree, id: InstanceId, out: &mut Vec<InstanceId>) {
        for &child in tree.children(id).unwrap_or_default() {
            out.push(child);
            walk(tree, child, out);
        }
    }
    walk(tree, tree.root(), &mut order);
    for id in order.into_iter().filter(|id| wanted.contains(id)) {
        let n = ids.len() as u64 + 1;
        ids.insert(id, n);
    }
    ids
}

fn node_to_json(tree: &InstanceTree, id: InstanceId, refs: &RefIds) -> Result<Value, TreeError> {
    let class = tree.class(id)?;
    let mut props = Map::new();
    for &property in class.properties() {
        if property == Property::Name {
            continue;
        }
        let value = tree.get(id, property)?;
        // Links are saved as the number of what they point at; links to
        // things that aren't saved (like a player's character) are dropped.
        if let PropertyValue::Ref(target) = value {
            if let Some(n) = target.and_then(|t| refs.get(&t)) {
                props.insert(property.file_name().into(), json!({ "ref": n }));
            }
            continue;
        }
        // Only store what differs from the default, to keep files small and diffable.
        if class.default_value(property).as_ref() != Some(&value) {
            props.insert(property.file_name().into(), value_to_json(&value));
        }
    }
    let children =
        tree.children(id)?.iter().map(|c| node_to_json(tree, *c, refs)).collect::<Result<Vec<_>, _>>()?;

    let mut node = Map::new();
    node.insert("class".into(), json!(class.file_name()));
    node.insert("name".into(), json!(tree.name(id)?));
    if let Some(n) = refs.get(&id) {
        node.insert("ref_id".into(), json!(n));
    }
    if !props.is_empty() {
        node.insert("properties".into(), Value::Object(props));
    }
    let attributes = tree.attributes(id)?;
    if !attributes.is_empty() {
        let map: Map<String, Value> =
            attributes.iter().map(|(k, v)| (k.clone(), attribute_to_json(v))).collect();
        node.insert("attributes".into(), Value::Object(map));
    }
    let tags = tree.tags(id)?;
    if !tags.is_empty() {
        node.insert("tags".into(), json!(tags));
    }
    let voxels = tree.voxels(id)?;
    if !voxels.is_empty() {
        node.insert("voxels".into(), json!(voxels.encode()));
    }
    if !children.is_empty() {
        node.insert("children".into(), Value::Array(children));
    }
    Ok(Value::Object(node))
}

/// Serialises everything under the root as pretty-printed JSON.
pub fn save(tree: &InstanceTree) -> Result<String, TreeError> {
    let refs = ref_targets(tree);
    let children = tree
        .children(tree.root())?
        .iter()
        .map(|c| node_to_json(tree, *c, &refs))
        .collect::<Result<Vec<_>, _>>()?;
    let doc = json!({ "format": FORMAT, "version": VERSION, "children": children });
    Ok(serde_json::to_string_pretty(&doc).expect("JSON values always serialise"))
}

/// Things in a file that were skipped rather than failing the load.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LoadReport {
    pub warnings: Vec<String>,
}

/// Links read so far: saved numbers to the instances loaded for them, and
/// links waiting for their target to be loaded.
#[derive(Default)]
struct LoadRefs {
    ids: std::collections::HashMap<u64, InstanceId>,
    pending: Vec<(InstanceId, Property, u64)>,
}

fn load_node(
    tree: &mut InstanceTree,
    node: &Value,
    parent: InstanceId,
    report: &mut LoadReport,
    refs: &mut LoadRefs,
) -> Result<(), PlaceError> {
    let obj = node.as_object().ok_or_else(|| PlaceError::Malformed("expected an object".into()))?;
    let class_name = obj
        .get("class")
        .and_then(Value::as_str)
        .ok_or_else(|| PlaceError::Malformed("missing `class`".into()))?;
    let class = ClassKind::from_file_name(class_name)
        .ok_or_else(|| PlaceError::UnknownClass(class_name.to_owned()))?;
    let name = obj.get("name").and_then(Value::as_str).unwrap_or(class_name);

    // Services already exist; merge into them instead of creating new ones.
    let id = if let Some(service) = tree.service(class).filter(|_| parent == tree.root()) {
        service
    } else if let Some(folder) =
        tree.starter_scripts(class).filter(|_| Some(parent) == tree.service(ClassKind::StarterPlayer))
    {
        folder
    } else if !class.is_user_creatable() {
        report.warnings.push(format!("skipped {name}: a {class_name} can't be saved in a place"));
        return Ok(());
    } else {
        let id = tree.create(class, name)?;
        tree.set_parent(id, Some(parent))?;
        id
    };
    if let Some(n) = obj.get("ref_id").and_then(Value::as_u64) {
        refs.ids.insert(n, id);
    }

    if let Some(props) = obj.get("properties").and_then(Value::as_object) {
        for (key, raw) in props {
            let Some(property) = Property::from_file_name(key).filter(|p| class.has_property(*p)) else {
                report.warnings.push(format!("{name}: ignored unknown property `{key}`"));
                continue;
            };
            if property.expected_kind() == "ref" {
                match raw.get("ref").and_then(Value::as_u64) {
                    Some(n) => refs.pending.push((id, property, n)),
                    None => report.warnings.push(format!("{name}: ignored bad link `{key}`")),
                }
                continue;
            }
            match value_from_json(property, raw) {
                Some(value) => tree.set(id, property, value)?,
                None => report.warnings.push(format!("{name}: ignored bad value for `{key}`")),
            }
        }
    }
    if let Some(attributes) = obj.get("attributes").and_then(Value::as_object) {
        for (key, raw) in attributes {
            match attribute_from_json(raw) {
                Some(value) => tree.set_attribute(id, key, Some(value))?,
                None => report.warnings.push(format!("{name}: ignored bad attribute `{key}`")),
            }
        }
    }
    if let Some(tags) = obj.get("tags").and_then(Value::as_array) {
        for tag in tags.iter().filter_map(Value::as_str) {
            tree.add_tag(id, tag)?;
        }
    }
    if let Some(text) = obj.get("voxels").and_then(Value::as_str) {
        tree.edit_voxels(id, |v| *v = crate::voxels::Voxels::decode(text))?;
    }
    if let Some(children) = obj.get("children").and_then(Value::as_array) {
        for child in children {
            load_node(tree, child, id, report, refs)?;
        }
    }
    Ok(())
}

/// Builds a fresh tree from a saved place.
pub fn load(text: &str) -> Result<(InstanceTree, LoadReport), PlaceError> {
    let doc: Value = serde_json::from_str(text)?;
    if doc.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err(PlaceError::WrongFormat);
    }
    let version = doc.get("version").and_then(Value::as_u64).unwrap_or(0);
    if version > VERSION {
        return Err(PlaceError::TooNew(version));
    }
    let mut tree = InstanceTree::new();
    let mut report = LoadReport::default();
    let root = tree.root();
    let mut refs = LoadRefs::default();
    if let Some(children) = doc.get("children").and_then(Value::as_array) {
        for child in children {
            load_node(&mut tree, child, root, &mut report, &mut refs)?;
        }
    }
    for (id, property, n) in refs.pending {
        match refs.ids.get(&n) {
            Some(&target) => tree.set(id, property, PropertyValue::Ref(Some(target)))?,
            None => report.warnings.push(format!("a link points at something that isn't in the file ({n})")),
        }
    }
    tree.drain_changes();
    Ok((tree, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terrain_cells_are_saved() {
        let mut tree = InstanceTree::new();
        let terrain = tree.terrain_or_make().unwrap();
        let axes = [Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)];
        tree.edit_voxels(terrain, |v| {
            v.fill_block(
                Vec3::new(0.0, -2.0, 0.0),
                axes,
                Vec3::new(40.0, 4.0, 40.0),
                Some(crate::Material::Grass),
            )
        })
        .unwrap();
        let text = save(&tree).unwrap();
        let (back, report) = load(&text).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let t = back.terrain().unwrap();
        assert_eq!(back.voxels(t).unwrap().get(-20, -4, 19), Some(crate::Material::Grass));
        assert_eq!(back.voxels(t).unwrap().encode(), tree.voxels(terrain).unwrap().encode());
    }

    #[test]
    fn round_trip() {
        let mut tree = InstanceTree::new();
        let ws = tree.workspace();
        let folder = tree.create(ClassKind::Folder, "Level").unwrap();
        tree.set_parent(folder, Some(ws)).unwrap();
        let floor = tree.create(ClassKind::Part, "Floor").unwrap();
        tree.set_parent(floor, Some(folder)).unwrap();
        tree.set(floor, Property::Size, PropertyValue::Vec3(Vec3::new(32.0, 1.0, 32.0))).unwrap();
        tree.set(floor, Property::Anchored, PropertyValue::Bool(true)).unwrap();
        let script = tree.create(ClassKind::Script, "Main").unwrap();
        tree.set_parent(script, Some(ws)).unwrap();
        tree.set(script, Property::Source, PropertyValue::String("print('hi')".into())).unwrap();
        let ball = tree.create(ClassKind::Part, "Ball").unwrap();
        tree.set_parent(ball, Some(ws)).unwrap();
        let shape = PropertyValue::Enum(crate::EnumValue::Shape(crate::PartShape::Ball));
        tree.set(ball, Property::Shape, shape.clone()).unwrap();
        tree.set(ball, Property::Orientation, PropertyValue::Vec3(Vec3::new(0.0, 45.0, 0.0))).unwrap();
        let storage = tree.service(ClassKind::ReplicatedStorage).unwrap();
        let module = tree.create(ClassKind::ModuleScript, "Util").unwrap();
        tree.set_parent(module, Some(storage)).unwrap();
        tree.set_attribute(floor, "Points", Some(PropertyValue::Number(5.0))).unwrap();
        tree.set_attribute(floor, "Spawn", Some(PropertyValue::Vec3(Vec3::new(1.0, 2.0, 3.0)))).unwrap();
        tree.set_attribute(floor, "Tint", Some(PropertyValue::Color3(Color3::new(1.0, 0.5, 0.0)))).unwrap();
        tree.add_tag(floor, "Ground").unwrap();
        tree.add_tag(floor, "Big").unwrap();

        let text = save(&tree).unwrap();
        let (loaded, report) = load(&text).unwrap();
        assert!(report.warnings.is_empty());
        assert_eq!(save(&loaded).unwrap(), text, "save(load(x)) == x");

        let ws = loaded.workspace();
        let level = loaded.find_first_child(ws, "Level").unwrap().unwrap();
        let floor = loaded.find_first_child(level, "Floor").unwrap().unwrap();
        assert_eq!(loaded.get_vec3(floor, Property::Size), Some(Vec3::new(32.0, 1.0, 32.0)));
        assert_eq!(loaded.get_bool(floor, Property::Anchored), Some(true));
        assert_eq!(
            loaded.get_attribute(floor, "Spawn").unwrap(),
            Some(PropertyValue::Vec3(Vec3::new(1.0, 2.0, 3.0)))
        );
        assert_eq!(loaded.get_attribute(floor, "Points").unwrap(), Some(PropertyValue::Number(5.0)));
        assert_eq!(loaded.tags(floor).unwrap(), ["Big", "Ground"]);
        assert_eq!(loaded.tagged("Ground"), vec![floor]);
        let ball = loaded.find_first_child(ws, "Ball").unwrap().unwrap();
        assert_eq!(loaded.get(ball, Property::Shape).unwrap(), shape);
        let storage = loaded.service(ClassKind::ReplicatedStorage).unwrap();
        assert!(loaded.find_first_child(storage, "Util").unwrap().is_some());
    }

    #[test]
    fn links_between_saved_instances_survive() {
        let mut tree = InstanceTree::new();
        let ws = tree.workspace();
        let a = tree.create(ClassKind::Part, "A").unwrap();
        let b = tree.create(ClassKind::Part, "B").unwrap();
        let weld = tree.create(ClassKind::WeldConstraint, "Weld").unwrap();
        for id in [a, b] {
            tree.set_parent(id, Some(ws)).unwrap();
        }
        tree.set_parent(weld, Some(a)).unwrap();
        tree.set(weld, Property::Part0, PropertyValue::Ref(Some(a))).unwrap();
        tree.set(weld, Property::Part1, PropertyValue::Ref(Some(b))).unwrap();

        let text = save(&tree).unwrap();
        let (loaded, report) = load(&text).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(save(&loaded).unwrap(), text, "save(load(x)) == x");
        let ws = loaded.workspace();
        let a = loaded.find_first_child(ws, "A").unwrap().unwrap();
        let b = loaded.find_first_child(ws, "B").unwrap().unwrap();
        let weld = loaded.find_first_child(a, "Weld").unwrap().unwrap();
        assert_eq!(loaded.get_ref(weld, Property::Part0), Some(a));
        assert_eq!(loaded.get_ref(weld, Property::Part1), Some(b));
    }

    #[test]
    fn rejects_other_files_and_tolerates_unknown_properties() {
        assert!(matches!(load("{}"), Err(PlaceError::WrongFormat)));
        assert!(matches!(load(r#"{"format":"libroxel-place","version":99}"#), Err(PlaceError::TooNew(99))));
        let text = r#"{"format":"libroxel-place","version":1,"children":[
            {"class":"workspace","name":"Workspace","children":[
                {"class":"part","name":"P","properties":{"sparkles":true,"size":"big"}}]}]}"#;
        let (_, report) = load(text).unwrap();
        assert_eq!(report.warnings.len(), 2);
        let bad = r#"{"format":"libroxel-place","version":1,"children":[{"class":"dragon"}]}"#;
        assert!(matches!(load(bad), Err(PlaceError::UnknownClass(_))));
    }
}
