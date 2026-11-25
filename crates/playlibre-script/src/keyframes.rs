//! Animations kept as keyframe sequences, turned into motions playlibre's
//! characters can play.
//!
//! A sequence holds a keyframe for each moment, and each keyframe holds a
//! pose for each joint it turns, nested the way the joints are (the torso's
//! pose holds the arms' and the head's). A pose's CFrame turns its joint
//! from where it rests. PlayLibre's characters move their arms, legs and
//! head as a whole, so each pose is read as which way its limb points (or
//! its head faces) and the twist along the limb is left out. Joints playlibre
//! doesn't have (elbows, hands, the upper torso) are skipped.

use mlua::{Lua, ObjectLike, Value};
use playlibre_instance::animation::{Clip, Limb, Pose, mix_pose, sample_keys};
use playlibre_instance::{InstanceId, Rotation, Vec3};

use crate::bindings::{host, push_instance};
use crate::names::keyframes as names;

/// What a pose turns on playlibre's characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Channel {
    LeftArm,
    RightArm,
    LeftLeg,
    RightLeg,
    Head,
    Bob,
}

const CHANNELS: usize = 6;

/// Which parts of the body a motion moves, by [`Channel`].
pub(crate) type Moves = [bool; CHANNELS];

/// Something a track can play: a built-in clip or a keyframe sequence.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Motion {
    pub name: String,
    pub length: f32,
    pub looped: bool,
    /// Whether moves go gently in and out of each key (built-in clips with
    /// few keys) or straight from one to the next (sequences).
    ease: bool,
    keys: Vec<(f32, Pose)>,
    pub moves: Moves,
    /// Keyframes that have a name of their own, for `KeyframeReached`.
    pub keyframes: Vec<(f32, String)>,
    /// Markers: when, their name, and their value.
    pub markers: Vec<(f32, String, String)>,
    /// The `AnimationPriority` number it plays at.
    pub priority: u32,
}

/// The priority a track plays at unless told otherwise (`Core`).
pub(crate) const CORE: u32 = 1000;

impl Motion {
    pub(crate) fn built_in(clip: &'static Clip) -> Self {
        let legs = clip.legs;
        Self {
            name: clip.name.to_owned(),
            length: clip.length,
            looped: clip.looped,
            ease: true,
            keys: clip.keys.to_vec(),
            moves: [true, true, legs, legs, true, true],
            keyframes: Vec::new(),
            markers: Vec::new(),
            priority: CORE,
        }
    }

    pub(crate) fn legs(&self) -> bool {
        self.moves[Channel::LeftLeg as usize] || self.moves[Channel::RightLeg as usize]
    }

    pub(crate) fn is_still(&self) -> bool {
        self.keys.is_empty()
    }

    pub(crate) fn sample(&self, time: f32) -> Pose {
        sample_keys(&self.keys, self.length, self.looped, time, self.ease)
    }
}

/// Blends `top` over `base` by `t`, only where `moves` says it moves.
pub(crate) fn mix_moving(base: Pose, top: Pose, t: f32, moves: &Moves) -> Pose {
    let mixed = mix_pose(base, top, t);
    let pick = |c: Channel| moves[c as usize];
    Pose {
        left_arm: if pick(Channel::LeftArm) { mixed.left_arm } else { base.left_arm },
        right_arm: if pick(Channel::RightArm) { mixed.right_arm } else { base.right_arm },
        left_leg: if pick(Channel::LeftLeg) { mixed.left_leg } else { base.left_leg },
        right_leg: if pick(Channel::RightLeg) { mixed.right_leg } else { base.right_leg },
        nod: if pick(Channel::Head) { mixed.nod } else { base.nod },
        turn: if pick(Channel::Head) { mixed.turn } else { base.turn },
        bob: if pick(Channel::Bob) { mixed.bob } else { base.bob },
    }
}

/// The joint a pose named `name` turns, and how that joint rests in the
/// body: turned a quarter about Y for the classic body's shoulders and
/// hips, Z up for its neck, and straight for the fifteen-part body.
fn channel(name: &str, classic: bool) -> Option<(Channel, Rotation)> {
    let quarter = std::f32::consts::FRAC_PI_2;
    let right = Rotation::about_y(quarter);
    let left = Rotation::about_y(-quarter);
    let upright =
        Rotation { x: Vec3::new(-1.0, 0.0, 0.0), y: Vec3::new(0.0, 0.0, 1.0), z: Vec3::new(0.0, 1.0, 0.0) };
    Some(match name {
        names::R6_LEFT_ARM => (Channel::LeftArm, left),
        names::R6_RIGHT_ARM => (Channel::RightArm, right),
        names::R6_LEFT_LEG => (Channel::LeftLeg, left),
        names::R6_RIGHT_LEG => (Channel::RightLeg, right),
        names::R6_TORSO => (Channel::Bob, upright),
        names::HEAD if classic => (Channel::Head, upright),
        names::HEAD => (Channel::Head, Rotation::IDENTITY),
        names::R15_LEFT_ARM => (Channel::LeftArm, Rotation::IDENTITY),
        names::R15_RIGHT_ARM => (Channel::RightArm, Rotation::IDENTITY),
        names::R15_LEFT_LEG => (Channel::LeftLeg, Rotation::IDENTITY),
        names::R15_RIGHT_LEG => (Channel::RightLeg, Rotation::IDENTITY),
        names::R15_LOWER_TORSO => (Channel::Bob, Rotation::IDENTITY),
        _ => return None,
    })
}

/// A pose's turn as playlibre's two numbers for its channel: a limb's
/// swing and out (degrees), the head's nod and turn (degrees), or the
/// body's lift (studs).
fn values(channel: Channel, rest: Rotation, turn: Rotation, at: Vec3) -> [f32; 2] {
    // The turn in the body's axes.
    let body = rest * turn * rest.transpose();
    match channel {
        Channel::Bob => [rest.apply(at).y, 0.0],
        Channel::Head => {
            // Which way the face points: nodding tips it down, turning
            // swings it to the head's left.
            let f = body.apply(Vec3::new(0.0, 0.0, -1.0));
            [(-f.y).clamp(-1.0, 1.0).asin().to_degrees(), (-f.x).atan2(-f.z).to_degrees()]
        }
        _ => {
            // Where the limb points, from straight down: swung forward
            // (towards -Z) and out to the side (+X is the right side).
            let d = body.apply(Vec3::new(0.0, -1.0, 0.0));
            let below = (d.y * d.y + d.z * d.z).sqrt();
            // Straight out to the side, any swing points it the same way.
            let swing = if below < 1e-3 { 0.0 } else { (-d.z).atan2(-d.y).to_degrees() };
            let side = d.x.atan2(below).to_degrees();
            let out = if matches!(channel, Channel::LeftArm | Channel::LeftLeg) { -side } else { side };
            [swing, out]
        }
    }
}

fn get(lua: &Lua, id: InstanceId, key: &str) -> mlua::Result<Value> {
    push_instance(lua, id)?.get(key)
}

fn children(lua: &Lua, id: InstanceId) -> Vec<InstanceId> {
    host(lua).tree.borrow().children(id).map(|c| c.to_vec()).unwrap_or_default()
}

fn text(v: Value) -> String {
    match v {
        Value::String(s) => s.to_string_lossy(),
        Value::Nil => String::new(),
        other => other.to_string().unwrap_or_default(),
    }
}

fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Integer(n) => Some(*n as f64),
        Value::Number(n) => Some(*n),
        _ => None,
    }
}

/// An enumeration item's number (or a plain number).
pub(crate) fn enum_number(v: &Value) -> Option<u32> {
    let n = match v {
        Value::UserData(u) => u.get::<f64>(names::VALUE).ok(),
        Value::Table(t) => t.get::<f64>(names::VALUE).ok(),
        _ => number(v),
    }?;
    (n >= 0.0).then_some(n as u32)
}

fn class_of(lua: &Lua, id: InstanceId) -> String {
    get(lua, id, names::CLASS_NAME).map(text).unwrap_or_default()
}

/// Every pose under `id` (a keyframe or a pose), with its name.
fn poses(lua: &Lua, id: InstanceId, out: &mut Vec<(String, InstanceId)>) {
    for child in children(lua, id) {
        if class_of(lua, child) == names::POSE {
            out.push((get(lua, child, names::NAME).map(text).unwrap_or_default(), child));
            poses(lua, child, out);
        }
    }
}

/// The value of one channel `time` seconds in, between its keys.
fn channel_at(keys: &[(f32, [f32; 2])], time: f32) -> [f32; 2] {
    let Some(first) = keys.first() else { return [0.0; 2] };
    if time <= first.0 {
        return first.1;
    }
    for pair in keys.windows(2) {
        let ((t0, a), (t1, b)) = (pair[0], pair[1]);
        if time <= t1 {
            let t = if t1 > t0 { (time - t0) / (t1 - t0) } else { 1.0 };
            return [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        }
    }
    keys.last().map_or([0.0; 2], |k| k.1)
}

/// The motion a keyframe sequence describes. A joint that a keyframe has
/// no pose for goes smoothly between the keyframes either side that do.
pub(crate) fn from_sequence(lua: &Lua, sequence: InstanceId) -> mlua::Result<Motion> {
    let mut frames: Vec<(f32, String, InstanceId)> = Vec::new();
    for child in children(lua, sequence) {
        if class_of(lua, child) == names::KEYFRAME {
            let time = number(&get(lua, child, names::TIME)?).unwrap_or(0.0) as f32;
            frames.push((time.max(0.0), get(lua, child, names::NAME).map(text)?, child));
        }
    }
    frames.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut all = Vec::new();
    for (_, _, frame) in &frames {
        poses(lua, *frame, &mut all);
    }
    let classic = all.iter().any(|(name, _)| name == names::R6_TORSO);
    let mut channels: [Vec<(f32, [f32; 2])>; CHANNELS] = Default::default();
    let mut keyframes = Vec::new();
    let mut markers = Vec::new();
    for (time, name, frame) in &frames {
        if name != names::UNNAMED {
            keyframes.push((*time, name.clone()));
        }
        for child in children(lua, *frame) {
            if class_of(lua, child) == names::MARKER {
                let marker = get(lua, child, names::NAME).map(text)?;
                markers.push((*time, marker, get(lua, child, names::VALUE).map(text)?));
            }
        }
        let mut here = Vec::new();
        poses(lua, *frame, &mut here);
        for (name, pose) in here {
            let Some((channel, rest)) = channel(&name, classic) else { continue };
            if number(&get(lua, pose, names::WEIGHT)?).is_some_and(|w| w <= 0.0) {
                continue;
            }
            let Some(cf) = crate::types::as_cframe(&get(lua, pose, names::CFRAME)?) else { continue };
            channels[channel as usize].push((*time, values(channel, rest, cf.rot, cf.pos)));
        }
    }
    let mut times: Vec<f32> = frames.iter().map(|f| f.0).collect();
    times.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
    let moves: Moves = std::array::from_fn(|c| !channels[c].is_empty());
    let keys = if moves.iter().any(|m| *m) {
        times
            .iter()
            .map(|&t| {
                let at = |c: Channel| channel_at(&channels[c as usize], t);
                let limb = |c: Channel| {
                    let [swing, out] = at(c);
                    Limb { swing, out }
                };
                let [nod, turn] = at(Channel::Head);
                let pose = Pose {
                    left_arm: limb(Channel::LeftArm),
                    right_arm: limb(Channel::RightArm),
                    left_leg: limb(Channel::LeftLeg),
                    right_leg: limb(Channel::RightLeg),
                    nod,
                    turn,
                    bob: at(Channel::Bob)[0],
                };
                (t, pose)
            })
            .collect()
    } else {
        Vec::new()
    };
    let looped = matches!(get(lua, sequence, names::LOOP)?, Value::Boolean(true));
    let priority = enum_number(&get(lua, sequence, names::PRIORITY)?).unwrap_or(CORE);
    Ok(Motion {
        name: get(lua, sequence, names::NAME).map(text)?,
        length: times.last().copied().unwrap_or(0.0),
        looped,
        ease: false,
        keys,
        moves,
        keyframes,
        markers,
        priority,
    })
}

/// The key an animation id is registered under: the asset number for an
/// asset address (`rbxassetid://123`, `…?id=123`), else the id itself.
pub(crate) fn id_key(id: &str) -> String {
    use crate::names::animation::{ASSET_ID, ASSET_QUERY};
    let id = id.trim();
    let lower = id.to_ascii_lowercase();
    let digits = |rest: &str| -> Option<String> {
        let n: String = rest.chars().take_while(char::is_ascii_digit).collect();
        (!n.is_empty()).then_some(n)
    };
    lower
        .strip_prefix(ASSET_ID)
        .and_then(digits)
        .or_else(|| lower.rfind(ASSET_QUERY).and_then(|i| digits(&lower[i + ASSET_QUERY.len()..])))
        .unwrap_or_else(|| id.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn asset_addresses_share_a_key() {
        assert_eq!(id_key("rbxassetid://123"), "123");
        assert_eq!(id_key(" RBXASSETID://123 "), "123");
        assert_eq!(id_key("http://assets.example/asset/?id=123"), "123");
        assert_eq!(id_key("rbxtemp://4"), "rbxtemp://4");
    }

    #[test]
    fn poses_read_as_which_way_limbs_point() {
        let quarter = std::f32::consts::FRAC_PI_2;
        let (c, rest) = channel(names::R6_RIGHT_ARM, true).unwrap();
        // The classic right shoulder turned a quarter about its own Z
        // raises the arm straight out in front.
        let [swing, out] = values(c, rest, Rotation::about_z(quarter), Vec3::ZERO);
        assert!(close(swing, 90.0) && close(out, 0.0), "{swing} {out}");
        // The fifteen-part arm turned about Z lifts out to the side.
        let (c, rest) = channel(names::R15_RIGHT_ARM, false).unwrap();
        let [swing, out] = values(c, rest, Rotation::about_z(quarter), Vec3::ZERO);
        assert!(close(swing, 0.0) && close(out, 90.0), "{swing} {out}");
        let (c, rest) = channel(names::R15_LEFT_ARM, false).unwrap();
        let [_, out] = values(c, rest, Rotation::about_z(-quarter), Vec3::ZERO);
        assert!(close(out, 90.0), "{out}");
        // The head tipped forward nods; turned about Y it looks aside.
        let (c, rest) = channel(names::HEAD, false).unwrap();
        let [nod, turn] = values(c, rest, Rotation::about_x(-0.3), Vec3::ZERO);
        assert!(close(nod, 0.3f32.to_degrees()) && close(turn, 0.0), "{nod} {turn}");
        let [nod, turn] = values(c, rest, Rotation::about_y(0.4), Vec3::ZERO);
        assert!(close(nod, 0.0) && close(turn, 0.4f32.to_degrees()), "{nod} {turn}");
        // The classic torso's lift is along its joint's Z.
        let (c, rest) = channel(names::R6_TORSO, true).unwrap();
        assert!(close(values(c, rest, Rotation::IDENTITY, Vec3::new(0.0, 0.0, 0.5))[0], 0.5));
    }
}
