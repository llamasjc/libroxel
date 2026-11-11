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

fn value_to_json(value: &PropertyValue) -> Value {
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
    }
}

fn triple(value: &Value) -> Option<[f32; 3]> {
    let arr = value.as_array()?;
    if arr.len() != 3 {
        return None;
    }
    Some([arr[0].as_f64()? as f32, arr[1].as_f64()? as f32, arr[2].as_f64()? as f32])
}

fn value_from_json(property: Property, value: &Value) -> Option<PropertyValue> {
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
        _ => return None,
    })
}

fn node_to_json(tree: &InstanceTree, id: InstanceId) -> Result<Value, TreeError> {
    let class = tree.class(id)?;
    let mut props = Map::new();
    for &property in class.properties() {
        if property == Property::Name {
            continue;
        }
        let value = tree.get(id, property)?;
        // Links point at runtime objects (like a player's character) and aren't saved.
        if matches!(value, PropertyValue::Ref(_)) {
            continue;
        }
        // Only store what differs from the default, to keep files small and diffable.
        if class.default_value(property).as_ref() != Some(&value) {
            props.insert(property.file_name().into(), value_to_json(&value));
        }
    }
    let children =
        tree.children(id)?.iter().map(|c| node_to_json(tree, *c)).collect::<Result<Vec<_>, _>>()?;

    let mut node = Map::new();
    node.insert("class".into(), json!(class.file_name()));
    node.insert("name".into(), json!(tree.name(id)?));
    if !props.is_empty() {
        node.insert("properties".into(), Value::Object(props));
    }
    if !children.is_empty() {
        node.insert("children".into(), Value::Array(children));
    }
    Ok(Value::Object(node))
}

/// Serialises everything under the root as pretty-printed JSON.
pub fn save(tree: &InstanceTree) -> Result<String, TreeError> {
    let children =
        tree.children(tree.root())?.iter().map(|c| node_to_json(tree, *c)).collect::<Result<Vec<_>, _>>()?;
    let doc = json!({ "format": FORMAT, "version": VERSION, "children": children });
    Ok(serde_json::to_string_pretty(&doc).expect("JSON values always serialise"))
}

/// Things in a file that were skipped rather than failing the load.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LoadReport {
    pub warnings: Vec<String>,
}

fn load_node(
    tree: &mut InstanceTree,
    node: &Value,
    parent: InstanceId,
    report: &mut LoadReport,
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
    } else if !class.is_user_creatable() {
        report.warnings.push(format!("skipped {name}: a {class_name} can't be saved in a place"));
        return Ok(());
    } else {
        let id = tree.create(class, name)?;
        tree.set_parent(id, Some(parent))?;
        id
    };

    if let Some(props) = obj.get("properties").and_then(Value::as_object) {
        for (key, raw) in props {
            let Some(property) = Property::from_file_name(key).filter(|p| class.has_property(*p)) else {
                report.warnings.push(format!("{name}: ignored unknown property `{key}`"));
                continue;
            };
            match value_from_json(property, raw) {
                Some(value) => tree.set(id, property, value)?,
                None => report.warnings.push(format!("{name}: ignored bad value for `{key}`")),
            }
        }
    }
    if let Some(children) = obj.get("children").and_then(Value::as_array) {
        for child in children {
            load_node(tree, child, id, report)?;
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
    if let Some(children) = doc.get("children").and_then(Value::as_array) {
        for child in children {
            load_node(&mut tree, child, root, &mut report)?;
        }
    }
    tree.drain_changes();
    Ok((tree, report))
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let text = save(&tree).unwrap();
        let (loaded, report) = load(&text).unwrap();
        assert!(report.warnings.is_empty());
        assert_eq!(save(&loaded).unwrap(), text, "save(load(x)) == x");

        let ws = loaded.workspace();
        let level = loaded.find_first_child(ws, "Level").unwrap().unwrap();
        let floor = loaded.find_first_child(level, "Floor").unwrap().unwrap();
        assert_eq!(loaded.get_vec3(floor, Property::Size), Some(Vec3::new(32.0, 1.0, 32.0)));
        assert_eq!(loaded.get_bool(floor, Property::Anchored), Some(true));
        let ball = loaded.find_first_child(ws, "Ball").unwrap().unwrap();
        assert_eq!(loaded.get(ball, Property::Shape).unwrap(), shape);
        let storage = loaded.service(ClassKind::ReplicatedStorage).unwrap();
        assert!(loaded.find_first_child(storage, "Util").unwrap().is_some());
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
