//! What drawing a big, decorated map costs: the 3D view drawn off screen
//! (on whatever graphics the machine has; a software one works) for a map
//! of 30,000 parts in many colours and materials, 1,500 decals and 600
//! parts with their own model and picture. Prints the frame time once the
//! view has settled, the memory the process uses, and how many materials,
//! meshes and pictures the view made.
//!
//! `cargo test --release -p playlibre-engine --test render_cost -- --ignored --nocapture`
//!
//! `RC_SHOT=view.png` also saves what the view drew, `RC_FRAMES=240` draws
//! fewer frames, `RC_LEVEL=1` draws at that graphics level, and `RC_NO_DECALS=1` or `RC_NO_MODELS=1` leaves those out, and `RC_PHONE=1` draws
//! as a phone or tablet at level 5 does.

use std::io::Write;
use std::time::{Duration, Instant};

use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::time::TimeUpdateStrategy;
use bevy::window::ExitCondition;
use playlibre_engine::view::{CameraRig, EnginePlugin, EngineState, RenderPlugin};
use playlibre_instance::{ClassKind, InstanceTree, NormalId, Property, PropertyValue};

const DT: f32 = 1.0 / 60.0;

/// A PNG of `side` by `side` pixels, made from `pixel`.
fn png(side: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend((data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend(kind);
        out.extend(data);
        let mut crc = !0u32;
        for b in &out[start..] {
            crc ^= u32::from(*b);
            for _ in 0..8 {
                crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
            }
        }
        out.extend((!crc).to_be_bytes());
    }
    let mut raw = Vec::new();
    for y in 0..side {
        raw.push(0);
        for x in 0..side {
            raw.extend(pixel(x, y));
        }
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    z.write_all(&raw).unwrap();
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::new();
    header.extend(side.to_be_bytes());
    header.extend(side.to_be_bytes());
    header.extend([8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &z.finish().unwrap());
    chunk(&mut out, b"IEND", &[]);
    out
}

/// A ball of about 2,000 triangles as an .obj file.
fn ball_obj() -> String {
    let (rings, segments) = (24, 42);
    let mut text = String::new();
    for r in 0..=rings {
        let a = std::f32::consts::PI * r as f32 / rings as f32;
        for s in 0..=segments {
            let b = std::f32::consts::TAU * s as f32 / segments as f32;
            let (x, y, z) = (a.sin() * b.cos(), a.cos(), a.sin() * b.sin());
            text += &format!(
                "v {x} {y} {z}\nvn {x} {y} {z}\nvt {} {}\n",
                s as f32 / segments as f32,
                r as f32 / rings as f32
            );
        }
    }
    let at = |r: usize, s: usize| r * (segments + 1) + s + 1;
    for r in 0..rings {
        for s in 0..segments {
            let (a, b, c, d) = (at(r, s), at(r, s + 1), at(r + 1, s + 1), at(r + 1, s));
            text +=
                &format!("f {a}/{a}/{a} {b}/{b}/{b} {c}/{c}/{c}\nf {a}/{a}/{a} {c}/{c}/{c} {d}/{d}/{d}\n");
        }
    }
    text
}

fn map(dir: &std::path::Path) -> InstanceTree {
    let pictures: Vec<String> = (0..3u32)
        .map(|n| {
            let path = dir.join(format!("decal{n}.png"));
            std::fs::write(&path, png(512, |x, y| [(x * (n + 1)) as u8, y as u8, (x ^ y) as u8, 255]))
                .unwrap();
            format!("decal{n}.png")
        })
        .collect();
    let model = dir.join("ball.obj");
    std::fs::write(&model, ball_obj()).unwrap();
    let skin = dir.join("skin.png");
    std::fs::write(&skin, png(1024, |x, y| [x as u8, (x + y) as u8, y as u8, 255])).unwrap();

    // The game's files are found in its folder.
    playlibre_engine::pictures::set_folders(vec![dir.to_path_buf()]);
    let mut tree = InstanceTree::new();
    let ws = tree.workspace();
    let v = |x, y, z| PropertyValue::Vec3(playlibre_instance::Vec3::new(x, y, z));
    let floor = tree.create(ClassKind::Part, "Floor").unwrap();
    tree.set_parent(floor, Some(ws)).unwrap();
    tree.set(floor, Property::Size, v(1200.0, 1.0, 1200.0)).unwrap();
    tree.set(floor, Property::Position, v(0.0, -0.5, 0.0)).unwrap();
    let materials = [
        playlibre_instance::Material::Plastic,
        playlibre_instance::Material::Wood,
        playlibre_instance::Material::Brick,
        playlibre_instance::Material::SmoothPlastic,
        playlibre_instance::Material::Metal,
    ];
    let mut n = 0usize;
    for m in 0..1500 {
        let group = tree.create(ClassKind::Model, format!("House{m}")).unwrap();
        tree.set_parent(group, Some(ws)).unwrap();
        let (x, z) = ((m % 40) as f32 * 28.0 - 560.0, (m / 40) as f32 * 28.0 - 560.0);
        for p in 0..20 {
            let part = tree.create(ClassKind::Part, format!("Wall{p}")).unwrap();
            tree.set_parent(part, Some(group)).unwrap();
            tree.set(part, Property::Position, v(x + (p % 5) as f32 * 3.0, 1.0 + (p / 5) as f32 * 2.0, z))
                .unwrap();
            tree.set(part, Property::Size, v(3.0, 2.0, 1.0)).unwrap();
            tree.set(part, Property::Anchored, PropertyValue::Bool(true)).unwrap();
            let c = (n * 37 % 24) as f32 / 24.0;
            tree.set(
                part,
                Property::Color,
                PropertyValue::Color3(playlibre_instance::Color3::new(c, 1.0 - c, 0.5)),
            )
            .unwrap();
            tree.set(
                part,
                Property::Material,
                PropertyValue::Enum(playlibre_instance::EnumValue::Material(materials[n % materials.len()])),
            )
            .unwrap();
            if p == 0 && std::env::var_os("RC_NO_DECALS").is_none() {
                let decal = tree.create(ClassKind::Decal, "Sign").unwrap();
                tree.set_parent(decal, Some(part)).unwrap();
                tree.set(decal, Property::Texture, PropertyValue::String(pictures[m % 3].clone())).unwrap();
                tree.set(
                    decal,
                    Property::Face,
                    PropertyValue::Enum(playlibre_instance::EnumValue::NormalId(NormalId::Front)),
                )
                .unwrap();
            }
            n += 1;
        }
        if m % 5 < 2 && std::env::var_os("RC_NO_MODELS").is_none() {
            let ball = tree.create(ClassKind::Part, "Statue").unwrap();
            tree.set_parent(ball, Some(group)).unwrap();
            tree.set(ball, Property::Position, v(x + 6.0, 3.0, z + 6.0)).unwrap();
            tree.set(ball, Property::Size, v(4.0, 4.0, 4.0)).unwrap();
            tree.set(ball, Property::Anchored, PropertyValue::Bool(true)).unwrap();
            tree.set(ball, Property::MeshId, PropertyValue::String("ball.obj".into())).unwrap();
            tree.set(ball, Property::MeshTexture, PropertyValue::String("skin.png".into())).unwrap();
        }
    }
    tree
}

/// The memory the process holds now, in MiB (the software renderer's
/// "graphics memory" is in it too).
fn resident_mib() -> f64 {
    let statm = std::fs::read_to_string("/proc/self/statm").unwrap_or_default();
    let pages: f64 = statm.split_whitespace().nth(1).and_then(|p| p.parse().ok()).unwrap_or(0.0);
    pages * 4096.0 / (1024.0 * 1024.0)
}

#[test]
#[ignore = "draws with the machine's graphics; run by hand"]
fn a_decorated_map_draws_cheaply() {
    let dir = std::env::temp_dir().join(format!("playlibre-render-cost-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let start = resident_mib();
    let tree = map(&dir);
    let before = resident_mib();
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .build()
            .disable::<bevy::winit::WinitPlugin>()
            .disable::<bevy::audio::AudioPlugin>()
            .set(WindowPlugin { primary_window: None, exit_condition: ExitCondition::DontExit, ..default() }),
    )
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(DT as f64)))
    .insert_non_send(EngineState::new(tree))
    // A fixed level, so slow frames here don't turn things off part way.
    .insert_resource(if std::env::var_os("RC_PHONE").is_some() {
        playlibre_engine::view::GraphicsQuality::new(false, 5).phone()
    } else {
        playlibre_engine::view::GraphicsQuality::new(
            false,
            std::env::var("RC_LEVEL").ok().and_then(|v| v.parse().ok()).unwrap_or(6),
        )
    })
    .add_plugins((EnginePlugin, RenderPlugin))
    .add_systems(Update, aim_off_screen);
    app.finish();
    app.cleanup();

    let mut frames = Vec::new();
    let count: usize = std::env::var("RC_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(420);
    for _ in 0..count {
        let t = Instant::now();
        app.update();
        frames.push(t.elapsed());
    }
    let settled = &frames[frames.len() - 120..];
    let mean = settled.iter().sum::<Duration>() / settled.len() as u32;
    let mut sorted = settled.to_vec();
    sorted.sort();
    let world = app.world();
    let materials = world.resource::<Assets<StandardMaterial>>().len();
    let meshes = world.resource::<Assets<Mesh>>().len();
    let images = world.resource::<Assets<Image>>();
    let picture_bytes: usize = images.iter().filter_map(|(_, i)| i.data.as_ref().map(Vec::len)).sum();
    let (parts, merged, clusters) = playlibre_engine::view::cluster_counts();
    let first: Duration = frames[..120].iter().sum();
    let sync = playlibre_engine::view::take_sync_seconds();
    println!(
        "render cost: settled frame {:.2} ms (95th {:.2} ms), first 120 frames {:.0} ms; \
         copying parts in {:.0} ms in all; \
         memory {:.0} MiB (map {:.0}, view +{:.0}); {materials} materials, {meshes} meshes, {} pictures \
         ({:.1} MiB of picture data kept in RAM); {parts} parts, {merged} merged into {clusters} clusters",
        mean.as_secs_f64() * 1e3,
        sorted[sorted.len() * 95 / 100].as_secs_f64() * 1e3,
        first.as_secs_f64() * 1e3,
        sync * 1e3,
        resident_mib(),
        before - start,
        resident_mib() - before,
        images.len(),
        picture_bytes as f64 / (1024.0 * 1024.0),
    );
    // RC_SHOT=file.png saves what the view drew, to compare by eye.
    if let Some(path) = std::env::var_os("RC_SHOT") {
        let target = app.world().resource::<Target>().0.clone();
        app.world_mut()
            .spawn(bevy::render::view::screenshot::Screenshot::image(target))
            .observe(bevy::render::view::screenshot::save_to_disk(std::path::PathBuf::from(path)));
        for _ in 0..10 {
            app.update();
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The picture the view draws into.
#[derive(Resource)]
struct Target(Handle<Image>);

/// Points the view's camera at a picture instead of a window, over the
/// middle of the map.
fn aim_off_screen(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut cameras: Query<(Entity, &mut Transform), With<CameraRig>>,
    mut done: Local<bool>,
) {
    for (entity, mut transform) in &mut cameras {
        *transform =
            Transform::from_xyz(-60.0, 40.0, -60.0).looking_at(Vec3::new(120.0, 0.0, 120.0), Vec3::Y);
        if !*done {
            let image = images.add(Image::new_target_texture(480, 270, TextureFormat::Rgba8UnormSrgb, None));
            commands.entity(entity).insert(RenderTarget::Image(image.clone().into()));
            commands.insert_resource(Target(image));
            *done = true;
        }
    }
}
