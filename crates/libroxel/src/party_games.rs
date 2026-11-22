//! Party games: Tag, Hide and seek and a battle royale. Each is a
//! map built here and scripts written the way any creator writes them:
//! a shared `Arena` module and the game's own script in
//! ServerScriptService, and a `Hud` local script that builds its GUI in
//! code. Computer players (a `Bot` model in ServerStorage) fill the places
//! people haven't taken, so a game is fun on your own too.

use libroxel_instance::{
    ClassKind, Color3, InstanceId, InstanceTree, Material, PartShape, Property, PropertyValue, Vec3,
};

use crate::templates::{add, block, code, material, orient, script, set, shape};

const ARENA: &str = include_str!("party_games/Arena.luau");
const HUD: &str = include_str!("party_games/Hud.luau");

/// The scripts and pieces every party game has: the shared module, the
/// game's script, the HUD and the bot template.
fn scripts(tree: &mut InstanceTree, name: &str, source: &str) {
    let server = tree.service(ClassKind::ServerScriptService).expect("every tree has ServerScriptService");
    code(tree, server, ClassKind::ModuleScript, "Arena", ARENA);
    script(tree, server, name, source);

    let starter_gui = tree.service(ClassKind::StarterGui).expect("every tree has StarterGui");
    let hud = add(tree, starter_gui, ClassKind::ScreenGui, "RoundHud");
    set(tree, hud, Property::ResetOnSpawn, PropertyValue::Bool(false));
    code(tree, hud, ClassKind::LocalScript, "Hud", HUD);

    let storage = tree.service(ClassKind::ServerStorage).expect("every tree has ServerStorage");
    bot(tree, storage);
}

/// A computer player: a body like a player's with a humanoid. The game
/// scripts clone it into the workspace and walk it about with `MoveTo`.
fn bot(tree: &mut InstanceTree, parent: InstanceId) -> InstanceId {
    use libroxel_engine::character::{BODY_SIZE, BodyColors, add_joints, build_rig, pose_rig};
    let model = add(tree, parent, ClassKind::Model, "Bot");
    let root = add(tree, model, ClassKind::Part, libroxel_script::names::character::ROOT_PART);
    set(tree, root, Property::Size, PropertyValue::Vec3(BODY_SIZE));
    set(tree, root, Property::Position, PropertyValue::Vec3(Vec3::new(0.0, 3.0, 0.0)));
    set(tree, root, Property::Transparency, PropertyValue::Number(1.0));
    let colors = BodyColors {
        skin: Color3::from_rgb8(245, 205, 150),
        shirt: Color3::from_rgb8(80, 130, 220),
        pants: Color3::from_rgb8(50, 60, 80),
    };
    let (head, limbs) = build_rig(tree, model, colors);
    add_joints(tree, root, head, &limbs);
    pose_rig(tree, root, head, &limbs, Default::default());
    add(tree, model, ClassKind::Humanoid, libroxel_script::names::character::HUMANOID);
    set(tree, model, Property::PrimaryPart, PropertyValue::Ref(Some(root)));
    model
}

/// A marker the scripts read positions from: an invisible, non-solid part.
fn marker(tree: &mut InstanceTree, parent: InstanceId, name: &str, at: [f32; 3]) {
    let id = block(tree, parent, name, at, [1.0, 1.0, 1.0], [255, 255, 255]);
    set(tree, id, Property::Transparency, PropertyValue::Number(1.0));
    set(tree, id, Property::CanCollide, PropertyValue::Bool(false));
}

/// Paint on the ground: a thin floor nothing collides with, as a lip that
/// thin catches feet (the ground underneath holds everyone up).
fn paint(tree: &mut InstanceTree, id: InstanceId) -> InstanceId {
    set(tree, id, Property::CanCollide, PropertyValue::Bool(false));
    id
}

fn tree_at(tree: &mut InstanceTree, parent: InstanceId, x: f32, z: f32, height: f32) {
    let trunk = block(tree, parent, "Trunk", [x, height / 2.0, z], [1.2, height, 1.2], [120, 85, 55]);
    material(tree, trunk, Material::Wood);
    let leaves = block(tree, parent, "Leaves", [x, height + 2.0, z], [6.0, 6.0, 6.0], [70, 155, 70]);
    shape(tree, leaves, PartShape::Ball);
    material(tree, leaves, Material::Grass);
}

fn spawn(tree: &mut InstanceTree, parent: InstanceId, at: [f32; 3], rgb: [u8; 3]) {
    let spawn = add(tree, parent, ClassKind::SpawnLocation, "SpawnLocation");
    set(tree, spawn, Property::Position, PropertyValue::Vec3(Vec3::new(at[0], at[1], at[2])));
    set(tree, spawn, Property::Size, PropertyValue::Vec3(Vec3::new(8.0, 1.0, 8.0)));
    set(tree, spawn, Property::Color, PropertyValue::Color3(Color3::from_rgb8(rgb[0], rgb[1], rgb[2])));
}

// ---------------------------------------------------------------------------
// Tag
// ---------------------------------------------------------------------------

/// Tag in a walled playground: pillars to dodge round, platforms with
/// ramps up to them, and tunnels to run through.
pub fn tag() -> InstanceTree {
    let mut tree = InstanceTree::new();
    let ws = tree.workspace();
    let arena = add(&mut tree, ws, ClassKind::Folder, "Arena");
    let floor = block(&mut tree, arena, "Floor", [0.0, -0.5, 0.0], [140.0, 1.0, 140.0], [110, 175, 95]);
    material(&mut tree, floor, Material::Grass);
    // Walls round the edge, in alternating colours.
    for (i, (at, size)) in [
        ([0.0, -70.5], [142.0, 1.0]),
        ([0.0, 70.5], [142.0, 1.0]),
        ([-70.5, 0.0], [1.0, 142.0]),
        ([70.5, 0.0], [1.0, 142.0]),
    ]
    .into_iter()
    .enumerate()
    {
        let colour = [[240, 120, 60], [70, 140, 230], [250, 200, 60], [150, 90, 210]][i];
        block(&mut tree, arena, "Wall", [at[0], 3.0, at[1]], [size[0], 6.0, size[1]], colour);
    }

    let play = add(&mut tree, ws, ClassKind::Folder, "Playground");
    // Pillars in a loose grid.
    let colours = [[230, 80, 70], [70, 130, 230], [250, 190, 50], [80, 190, 100], [170, 90, 210]];
    let mut n = 0;
    for gx in [-45.0f32, -15.0, 15.0, 45.0] {
        for gz in [-45.0f32, -15.0, 15.0, 45.0] {
            if gx.abs() == 15.0 && gz.abs() == 15.0 {
                continue;
            }
            let wobble = if n % 2 == 0 { 4.0 } else { -4.0 };
            let pillar = block(
                &mut tree,
                play,
                "Pillar",
                [gx + wobble, 4.0, gz - wobble],
                [5.0, 8.0, 5.0],
                colours[n % colours.len()],
            );
            material(&mut tree, pillar, Material::SmoothPlastic);
            n += 1;
        }
    }
    // A raised platform in the middle with ramps up two sides.
    let deck = block(&mut tree, play, "Deck", [0.0, 3.5, 0.0], [22.0, 1.0, 22.0], [200, 170, 120]);
    material(&mut tree, deck, Material::Wood);
    for (x, z) in [(-9.0, -9.0), (9.0, -9.0), (-9.0, 9.0), (9.0, 9.0)] {
        block(&mut tree, play, "Leg", [x, 1.5, z], [1.5, 3.0, 1.5], [150, 120, 80]);
    }
    for (name, at, yaw) in [("RampNorth", [0.0, -18.0], 0.0), ("RampSouth", [0.0, 18.0], 180.0)] {
        let ramp = add(&mut tree, play, ClassKind::Part, name);
        shape(&mut tree, ramp, PartShape::Wedge);
        set(&mut tree, ramp, Property::Size, PropertyValue::Vec3(Vec3::new(8.0, 4.0, 14.0)));
        set(&mut tree, ramp, Property::Position, PropertyValue::Vec3(Vec3::new(at[0], 2.0, at[1])));
        set(&mut tree, ramp, Property::Color, PropertyValue::Color3(Color3::from_rgb8(200, 170, 120)));
        set(&mut tree, ramp, Property::Anchored, PropertyValue::Bool(true));
        material(&mut tree, ramp, Material::Wood);
        orient(&mut tree, ramp, [0.0, yaw, 0.0]);
    }
    // Two tunnels: a roof on two walls.
    for (x, z, long_x) in [(-30.0, 0.0, false), (30.0, 0.0, false), (0.0, -32.0, true)] {
        let (sx, sz) = if long_x { (16.0, 1.0) } else { (1.0, 16.0) };
        let (ox, oz) = if long_x { (0.0, 3.5) } else { (3.5, 0.0) };
        for side in [-1.0, 1.0] {
            block(
                &mut tree,
                play,
                "TunnelWall",
                [x + ox * side, 2.5, z + oz * side],
                [sx, 5.0, sz],
                [160, 160, 170],
            );
        }
        let roof_size = if long_x { [16.0, 1.0, 8.0] } else { [8.0, 1.0, 16.0] };
        block(&mut tree, play, "TunnelRoof", [x, 5.5, z], roof_size, [120, 120, 130]);
    }
    for (x, z) in [(-60.0, -60.0), (60.0, 60.0), (-60.0, 60.0), (60.0, -60.0)] {
        tree_at(&mut tree, play, x, z, 6.0);
    }

    let spawns = add(&mut tree, ws, ClassKind::Folder, "Spawns");
    let mut i = 0;
    for x in [-55.0, -25.0, 25.0, 55.0] {
        for z in [-55.0, -25.0, 25.0, 55.0] {
            i += 1;
            marker(&mut tree, spawns, &format!("Spawn{i}"), [x, 0.5, z]);
        }
    }
    spawn(&mut tree, ws, [0.0, 4.5, 0.0], [200, 170, 120]);
    scripts(&mut tree, "Tag", include_str!("party_games/Tag.luau"));
    tree.drain_changes();
    tree
}

// ---------------------------------------------------------------------------
// Hide and seek
// ---------------------------------------------------------------------------

/// A little house with a doorway in its front wall, open inside, with a
/// cupboard and a bed to hide behind. `door` is the side the door is on:
/// +1 faces +Z, -1 faces -Z.
fn house(
    tree: &mut InstanceTree,
    parent: InstanceId,
    spots: InstanceId,
    at: [f32; 2],
    door: f32,
    colour: [u8; 3],
) {
    let (x, z) = (at[0], at[1]);
    let model = add(tree, parent, ClassKind::Model, "House");
    let floor = block(tree, model, "Floor", [x, 0.05, z], [20.0, 0.1, 16.0], [190, 160, 120]);
    paint(tree, floor);
    material(tree, floor, Material::Wood);
    // Back and side walls.
    block(tree, model, "Wall", [x, 4.0, z - 8.0 * door], [20.0, 8.0, 1.0], colour);
    block(tree, model, "Wall", [x - 10.0, 4.0, z], [1.0, 8.0, 16.0], colour);
    block(tree, model, "Wall", [x + 10.0, 4.0, z], [1.0, 8.0, 16.0], colour);
    // The front wall, with a doorway and a window.
    block(tree, model, "Wall", [x - 6.5, 4.0, z + 8.0 * door], [7.0, 8.0, 1.0], colour);
    block(tree, model, "Wall", [x + 6.5, 4.0, z + 8.0 * door], [7.0, 8.0, 1.0], colour);
    block(tree, model, "Wall", [x, 7.0, z + 8.0 * door], [6.0, 2.0, 1.0], colour);
    // An inner wall makes a back room.
    block(tree, model, "Wall", [x + 4.0, 4.0, z - 1.0 * door], [12.0, 8.0, 1.0], [235, 230, 215]);
    let roof = block(tree, model, "Roof", [x, 8.5, z], [22.0, 1.0, 18.0], [150, 70, 60]);
    material(tree, roof, Material::Brick);
    // Furniture.
    let bed = block(tree, model, "Bed", [x + 7.0, 1.0, z - 6.0 * door], [4.0, 2.0, 3.0], [220, 220, 240]);
    material(tree, bed, Material::Fabric);
    block(tree, model, "Cupboard", [x - 8.0, 3.0, z - 5.0 * door], [2.5, 6.0, 4.0], [130, 90, 60]);
    block(tree, model, "Table", [x - 3.0, 1.5, z + 3.0 * door], [4.0, 0.4, 3.0], [160, 120, 80]);
    // Where bots like to hide in here.
    marker(tree, spots, "Spot", [x + 3.0, 0.5, z - 4.5 * door]);
    marker(tree, spots, "Spot", [x - 5.0, 0.5, z - 5.0 * door]);
    marker(tree, spots, "Spot", [x - 7.0, 0.5, z + 5.0 * door]);
}

/// Hide and seek round a little neighbourhood: four houses, a shed where
/// the seeker waits, hedges, crates and trees.
pub fn hide_and_seek() -> InstanceTree {
    let mut tree = InstanceTree::new();
    let ws = tree.workspace();
    let map = add(&mut tree, ws, ClassKind::Folder, "Neighbourhood");
    let ground = block(&mut tree, map, "Ground", [0.0, -0.5, 0.0], [180.0, 1.0, 180.0], [105, 165, 90]);
    material(&mut tree, ground, Material::Grass);
    // A fence all round.
    for (at, size) in [
        ([0.0, -90.0], [180.0, 1.0]),
        ([0.0, 90.0], [180.0, 1.0]),
        ([-90.0, 0.0], [1.0, 180.0]),
        ([90.0, 0.0], [1.0, 180.0]),
    ] {
        let fence =
            block(&mut tree, map, "Fence", [at[0], 2.5, at[1]], [size[0], 5.0, size[1]], [200, 170, 130]);
        material(&mut tree, fence, Material::WoodPlanks);
    }
    // Paths in a cross, and the square where they meet.
    for (at, size) in [([0.0, 0.0], [10.0, 170.0]), ([0.0, 0.0], [170.0, 10.0])] {
        let path =
            block(&mut tree, map, "Path", [at[0], 0.03, at[1]], [size[0], 0.05, size[1]], [190, 185, 170]);
        paint(&mut tree, path);
        material(&mut tree, path, Material::Concrete);
    }
    let square = block(&mut tree, ws, "Square", [0.0, 0.05, 0.0], [20.0, 0.1, 20.0], [205, 200, 185]);
    paint(&mut tree, square);
    material(&mut tree, square, Material::Cobblestone);
    // A lamp post at each corner of the square.
    for (x, z) in [(-9.0, -9.0), (9.0, -9.0), (-9.0, 9.0), (9.0, 9.0)] {
        block(&mut tree, map, "LampPost", [x, 3.0, z], [0.5, 6.0, 0.5], [50, 50, 60]);
        let lamp = block(&mut tree, map, "Lamp", [x, 6.3, z], [1.2, 1.2, 1.2], [255, 240, 190]);
        material(&mut tree, lamp, Material::Neon);
    }

    let spots = add(&mut tree, ws, ClassKind::Folder, "HidingSpots");
    let houses = add(&mut tree, map, ClassKind::Folder, "Houses");
    house(&mut tree, houses, spots, [-40.0, -40.0], 1.0, [235, 200, 120]);
    house(&mut tree, houses, spots, [40.0, -40.0], 1.0, [150, 200, 230]);
    house(&mut tree, houses, spots, [-40.0, 40.0], -1.0, [230, 160, 170]);
    house(&mut tree, houses, spots, [40.0, 40.0], -1.0, [170, 220, 160]);

    // Hedges, crates and trees outside, each with a spot behind it.
    let garden = add(&mut tree, map, ClassKind::Folder, "Garden");
    for (x, z, long_x) in [
        (-70.0, 0.0, false),
        (70.0, 10.0, false),
        (-15.0, -70.0, true),
        (20.0, 70.0, true),
        (-65.0, -70.0, true),
        (65.0, 70.0, true),
    ] {
        let size = if long_x { [18.0, 5.0, 3.0] } else { [3.0, 5.0, 18.0] };
        let hedge = block(&mut tree, garden, "Hedge", [x, 2.5, z], size, [60, 130, 60]);
        material(&mut tree, hedge, Material::Grass);
        let behind = if long_x {
            [x, 0.5, z + if z > 0.0 { 3.5 } else { -3.5 }]
        } else {
            [x + if x > 0.0 { 3.5 } else { -3.5 }, 0.5, z]
        };
        marker(&mut tree, spots, "Spot", behind);
    }
    for (x, z) in [(-20.0, 20.0), (25.0, -18.0), (-75.0, 45.0), (75.0, -50.0), (15.0, 35.0)] {
        let crate_part = block(&mut tree, garden, "Crate", [x, 2.0, z], [4.0, 4.0, 4.0], [180, 135, 80]);
        material(&mut tree, crate_part, Material::WoodPlanks);
        block(&mut tree, garden, "Crate", [x + 4.0, 1.5, z + 1.0], [3.0, 3.0, 3.0], [170, 125, 75]);
        marker(&mut tree, spots, "Spot", [x + 1.0, 0.5, z + 4.0]);
    }
    for (x, z) in [(-20.0, -20.0), (20.0, 20.0), (-60.0, 15.0), (60.0, -15.0), (-25.0, 75.0), (30.0, -75.0)] {
        tree_at(&mut tree, garden, x, z, 7.0);
    }

    // The shed the seeker waits in: closed all round.
    let shed = add(&mut tree, ws, ClassKind::Model, "Shed");
    let (sx, sz) = (0.0, -80.0);
    let shed_floor = block(&mut tree, shed, "Floor", [sx, 0.05, sz], [10.0, 0.1, 10.0], [120, 100, 80]);
    paint(&mut tree, shed_floor);
    for (at, size) in [
        ([0.0, -5.0], [10.0, 1.0]),
        ([0.0, 5.0], [10.0, 1.0]),
        ([-5.0, 0.0], [1.0, 10.0]),
        ([5.0, 0.0], [1.0, 10.0]),
    ] {
        let wall = block(
            &mut tree,
            shed,
            "Wall",
            [sx + at[0], 4.0, sz + at[1]],
            [size[0], 8.0, size[1]],
            [110, 90, 70],
        );
        material(&mut tree, wall, Material::WoodPlanks);
    }
    block(&mut tree, shed, "Roof", [sx, 8.5, sz], [12.0, 1.0, 12.0], [80, 70, 60]);
    marker(&mut tree, shed, "Inside", [sx, 0.5, sz]);

    let spawns = add(&mut tree, ws, ClassKind::Folder, "Spawns");
    for (i, (x, z)) in [
        (-6.0, 12.0),
        (6.0, 12.0),
        (-12.0, -6.0),
        (12.0, -6.0),
        (-6.0, -12.0),
        (6.0, -12.0),
        (12.0, 6.0),
        (-12.0, 6.0),
    ]
    .into_iter()
    .enumerate()
    {
        marker(&mut tree, spawns, &format!("Spawn{}", i + 1), [x, 0.5, z]);
    }
    spawn(&mut tree, ws, [0.0, 0.0, 15.0], [190, 185, 170]);
    scripts(&mut tree, "HideAndSeek", include_str!("party_games/HideAndSeek.luau"));
    tree.drain_changes();
    tree
}

// ---------------------------------------------------------------------------
// Battle royale
// ---------------------------------------------------------------------------

/// A tool with a handle, kept in ServerStorage for the game to hand out.
fn weapon(tree: &mut InstanceTree, parent: InstanceId, name: &str, size: [f32; 3], rgb: [u8; 3], neon: bool) {
    let tool = add(tree, parent, ClassKind::Tool, name);
    let handle = block(tree, tool, "Handle", [0.0, 0.0, 0.0], size, rgb);
    set(tree, handle, Property::Anchored, PropertyValue::Bool(false));
    set(tree, handle, Property::CanCollide, PropertyValue::Bool(false));
    material(tree, handle, if neon { Material::Neon } else { Material::Metal });
}

/// A battle royale on an island: cover, trees to chop, chests, a lobby
/// floating off the shore, the airship everyone drops from, and the storm
/// ring that closes in.
pub fn battle_royale() -> InstanceTree {
    let mut tree = InstanceTree::new();
    let ws = tree.workspace();
    let water = block(&mut tree, ws, "Sea", [0.0, -2.0, 0.0], [600.0, 1.0, 600.0], [60, 140, 210]);
    set(&mut tree, water, Property::Transparency, PropertyValue::Number(0.2));
    let island = add(&mut tree, ws, ClassKind::Folder, "Island");
    let ground = block(&mut tree, island, "Ground", [0.0, -0.5, 0.0], [240.0, 1.0, 240.0], [115, 170, 85]);
    material(&mut tree, ground, Material::Grass);
    // A sandy beach round the edge.
    for (at, size) in [
        ([0.0, -124.0], [256.0, 8.0]),
        ([0.0, 124.0], [256.0, 8.0]),
        ([-124.0, 0.0], [8.0, 240.0]),
        ([124.0, 0.0], [8.0, 240.0]),
    ] {
        let sand =
            block(&mut tree, island, "Beach", [at[0], -0.6, at[1]], [size[0], 1.0, size[1]], [230, 210, 150]);
        material(&mut tree, sand, Material::Sand);
    }
    // Cover: rocks, walls and a few ruined buildings.
    let cover = add(&mut tree, island, ClassKind::Folder, "Cover");
    for (i, (x, z)) in [
        (-80.0, -60.0),
        (-30.0, -90.0),
        (50.0, -70.0),
        (90.0, -20.0),
        (70.0, 60.0),
        (10.0, 90.0),
        (-60.0, 80.0),
        (-95.0, 20.0),
        (-20.0, 30.0),
        (30.0, -20.0),
        (0.0, -45.0),
        (45.0, 25.0),
    ]
    .into_iter()
    .enumerate()
    {
        let rock = block(
            &mut tree,
            cover,
            "Rock",
            [x, 2.0, z],
            [6.0 + (i % 3) as f32 * 2.0, 4.0 + (i % 2) as f32 * 2.0, 6.0],
            [130, 130, 135],
        );
        material(&mut tree, rock, Material::Slate);
    }
    for (x, z, long_x) in
        [(-50.0, -20.0, true), (60.0, 0.0, false), (-10.0, 60.0, true), (20.0, -100.0, true)]
    {
        let size = if long_x { [20.0, 6.0, 1.5] } else { [1.5, 6.0, 20.0] };
        let wall = block(&mut tree, cover, "Wall", [x, 3.0, z], size, [180, 160, 140]);
        material(&mut tree, wall, Material::Brick);
    }
    for (x, z) in [(-70.0, 40.0), (70.0, -55.0), (0.0, 0.0)] {
        // Four corner walls with gaps between: a ruin you can run through.
        for (dx, dz, sx, sz) in
            [(-5.0, -8.0, 8.0, 1.0), (5.0, 8.0, 8.0, 1.0), (-8.0, 4.0, 1.0, 8.0), (8.0, -4.0, 1.0, 8.0)]
        {
            let ruin = block(&mut tree, cover, "Ruin", [x + dx, 3.5, z + dz], [sx, 7.0, sz], [200, 190, 170]);
            material(&mut tree, ruin, Material::Concrete);
        }
    }
    for (x, z) in [
        (-100.0, -100.0),
        (100.0, 100.0),
        (-100.0, 90.0),
        (95.0, -95.0),
        (-40.0, 0.0),
        (35.0, 55.0),
        (-15.0, -60.0),
        (-60.0, -35.0),
        (-75.0, -85.0),
        (20.0, 70.0),
        (85.0, 35.0),
        (105.0, -60.0),
        (-30.0, 105.0),
        (55.0, -40.0),
        (-105.0, -30.0),
        (10.0, -80.0),
        (-50.0, 55.0),
    ] {
        tree_at(&mut tree, island, x, z, 8.0);
    }

    // Chests: walk into one to open it.
    let loot = add(&mut tree, ws, ClassKind::Folder, "Loot");
    for (x, z) in [
        (-70.0, 40.0),
        (70.0, -55.0),
        (0.0, 0.0),
        (-85.0, -60.0),
        (55.0, -75.0),
        (95.0, -15.0),
        (75.0, 65.0),
        (15.0, 95.0),
        (-55.0, 85.0),
        (-100.0, 25.0),
        (-25.0, 35.0),
        (35.0, -25.0),
        (-5.0, -50.0),
        (40.0, 30.0),
        (-45.0, -95.0),
        (100.0, 40.0),
    ] {
        let chest = block(&mut tree, loot, "Chest", [x, 1.0, z], [3.0, 2.0, 2.0], [245, 190, 60]);
        material(&mut tree, chest, Material::Neon);
        set(&mut tree, chest, Property::CanCollide, PropertyValue::Bool(false));
    }

    // The storm ring: a see-through cylinder the script resizes.
    let storm = block(&mut tree, ws, "Storm", [0.0, 58.0, 0.0], [120.0, 270.0, 270.0], [90, 140, 255]);
    shape(&mut tree, storm, PartShape::Cylinder);
    orient(&mut tree, storm, [0.0, 0.0, 90.0]);
    set(&mut tree, storm, Property::CanCollide, PropertyValue::Bool(false));
    set(&mut tree, storm, Property::Transparency, PropertyValue::Number(0.75));
    set(&mut tree, storm, Property::CastShadow, PropertyValue::Bool(false));
    material(&mut tree, storm, Material::ForceField);
    let next = block(&mut tree, ws, "NextRing", [0.0, 0.2, 0.0], [0.3, 1.0, 1.0], [255, 255, 255]);
    shape(&mut tree, next, PartShape::Cylinder);
    orient(&mut tree, next, [0.0, 0.0, 90.0]);
    set(&mut tree, next, Property::CanCollide, PropertyValue::Bool(false));
    set(&mut tree, next, Property::Transparency, PropertyValue::Number(1.0));
    set(&mut tree, next, Property::CastShadow, PropertyValue::Bool(false));
    material(&mut tree, next, Material::Neon);

    let airship = add(&mut tree, ws, ClassKind::Folder, "Airship");
    let hull = block(&mut tree, airship, "Hull", [0.0, 110.0, 260.0], [10.0, 3.0, 28.0], [150, 100, 60]);
    material(&mut tree, hull, Material::WoodPlanks);
    let balloon =
        block(&mut tree, airship, "Balloon", [0.0, 126.0, 260.0], [16.0, 16.0, 34.0], [235, 90, 70]);
    shape(&mut tree, balloon, PartShape::Ball);
    set(&mut tree, balloon, Property::CanCollide, PropertyValue::Bool(false));
    for x in [-4.5f32, 4.5] {
        let rope = block(&mut tree, airship, "Rope", [x, 115.0, 260.0], [0.3, 7.0, 0.3], [90, 70, 50]);
        set(&mut tree, rope, Property::CanCollide, PropertyValue::Bool(false));
    }
    let fin = block(&mut tree, airship, "Fin", [0.0, 113.0, 274.0], [0.6, 6.0, 4.0], [250, 200, 60]);
    set(&mut tree, fin, Property::CanCollide, PropertyValue::Bool(false));

    // The lobby, on a platform out at sea.
    let lobby = add(&mut tree, ws, ClassKind::Folder, "Lobby");
    let floor = block(&mut tree, lobby, "LobbyFloor", [0.0, 0.0, 175.0], [40.0, 1.0, 30.0], [200, 205, 215]);
    material(&mut tree, floor, Material::DiamondPlate);
    for (at, size) in [
        ([0.0, 160.0], [40.0, 1.0]),
        ([0.0, 190.0], [40.0, 1.0]),
        ([-20.0, 175.0], [1.0, 30.0]),
        ([20.0, 175.0], [1.0, 30.0]),
    ] {
        let rail =
            block(&mut tree, lobby, "Rail", [at[0], 2.0, at[1]], [size[0], 3.0, size[1]], [240, 120, 60]);
        set(&mut tree, rail, Property::Transparency, PropertyValue::Number(0.3));
    }
    spawn(&mut tree, lobby, [0.0, 1.0, 175.0], [80, 150, 230]);

    let storage = tree.service(ClassKind::ServerStorage).expect("every tree has ServerStorage");
    weapon(&mut tree, storage, "Sword", [0.4, 4.0, 0.4], [200, 210, 225], false);
    weapon(&mut tree, storage, "Blaster", [0.6, 0.9, 2.2], [90, 230, 255], true);
    weapon(&mut tree, storage, "Scattergun", [0.8, 0.9, 1.8], [90, 230, 255], true);
    weapon(&mut tree, storage, "Longshot", [0.4, 0.6, 3.4], [90, 230, 255], true);
    let glider = block(&mut tree, storage, "Glider", [0.0, 0.0, 0.0], [7.0, 0.3, 3.0], [70, 160, 245]);
    set(&mut tree, glider, Property::Anchored, PropertyValue::Bool(false));
    set(&mut tree, glider, Property::CanCollide, PropertyValue::Bool(false));
    set(&mut tree, glider, Property::Massless, PropertyValue::Bool(true));
    scripts(&mut tree, "BattleRoyale", include_str!("party_games/BattleRoyale.luau"));
    let starter =
        tree.starter_scripts(ClassKind::StarterPlayerScripts).expect("every tree has StarterPlayerScripts");
    code(&mut tree, starter, ClassKind::LocalScript, "Aim", include_str!("party_games/Aim.luau"));
    code(&mut tree, starter, ClassKind::LocalScript, "Royale", include_str!("party_games/Royale.luau"));
    tree.drain_changes();
    tree
}

#[cfg(test)]
mod tests {
    use super::*;
    use libroxel_engine::session::{PlaySession, start_on};
    use libroxel_engine::{CharacterInput, LogLevel};

    fn errors(session: &mut PlaySession) -> Vec<String> {
        session.take_log().into_iter().filter(|l| l.level == LogLevel::Error).map(|l| l.text).collect()
    }

    fn value(session: &PlaySession, script: &str) -> String {
        session.runtime().run("check", &format!("print({script})")).unwrap();
        session.runtime().take_output().last().map(|l| l.text.clone()).unwrap_or_default()
    }

    fn seconds(session: &mut PlaySession, seconds: f32) {
        for _ in 0..(seconds * 30.0) as usize {
            session.step(1.0 / 30.0, CharacterInput::default());
        }
    }

    /// Starts `tree` and has the HUD's messages printed, so tests can read them.
    fn start(tree: InstanceTree) -> PlaySession {
        let session = start_on(tree, true).unwrap();
        session
            .runtime()
            .run(
                "listen",
                "game.ReplicatedStorage:WaitForChild('Round').Notify.OnClientEvent:Connect(function(text) print('NOTE ' .. text) end)",
            )
            .unwrap();
        session
    }

    /// Runs for `seconds`, returning the messages shown and any script errors.
    fn run(session: &mut PlaySession, secs: f32) -> (Vec<String>, Vec<String>) {
        seconds(session, secs);
        let log = session.take_log();
        let notes = log.iter().filter_map(|l| l.text.strip_prefix("NOTE ").map(str::to_owned)).collect();
        let errors = log.into_iter().filter(|l| l.level == LogLevel::Error).map(|l| l.text).collect();
        (notes, errors)
    }

    /// Keeps the local player up out of the way, so only the bots play.
    fn sit_out(session: &PlaySession) {
        session
            .runtime()
            .run(
                "sit out",
                "workspace.Player1.HumanoidRootPart.Anchored = true
                workspace.Player1.HumanoidRootPart.Position = Vector3.new(500, 40, 500)",
            )
            .unwrap();
    }

    fn roles(session: &PlaySession) -> String {
        value(
            session,
            "(function() local t = {} for _, m in workspace.Bots:GetChildren() do t[#t + 1] = m:GetAttribute('Role') end
            t[#t + 1] = workspace.Player1:GetAttribute('Role') table.sort(t) return table.concat(t, ',') end)()",
        )
    }

    #[test]
    fn tag_bots_chase_and_tag_each_other() {
        let mut session = start(tag());
        let (notes, errors) = run(&mut session, 10.0);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(value(&session, "#workspace.Bots:GetChildren()"), "5");
        assert_eq!(roles(&session), "It,Runner,Runner,Runner,Runner,Runner");
        assert!(notes.iter().any(|n| n.ends_with(" is It")), "{notes:?}");
        assert_ne!(value(&session, "game.Players.Player1.PlayerGui.RoundHud.Top.Status.Text"), "");
        // If the player is It, they tag a bot first. Then, with the player out
        // of reach, the bots chase each other down.
        if value(&session, "workspace.Player1:GetAttribute('Role')") == "It" {
            run(&mut session, 2.0);
            session
                .runtime()
                .run(
                    "tag a bot",
                    "local bot = workspace.Bots:GetChildren()[1].HumanoidRootPart
                    workspace.Player1.HumanoidRootPart.Position = bot.Position + Vector3.new(1, 0, 0)",
                )
                .unwrap();
            let (notes, _) = run(&mut session, 0.5);
            assert!(notes.iter().any(|n| n.starts_with("Player1 tagged ")), "{notes:?}");
        }
        sit_out(&session);
        let (notes, errors) = run(&mut session, 90.0);
        assert!(errors.is_empty(), "{errors:?}");
        assert!(notes.iter().any(|n| n.contains(" tagged ")), "{notes:?}");
        assert!(notes.iter().any(|n| n.contains("s as It)")), "{notes:?}");
        // The bots' bodies went everywhere with them.
        let apart = "(function() local most = 0 for _, m in workspace.Bots:GetChildren() do
            local d = (m.Torso.Position - m.HumanoidRootPart.Position) * Vector3.new(1, 0, 1)
            most = math.max(most, d.Magnitude) end return most < 1 end)()";
        assert_eq!(value(&session, apart), "true");
    }

    /// A second computer joins a hosted game of Tag: it takes a bot's place,
    /// and its HUD shows the round and its role.
    #[test]
    fn tag_over_the_network() {
        use std::{cell::RefCell, rc::Rc};
        let mut host = PlaySession::host(Rc::new(RefCell::new(tag())), 0, None, true).unwrap();
        let port = host.host_address().unwrap().port();
        let mut guest = PlaySession::join(
            Rc::new(RefCell::new(InstanceTree::new())),
            &format!("127.0.0.1:{port}"),
            "Bea",
        )
        .unwrap();
        let role = "game.Players.LocalPlayer.PlayerGui:FindFirstChild('RoundHud') and game.Players.LocalPlayer.PlayerGui.RoundHud.Role.Text";
        let mut seen = String::new();
        for _ in 0..900 {
            host.step(1.0 / 30.0, CharacterInput::default());
            guest.step(1.0 / 30.0, CharacterInput::default());
            seen = value(&guest, role);
            if seen == "It" || seen == "Runner" {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(seen == "It" || seen == "Runner", "the guest's role: {seen:?}");
        // Two people, so four bots.
        assert_eq!(value(&host, "#workspace.Bots:GetChildren()"), "4");
        assert_ne!(value(&guest, "game.Players.LocalPlayer.PlayerGui.RoundHud.Top.Status.Text"), "");
        assert!(errors(&mut host).is_empty(), "{:?}", errors(&mut host));
        assert!(errors(&mut guest).is_empty(), "{:?}", errors(&mut guest));
    }

    #[test]
    fn hide_and_seek_blindfolds_the_seeker_then_finds_hiders() {
        let mut session = start(hide_and_seek());
        let (_, errors) = run(&mut session, 10.0);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(roles(&session), "Hider,Hider,Hider,Hider,Hider,Seeker");
        let me = value(&session, "workspace.Player1:GetAttribute('Role')");
        let blind = value(&session, "game.Players.Player1.PlayerGui.RoundHud.Blindfold.Visible");
        assert_eq!(blind, if me == "Seeker" { "true" } else { "false" });
        // Bots hide in the houses and behind things, and a seeking bot finds them.
        if me == "Hider" {
            sit_out(&session);
            let (notes, errors) = run(&mut session, 140.0);
            assert!(errors.is_empty(), "{errors:?}");
            assert!(notes.iter().any(|n| n.contains(" found ")), "{notes:?}");
            // The player was never found, so the hiders won.
            assert!(
                notes.iter().any(|n| n.starts_with("Never found:") && n.contains("Player1")),
                "{notes:?}"
            );
            assert_eq!(value(&session, "game.Players.Player1.leaderstats.Wins.Value"), "1");
        } else {
            let (_, errors) = run(&mut session, 30.0);
            assert!(errors.is_empty(), "{errors:?}");
            assert_eq!(value(&session, "game.Players.Player1.PlayerGui.RoundHud.Blindfold.Visible"), "false");
        }
    }

    fn press(session: &mut PlaySession, key: libroxel_instance::KeyCode) {
        session.input_began(libroxel_script::UserInput::Key(key), false);
        seconds(session, 0.1);
        session.input_ended(libroxel_script::UserInput::Key(key), false);
    }

    fn click(session: &mut PlaySession, times: usize, gap: f32) {
        for _ in 0..times {
            session.input_began(libroxel_script::UserInput::MouseButton1, false);
            seconds(session, 0.1);
            session.input_ended(libroxel_script::UserInput::MouseButton1, false);
            seconds(session, gap);
        }
    }

    fn number(session: &PlaySession, script: &str) -> f32 {
        value(session, script).parse().unwrap_or_else(|_| panic!("{script}: {}", value(session, script)))
    }

    fn role_of(session: &PlaySession) -> String {
        value(session, "workspace.Player1:GetAttribute('Role')")
    }

    /// Starts a battle royale, jumps out of the airship and glides down.
    fn landed() -> PlaySession {
        let mut session = start(battle_royale());
        seconds(&mut session, 11.0);
        assert_eq!(role_of(&session), "Riding");
        press(&mut session, libroxel_instance::KeyCode::E);
        for _ in 0..200 {
            if role_of(&session) == "Fighter" {
                break;
            }
            seconds(&mut session, 0.1);
        }
        assert_eq!(role_of(&session), "Fighter");
        session
    }

    /// Leaves one bot, once it has landed, standing still where `at` says.
    fn one_bot(session: &mut PlaySession, at: &str) {
        session
            .runtime()
            .run("one bot", "for i, b in workspace.Bots:GetChildren() do if i > 1 then b:Destroy() end end")
            .unwrap();
        for _ in 0..400 {
            if value(session, &format!("{BOT}:GetAttribute('Role')")) == "Fighter" {
                break;
            }
            seconds(session, 0.1);
        }
        assert_eq!(value(session, &format!("{BOT}:GetAttribute('Role')")), "Fighter");
        session
            .runtime()
            .run(
                "stand",
                &format!(
                    "workspace.Builds:ClearAllChildren() {BOT}.Humanoid.WalkSpeed = 0 {BOT}:PivotTo({at})"
                ),
            )
            .unwrap();
    }

    const BOT: &str = "workspace.Bots:GetChildren()[1]";

    #[test]
    fn battle_royale_drops_everyone_from_the_airship() {
        let mut session = start(battle_royale());
        seconds(&mut session, 11.0);
        assert_eq!(value(&session, "#workspace.Bots:GetChildren()"), "7");
        assert_eq!(role_of(&session), "Riding");
        assert!(number(&session, "workspace.Player1.HumanoidRootPart.Position.Y") > 100.0);
        assert_eq!(value(&session, "game.Players.Player1.PlayerGui.Royale.Jump.Visible"), "true");
        assert_eq!(value(&session, "game.Players.Player1.Backpack:FindFirstChild('Sword') ~= nil"), "true");
        let ship = |s: &PlaySession| value(s, "workspace.Airship.Hull.Position");
        let before = ship(&session);
        seconds(&mut session, 1.0);
        assert_ne!(ship(&session), before, "the airship flies");

        press(&mut session, libroxel_instance::KeyCode::E);
        seconds(&mut session, 1.0);
        assert_eq!(role_of(&session), "Gliding");
        assert_eq!(value(&session, "workspace.Player1:FindFirstChild('Glider') ~= nil"), "true");
        let fall = number(&session, "workspace.Player1.HumanoidRootPart.AssemblyLinearVelocity.Y");
        assert!(fall > -30.0, "gliding, not falling: {fall}");
        for _ in 0..100 {
            if role_of(&session) == "Fighter" {
                break;
            }
            seconds(&mut session, 0.1);
        }
        assert_eq!(role_of(&session), "Fighter");
        assert_eq!(value(&session, "workspace.Player1:FindFirstChild('Glider') == nil"), "true");
        assert_eq!(value(&session, "game.Players.Player1.PlayerGui.Royale.Bars.Visible"), "true");
        seconds(&mut session, 25.0);
        let up = "(function() local n = 0 for _, m in workspace.Bots:GetChildren() do
            local r = m:GetAttribute('Role') if r == 'Riding' or r == 'Gliding' then n += 1 end end return n end)()";
        assert_eq!(value(&session, up), "0");
        let errs = errors(&mut session);
        assert!(errs.is_empty(), "{errs:?}");
    }

    #[test]
    fn battle_royale_storm_closes_and_the_last_one_wins() {
        let mut session = landed();
        one_bot(&mut session, "CFrame.new(-60, 3, -60)");
        session.runtime().run("me", "workspace.Player1:PivotTo(CFrame.new(-60, 3, 60))").unwrap();
        let size = |s: &PlaySession| number(s, "workspace.Storm.Size.Z");
        let before = size(&session);
        assert!(number(&session, "workspace.NextRing.Transparency") < 1.0, "the next ring shows");
        seconds(&mut session, 55.0);
        assert!(size(&session) < before, "the storm shrank: {before} -> {}", size(&session));
        session.toggle_tool(0);
        seconds(&mut session, 0.5);
        session
            .runtime()
            .run(
                "line up",
                &format!(
                    "local me = workspace.Player1.HumanoidRootPart
                    {BOT}:PivotTo(CFrame.new(me.Position + me.CFrame.LookVector * 3))"
                ),
            )
            .unwrap();
        let health = |s: &PlaySession| {
            number(s, &format!("{BOT}.Humanoid.Health + ({BOT}:GetAttribute('Shield') or 0)"))
        };
        let before = health(&session);
        click(&mut session, 1, 0.1);
        assert!(health(&session) < before, "the sword hit: {before} -> {}", health(&session));
        for _ in 0..60 {
            if role_of(&session) == "Winner" {
                break;
            }
            click(&mut session, 1, 0.4);
        }
        seconds(&mut session, 1.0);
        assert_eq!(role_of(&session), "Winner");
        assert_eq!(value(&session, "game.Players.Player1.leaderstats.Wins.Value"), "1");
        assert_eq!(value(&session, "game.Players.Player1.PlayerGui.Royale.Result.Visible"), "true");
        assert_eq!(value(&session, "game.Players.Player1.PlayerGui.Royale.Result.Place.Text"), "#1");
        let errs = errors(&mut session);
        assert!(errs.is_empty(), "{errs:?}");
    }

    #[test]
    fn battle_royale_chests_give_tiered_weapons_and_shields_soak_damage() {
        let mut session = landed();
        one_bot(&mut session, "CFrame.new(-60, 3, -60)");
        let ranged = "(function() for _, n in {'Blaster', 'Scattergun', 'Longshot'} do
            local t = game.Players.Player1.Backpack:FindFirstChild(n) if t then return t:GetAttribute('Tier') end end end)()";
        let shield = |s: &PlaySession| number(s, "game.Players.Player1:GetAttribute('Shield') or 0");
        for i in 1..=16 {
            if value(&session, ranged) != "nil" && shield(&session) > 0.0 {
                break;
            }
            session
                .runtime()
                .run(
                    "chest",
                    &format!(
                        "local c = workspace.Loot:GetChildren()[{i}]
                        workspace.Player1:PivotTo(CFrame.new(c.Position + Vector3.new(0, 3, 0)))"
                    ),
                )
                .unwrap();
            seconds(&mut session, 0.3);
        }
        let tier: u32 = value(&session, ranged).parse().unwrap();
        assert!((1..=5).contains(&tier));
        assert!(number(&session, "game.Players.Player1:GetAttribute('Wood')") >= 30.0);
        assert!(shield(&session) > 0.0);
        let before = shield(&session);
        session.runtime().run("full", "workspace.Player1.Humanoid.Health = 100").unwrap();
        session
            .runtime()
            .run(
                "bot attacks",
                &format!(
                    "workspace.Builds:ClearAllChildren()
                    workspace.Player1:PivotTo(CFrame.new(-20, 3, -20))
                    {BOT}:PivotTo(CFrame.new(-20, 3, -17))"
                ),
            )
            .unwrap();
        for _ in 0..80 {
            if shield(&session) < before {
                break;
            }
            seconds(&mut session, 0.1);
        }
        assert!(shield(&session) < before, "the shield took the hit: {before} -> {}", shield(&session));
        assert_eq!(number(&session, "workspace.Player1.Humanoid.Health"), 100.0);
        let errs = errors(&mut session);
        assert!(errs.is_empty(), "{errs:?}");
    }

    #[test]
    fn battle_royale_chop_wood_and_build() {
        let mut session = landed();
        one_bot(&mut session, "CFrame.new(-60, 3, -60)");
        let wood = |s: &PlaySession| number(s, "game.Players.Player1:GetAttribute('Wood')");
        session
            .runtime()
            .run(
                "by a tree",
                "local trunk = workspace.Island.Trunk
                local at = trunk.Position * Vector3.new(1, 0, 1) + Vector3.new(0, 3, 4)
                workspace.Player1:PivotTo(CFrame.lookAt(at, Vector3.new(trunk.Position.X, 3, trunk.Position.Z)))",
            )
            .unwrap();
        session.toggle_tool(0);
        seconds(&mut session, 0.3);
        let start = wood(&session);
        click(&mut session, 3, 0.4);
        assert_eq!(wood(&session), start + 30.0);

        session
            .runtime()
            .run("open ground", "workspace.Player1:PivotTo(CFrame.lookAt(Vector3.new(-20, 3, -20), Vector3.new(-20, 3, -40)))")
            .unwrap();
        seconds(&mut session, 0.3);
        press(&mut session, libroxel_instance::KeyCode::Z);
        seconds(&mut session, 0.2);
        assert_eq!(value(&session, "workspace.Builds:FindFirstChild('Wall') ~= nil"), "true");
        assert_eq!(wood(&session), start + 20.0);
        press(&mut session, libroxel_instance::KeyCode::C);
        seconds(&mut session, 0.2);
        assert_eq!(value(&session, "workspace.Builds:FindFirstChild('Ramp') ~= nil"), "true");
        assert_eq!(wood(&session), start + 10.0);
        press(&mut session, libroxel_instance::KeyCode::Z);
        seconds(&mut session, 0.2);
        assert_eq!(
            value(&session, "#workspace.Builds:GetChildren()"),
            "2",
            "the same wall twice is one wall"
        );

        click(&mut session, 3, 0.4);
        seconds(&mut session, 0.2);
        assert_eq!(
            value(&session, "workspace.Builds:FindFirstChild('Wall') == nil"),
            "true",
            "the sword broke the wall"
        );
        let errs = errors(&mut session);
        assert!(errs.is_empty(), "{errs:?}");
    }

    #[test]
    fn battle_royale_blaster_fires_where_you_aim() {
        let mut session = landed();
        one_bot(&mut session, "CFrame.new(-60, 3, -60)");
        let has = "game.Players.Player1.Backpack:FindFirstChild('Blaster') ~= nil";
        for round in 0..20 {
            if value(&session, has) == "true" {
                break;
            }
            session
                .runtime()
                .run(
                    "chest",
                    &format!(
                        "for _, c in workspace.Loot:GetChildren() do c:SetAttribute('Open', false) end
                        local c = workspace.Loot:GetChildren()[{}]
                        workspace.Player1:PivotTo(CFrame.new(c.Position + Vector3.new(0, 3, 0)))",
                        round % 16 + 1
                    ),
                )
                .unwrap();
            seconds(&mut session, 0.3);
        }
        assert_eq!(value(&session, has), "true");
        session
            .runtime()
            .run(
                "behind",
                &format!(
                    "for _, t in game.Players.Player1.Backpack:GetChildren() do if t.Name ~= 'Sword' and t.Name ~= 'Blaster' then t:Destroy() end end
                    workspace.Builds:ClearAllChildren()
                    workspace.Player1:PivotTo(CFrame.new(0, 3, 0))
                    {BOT}:PivotTo(CFrame.new(0, 3, 25))"
                ),
            )
            .unwrap();
        seconds(&mut session, 0.3);
        session.toggle_tool(1);
        seconds(&mut session, 0.3);
        assert_eq!(value(&session, "workspace.Player1:FindFirstChild('Blaster') ~= nil"), "true");
        let at = value(&session, &format!("{BOT}.HumanoidRootPart.Position"));
        let at: Vec<f32> = at.split(',').map(|v| v.trim().parse().unwrap()).collect();
        session.set_mouse(Some(libroxel_script::MouseRay {
            origin: Vec3::new(at[0], at[1] + 20.0, at[2]),
            direction: Vec3::new(0.0, -1.0, 0.0),
            x: 0.0,
            y: 0.0,
        }));
        seconds(&mut session, 0.3);
        let health = |s: &PlaySession| {
            number(s, &format!("{BOT}.Humanoid.Health + ({BOT}:GetAttribute('Shield') or 0)"))
        };
        let before = health(&session);
        click(&mut session, 1, 0.2);
        assert!(health(&session) < before, "the blast hit: {before} -> {}", health(&session));
        let errs = errors(&mut session);
        assert!(errs.is_empty(), "{errs:?}");
    }
}
