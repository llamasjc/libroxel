//! Files saved by older and newer Studios read the same: properties the
//! readers move onto newer names (a sound's `SoundId`, a label's `Font`, a
//! corner's `CornerRadius`), values PlayLibre keeps only for scripts
//! (CFrames, ranges, links), the place's collision groups, and a property
//! saved in a form the class list doesn't expect.

use playlibre_engine::session::PlaySession;
use playlibre_engine::{CharacterInput, LogLevel};
use playlibre_instance::{InstanceTree, Property, PropertyValue, UDim};

/// A place as an older Studio saved it, with the older property names.
const OLD_PLACE: &str = r#"<roblox xmlns:xmime="http://www.w3.org/2005/05/xmlmime" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="http://www.roblox.com/roblox.xsd" version="4">
<Item class="Workspace" referent="RBX0"><Properties>
  <string name="CollisionGroups">Default^0^-3\Ghosts^1^-2</string>
</Properties>
  <Item class="Part" referent="RBX1"><Properties>
    <string name="Name">Base</string>
    <bool name="Anchored">true</bool>
    <int name="BrickColor">21</int>
    <int name="CollisionGroupId">1</int>
  </Properties>
    <Item class="Attachment" referent="RBX2"><Properties><string name="Name">A1</string></Properties></Item>
    <Item class="VectorForce" referent="RBX3"><Properties>
      <string name="Name">Push</string>
      <Ref name="Attachment0">RBX2</Ref>
      <Ref name="Attachment1">RBX2</Ref>
    </Properties></Item>
    <Item class="Sound" referent="RBX4"><Properties>
      <string name="Name">Boom</string>
      <Content name="SoundId"><url>rbxassetid://1234</url></Content>
    </Properties></Item>
    <Item class="ParticleEmitter" referent="RBX5"><Properties>
      <string name="Name">Sparks</string>
      <NumberRange name="Rotation">10 20 </NumberRange>
    </Properties></Item>
  </Item>
  <Item class="Animation" referent="RBX6"><Properties>
    <string name="Name">Wave</string>
    <Content name="AnimationId"><url>rbxassetid://5678</url></Content>
  </Properties></Item>
  <Item class="CFrameValue" referent="RBX7"><Properties>
    <string name="Name">Spot</string>
    <CoordinateFrame name="Value"><X>1</X><Y>7</Y><Z>3</Z><R00>1</R00><R01>0</R01><R02>0</R02><R10>0</R10><R11>1</R11><R12>0</R12><R20>0</R20><R21>0</R21><R22>1</R22></CoordinateFrame>
  </Properties></Item>
</Item>
<Item class="StarterGui" referent="RBX8"><Properties></Properties>
  <Item class="ScreenGui" referent="RBX9"><Properties>
    <string name="Name">HUD</string>
    <bool name="IgnoreGuiInset">true</bool>
  </Properties>
    <Item class="TextLabel" referent="RBX10"><Properties>
      <string name="Name">Title</string>
      <token name="Font">4</token>
      <string name="Text">Hi</string>
    </Properties>
      <Item class="UICorner" referent="RBX11"><Properties>
        <UDim name="CornerRadius"><S>0</S><O>20</O></UDim>
      </Properties></Item>
    </Item>
  </Item>
</Item>
<Item class="ServerScriptService" referent="RBX12"><Properties></Properties>
  <Item class="Script" referent="RBX13"><Properties>
    <string name="Name">Check</string>
    <ProtectedString name="Source"><![CDATA[
local base = workspace.Base
print("sound", base.Boom.SoundId)
print("anim", workspace.Wave.AnimationId)
local title = game.StarterGui.HUD.Title
print("corner", title.UICorner.CornerRadius.Offset)
print("font", title.Font == Enum.Font.SourceSansBold, title.FontFace.Weight == Enum.FontWeight.Bold)
print("inset", game.StarterGui.HUD.IgnoreGuiInset)
print("spot", workspace.Spot.Value.Position.Y)
print("range", base.Sparks.Rotation.Min, base.Sparks.Rotation.Max)
print("link", base.Push.Attachment1 == base.A1)
print("group", base.CollisionGroup)
local physics = game:GetService("PhysicsService")
print("ghosts", physics:CollisionGroupsAreCollidable("Ghosts", "Default"), physics:CollisionGroupsAreCollidable("Ghosts", "Ghosts"))
]]></ProtectedString>
  </Properties></Item>
</Item>
</roblox>"#;

fn find(tree: &InstanceTree, name: &str) -> playlibre_instance::InstanceId {
    tree.descendants(tree.root())
        .unwrap()
        .into_iter()
        .find(|id| tree.name(*id) == Ok(name))
        .unwrap_or_else(|| panic!("no {name}"))
}

/// The old place as read, and the same place saved again as binary by
/// today's writer (which saves the newer names).
fn both_ways() -> Vec<(&'static str, Vec<u8>)> {
    let dom = playlibre_rbx::read_dom(OLD_PLACE.as_bytes()).unwrap();
    let mut binary = Vec::new();
    rbx_binary::to_writer(&mut binary, &dom, dom.root().children()).unwrap();
    vec![("xml", OLD_PLACE.as_bytes().to_vec()), ("binary", binary)]
}

#[test]
fn moved_properties_come_back_under_the_names_playlibre_knows() {
    for (kind, bytes) in both_ways() {
        let dom = playlibre_rbx::read_dom(&bytes).unwrap();
        let mut tree = InstanceTree::new();
        let report = playlibre_rbx::import_place(&mut tree, &dom);
        assert!(report.skipped_properties.is_empty(), "{kind}: {:?}", report.skipped_properties);

        let sound = find(&tree, "Boom");
        assert_eq!(tree.get_string(sound, Property::SoundId).as_deref(), Some("rbxassetid://1234"), "{kind}");
        let wave = find(&tree, "Wave");
        assert_eq!(
            tree.get_string(wave, Property::AnimationId).as_deref(),
            Some("rbxassetid://5678"),
            "{kind}"
        );
        let corner = tree.children(find(&tree, "Title")).unwrap()[0];
        assert_eq!(
            tree.get(corner, Property::CornerRadius).unwrap(),
            PropertyValue::UDim(UDim::new(0.0, 20.0)),
            "{kind}"
        );
        let base = find(&tree, "Base");
        assert_eq!(tree.get_string(base, Property::CollisionGroup).as_deref(), Some("Ghosts"), "{kind}");
        let red = tree.get_color(base, Property::Color).unwrap();
        assert!(red.r > 0.7 && red.g < 0.2, "{kind}: bright red, {red:?}");
    }
}

#[test]
fn scripts_read_what_the_file_saved() {
    for (kind, bytes) in both_ways() {
        let dom = playlibre_rbx::read_dom(&bytes).unwrap();
        let mut tree = InstanceTree::new();
        playlibre_rbx::import_place(&mut tree, &dom);
        let shared = std::rc::Rc::new(std::cell::RefCell::new(tree));
        let mut session = PlaySession::start(shared, true).unwrap();
        for _ in 0..5 {
            session.step(1.0 / 60.0, CharacterInput::default());
        }
        let log = session.take_log();
        assert!(log.iter().all(|l| l.level != LogLevel::Error), "{kind}: {log:?}");
        let text: Vec<String> = log.into_iter().map(|l| l.text).collect();
        for line in [
            "sound rbxassetid://1234",
            "anim rbxassetid://5678",
            "corner 20",
            "font true true",
            "inset true",
            "spot 7",
            "range 10 20",
            "link true",
            "group Ghosts",
            "ghosts false true",
        ] {
            assert!(text.iter().any(|t| t == line), "{kind}: missing {line:?} in {text:?}");
        }
    }
}

/// A newer Studio saved a property as another type than the class list
/// here says: the file still opens, with that one property as saved.
#[test]
fn a_property_saved_in_a_newer_form_does_not_stop_the_file() {
    use rbx_dom_weak::{InstanceBuilder, WeakDom};
    let dom = WeakDom::new(
        InstanceBuilder::new("DataModel").with_child(
            InstanceBuilder::new("Workspace").with_child(
                InstanceBuilder::new("Part")
                    .with_name("Odd")
                    .with_property("Anchored", "yes")
                    .with_property("Transparency", 0.5f32),
            ),
        ),
    );
    // Written as if Part had no Anchored of its own, so it's saved as text.
    let mut db = rbx_reflection_database::get_bundled().clone();
    db.classes.get_mut("BasePart").unwrap().properties.remove("Anchored");
    let mut bytes = Vec::new();
    rbx_binary::Serializer::new()
        .reflection_database(&db)
        .serialize(&mut bytes, &dom, dom.root().children())
        .unwrap();
    assert!(rbx_binary::from_reader(bytes.as_slice()).is_err(), "the plain reader gives up");

    let mut notes = Vec::new();
    let read = playlibre_rbx::read_dom_noting(&bytes, &mut notes).unwrap();
    assert!(notes.iter().any(|n| n.contains("Anchored")), "{notes:?}");
    let mut tree = InstanceTree::new();
    playlibre_rbx::import_place(&mut tree, &read);
    let odd = find(&tree, "Odd");
    assert_eq!(tree.get_number(odd, Property::Transparency), Some(0.5));
}

/// An animation whose keyframes were saved beside the place (as
/// `assets/<number>.rbxm`, the way models for unions are) plays them.
#[test]
fn an_animation_saved_beside_the_place_plays() {
    use rbx_dom_weak::types::{CFrame, Enum, Matrix3, Vector3};
    use rbx_dom_weak::{InstanceBuilder, WeakDom};
    let pose = |name: &str| {
        InstanceBuilder::new("Pose")
            .with_name(name)
            .with_property("CFrame", CFrame::new(Vector3::new(0.0, 0.0, 0.0), Matrix3::identity()))
            .with_property("Weight", 1.0f32)
    };
    let keyframe = |time: f32| {
        InstanceBuilder::new("Keyframe")
            .with_property("Time", time)
            .with_child(pose("HumanoidRootPart").with_child(pose("Torso").with_child(pose("Right Arm"))))
    };
    let asset = WeakDom::new(
        InstanceBuilder::new("DataModel").with_child(
            InstanceBuilder::new("KeyframeSequence")
                .with_name("Wave")
                .with_property("Loop", false)
                .with_property("Priority", Enum::from_u32(2))
                .with_child(keyframe(0.0))
                .with_child(keyframe(1.5)),
        ),
    );
    let place =
        WeakDom::new(
            InstanceBuilder::new("DataModel")
                .with_child(InstanceBuilder::new("Workspace").with_child(
                    InstanceBuilder::new("Animation").with_name("Wave").with_property(
                        "AnimationId",
                        rbx_dom_weak::types::ContentId::from("rbxassetid://777"),
                    ),
                ))
                .with_child(InstanceBuilder::new("ServerStorage"))
                .with_child(InstanceBuilder::new("ServerScriptService").with_child(
                    InstanceBuilder::new("Script").with_name("Play").with_property(
                        "Source",
                        "local rig = Instance.new('Model')\n\
                     local humanoid = Instance.new('Humanoid')\n\
                     humanoid.Parent = rig\n\
                     rig.Parent = workspace\n\
                     print('saved', humanoid:LoadAnimation(workspace.Wave).Length)",
                    ),
                )),
        );
    let dir = std::env::temp_dir().join(format!("playlibre-anim-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    let mut bytes = Vec::new();
    rbx_binary::to_writer(&mut bytes, &asset, asset.root().children()).unwrap();
    std::fs::write(dir.join("assets/777.rbxm"), bytes).unwrap();
    let mut bytes = Vec::new();
    rbx_binary::to_writer(&mut bytes, &place, place.root().children()).unwrap();
    std::fs::write(dir.join("place.rbxl"), bytes).unwrap();

    let (tree, report) = playlibre_rbx::open(&dir.join("place.rbxl")).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(report.animations, 1, "{report:?}");
    let shared = std::rc::Rc::new(std::cell::RefCell::new(tree));
    let mut session = PlaySession::start(shared, true).unwrap();
    for _ in 0..3 {
        session.step(1.0 / 60.0, CharacterInput::default());
    }
    let log = session.take_log();
    assert!(log.iter().all(|l| l.level != LogLevel::Error), "{log:?}");
    let text: Vec<String> = log.into_iter().map(|l| l.text).collect();
    assert!(text.iter().any(|t| t == "saved 1.5"), "{text:?}");
}
