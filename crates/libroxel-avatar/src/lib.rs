//! How a player's avatar looks, and pictures of it.

use libroxel_instance::{
    Color3, EnumValue, InstanceId, InstanceTree, Material, PartShape, Property, PropertyValue, Rotation, Vec3,
};

mod picture;
pub use picture::{Picture, Shot, render};

pub const HEAD_SIZE: Vec3 = Vec3::new(1.6, 1.4, 1.4);
pub const TORSO_SIZE: Vec3 = Vec3::new(2.2, 1.8, 1.1);
pub const ARM_SIZE: Vec3 = Vec3::new(0.8, 1.9, 0.9);
pub const LEG_SIZE: Vec3 = Vec3::new(0.95, 2.0, 1.0);
pub const EYE_SIZE: Vec3 = Vec3::new(0.25, 0.35, 0.05);
pub const EYE_COLOR: [u8; 3] = [30, 30, 40];
/// Where things sit relative to the root part's middle (the hips).
pub const SHOULDER: Vec3 = Vec3::new(1.5, 1.7, 0.0);
pub const HIP: Vec3 = Vec3::new(0.5, 0.0, 0.0);
pub const TORSO_AT: Vec3 = Vec3::new(0.0, 0.9, 0.0);
pub const HEAD_AT: Vec3 = Vec3::new(0.0, 2.55, 0.0);
/// Where the head turns and nods about: the top of the torso.
pub const NECK: Vec3 = Vec3::new(0.0, 1.8, 0.0);
pub const EYE_AT: Vec3 = Vec3::new(0.35, 2.65, -0.71);
/// How far below the top of an arm its shoulder joint is.
pub const ARM_TOP: f32 = 0.1;

/// The colours a body is made in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyColors {
    pub skin: Color3,
    pub shirt: Color3,
    pub pants: Color3,
}

/// The avatar's colours and hats, shared with the website.
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
        Some(Avatar {
            skins: colours("skins")?,
            clothes: colours("clothes")?,
            shirts: colours("shirts")?,
            skin: rgb(j.get("skin")?)?,
            pants: rgb(j.get("pants")?)?,
            hat_color: rgb(j.get("hat_color")?)?,
            dark: rgb(j.get("dark")?)?,
            hats,
        })
    }
}

pub fn color(c: [u8; 3]) -> Color3 {
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

/// A part of the avatar standing still, with its hips at the origin and
/// facing -Z, as the character is in a game.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Piece {
    pub pos: Vec3,
    pub rot: Rotation,
    pub size: Vec3,
    pub shape: PartShape,
    pub color: Color3,
    pub neon: bool,
    pub on_head: bool,
}

/// Every part `look` has: the body, its eyes and what it wears.
pub fn pieces(look: &Look) -> Vec<Piece> {
    let flip = |v: Vec3| Vec3::new(-v.x, v.y, v.z);
    let block = |pos: Vec3, size: Vec3, color: Color3, on_head: bool| Piece {
        pos,
        rot: Rotation::IDENTITY,
        size,
        shape: PartShape::Block,
        color,
        neon: false,
        on_head,
    };
    let BodyColors { skin, shirt, pants } = look.colors;
    let arm = SHOULDER - Vec3::new(0.0, ARM_SIZE.y * 0.5 - ARM_TOP, 0.0);
    let leg = HIP - Vec3::new(0.0, LEG_SIZE.y * 0.5, 0.0);
    let eye = color(EYE_COLOR);
    let mut out = vec![
        block(HEAD_AT, HEAD_SIZE, skin, true),
        block(TORSO_AT, TORSO_SIZE, shirt, false),
        block(flip(arm), ARM_SIZE, skin, false),
        block(arm, ARM_SIZE, skin, false),
        block(flip(leg), LEG_SIZE, pants, false),
        block(leg, LEG_SIZE, pants, false),
        block(flip(EYE_AT), EYE_SIZE, eye, true),
        block(EYE_AT, EYE_SIZE, eye, true),
    ];
    let worn = |at: Vec3, turn: Vec3, size: Vec3, shape, color, neon| Piece {
        pos: HEAD_AT + at,
        rot: Rotation::from_orientation(turn),
        size,
        shape,
        color,
        neon,
        on_head: true,
    };
    match &look.item {
        Some(item) => {
            out.extend(item.parts.iter().map(|p| worn(p.offset, p.turn, p.size, p.shape, p.color, p.neon)))
        }
        None => out.extend(look.hat.parts().iter().map(|p| {
            let c = if p.tinted { look.hat_color } else { color(avatar().dark) };
            let v = |[x, y, z]: [f32; 3]| Vec3::new(x, y, z);
            worn(v(p.at), Vec3::new(0.0, 0.0, p.turn), v(p.size), p.shape, c, p.glow)
        })),
    }
    out
}

/// The seed the app and the website give a name, for its usual look.
pub fn seed(name: &str) -> usize {
    name.bytes().fold(7usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize))
}

/// `json`'s look, with anything missing taken from the usual look for `name`.
pub fn look_of(json: &libroxel_instance::serde_json::Value, name: &str) -> Look {
    Look::from_json(json, Look::for_player(seed(name)))
}
