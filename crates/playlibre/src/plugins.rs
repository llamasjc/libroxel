use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::rc::Rc;

use playlibre_instance::serde_json::{self, Value, json};
use playlibre_instance::{Change, ClassKind, InstanceId, InstanceTree, Property};
use playlibre_script::ScriptRuntime;

use crate::server::data_dir;

#[derive(Debug, Clone, PartialEq)]
pub struct Plugin {
    pub asset_id: String,
    pub hub: String,
    pub name: String,
    pub creator: String,
    pub version: u32,
    pub permissions: Vec<String>,
    pub source: String,
}

impl Plugin {
    fn to_json(&self) -> Value {
        json!({
            "asset_id": self.asset_id, "hub": self.hub, "name": self.name, "creator": self.creator,
            "version": self.version, "permissions": self.permissions, "source": self.source,
        })
    }

    fn from_json(value: &Value) -> Option<Plugin> {
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
        Some(Plugin {
            asset_id: text("asset_id")?,
            hub: text("hub").unwrap_or_default(),
            name: text("name")?,
            creator: text("creator").unwrap_or_default(),
            version: value.get("version").and_then(Value::as_u64)? as u32,
            permissions: value
                .get("permissions")
                .and_then(Value::as_array)?
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
            source: text("source")?,
        })
    }
}

pub fn permission_words(permission: &str) -> &'static str {
    match permission {
        "add" => "Add new things",
        "change" => "Change, move or delete what's there",
        "scripts" => "Write or change scripts",
        _ => "Something unknown",
    }
}

fn path() -> PathBuf {
    data_dir().join("plugins.json")
}

pub fn load() -> Vec<Plugin> {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|text| serde_json::from_str::<Vec<Value>>(&text).ok())
        .map(|list| list.iter().filter_map(Plugin::from_json).collect())
        .unwrap_or_default()
}

pub fn save(plugins: &[Plugin]) -> Result<(), String> {
    let text = serde_json::to_string_pretty(&plugins.iter().map(Plugin::to_json).collect::<Vec<_>>())
        .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(data_dir()).and_then(|_| std::fs::write(path(), text)).map_err(|e| e.to_string())
}

fn is_script(class: ClassKind) -> bool {
    matches!(class, ClassKind::Script | ClassKind::LocalScript | ClassKind::ModuleScript)
}

fn needed(before: &InstanceTree, after: &InstanceTree, changes: &[Change]) -> BTreeSet<&'static str> {
    let mut need = BTreeSet::new();
    let old = |id: &InstanceId| before.exists(*id);
    let script = |id: &InstanceId| before.class(*id).or_else(|_| after.class(*id)).is_ok_and(is_script);
    for change in changes {
        match change {
            Change::Created(id) => {
                if after.exists(*id) {
                    need.insert("add");
                    if script(id) {
                        need.insert("scripts");
                    }
                }
            }
            Change::PropertyChanged { id, property } => {
                if old(id) {
                    need.insert("change");
                }
                if script(id) && (old(id) || *property == Property::Source) {
                    need.insert("scripts");
                }
            }
            Change::Reparented { id, .. } | Change::Destroyed(id) => {
                if old(id) {
                    need.insert("change");
                    if script(id) {
                        need.insert("scripts");
                    }
                }
            }
            Change::AttributeChanged { id, .. }
            | Change::TagAdded { id, .. }
            | Change::TagRemoved { id, .. }
            | Change::VoxelsChanged(id) => {
                if old(id) {
                    need.insert("change");
                }
            }
        }
    }
    need
}

pub fn run(
    tree: &InstanceTree,
    selection: &[InstanceId],
    plugin: &Plugin,
) -> Result<(InstanceTree, Vec<InstanceId>, Vec<String>), String> {
    let mut copy = tree.clone();
    copy.drain_changes();
    let shared = Rc::new(RefCell::new(copy));
    let runtime = ScriptRuntime::new(shared.clone()).map_err(|e| e.to_string())?;
    shared.borrow_mut().drain_changes();
    let ran = runtime.run_plugin(&plugin.name, &plugin.source, selection);
    let printed: Vec<String> = runtime.take_output().into_iter().map(|line| line.text).collect();
    drop(runtime);
    let picked = ran.map_err(|e| e.to_string())?;
    let mut after = shared.borrow().clone();
    let changes = after.drain_changes();
    let missing: Vec<&str> = needed(tree, &after, &changes)
        .into_iter()
        .filter(|p| !plugin.permissions.iter().any(|q| q == p))
        .collect();
    if !missing.is_empty() {
        let words: Vec<String> = missing.iter().map(|p| permission_words(p).to_lowercase()).collect();
        return Err(format!(
            "{} tried to {}, which it isn't allowed to. Nothing was changed.",
            plugin.name,
            words.join(" and ")
        ));
    }
    let picked = picked.into_iter().filter(|id| after.exists(*id)).collect();
    Ok((after, picked, printed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(source: &str, permissions: &[&str]) -> Plugin {
        Plugin {
            asset_id: "x".into(),
            hub: String::new(),
            name: "Test".into(),
            creator: "Someone".into(),
            version: 1,
            permissions: permissions.iter().map(|p| (*p).to_owned()).collect(),
            source: source.into(),
        }
    }

    fn place() -> (InstanceTree, InstanceId) {
        let mut tree = InstanceTree::new();
        let part = tree.create(ClassKind::Part, "Base").unwrap();
        let ws = tree.workspace();
        tree.set_parent(part, Some(ws)).unwrap();
        let script = tree.create(ClassKind::Script, "Main").unwrap();
        tree.set_parent(script, Some(ws)).unwrap();
        tree.drain_changes();
        (tree, part)
    }

    #[test]
    fn looking_needs_nothing() {
        let (tree, part) = place();
        let (after, picked, printed) =
            run(&tree, &[part], &plugin("print(#plugin:GetSelection(), workspace.Base.Name)", &[])).unwrap();
        assert_eq!(picked, [part]);
        assert_eq!(printed, ["1 Base"]);
        assert_eq!(
            playlibre_instance::place::save(&after).unwrap(),
            playlibre_instance::place::save(&tree).unwrap()
        );
    }

    #[test]
    fn each_change_needs_its_permission() {
        let (tree, part) = place();
        let add = "local p = Instance.new('Part') p.Parent = workspace plugin:SetSelection({p})";
        assert!(run(&tree, &[], &plugin(add, &[])).is_err());
        let (after, picked, _) = run(&tree, &[], &plugin(add, &["add"])).unwrap();
        assert_eq!(after.workspace_descendants().len(), tree.workspace_descendants().len() + 1);
        assert_eq!(picked.len(), 1);

        let change = "workspace.Base.Name = 'Floor'";
        assert!(run(&tree, &[part], &plugin(change, &["add"])).is_err());
        assert!(run(&tree, &[part], &plugin(change, &["change"])).is_ok());
        assert!(run(&tree, &[], &plugin("workspace.Base:Destroy()", &["add"])).is_err());

        let code = "workspace.Main.Source = 'print(1)'";
        assert!(run(&tree, &[], &plugin(code, &["change"])).is_err());
        assert!(run(&tree, &[], &plugin(code, &["change", "scripts"])).is_ok());
        let new_script = "local s = Instance.new('Script') s.Source = 'print(1)' s.Parent = workspace";
        assert!(run(&tree, &[], &plugin(new_script, &["add"])).is_err());
        assert!(run(&tree, &[], &plugin(new_script, &["add", "scripts"])).is_ok());
    }

    #[test]
    fn a_plugin_that_never_ends_is_stopped() {
        let (tree, _) = place();
        assert!(run(&tree, &[], &plugin("while true do end", &["add", "change", "scripts"])).is_err());
    }
}
