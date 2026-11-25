//! Two or three computers editing one game, over real connections on this
//! machine.

use libroxel_engine::coedit::CoEdit;
use libroxel_instance::{ClassKind, Color3, InstanceId, InstanceTree, Property, PropertyValue, Vec3};

fn add(tree: &mut InstanceTree, class: ClassKind, parent: InstanceId, name: &str) -> InstanceId {
    let id = tree.create(class, name).unwrap();
    tree.set_parent(id, Some(parent)).unwrap();
    id
}

fn named(tree: &InstanceTree, parent: InstanceId, name: &str) -> Option<InstanceId> {
    tree.find_first_child(parent, name).ok().flatten()
}

struct Desk {
    edit: CoEdit,
    tree: InstanceTree,
}

impl Desk {
    fn step(&mut self) {
        self.edit.step(&mut self.tree);
        // The sharing computer's person lets everyone in.
        for (ticket, _) in self.edit.asking() {
            self.edit.answer(ticket, true);
        }
    }
}

/// The address to join `host` with, password and all.
fn address(host: &Desk) -> String {
    let port = host.edit.local_addr().unwrap().port();
    format!("127.0.0.1:{port}#{}", host.edit.password().unwrap())
}

/// Steps every computer until `done` holds (or gives up).
fn until(desks: &mut [&mut Desk], mut done: impl FnMut(&[&mut Desk]) -> bool) {
    for _ in 0..400 {
        for desk in desks.iter_mut() {
            desk.step();
        }
        if done(desks) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("the computers never agreed");
}

fn shared() -> (Desk, Desk) {
    let mut tree = InstanceTree::new();
    let ws = tree.workspace();
    let part = add(&mut tree, ClassKind::Part, ws, "Floor");
    tree.set(part, Property::Size, PropertyValue::Vec3(Vec3::new(20.0, 1.0, 20.0))).unwrap();
    let sss = tree.service(ClassKind::ServerScriptService).unwrap();
    let script = add(&mut tree, ClassKind::Script, sss, "Main");
    tree.set(script, Property::Source, PropertyValue::String("print('hi')".into())).unwrap();
    let mut host = Desk { edit: CoEdit::host(0).unwrap(), tree };
    let mut guest = Desk { edit: CoEdit::join(&address(&host), "Sam").unwrap(), tree: InstanceTree::new() };
    until(&mut [&mut host, &mut guest], |d| {
        d[1].edit.ready() && named(&d[1].tree, d[1].tree.workspace(), "Floor").is_some()
    });
    (host, guest)
}

fn floor(tree: &InstanceTree) -> InstanceId {
    named(tree, tree.workspace(), "Floor").unwrap()
}

#[test]
fn a_guest_gets_the_whole_game_scripts_and_all() {
    let (host, guest) = shared();
    assert_eq!(host.edit.people(), vec!["Sam".to_owned()]);
    let t = &guest.tree;
    assert_eq!(t.get(floor(t), Property::Size).unwrap(), PropertyValue::Vec3(Vec3::new(20.0, 1.0, 20.0)));
    let sss = t.service(ClassKind::ServerScriptService).unwrap();
    let main = named(t, sss, "Main").expect("server scripts are shared too");
    assert_eq!(t.get(main, Property::Source).unwrap(), PropertyValue::String("print('hi')".into()));
}

#[test]
fn edits_go_both_ways() {
    let (mut host, mut guest) = shared();
    // The guest makes a model with a part in it and recolours the floor.
    let ws = guest.tree.workspace();
    let model = add(&mut guest.tree, ClassKind::Model, ws, "House");
    let wall = add(&mut guest.tree, ClassKind::Part, model, "Wall");
    guest.tree.set(wall, Property::Position, PropertyValue::Vec3(Vec3::new(3.0, 4.0, 5.0))).unwrap();
    let red = PropertyValue::Color3(Color3::new(1.0, 0.0, 0.0));
    let f = floor(&guest.tree);
    guest.tree.set(f, Property::Color, red.clone()).unwrap();
    until(&mut [&mut host, &mut guest], |d| {
        let t = &d[0].tree;
        named(t, t.workspace(), "House").and_then(|m| named(t, m, "Wall")).is_some()
            && t.get(floor(t), Property::Color).unwrap() == red
    });
    let t = &host.tree;
    let wall_there = named(t, named(t, t.workspace(), "House").unwrap(), "Wall").unwrap();
    assert_eq!(t.get(wall_there, Property::Position).unwrap(), PropertyValue::Vec3(Vec3::new(3.0, 4.0, 5.0)));

    // The sharing computer renames the wall; the guest then moves it.
    host.tree.set_name(wall_there, "North wall").unwrap();
    until(&mut [&mut host, &mut guest], |d| d[1].tree.name(wall).unwrap() == "North wall");
    guest.tree.set(wall, Property::Position, PropertyValue::Vec3(Vec3::new(0.0, 9.0, 0.0))).unwrap();
    until(&mut [&mut host, &mut guest], |d| {
        d[0].tree.get(wall_there, Property::Position).unwrap()
            == PropertyValue::Vec3(Vec3::new(0.0, 9.0, 0.0))
    });

    // Removing works both ways, and nothing was made twice.
    host.tree.destroy(f).ok();
    guest.tree.destroy(model).unwrap();
    until(&mut [&mut host, &mut guest], |d| {
        let (h, g) = (&d[0].tree, &d[1].tree);
        h.children(h.workspace()).unwrap().iter().all(|c| h.class(*c).unwrap() == ClassKind::Camera)
            && g.children(g.workspace()).unwrap().iter().all(|c| g.class(*c).unwrap() == ClassKind::Camera)
    });
}

#[test]
fn a_dragged_part_does_not_jump_back_and_everyone_ends_up_agreeing() {
    let (mut host, mut guest) = shared();
    let f = floor(&guest.tree);
    // The guest drags the floor, a little each frame, while the sharing
    // computer's echoes of earlier positions are still on their way.
    for i in 0..60 {
        let at = PropertyValue::Vec3(Vec3::new(i as f32, 0.0, 0.0));
        guest.tree.set(f, Property::Position, at.clone()).unwrap();
        guest.step();
        assert_eq!(guest.tree.get(f, Property::Position).unwrap(), at, "jumped back at frame {i}");
        host.step();
    }
    let end = PropertyValue::Vec3(Vec3::new(59.0, 0.0, 0.0));
    until(&mut [&mut host, &mut guest], |d| {
        d[0].tree.get(floor(&d[0].tree), Property::Position).unwrap() == end
            && d[1].tree.get(f, Property::Position).unwrap() == end
    });
    // Both change the same thing at once: they settle on one answer.
    let hf = floor(&host.tree);
    host.tree.set_name(hf, "Ground").unwrap();
    guest.tree.set_name(f, "Grass").unwrap();
    for _ in 0..50 {
        host.step();
        guest.step();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(host.tree.name(hf).unwrap(), guest.tree.name(f).unwrap());
}

#[test]
fn a_second_guest_sees_what_the_first_one_made() {
    let (mut host, mut sam) = shared();
    let mut ana = Desk { edit: CoEdit::join(&address(&host), "Ana").unwrap(), tree: InstanceTree::new() };
    until(&mut [&mut host, &mut sam, &mut ana], |d| d[2].edit.ready());
    let ws = sam.tree.workspace();
    let tower = add(&mut sam.tree, ClassKind::Part, ws, "Tower");
    sam.tree.set(tower, Property::Size, PropertyValue::Vec3(Vec3::new(2.0, 30.0, 2.0))).unwrap();
    until(&mut [&mut host, &mut sam, &mut ana], |d| {
        named(&d[2].tree, d[2].tree.workspace(), "Tower").is_some()
    });
    let there = named(&ana.tree, ana.tree.workspace(), "Tower").unwrap();
    assert_eq!(ana.tree.get(there, Property::Size).unwrap(), PropertyValue::Vec3(Vec3::new(2.0, 30.0, 2.0)));
    // Ana changes it, Sam sees it.
    ana.tree.set(there, Property::Transparency, PropertyValue::Number(0.5)).unwrap();
    until(&mut [&mut host, &mut sam, &mut ana], |d| {
        d[1].tree.get(tower, Property::Transparency).unwrap() == PropertyValue::Number(0.5)
    });
    let mut names = host.edit.people();
    names.sort();
    assert_eq!(names, vec!["Ana".to_owned(), "Sam".to_owned()]);
}

#[test]
fn opening_another_game_starts_every_guest_over() {
    let (mut host, mut guest) = shared();
    let mut other = InstanceTree::new();
    let ws = other.workspace();
    add(&mut other, ClassKind::Part, ws, "Island");
    host.tree = other;
    until(&mut [&mut host, &mut guest], |d| {
        let t = &d[1].tree;
        named(t, t.workspace(), "Island").is_some() && named(t, t.workspace(), "Floor").is_none()
    });
}

/// Steps the host (without letting anyone in) and `guest` a while.
fn wait_a_while(host: &mut CoEdit, host_tree: &mut InstanceTree, guest: &mut Desk) {
    for _ in 0..60 {
        host.step(host_tree);
        guest.edit.step(&mut guest.tree);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn secret_game() -> InstanceTree {
    let mut tree = InstanceTree::new();
    let sss = tree.service(ClassKind::ServerScriptService).unwrap();
    let script = add(&mut tree, ClassKind::Script, sss, "Secret");
    tree.set(script, Property::Source, PropertyValue::String("print('mine')".into())).unwrap();
    tree
}

#[test]
fn nobody_gets_in_without_the_password() {
    let mut tree = secret_game();
    let mut host = CoEdit::host(0).unwrap();
    let port = host.local_addr().unwrap().port();
    assert!(CoEdit::join(&format!("127.0.0.1:{port}"), "Eve").is_err(), "a password is needed");
    let mut eve = Desk {
        edit: CoEdit::join(&format!("127.0.0.1:{port}#wrong-guess"), "Eve").unwrap(),
        tree: InstanceTree::new(),
    };
    wait_a_while(&mut host, &mut tree, &mut eve);
    assert!(host.asking().is_empty(), "a wrong password isn't even asked about");
    assert!(!eve.edit.ready());
    let sss = eve.tree.service(ClassKind::ServerScriptService).unwrap();
    assert!(named(&eve.tree, sss, "Secret").is_none());
}

#[test]
fn the_sharing_person_decides_who_gets_in() {
    let mut tree = secret_game();
    let mut host = CoEdit::host(0).unwrap();
    let port = host.local_addr().unwrap().port();
    let password = host.password().unwrap().to_owned();
    let mut eve = Desk {
        edit: CoEdit::join(&format!("127.0.0.1:{port}#{}", password.to_uppercase()), "Eve").unwrap(),
        tree: InstanceTree::new(),
    };
    wait_a_while(&mut host, &mut tree, &mut eve);
    let asking = host.asking();
    assert_eq!(asking.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>(), ["Eve"]);
    assert!(eve.edit.waiting() && !eve.edit.ready(), "nothing is sent while the host decides");
    assert!(host.people().is_empty());
    host.answer(asking[0].0, false);
    wait_a_while(&mut host, &mut tree, &mut eve);
    assert!(!eve.edit.ready());
    let sss = eve.tree.service(ClassKind::ServerScriptService).unwrap();
    assert!(named(&eve.tree, sss, "Secret").is_none());
    assert!(host.asking().is_empty() && host.people().is_empty());
}
