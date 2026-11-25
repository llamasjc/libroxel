//! Looking children up by name in big folders (which uses a table) gives
//! the same answers as looking through them in order.

use playlibre_instance::{ClassKind, InstanceTree};

fn folder_with(
    tree: &mut InstanceTree,
    count: usize,
) -> (playlibre_instance::InstanceId, Vec<playlibre_instance::InstanceId>) {
    let folder = tree.create(ClassKind::Folder, "Big").unwrap();
    let ws = tree.workspace();
    tree.set_parent(folder, Some(ws)).unwrap();
    let children = (0..count)
        .map(|i| {
            let c = tree.create(ClassKind::Folder, format!("Child{i}")).unwrap();
            tree.set_parent(c, Some(folder)).unwrap();
            c
        })
        .collect();
    (folder, children)
}

#[test]
fn big_folders_find_children_by_name() {
    let mut tree = InstanceTree::new();
    let (folder, kids) = folder_with(&mut tree, 100);
    assert_eq!(tree.find_first_child(folder, "Child42").unwrap(), Some(kids[42]));
    assert_eq!(tree.find_first_child(folder, "Nope").unwrap(), None);

    // A rename is seen straight away, both ways.
    tree.set_name(kids[42], "Renamed").unwrap();
    assert_eq!(tree.find_first_child(folder, "Child42").unwrap(), None);
    assert_eq!(tree.find_first_child(folder, "Renamed").unwrap(), Some(kids[42]));

    // The first of several with one name wins, in child order.
    tree.set_name(kids[90], "Child10").unwrap();
    assert_eq!(tree.find_first_child(folder, "Child10").unwrap(), Some(kids[10]));
    tree.set_parent(kids[10], None).unwrap();
    assert_eq!(tree.find_first_child(folder, "Child10").unwrap(), Some(kids[90]));

    // Moving in, moving out and destroying.
    let newcomer = tree.create(ClassKind::Part, "Newcomer").unwrap();
    tree.set_parent(newcomer, Some(folder)).unwrap();
    assert_eq!(tree.find_first_child(folder, "Newcomer").unwrap(), Some(newcomer));
    tree.destroy(newcomer).unwrap();
    assert_eq!(tree.find_first_child(folder, "Newcomer").unwrap(), None);
    tree.destroy(folder).unwrap();
    assert_eq!(tree.find_first_child(folder, "Child1").unwrap(), None);
}

#[test]
fn a_copied_folder_has_its_own_names() {
    let mut tree = InstanceTree::new();
    let (folder, _) = folder_with(&mut tree, 40);
    assert!(tree.find_first_child(folder, "Child5").unwrap().is_some());
    let copy = tree.clone_subtree(folder).unwrap();
    let found = tree.find_first_child(copy, "Child5").unwrap().unwrap();
    assert_eq!(tree.parent(found).unwrap(), Some(copy));
}
