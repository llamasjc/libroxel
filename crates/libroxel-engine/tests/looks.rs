//! How characters look: body colours and hats.

use libroxel_engine::character::{Hat, Look};
use libroxel_engine::session::start_on;
use libroxel_engine::{CharacterInput, LogLevel, PlaySession};
use libroxel_instance::{ClassKind, InstanceTree, Property};

fn step(session: &mut PlaySession, frames: usize) {
    for _ in 0..frames {
        session.step(1.0 / 60.0, CharacterInput::default());
    }
    let errors: Vec<_> = session.take_log().into_iter().filter(|l| l.level == LogLevel::Error).collect();
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn scripts_recolour_a_character_through_its_body_colours() {
    let mut session = start_on(InstanceTree::new(), true).unwrap();
    step(&mut session, 1);
    session
        .runtime()
        .run(
            "paint",
            r#"
            local colors = workspace.Player1:FindFirstChildOfClass("BodyColors")
            colors.TorsoColor3 = Color3.fromRGB(10, 20, 250)
            colors.HeadColor3 = Color3.fromRGB(250, 250, 0)
        "#,
        )
        .unwrap();
    step(&mut session, 2);
    let tree = session.runtime().tree().borrow();
    let model = tree.find_first_child(tree.workspace(), "Player1").unwrap().unwrap();
    let torso = tree.find_first_child(model, "Torso").unwrap().unwrap();
    let head = tree.find_first_child(model, "Head").unwrap().unwrap();
    assert!(tree.get_color(torso, Property::Color).unwrap().b > 0.95);
    assert!(tree.get_color(head, Property::Color).unwrap().g > 0.95);
}

#[test]
fn hats_sit_on_the_head_and_follow_it() {
    let mut session = start_on(InstanceTree::new(), true).unwrap();
    step(&mut session, 1);
    let look = Look { hat: Hat::TallHat, ..Look::for_player(0) };
    session.set_local_look(look);
    let head_and_hat = |session: &PlaySession| {
        let tree = session.runtime().tree().borrow();
        let model = tree.find_first_child(tree.workspace(), "Player1").unwrap().unwrap();
        let head = tree.find_first_child(model, "Head").unwrap().unwrap();
        let hat = tree.find_first_child(model, "Tall hat").unwrap().unwrap();
        assert_eq!(tree.class(hat).unwrap(), ClassKind::Accessory);
        let tower = tree.find_first_child(hat, "Tower").unwrap().unwrap();
        (tree.get_vec3(head, Property::Position).unwrap(), tree.get_vec3(tower, Property::Position).unwrap())
    };
    let (head, tower) = head_and_hat(&session);
    assert!((tower.y - head.y - 1.5).abs() < 0.01, "{head:?} {tower:?}");
    session
        .runtime()
        .run("move", "workspace.Player1.HumanoidRootPart.Position += Vector3.new(10, 0, 0)")
        .unwrap();
    step(&mut session, 2);
    let (head, tower) = head_and_hat(&session);
    assert!((tower.x - head.x).abs() < 0.01 && (tower.y - head.y - 1.5).abs() < 0.1, "{head:?} {tower:?}");
    // Respawning keeps the hat.
    session.runtime().run("die", "workspace.Player1.Humanoid.Health = 0").unwrap();
    step(&mut session, 60 * 7);
    head_and_hat(&session);
}

#[test]
fn looks_survive_being_saved_and_read_back() {
    let look = Look { hat: Hat::Halo, ..Look::for_player(3) };
    assert_eq!(Look::from_json(&look.to_json(), Look::for_player(0)), look);
    // Rubbish falls back.
    let rubbish = libroxel_instance::serde_json::json!({ "hat": "Space helmet", "skin": [1, 2] });
    assert_eq!(Look::from_json(&rubbish, look).colors, look.colors);
    assert_eq!(Look::from_json(&rubbish, look).hat, Hat::None);
}
