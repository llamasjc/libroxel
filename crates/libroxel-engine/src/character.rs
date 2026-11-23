//! The player's character: a model in the workspace that scripts can see,
//! moved by input instead of by physics forces.

use libroxel_instance::{
    CFrame, ClassKind, Color3, EnumValue, InstanceId, InstanceTree, Material, PartShape, Property,
    PropertyValue, Rotation, Vec3,
};
use libroxel_script::names::character as names;

use crate::physics::Physics;

/// The size of the character's collision box.
pub const BODY_SIZE: Vec3 = Vec3::new(2.0, 4.0, 1.5);
const HEAD_SIZE: Vec3 = Vec3::new(1.6, 1.4, 1.4);
const DEFAULT_SPAWN: Vec3 = Vec3::new(0.0, 10.0, 0.0);

/// What the player is asking the character to do this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CharacterInput {
    /// Desired direction in world space. Y is ignored; length is clamped to 1.
    pub move_dir: Vec3,
    pub jump: bool,
    /// The forward (1) and back (-1) keys, for driving a vehicle seat.
    pub throttle: f32,
    /// The right (1) and left (-1) keys, for driving a vehicle seat.
    pub steer: f32,
    /// The way to face (yaw, in degrees), in first person.
    pub face: Option<f32>,
}

/// The instances that make up a character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Character {
    pub model: InstanceId,
    pub root: InstanceId,
    pub head: InstanceId,
    pub humanoid: InstanceId,
    pub limbs: Limbs,
}

// ---------------------------------------------------------------------------
// The body: a blocky figure drawn around the (invisible) root part.
// ---------------------------------------------------------------------------

/// The parts of a character's body other than its root and head. They are
/// anchored and don't collide; [`pose_rig`] puts them in place every frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limbs {
    pub torso: InstanceId,
    pub left_arm: InstanceId,
    pub right_arm: InstanceId,
    pub left_leg: InstanceId,
    pub right_leg: InstanceId,
    pub left_eye: InstanceId,
    pub right_eye: InstanceId,
}

/// Part names, which scripts can find with `FindFirstChild`.
pub mod part_names {
    pub const HEAD: &str = "Head";
    pub const TORSO: &str = "Torso";
    pub const LEFT_ARM: &str = "Left Arm";
    pub const RIGHT_ARM: &str = "Right Arm";
    pub const LEFT_LEG: &str = "Left Leg";
    pub const RIGHT_LEG: &str = "Right Leg";
    pub const LEFT_EYE: &str = "LeftEye";
    pub const RIGHT_EYE: &str = "RightEye";
}

/// The colours a body is made in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyColors {
    pub skin: Color3,
    pub shirt: Color3,
    pub pants: Color3,
}

/// The avatar's colours, hats and front view, shared with the website.
pub struct Avatar {
    pub skins: Vec<[u8; 3]>,
    pub clothes: Vec<[u8; 3]>,
    /// Shirt colours, handed out to players in turn by their user id.
    pub shirts: Vec<[u8; 3]>,
    pub skin: [u8; 3],
    pub pants: [u8; 3],
    pub hat_color: [u8; 3],
    /// The colour of hat parts that aren't in the hat's colour.
    pub dark: [u8; 3],
    hats: Vec<Vec<HatPiece>>,
    pub front: Front,
}

/// One part of a [`Hat`].
#[derive(Debug, Clone, PartialEq)]
pub struct HatPiece {
    pub name: String,
    pub size: [f32; 3],
    /// From the middle of the head.
    pub at: [f32; 3],
    /// Degrees about Z.
    pub turn: f32,
    pub shape: PartShape,
    pub glow: bool,
    /// In the hat's colour, or else [`Avatar::dark`].
    pub tinted: bool,
}

/// How the Avatar tab draws the figure from the front.
pub struct Front {
    pub width: f32,
    pub height: f32,
    pub scale: f32,
    pub hips: f32,
    pub head: f32,
    pub edge: f32,
    pub eye: [u8; 3],
    /// Middle, size, and which colour: skin, shirt, pants or eye.
    pub blocks: Vec<([f32; 2], [f32; 2], String)>,
}

/// The avatar, from web/avatar.json.
pub fn avatar() -> &'static Avatar {
    static AVATAR: std::sync::LazyLock<Avatar> = std::sync::LazyLock::new(|| {
        let text = include_str!("../../../web/avatar.json");
        let json = libroxel_instance::serde_json::from_str(text).expect("avatar.json is JSON");
        Avatar::from_json(&json).expect("avatar.json has everything")
    });
    &AVATAR
}

impl Avatar {
    fn from_json(j: &libroxel_instance::serde_json::Value) -> Option<Avatar> {
        use libroxel_instance::serde_json::Value;
        let num = |v: &Value| v.as_f64().map(|n| n as f32);
        let byte = |v: &Value| v.as_u64().filter(|n| *n <= 255).map(|n| n as u8);
        let rgb = |v: &Value| -> Option<[u8; 3]> {
            let a = v.as_array().filter(|a| a.len() == 3)?;
            Some([byte(&a[0])?, byte(&a[1])?, byte(&a[2])?])
        };
        let floats = |v: &Value, n: usize| -> Option<Vec<f32>> {
            let a = v.as_array().filter(|a| a.len() == n)?;
            a.iter().map(num).collect()
        };
        let colours =
            |key: &str| -> Option<Vec<[u8; 3]>> { j.get(key)?.as_array()?.iter().map(rgb).collect() };
        let mut hats = Vec::new();
        for (hat, entry) in Hat::ALL.into_iter().zip(j.get("hats")?.as_array()?) {
            if entry.get("name")?.as_str()? != hat.name() {
                return None;
            }
            let mut pieces = Vec::new();
            for p in entry.get("parts")?.as_array()? {
                let flag = |key: &str| p.get(key).and_then(Value::as_bool).unwrap_or(false);
                let size = floats(p.get("size")?, 3)?;
                let at = floats(p.get("at")?, 3)?;
                pieces.push(HatPiece {
                    name: p.get("name")?.as_str()?.to_owned(),
                    size: [size[0], size[1], size[2]],
                    at: [at[0], at[1], at[2]],
                    turn: p.get("turn").and_then(num).unwrap_or(0.0),
                    shape: match p.get("shape")?.as_str()? {
                        "Ball" => PartShape::Ball,
                        "Cylinder" => PartShape::Cylinder,
                        "Wedge" => PartShape::Wedge,
                        _ => PartShape::Block,
                    },
                    glow: flag("glow"),
                    tinted: flag("tinted"),
                });
            }
            hats.push(pieces);
        }
        if hats.len() != Hat::ALL.len() {
            return None;
        }
        let f = j.get("front")?;
        let mut blocks = Vec::new();
        for b in f.get("blocks")?.as_array()? {
            let (at, size) = (floats(b.get("at")?, 2)?, floats(b.get("size")?, 2)?);
            blocks.push(([at[0], at[1]], [size[0], size[1]], b.get("color")?.as_str()?.to_owned()));
        }
        let fnum = |key: &str| f.get(key).and_then(num);
        Some(Avatar {
            skins: colours("skins")?,
            clothes: colours("clothes")?,
            shirts: colours("shirts")?,
            skin: rgb(j.get("skin")?)?,
            pants: rgb(j.get("pants")?)?,
            hat_color: rgb(j.get("hat_color")?)?,
            dark: rgb(j.get("dark")?)?,
            hats,
            front: Front {
                width: fnum("width")?,
                height: fnum("height")?,
                scale: fnum("scale")?,
                hips: fnum("hips")?,
                head: fnum("head")?,
                edge: fnum("edge")?,
                eye: rgb(f.get("eye")?)?,
                blocks,
            },
        })
    }
}

fn color(c: [u8; 3]) -> Color3 {
    Color3::from_rgb8(c[0], c[1], c[2])
}

impl BodyColors {
    /// The usual look, with the shirt for player number `n`.
    pub fn for_player(n: usize) -> BodyColors {
        let a = avatar();
        BodyColors { skin: color(a.skin), shirt: color(a.shirts[n % a.shirts.len()]), pants: color(a.pants) }
    }
}

/// Something to wear on the head. Every one is made of plain parts in an
/// accessory, so scripts and the editor can change it like anything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Hat {
    #[default]
    None,
    Cap,
    TallHat,
    Crown,
    Halo,
    Antenna,
}

impl Hat {
    pub const ALL: [Hat; 6] = [Hat::None, Hat::Cap, Hat::TallHat, Hat::Crown, Hat::Halo, Hat::Antenna];

    /// What it's called, on screen and as the accessory's name.
    pub fn name(self) -> &'static str {
        match self {
            Hat::None => "No hat",
            Hat::Cap => "Cap",
            Hat::TallHat => "Tall hat",
            Hat::Crown => "Crown",
            Hat::Halo => "Halo",
            Hat::Antenna => "Antenna",
        }
    }

    /// The hat called `name`, as [`name`](Self::name) gives it.
    pub fn named(name: &str) -> Hat {
        Hat::ALL.into_iter().find(|h| h.name() == name).unwrap_or_default()
    }

    /// The parts it's made of.
    pub fn parts(self) -> &'static [HatPiece] {
        &avatar().hats[self as usize]
    }
}

/// One part of a [`CustomHat`].
#[derive(Debug, Clone, PartialEq)]
pub struct HatPart {
    pub size: Vec3,
    /// From the middle of the head.
    pub offset: Vec3,
    /// Turned this many degrees about X, Y and Z.
    pub turn: Vec3,
    pub shape: PartShape,
    pub color: Color3,
    pub neon: bool,
}

/// A hat someone made in the editor and shared in the catalog.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomHat {
    pub name: String,
    pub parts: Vec<HatPart>,
}

impl CustomHat {
    /// The most parts a hat may have.
    pub const MAX_PARTS: usize = 32;
    /// How far (in studs) from the middle of the head a part may be, and how big.
    pub const REACH: f32 = 6.0;

    /// Makes a hat from the parts under `model` (a model, a folder or an
    /// accessory), or from `model` itself if it's a part. It sits on the
    /// head so that the bottom of the parts' box rests on top of the head,
    /// centred over it.
    pub fn from_parts(tree: &InstanceTree, model: InstanceId) -> Result<CustomHat, String> {
        let mut parts = Vec::new();
        let mut found = Vec::new();
        if tree.class(model).is_ok_and(|c| c.is_physical()) {
            found.push(model);
        }
        found.extend(
            tree.descendants(model)
                .unwrap_or_default()
                .into_iter()
                .filter(|d| tree.class(*d).is_ok_and(|c| c.is_physical())),
        );
        if found.is_empty() {
            return Err("there are no parts in it".into());
        }
        if found.len() > Self::MAX_PARTS {
            return Err(format!("a hat has at most {} parts", Self::MAX_PARTS));
        }
        let (mut low, mut high) =
            (Vec3::new(f32::MAX, f32::MAX, f32::MAX), Vec3::new(f32::MIN, f32::MIN, f32::MIN));
        let mut pieces = Vec::new();
        for id in found {
            let get = |p| tree.get_vec3(id, p).unwrap_or(Vec3::ZERO);
            let (pos, size, turn) =
                (get(Property::Position), get(Property::Size), get(Property::Orientation));
            let rotation = Rotation::from_orientation(turn);
            for corner in 0..8 {
                let pick = |bit: u32, v: f32| if corner & (1 << bit) == 0 { -v * 0.5 } else { v * 0.5 };
                let at = pos + rotation.apply(Vec3::new(pick(0, size.x), pick(1, size.y), pick(2, size.z)));
                low = Vec3::new(low.x.min(at.x), low.y.min(at.y), low.z.min(at.z));
                high = Vec3::new(high.x.max(at.x), high.y.max(at.y), high.z.max(at.z));
            }
            let shape = match tree.get(id, Property::Shape) {
                Ok(PropertyValue::Enum(EnumValue::Shape(shape))) => shape,
                _ => PartShape::Block,
            };
            let color = match tree.get(id, Property::Color) {
                Ok(PropertyValue::Color3(c)) => c,
                _ => Color3::WHITE,
            };
            let neon = matches!(
                tree.get(id, Property::Material),
                Ok(PropertyValue::Enum(EnumValue::Material(Material::Neon)))
            );
            pieces.push((pos, size, turn, shape, color, neon));
        }
        // The bottom of the hat sits on the top of the head.
        let seat = Vec3::new((low.x + high.x) * 0.5, low.y, (low.z + high.z) * 0.5);
        let head_top = Vec3::new(0.0, HEAD_SIZE.y * 0.5, 0.0);
        for (pos, size, turn, shape, color, neon) in pieces {
            parts.push(HatPart { size, offset: pos - seat + head_top, turn, shape, color, neon });
        }
        let name = tree.name(model).unwrap_or("Hat").to_owned();
        let hat = CustomHat { name, parts };
        hat.check()?;
        Ok(hat)
    }

    /// Whether every part is in reach and of a sensible size.
    pub fn check(&self) -> Result<(), String> {
        if self.parts.is_empty() || self.parts.len() > Self::MAX_PARTS {
            return Err(format!("a hat has 1 to {} parts", Self::MAX_PARTS));
        }
        let fits = |v: Vec3, low: f32| {
            [v.x, v.y, v.z].iter().all(|c| c.is_finite() && *c >= low && *c <= Self::REACH)
        };
        for part in &self.parts {
            if !fits(part.size, 0.05) {
                return Err(format!("its parts are at most {} studs across", Self::REACH));
            }
            if !fits(part.offset, -Self::REACH) {
                return Err(format!("it reaches more than {} studs from the head", Self::REACH));
            }
        }
        Ok(())
    }

    pub fn to_json(&self) -> libroxel_instance::serde_json::Value {
        use libroxel_instance::serde_json::json;
        let v = |v: Vec3| json!([v.x, v.y, v.z]);
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let parts: Vec<_> = self
            .parts
            .iter()
            .map(|p| {
                json!({
                    "size": v(p.size),
                    "at": v(p.offset),
                    "turn": v(p.turn),
                    "shape": format!("{:?}", p.shape),
                    "color": [byte(p.color.r), byte(p.color.g), byte(p.color.b)],
                    "neon": p.neon,
                })
            })
            .collect();
        json!({ "name": self.name, "parts": parts })
    }

    /// Reads [`to_json`](Self::to_json)'s object, if it's a hat that fits.
    pub fn from_json(value: &libroxel_instance::serde_json::Value) -> Option<CustomHat> {
        let v = |j: &libroxel_instance::serde_json::Value| {
            let a = j.as_array().filter(|a| a.len() == 3)?;
            let f = |i: usize| a[i].as_f64().map(|n| n as f32);
            Some(Vec3::new(f(0)?, f(1)?, f(2)?))
        };
        let parts = value.get("parts")?.as_array()?;
        let mut hat = CustomHat {
            name: value.get("name").and_then(|n| n.as_str()).unwrap_or("Hat").chars().take(40).collect(),
            parts: Vec::new(),
        };
        for part in parts.iter().take(Self::MAX_PARTS + 1) {
            let color =
                part.get("color").and_then(|c| c.as_array()).filter(|a| a.len() == 3).and_then(|a| {
                    let n = |i: usize| a[i].as_u64().filter(|n| *n <= 255).map(|n| n as u8);
                    Some(Color3::from_rgb8(n(0)?, n(1)?, n(2)?))
                })?;
            let shape = match part.get("shape").and_then(|s| s.as_str()) {
                Some("Ball") => PartShape::Ball,
                Some("Cylinder") => PartShape::Cylinder,
                Some("Wedge") => PartShape::Wedge,
                _ => PartShape::Block,
            };
            hat.parts.push(HatPart {
                size: v(part.get("size")?)?,
                offset: v(part.get("at")?)?,
                turn: part.get("turn").and_then(v).unwrap_or(Vec3::ZERO),
                shape,
                color,
                neon: part.get("neon").and_then(|n| n.as_bool()).unwrap_or(false),
            });
        }
        hat.check().ok()?;
        Some(hat)
    }
}

/// How a player looks: their body colours and what they wear.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    pub colors: BodyColors,
    pub hat: Hat,
    pub hat_color: Color3,
    /// A hat from the catalog, worn instead of `hat`.
    pub item: Option<std::sync::Arc<CustomHat>>,
}

impl Look {
    /// The usual look for player number `n`: no hat.
    pub fn for_player(n: usize) -> Look {
        Look {
            colors: BodyColors::for_player(n),
            hat: Hat::None,
            hat_color: color(avatar().hat_color),
            item: None,
        }
    }

    /// As a small JSON object, to save or send to a host.
    pub fn to_json(&self) -> libroxel_instance::serde_json::Value {
        let rgb = |c: Color3| {
            let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            libroxel_instance::serde_json::json!([byte(c.r), byte(c.g), byte(c.b)])
        };
        let mut json = libroxel_instance::serde_json::json!({
            "skin": rgb(self.colors.skin),
            "shirt": rgb(self.colors.shirt),
            "pants": rgb(self.colors.pants),
            "hat": self.hat.name(),
            "hat_color": rgb(self.hat_color),
        });
        if let Some(item) = &self.item {
            json["item"] = item.to_json();
        }
        json
    }

    /// Reads [`to_json`](Self::to_json)'s object; anything missing or wrong
    /// comes from `fallback`.
    pub fn from_json(value: &libroxel_instance::serde_json::Value, fallback: Look) -> Look {
        let rgb = |key: &str, or: Color3| {
            value
                .get(key)
                .and_then(|v| v.as_array())
                .filter(|a| a.len() == 3)
                .and_then(|a| {
                    let n = |i: usize| a[i].as_u64().filter(|n| *n <= 255).map(|n| n as u8);
                    Some(Color3::from_rgb8(n(0)?, n(1)?, n(2)?))
                })
                .unwrap_or(or)
        };
        Look {
            colors: BodyColors {
                skin: rgb("skin", fallback.colors.skin),
                shirt: rgb("shirt", fallback.colors.shirt),
                pants: rgb("pants", fallback.colors.pants),
            },
            hat: value.get("hat").and_then(|h| h.as_str()).map_or(fallback.hat, Hat::named),
            hat_color: rgb("hat_color", fallback.hat_color),
            item: match value.get("item") {
                Some(item) => CustomHat::from_json(item).map(std::sync::Arc::new),
                None => fallback.item,
            },
        }
    }
}

/// Puts `hat` on the character `model` as an accessory, coloured `color`.
pub fn wear_hat(tree: &mut InstanceTree, model: InstanceId, hat: Hat, color: Color3) -> Option<InstanceId> {
    if hat == Hat::None {
        return None;
    }
    let accessory = tree.create(ClassKind::Accessory, hat.name()).ok()?;
    tree.set_parent(accessory, Some(model)).ok()?;
    for part in hat.parts() {
        let id = tree.create(ClassKind::Part, part.name.as_str()).expect("parts can be created");
        tree.set_parent(id, Some(accessory)).expect("fresh parts can be parented");
        let [x, y, z] = part.size;
        set(tree, id, Property::Size, PropertyValue::Vec3(Vec3::new(x, y, z)));
        let color = if part.tinted { color } else { self::color(avatar().dark) };
        set(tree, id, Property::Color, PropertyValue::Color3(color));
        set(tree, id, Property::Anchored, PropertyValue::Bool(true));
        set(tree, id, Property::CanCollide, PropertyValue::Bool(false));
        set(tree, id, Property::Shape, PropertyValue::Enum(EnumValue::Shape(part.shape)));
        if part.glow {
            set(tree, id, Property::Material, PropertyValue::Enum(EnumValue::Material(Material::Neon)));
        }
        let [x, y, z] = part.at;
        let _ = tree.set_attribute(id, names::HEAD_OFFSET, Some(PropertyValue::Vec3(Vec3::new(x, y, z))));
        if part.turn != 0.0 {
            let turn = Vec3::new(0.0, 0.0, part.turn);
            let _ = tree.set_attribute(id, names::HEAD_ORIENTATION, Some(PropertyValue::Vec3(turn)));
        }
    }
    Some(accessory)
}

/// Puts what `look` wears on the character `model`: its catalog hat if it
/// has one, else its hat.
pub fn wear(tree: &mut InstanceTree, model: InstanceId, look: &Look) -> Option<InstanceId> {
    match &look.item {
        Some(item) => wear_custom_hat(tree, model, item),
        None => wear_hat(tree, model, look.hat, look.hat_color),
    }
}

/// Puts a catalog hat on the character `model` as an accessory.
pub fn wear_custom_hat(tree: &mut InstanceTree, model: InstanceId, hat: &CustomHat) -> Option<InstanceId> {
    let accessory = tree.create(ClassKind::Accessory, hat.name.as_str()).ok()?;
    tree.set_parent(accessory, Some(model)).ok()?;
    for (n, part) in hat.parts.iter().enumerate() {
        let id = tree.create(ClassKind::Part, format!("Part{}", n + 1)).expect("parts can be created");
        tree.set_parent(id, Some(accessory)).expect("fresh parts can be parented");
        set(tree, id, Property::Size, PropertyValue::Vec3(part.size));
        set(tree, id, Property::Color, PropertyValue::Color3(part.color));
        set(tree, id, Property::Anchored, PropertyValue::Bool(true));
        set(tree, id, Property::CanCollide, PropertyValue::Bool(false));
        set(tree, id, Property::Shape, PropertyValue::Enum(EnumValue::Shape(part.shape)));
        if part.neon {
            set(tree, id, Property::Material, PropertyValue::Enum(EnumValue::Material(Material::Neon)));
        }
        let _ = tree.set_attribute(id, names::HEAD_OFFSET, Some(PropertyValue::Vec3(part.offset)));
        if part.turn != Vec3::ZERO {
            let _ = tree.set_attribute(id, names::HEAD_ORIENTATION, Some(PropertyValue::Vec3(part.turn)));
        }
    }
    Some(accessory)
}

/// Gives `model` a body colours object holding `colors`.
fn add_body_colors(tree: &mut InstanceTree, model: InstanceId, colors: BodyColors) {
    let Ok(id) = tree.create(ClassKind::BodyColors, names::BODY_COLORS) else { return };
    let _ = tree.set_parent(id, Some(model));
    for (property, color) in [
        (Property::HeadColor, colors.skin),
        (Property::LeftArmColor, colors.skin),
        (Property::RightArmColor, colors.skin),
        (Property::TorsoColor, colors.shirt),
        (Property::LeftLegColor, colors.pants),
        (Property::RightLegColor, colors.pants),
    ] {
        set(tree, id, property, PropertyValue::Color3(color));
    }
}

/// Colours the head and limbs from the model's body colours, and puts
/// its accessories' parts on the head, turned the way it is.
fn dress(tree: &mut InstanceTree, model: InstanceId, head: InstanceId, limbs: &Limbs) {
    let Ok(children) = tree.children(model).map(<[InstanceId]>::to_vec) else { return };
    for child in children {
        match tree.class(child) {
            Ok(ClassKind::BodyColors) => {
                for (property, part) in [
                    (Property::HeadColor, head),
                    (Property::TorsoColor, limbs.torso),
                    (Property::LeftArmColor, limbs.left_arm),
                    (Property::RightArmColor, limbs.right_arm),
                    (Property::LeftLegColor, limbs.left_leg),
                    (Property::RightLegColor, limbs.right_leg),
                ] {
                    if let Some(color) = tree.get_color(child, property)
                        && tree.get_color(part, Property::Color) != Some(color)
                    {
                        let _ = tree.set(part, Property::Color, PropertyValue::Color3(color));
                    }
                }
            }
            Ok(ClassKind::Accessory) => {
                let Some(at) = tree.get_vec3(head, Property::Position) else { continue };
                let turn = Rotation::from_orientation(
                    tree.get_vec3(head, Property::Orientation).unwrap_or_default(),
                );
                let parts = tree.children(child).map(<[InstanceId]>::to_vec).unwrap_or_default();
                for part in parts {
                    let attribute = |name| match tree.get_attribute(part, name) {
                        Ok(Some(PropertyValue::Vec3(v))) => Some(v),
                        _ => None,
                    };
                    let Some(offset) = attribute(names::HEAD_OFFSET) else { continue };
                    let tilt = attribute(names::HEAD_ORIENTATION).unwrap_or_default();
                    let pos = at + turn.apply(offset);
                    let orientation = (turn * Rotation::from_orientation(tilt)).to_orientation();
                    if tree.get_vec3(part, Property::Position) != Some(pos) {
                        let _ = tree.set(part, Property::Position, PropertyValue::Vec3(pos));
                    }
                    if tree.get_vec3(part, Property::Orientation) != Some(orientation) {
                        let _ = tree.set(part, Property::Orientation, PropertyValue::Vec3(orientation));
                    }
                }
            }
            _ => {}
        }
    }
}

const TORSO_SIZE: Vec3 = Vec3::new(2.2, 1.8, 1.1);
const ARM_SIZE: Vec3 = Vec3::new(0.8, 1.9, 0.9);
const LEG_SIZE: Vec3 = Vec3::new(0.95, 2.0, 1.0);
const EYE_SIZE: Vec3 = Vec3::new(0.25, 0.35, 0.05);
/// Where things sit relative to the root part's middle (the hips).
const SHOULDER: Vec3 = Vec3::new(1.5, 1.7, 0.0);
const HIP: Vec3 = Vec3::new(0.5, 0.0, 0.0);
const TORSO_AT: Vec3 = Vec3::new(0.0, 0.9, 0.0);
const HEAD_AT: Vec3 = Vec3::new(0.0, 2.55, 0.0);
/// Where the head turns and nods about: the top of the torso.
const NECK: Vec3 = Vec3::new(0.0, 1.8, 0.0);
const EYE_AT: Vec3 = Vec3::new(0.35, 2.65, -0.71);
/// Where a held tool's handle goes, relative to the shoulder, with the arm
/// hanging straight down (it is turned with the arm).
const GRIP: Vec3 = Vec3::new(0.0, -2.3, 0.1);

/// Sounds every character has in its root part: its name, file and volume.
/// The session plays `Jumping` on a jump and `Splash` on falling into water.
pub const CHARACTER_SOUNDS: &[(&str, &str, f64)] =
    &[("Jumping", "libroxel://jump", 0.3), ("Splash", "libroxel://splash", 0.5)];

/// How long swinging a tool takes, in seconds.
pub const SLASH_TIME: f32 = 0.35;

/// How far a tool swing has the arm from pointing forward, in radians,
/// `left` seconds before the swing ends: up, down hard, then back.
pub fn slash_angle(left: f32) -> f32 {
    let p = (1.0 - left / SLASH_TIME).clamp(0.0, 1.0);
    let degrees = if p < 0.3 {
        55.0 * p / 0.3
    } else if p < 0.55 {
        55.0 - 110.0 * (p - 0.3) / 0.25
    } else {
        -55.0 * (1.0 - (p - 0.55) / 0.45)
    };
    degrees.to_radians()
}

/// Where a held tool's handle goes, relative to the root, and how far it
/// tips forward (radians about the character's sideways axis), with the arm
/// pointing forward and turned `slash` further up.
pub fn hand_offset(slash: f32) -> (Vec3, f32) {
    (SHOULDER + Rotation::about_x(90f32.to_radians() + slash).apply(GRIP), slash)
}

/// How the body is held this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Pose {
    /// How far arms and legs swing from straight down, in radians.
    pub swing: f32,
    pub sitting: bool,
    /// The right arm points forward, holding a tool.
    pub holding: bool,
    /// While holding a tool: how much further up the arm is, in radians
    /// (see [`slash_angle`]).
    pub slash: f32,
    /// What the animations playing on it say, and whether they move the
    /// legs too (if not, the legs walk as usual).
    pub anim: Option<(libroxel_instance::animation::Pose, bool)>,
}

/// Adds a head and limbs to `model`, coloured `colors`. Returns the head
/// and the limbs; place them with [`pose_rig`].
pub fn build_rig(tree: &mut InstanceTree, model: InstanceId, colors: BodyColors) -> (InstanceId, Limbs) {
    use part_names::*;
    let mut make = |name: &str, size: Vec3, color: Color3| {
        let id = tree.create(ClassKind::Part, name).expect("parts can be created");
        tree.set_parent(id, Some(model)).expect("fresh parts can be parented");
        set(tree, id, Property::Size, PropertyValue::Vec3(size));
        set(tree, id, Property::Color, PropertyValue::Color3(color));
        set(tree, id, Property::Anchored, PropertyValue::Bool(true));
        set(tree, id, Property::CanCollide, PropertyValue::Bool(false));
        id
    };
    let head = make(HEAD, HEAD_SIZE, colors.skin);
    let dark = Color3::from_rgb8(30, 30, 40);
    let limbs = Limbs {
        torso: make(TORSO, TORSO_SIZE, colors.shirt),
        left_arm: make(LEFT_ARM, ARM_SIZE, colors.skin),
        right_arm: make(RIGHT_ARM, ARM_SIZE, colors.skin),
        left_leg: make(LEFT_LEG, LEG_SIZE, colors.pants),
        right_leg: make(RIGHT_LEG, LEG_SIZE, colors.pants),
        left_eye: make(LEFT_EYE, EYE_SIZE, dark),
        right_eye: make(RIGHT_EYE, EYE_SIZE, dark),
    };
    (head, limbs)
}

/// The joints that hold a body together, named as scripts expect (R6).
pub mod joints {
    pub const ROOT: &str = "RootJoint";
    pub const NECK: &str = "Neck";
    pub const LEFT_SHOULDER: &str = "Left Shoulder";
    pub const RIGHT_SHOULDER: &str = "Right Shoulder";
    pub const LEFT_HIP: &str = "Left Hip";
    pub const RIGHT_HIP: &str = "Right Hip";
    pub const ALL: [&str; 6] = [ROOT, NECK, LEFT_SHOULDER, RIGHT_SHOULDER, LEFT_HIP, RIGHT_HIP];
}

/// How a joint of the body sits at rest: its name, its C0 (on Part0) and C1
/// (on Part1). The turns are the ones R6 bodies use, so a script's
/// `CFrame.Angles` on a C0 turns a limb the way it would there (a right
/// shoulder's Z turn lifts the arm forward).
fn joint_rest(name: &str) -> Option<(CFrame, CFrame)> {
    let flip = |v: Vec3| Vec3::new(-v.x, v.y, v.z);
    let quarter = std::f32::consts::FRAC_PI_2;
    let right = Rotation::about_y(quarter);
    let left = Rotation::about_y(-quarter);
    // Turned so the joint's Z is up: the root's and the neck's.
    let upright =
        Rotation { x: Vec3::new(-1.0, 0.0, 0.0), y: Vec3::new(0.0, 0.0, 1.0), z: Vec3::new(0.0, 1.0, 0.0) };
    let arm_top = Vec3::new(0.0, ARM_SIZE.y * 0.5 - 0.1, 0.0);
    let leg_top = Vec3::new(0.0, LEG_SIZE.y * 0.5, 0.0);
    let cf = CFrame::new;
    Some(match name {
        joints::ROOT => (cf(TORSO_AT, upright), cf(Vec3::ZERO, upright)),
        joints::NECK => (cf(NECK - TORSO_AT, upright), cf(NECK - HEAD_AT, upright)),
        joints::RIGHT_SHOULDER => (cf(SHOULDER - TORSO_AT, right), cf(arm_top, right)),
        joints::LEFT_SHOULDER => (cf(flip(SHOULDER) - TORSO_AT, left), cf(arm_top, left)),
        joints::RIGHT_HIP => (cf(HIP - TORSO_AT, right), cf(leg_top, right)),
        joints::LEFT_HIP => (cf(flip(HIP) - TORSO_AT, left), cf(leg_top, left)),
        _ => return None,
    })
}

/// Gives a body [`build_rig`] made its joints: `RootJoint` in the root part
/// and `Neck`, the shoulders and the hips in the torso, all `Motor6D`s, as
/// on an R6 character.
pub fn add_joints(tree: &mut InstanceTree, root: InstanceId, head: InstanceId, limbs: &Limbs) {
    let list = [
        (joints::ROOT, root, root, limbs.torso),
        (joints::NECK, limbs.torso, limbs.torso, head),
        (joints::LEFT_SHOULDER, limbs.torso, limbs.torso, limbs.left_arm),
        (joints::RIGHT_SHOULDER, limbs.torso, limbs.torso, limbs.right_arm),
        (joints::LEFT_HIP, limbs.torso, limbs.torso, limbs.left_leg),
        (joints::RIGHT_HIP, limbs.torso, limbs.torso, limbs.right_leg),
    ];
    for (name, holder, part0, part1) in list {
        let Some((c0, c1)) = joint_rest(name) else { continue };
        let Ok(motor) = tree.create(ClassKind::Motor6D, name) else { continue };
        set(tree, motor, Property::Part0, PropertyValue::Ref(Some(part0)));
        set(tree, motor, Property::Part1, PropertyValue::Ref(Some(part1)));
        set(tree, motor, Property::C0Position, PropertyValue::Vec3(c0.pos));
        set(tree, motor, Property::C0Orientation, PropertyValue::Vec3(c0.rot.to_orientation()));
        set(tree, motor, Property::C1Position, PropertyValue::Vec3(c1.pos));
        set(tree, motor, Property::C1Orientation, PropertyValue::Vec3(c1.rot.to_orientation()));
        set(tree, motor, Property::MaxVelocity, PropertyValue::Number(0.1));
        let _ = tree.set_parent(motor, Some(holder));
    }
}

/// Whether `joint` is one of the joints [`add_joints`] gives a body, still
/// in its body (the engine poses those itself, not as welds).
pub fn is_body_joint(tree: &InstanceTree, joint: InstanceId) -> bool {
    let Ok(name) = tree.name(joint) else { return false };
    if !joints::ALL.contains(&name) {
        return false;
    }
    let model = tree.parent(joint).ok().flatten().and_then(|holder| tree.parent(holder).ok().flatten());
    model.is_some_and(|m| find_rig(tree, m).is_some())
}

fn part_cframe(tree: &InstanceTree, part: InstanceId) -> Option<CFrame> {
    Some(CFrame::new(
        tree.get_vec3(part, Property::Position)?,
        Rotation::from_orientation(tree.get_vec3(part, Property::Orientation).unwrap_or_default()),
    ))
}

fn place_at(tree: &mut InstanceTree, id: InstanceId, cf: CFrame) {
    let orientation = cf.rot.to_orientation();
    if tree.get_vec3(id, Property::Position) != Some(cf.pos) {
        let _ = tree.set(id, Property::Position, PropertyValue::Vec3(cf.pos));
    }
    if tree.get_vec3(id, Property::Orientation) != Some(orientation) {
        let _ = tree.set(id, Property::Orientation, PropertyValue::Vec3(orientation));
    }
}

/// Puts `part1` where the joint `name` in `holder` holds it on a part at
/// `part0`, with the body turned by `r` about the joint (in the body's
/// own axes). Returns where it went, or nothing if the joint is gone or
/// switched off, or holds something else.
fn joint_place(
    tree: &mut InstanceTree,
    holder: InstanceId,
    name: &str,
    part1: InstanceId,
    part0: CFrame,
    r: Rotation,
) -> Option<CFrame> {
    let joint = tree.find_first_child(holder, name).ok().flatten()?;
    if tree.class(joint) != Ok(ClassKind::Motor6D)
        || tree.get_ref(joint, Property::Part1) != Some(part1)
        || tree.get_bool(joint, Property::Enabled) == Some(false)
    {
        return None;
    }
    let c0 = crate::weld::joint_offset(tree, joint, false);
    let c1 = crate::weld::joint_offset(tree, joint, true);
    // The turn in the joint's own axes (those it has at rest), so it stays
    // the same whatever a script does to C0.
    let rest = joint_rest(name).map_or(Rotation::IDENTITY, |(c0, _)| c0.rot);
    let script =
        match (tree.get_extra(joint, "TransformPosition"), tree.get_extra(joint, "TransformOrientation")) {
            (Ok(Some(PropertyValue::Vec3(p))), Ok(Some(PropertyValue::Vec3(o)))) => {
                Some(CFrame::new(p, Rotation::from_orientation(o)))
            }
            _ => None,
        };
    // As on Roblox, a script's Transform holds for the frame it was set in:
    // scripts that pose a joint set it every frame.
    if script.is_some() {
        let _ = tree.set_extra(joint, "TransformPosition", None);
        let _ = tree.set_extra(joint, "TransformOrientation", None);
    }
    // A Transform a script set takes over from the walk and animations.
    let transform = script.unwrap_or_else(|| CFrame::new(Vec3::ZERO, rest.transpose() * r * rest));
    let angle = tree.get_number(joint, Property::CurrentAngle).unwrap_or(0.0) as f32;
    let turn = CFrame::new(Vec3::ZERO, Rotation::about_z(angle)) * transform;
    let cf = part0 * c0 * turn * c1.inverse();
    place_at(tree, part1, cf);
    Some(cf)
}

/// Finds the head and limbs [`build_rig`] made in `model`, if they are all there.
pub fn find_rig(tree: &InstanceTree, model: InstanceId) -> Option<(InstanceId, Limbs)> {
    use part_names::*;
    let find = |name: &str| tree.find_first_child(model, name).ok().flatten();
    Some((
        find(HEAD)?,
        Limbs {
            torso: find(TORSO)?,
            left_arm: find(LEFT_ARM)?,
            right_arm: find(RIGHT_ARM)?,
            left_leg: find(LEFT_LEG)?,
            right_leg: find(RIGHT_LEG)?,
            left_eye: find(LEFT_EYE)?,
            right_eye: find(RIGHT_EYE)?,
        },
    ))
}

impl Limbs {
    /// Every limb part.
    pub fn parts(&self) -> [InstanceId; 7] {
        [
            self.torso,
            self.left_arm,
            self.right_arm,
            self.left_leg,
            self.right_leg,
            self.left_eye,
            self.right_eye,
        ]
    }
}

/// Puts the head and limbs around `root` (facing its yaw) in `pose`.
pub fn pose_rig(tree: &mut InstanceTree, root: InstanceId, head: InstanceId, limbs: &Limbs, pose: Pose) {
    let Some(at) = tree.get_vec3(root, Property::Position) else { return };
    let yaw = tree.get_vec3(root, Property::Orientation).unwrap_or_default().y;
    let turn = Rotation::about_y(yaw.to_radians());
    let (anim, anim_legs) = match pose.anim {
        Some((a, legs)) => (Some(a), legs),
        None => (None, false),
    };
    // Walking, the hips drop as the legs spread, so both feet stay down.
    let walking = !pose.sitting && !anim_legs;
    let drop = if walking { LEG_SIZE.y * (1.0 - pose.swing.cos()) } else { 0.0 };
    let at = at + Vec3::new(0.0, anim.map_or(0.0, |a| a.bob) - drop, 0.0);
    // A limb swung forward by `tilt` and out to the side by `roll` (radians).
    let limb = |tilt: f32, roll: f32| Rotation::about_x(tilt) * Rotation::about_z(roll);
    let (legs, arms) =
        if pose.sitting { (90f32.to_radians(), 20f32.to_radians()) } else { (pose.swing, pose.swing * 0.7) };
    let (left_leg, right_leg) = if pose.sitting { (legs, legs) } else { (legs, -legs) };
    let (left_arm, right_arm) = if pose.sitting { (arms, arms) } else { (-arms, arms) };
    let mut left_arm = limb(left_arm, 0.0);
    let mut right_arm = limb(right_arm, 0.0);
    let mut left_leg = limb(left_leg, 0.0);
    let mut right_leg = limb(right_leg, 0.0);
    let mut neck = Rotation::IDENTITY;
    if let Some(a) = anim {
        let deg = f32::to_radians;
        // Out to the side is +Z turn for the right side, -Z for the left.
        left_arm = limb(deg(a.left_arm.swing), -deg(a.left_arm.out));
        right_arm = limb(deg(a.right_arm.swing), deg(a.right_arm.out));
        if anim_legs && !pose.sitting {
            left_leg = limb(deg(a.left_leg.swing), -deg(a.left_leg.out));
            right_leg = limb(deg(a.right_leg.swing), deg(a.right_leg.out));
        }
        // A nod tips the head forward, which is back about X for a part above its pivot.
        neck = Rotation::about_y(deg(a.turn)) * Rotation::about_x(-deg(a.nod));
    }
    if pose.holding {
        right_arm = limb(90f32.to_radians() + pose.slash, 0.0);
    }
    // The torso hangs on the root by its RootJoint, the head and limbs on
    // the torso by their joints: Part1 = Part0 * C0 * Transform * C1⁻¹, with
    // the walk and animations as the Transform, so a script that turns a
    // joint's C0 (or its angle) turns the limb on top of them. A joint a
    // script took out leaves its part where it is.
    let root_cf = CFrame::new(at, turn);
    let torso = joint_place(tree, root, joints::ROOT, limbs.torso, root_cf, Rotation::IDENTITY);
    let torso_cf = torso.or_else(|| part_cframe(tree, limbs.torso));
    let mut head_cf = None;
    if let Some(torso_cf) = torso_cf {
        let t = limbs.torso;
        head_cf = joint_place(tree, t, joints::NECK, head, torso_cf, neck);
        joint_place(tree, t, joints::LEFT_SHOULDER, limbs.left_arm, torso_cf, left_arm);
        joint_place(tree, t, joints::RIGHT_SHOULDER, limbs.right_arm, torso_cf, right_arm);
        joint_place(tree, t, joints::LEFT_HIP, limbs.left_leg, torso_cf, left_leg);
        joint_place(tree, t, joints::RIGHT_HIP, limbs.right_leg, torso_cf, right_leg);
    }
    if let Some(head_cf) = head_cf.or_else(|| part_cframe(tree, head)) {
        place_eyes(tree, head_cf, limbs);
    }
    if let Ok(Some(model)) = tree.parent(root) {
        dress(tree, model, head, limbs);
    }
}

fn place_eyes(tree: &mut InstanceTree, head_cf: CFrame, limbs: &Limbs) {
    let flip = |v: Vec3| Vec3::new(-v.x, v.y, v.z);
    place_at(tree, limbs.left_eye, head_cf * CFrame::new(flip(EYE_AT) - HEAD_AT, Rotation::IDENTITY));
    place_at(tree, limbs.right_eye, head_cf * CFrame::new(EYE_AT - HEAD_AT, Rotation::IDENTITY));
}

pub fn stick_to_head(tree: &mut InstanceTree, model: InstanceId) {
    let Some((head, limbs)) = find_rig(tree, model) else { return };
    let Some(head_cf) = part_cframe(tree, head) else { return };
    place_eyes(tree, head_cf, &limbs);
    dress(tree, model, head, &limbs);
}

/// Radians of stride per stud walked.
pub const STRIDE_PER_STUD: f32 = 0.9;

/// How far the legs swing from straight down, `phase` radians into a stride.
pub fn swing_for(phase: f32) -> f32 {
    phase.sin() * 0.6
}

/// The stride after moving `moved` studs in `dt` seconds: on while
/// walking, and easing back to standing still once stopped.
pub fn stride_after(stride: f32, moved: f32, dt: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    if moved > 0.5 * dt {
        return (stride + moved * STRIDE_PER_STUD) % TAU;
    }
    let rest = (stride / PI).round() * PI;
    let eased = stride + (rest - stride) * (1.0 - (-12.0 * dt).exp());
    if (rest - eased).abs() < 1e-3 { rest % TAU } else { eased }
}

/// Where a new character should appear: on top of the first spawn location.
pub fn spawn_point(tree: &InstanceTree) -> Vec3 {
    spawn_point_for(tree, None)
}

fn same_color(a: Color3, b: Color3) -> bool {
    (a.r - b.r).abs() < 0.01 && (a.g - b.g).abs() < 0.01 && (a.b - b.b).abs() < 0.01
}

/// Where a character on a team of colour `team` should appear: the first
/// spawn location for that team, else the first neutral one, else any.
pub fn spawn_point_for(tree: &InstanceTree, team: Option<Color3>) -> Vec3 {
    spawn_location_for(tree, team)
        .and_then(|id| {
            let pos = tree.get_vec3(id, Property::Position)?;
            let size = tree.get_vec3(id, Property::Size)?;
            Some(pos + Vec3::new(0.0, size.y / 2.0 + BODY_SIZE.y / 2.0 + 0.1, 0.0))
        })
        .unwrap_or(DEFAULT_SPAWN)
}

/// The spawn location a character on a team of colour `team` appears on,
/// if the world has any (see [`spawn_point_for`]).
pub fn spawn_location_for(tree: &InstanceTree, team: Option<Color3>) -> Option<InstanceId> {
    let spawns = tree.descendants_of_class(tree.workspace(), ClassKind::SpawnLocation);
    let neutral = |id: InstanceId| tree.get_bool(id, Property::Neutral) != Some(false);
    let for_team = |id: InstanceId| {
        !neutral(id)
            && team.zip(tree.get_color(id, Property::TeamColor)).is_some_and(|(t, c)| same_color(t, c))
    };
    spawns
        .iter()
        .copied()
        .find(|s| for_team(*s))
        .or_else(|| spawns.iter().copied().find(|s| neutral(*s)))
        .or_else(|| spawns.first().copied())
}

fn set(tree: &mut InstanceTree, id: InstanceId, property: Property, value: PropertyValue) {
    tree.set(id, property, value).expect("character properties are valid");
}

impl Character {
    /// Builds a character in the workspace at the spawn point.
    pub fn spawn(tree: &mut InstanceTree) -> Character {
        Self::spawn_named(tree, names::MODEL)
    }

    /// Like [`spawn`](Self::spawn), with the model called `name` (usually the player's name).
    pub fn spawn_named(tree: &mut InstanceTree, name: &str) -> Character {
        Self::spawn_for_team(tree, name, None)
    }

    /// Like [`spawn_named`](Self::spawn_named), at a spawn for the team with colour `team`.
    pub fn spawn_for_team(tree: &mut InstanceTree, name: &str, team: Option<Color3>) -> Character {
        Self::spawn_dressed(tree, name, team, BodyColors::for_player(0))
    }

    /// Like [`spawn_for_team`](Self::spawn_for_team), with the body coloured `colors`.
    pub fn spawn_dressed(
        tree: &mut InstanceTree,
        name: &str,
        team: Option<Color3>,
        colors: BodyColors,
    ) -> Character {
        let look = Look { colors, ..Look::for_player(0) };
        Self::spawn_wearing(tree, name, team, look)
    }

    /// Like [`spawn_dressed`](Self::spawn_dressed), looking like `look`:
    /// coloured by a body colours object, wearing its hat.
    pub fn spawn_wearing(tree: &mut InstanceTree, name: &str, team: Option<Color3>, look: Look) -> Character {
        let colors = look.colors;
        let at = spawn_point_for(tree, team);
        let ws = tree.workspace();
        let make = |tree: &mut InstanceTree, class, name: &str, parent| {
            let id = tree.create(class, name).expect("character classes are creatable");
            tree.set_parent(id, Some(parent)).expect("fresh instances can be parented");
            id
        };
        let model = make(tree, ClassKind::Model, name, ws);
        let root = make(tree, ClassKind::Part, names::ROOT_PART, model);
        set(tree, root, Property::Size, PropertyValue::Vec3(BODY_SIZE));
        set(tree, root, Property::Position, PropertyValue::Vec3(at));
        // The root is the collision box; the body drawn around it is the rig.
        set(tree, root, Property::Transparency, PropertyValue::Number(1.0));
        let (head, limbs) = build_rig(tree, model, colors);
        add_joints(tree, root, head, &limbs);
        // The sounds a character makes by itself.
        for (name, sound, volume) in CHARACTER_SOUNDS {
            let id = make(tree, ClassKind::Sound, name, root);
            set(tree, id, Property::SoundId, PropertyValue::String((*sound).into()));
            set(tree, id, Property::Volume, PropertyValue::Number(*volume));
        }
        let humanoid = make(tree, ClassKind::Humanoid, names::HUMANOID, model);
        make(tree, ClassKind::Animator, libroxel_script::names::animation::ANIMATOR, humanoid);
        // The body is an R6 one (scripts check this to know which joints to look for).
        let _ = tree.set_extra(humanoid, "RigType", Some(PropertyValue::Number(0.0)));
        // New characters start with the starter player's walk speed and jump power.
        if let Some(starter) = tree.service(ClassKind::StarterPlayer) {
            for (from, to) in [
                (Property::CharacterWalkSpeed, Property::WalkSpeed),
                (Property::CharacterJumpPower, Property::JumpPower),
            ] {
                if let Some(n) = tree.get_number(starter, from) {
                    set(tree, humanoid, to, PropertyValue::Number(n));
                }
            }
        }
        add_body_colors(tree, model, colors);
        wear(tree, model, &look);
        let character = Character { model, root, head, humanoid, limbs };
        character.pose(tree, Pose::default());
        character
    }

    /// Changes how the character looks: its body colours and its hat.
    pub fn restyle(&self, tree: &mut InstanceTree, look: Look) {
        let children = tree.children(self.model).map(<[InstanceId]>::to_vec).unwrap_or_default();
        for child in children {
            if matches!(tree.class(child), Ok(ClassKind::BodyColors | ClassKind::Accessory)) {
                let _ = tree.destroy(child);
            }
        }
        add_body_colors(tree, self.model, look.colors);
        wear(tree, self.model, &look);
        self.pose(tree, Pose::default());
    }

    /// The character a model made by [`spawn_dressed`](Self::spawn_dressed)
    /// is, found by its parts' names (as on a computer that was sent it).
    pub fn from_model(tree: &InstanceTree, model: InstanceId) -> Option<Character> {
        let root = tree.find_first_child(model, names::ROOT_PART).ok().flatten()?;
        let humanoid = tree.find_first_child_of_class(model, ClassKind::Humanoid).ok().flatten()?;
        let (head, limbs) = find_rig(tree, model)?;
        Some(Character { model, root, head, humanoid, limbs })
    }

    /// Puts the body around the root in `pose`.
    pub fn pose(&self, tree: &mut InstanceTree, pose: Pose) {
        pose_rig(tree, self.root, self.head, &self.limbs, pose);
    }

    /// Whether the character has died: no health left, or a script took it apart.
    pub fn is_dead(&self, tree: &InstanceTree) -> bool {
        let gone = |id| !tree.exists(id) || tree.is_destroyed(id).unwrap_or(true);
        let detached = |id| !tree.is_descendant_of(id, tree.workspace()).unwrap_or(false);
        gone(self.root)
            || detached(self.root)
            || gone(self.humanoid)
            || tree.get_number(self.humanoid, Property::Health).is_some_and(|h| h <= 0.0)
            || tree
                .get_vec3(self.root, Property::Position)
                .is_some_and(|p| p.y < crate::physics::fallen_parts_height(tree))
    }

    /// Turns input into velocity. Call before [`Physics::step`].
    pub fn drive(&self, tree: &InstanceTree, physics: &mut Physics, input: CharacterInput, dt: f32) {
        drive(tree, physics, self.root, self.humanoid, input, dt);
    }

    /// Keeps the body on the root, standing straight. Call after
    /// [`Physics::step`]; the session poses it properly later in the frame.
    pub fn follow(&self, tree: &mut InstanceTree) {
        self.pose(tree, Pose::default());
    }
}

/// How fast swimmers rise holding Space, and drift up otherwise (studs a second).
const SWIM_UP: f32 = 10.0;
const FLOAT_UP: f32 = 2.0;

/// How quickly a walker speeds up, slows down or changes direction, on the
/// ground and in the air (studs a second, each second): from standing to
/// full walking speed in a few hundredths of a second, so it feels snappy
/// without starting and stopping with a jolt.
const GROUND_GRIP: f32 = 320.0;
const AIR_GRIP: f32 = 160.0;

/// How quickly characters turn to face where they walk (per second).
pub const TURN_RATE: f32 = 14.0;

/// A yaw (in degrees) turned part of the way from `from` towards `to`,
/// the short way round: most of a half turn takes about a fifth of a
/// second, like turning on the spot.
pub fn turn_toward(from: f32, to: f32, dt: f32) -> f32 {
    let diff = (to - from + 180.0).rem_euclid(360.0) - 180.0;
    if diff.abs() < 0.5 {
        return to;
    }
    let step = diff * (1.0 - (-TURN_RATE * dt).exp());
    let turned = from + step;
    // Keep it in -180..180 so it never grows without end.
    (turned + 180.0).rem_euclid(360.0) - 180.0
}

/// The yaw (in degrees) that faces along `dir`.
pub fn yaw_of(dir: Vec3) -> f32 {
    (-dir.x).atan2(-dir.z).to_degrees()
}

/// Moves any humanoid's root part: walking at its walk speed in the input's
/// direction, and jumping when on the ground.
pub fn drive(
    tree: &InstanceTree,
    physics: &mut Physics,
    root: InstanceId,
    humanoid: InstanceId,
    input: CharacterInput,
    dt: f32,
) {
    {
        let walk = tree.get_number(humanoid, Property::WalkSpeed).unwrap_or(16.0) as f32;
        let jump = tree.get_number(humanoid, Property::JumpPower).unwrap_or(50.0) as f32;
        let mut dir = Vec3::new(input.move_dir.x, 0.0, input.move_dir.z);
        if dir.length() > 1.0 {
            dir = dir.normalize_or_zero();
        }
        let mut v = physics.velocity(root);
        if physics.is_in_water(root) {
            // Swimming: a little slower. Swimmers drift up to the top and
            // tread water there; Space swims up faster, and at the top
            // leaps out (onto a bank, say).
            v.x = dir.x * walk * 0.75;
            v.z = dir.z * walk * 0.75;
            let head = tree.get_vec3(root, Property::Position).map(|p| p + Vec3::new(0.0, 1.5, 0.0));
            let surfacing = head.is_some_and(|h| {
                let wet = tree.terrain().and_then(|t| tree.voxels(t).ok()).is_some_and(|w| w.water_at(h));
                !wet
            });
            v.y = match (input.jump, surfacing) {
                (true, true) => jump * 0.8,
                (true, false) => SWIM_UP,
                (false, true) => 0.0,
                (false, false) => FLOAT_UP,
            };
            physics.set_velocity(root, v);
            return;
        }
        let (want_x, want_z) = (dir.x * walk, dir.z * walk);
        let flung = (v.x * v.x + v.z * v.z).sqrt() > walk + 1.0;
        if flung && !physics.is_grounded(root) {
            // Thrown by something (a launch pad, an explosion): keep the
            // momentum in the air, steering only a little.
            v.x += (want_x - v.x) * 0.03;
            v.z += (want_z - v.z) * 0.03;
        } else {
            let grip = if physics.is_grounded(root) { GROUND_GRIP } else { AIR_GRIP };
            let (dx, dz) = (want_x - v.x, want_z - v.z);
            let gap = (dx * dx + dz * dz).sqrt();
            let most = if dt > 0.0 { grip * dt } else { f32::INFINITY };
            let k = if gap > most { most / gap } else { 1.0 };
            v.x += dx * k;
            v.z += dz * k;
        }
        if input.jump && physics.is_grounded(root) {
            v.y = jump;
        }
        physics.set_velocity(root, v);
    }
}

/// Breaks a dead character (or any rigged model) apart, the classic way:
/// its head, body and limbs become loose pieces that tumble to the ground,
/// with a little push outwards from the middle. Eyes and hats stay stuck
/// to the head. The root part is held still and stops colliding, so the
/// pieces fall freely. Returns the pieces.
pub fn fall_apart(
    tree: &mut InstanceTree,
    physics: &mut crate::physics::Physics,
    model: InstanceId,
) -> Vec<InstanceId> {
    let Some((head, limbs)) = find_rig(tree, model) else { return Vec::new() };
    let root = tree.find_first_child(model, names::ROOT_PART).ok().flatten();
    let carried = root.map(|r| physics.velocity(r)).unwrap_or_default();
    if let Some(root) = root {
        set(tree, root, Property::Anchored, PropertyValue::Bool(true));
        set(tree, root, Property::CanCollide, PropertyValue::Bool(false));
    }
    let middle = tree.get_vec3(limbs.torso, Property::Position).unwrap_or_default();
    // What stays on the head: the eyes, and every accessory's parts.
    let mut stuck = vec![limbs.left_eye, limbs.right_eye];
    for accessory in tree.children(model).map(|c| c.to_vec()).unwrap_or_default() {
        if tree.class(accessory) == Ok(ClassKind::Accessory) {
            stuck.extend(tree.descendants_of_class(accessory, ClassKind::Part));
        }
    }
    // The body comes apart: its joints go, as they do on R6 bodies.
    let holders = [Some(limbs.torso), root];
    for holder in holders.into_iter().flatten() {
        for name in joints::ALL {
            if let Ok(Some(joint)) = tree.find_first_child(holder, name)
                && tree.class(joint) == Ok(ClassKind::Motor6D)
            {
                let _ = tree.destroy(joint);
            }
        }
    }
    let pieces = [head, limbs.torso, limbs.left_arm, limbs.right_arm, limbs.left_leg, limbs.right_leg];
    // It topples backwards: the higher a piece, the harder it goes back,
    // and everything spreads out a little and tumbles.
    let facing = tree.get_vec3(limbs.torso, Property::Orientation).unwrap_or_default();
    let turn = Rotation::from_orientation(facing);
    let (back, side) = (turn.apply(Vec3::new(0.0, 0.0, 1.0)), turn.apply(Vec3::new(1.0, 0.0, 0.0)));
    let floor = middle.y - 3.0;
    for (i, &piece) in pieces.iter().enumerate() {
        set(tree, piece, Property::Anchored, PropertyValue::Bool(false));
        set(tree, piece, Property::CanCollide, PropertyValue::Bool(true));
        let at = tree.get_vec3(piece, Property::Position).unwrap_or(middle);
        let out = Vec3::new(at.x - middle.x, 0.0, at.z - middle.z).normalize_or_zero();
        let height = (at.y - floor).max(0.0);
        let push = out * 5.0 + back * (2.0 * height) + Vec3::new(0.0, 3.0, 0.0);
        physics.set_velocity(piece, carried + push);
        let wobble = if i % 2 == 0 { 1.0 } else { -1.0 };
        physics.set_angular_velocity(piece, side * 4.0 + back * (2.0 * wobble));
    }
    for &part in &stuck {
        set(tree, part, Property::Anchored, PropertyValue::Bool(false));
        set(tree, part, Property::CanCollide, PropertyValue::Bool(false));
        if let Ok(weld) = tree.create(ClassKind::WeldConstraint, "Stuck") {
            set(tree, weld, Property::Part0, PropertyValue::Ref(Some(head)));
            set(tree, weld, Property::Part1, PropertyValue::Ref(Some(part)));
            let _ = tree.set_parent(weld, Some(head));
        }
    }
    pieces.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_avatar_file_has_every_hat() {
        let a = avatar();
        assert_eq!(a.skins.len(), 8);
        assert_eq!(a.clothes.len(), 16);
        assert_eq!(Hat::Cap.parts()[1].name, "Brim");
        assert_eq!(Hat::Antenna.parts()[1].shape, PartShape::Ball);
        assert!(Hat::None.parts().is_empty());
    }

    #[test]
    fn turning_goes_the_short_way_and_takes_a_moment() {
        // From facing 170 degrees to -170: twenty degrees, through 180, not 340 back.
        let one = turn_toward(170.0, -170.0, 1.0 / 60.0);
        assert!(!(-170.0..=170.0).contains(&one), "{one}");
        // A half turn is mostly done in a fifth of a second, and done in half a second.
        let mut yaw = 0.0;
        for _ in 0..12 {
            yaw = turn_toward(yaw, 179.0, 1.0 / 60.0);
        }
        assert!(yaw > 120.0 && yaw < 179.0, "{yaw}");
        for _ in 0..30 {
            yaw = turn_toward(yaw, 179.0, 1.0 / 60.0);
        }
        assert_eq!(yaw, 179.0);
        assert_eq!(yaw_of(Vec3::new(0.0, 0.0, -1.0)), 0.0);
    }
}
