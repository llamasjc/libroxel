//! Character animations: short clips that move a character's head, arms
//! and legs. The built-in ones are named `playlibre://wave` and so on.
//!
//! A clip is a list of key poses at times; between keys the angles are
//! blended smoothly. Angles are in degrees: an arm or leg's `swing` moves
//! it forward (positive) or back, and its `out` lifts it out to the side,
//! away from the body. The head can `nod` (down is positive) and `turn`
//! (to its left is positive). `bob` lifts the whole body, in studs.

/// One limb's angles, in degrees.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Limb {
    pub swing: f32,
    pub out: f32,
}

/// Where everything is at one moment of a clip.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pose {
    pub left_arm: Limb,
    pub right_arm: Limb,
    pub left_leg: Limb,
    pub right_leg: Limb,
    pub nod: f32,
    pub turn: f32,
    pub bob: f32,
}

/// A character animation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Clip {
    pub name: &'static str,
    /// How long it is, in seconds.
    pub length: f32,
    /// Whether it starts again when it ends (scripts can change this).
    pub looped: bool,
    /// Whether it moves the legs. If not, they walk as usual while it plays.
    pub legs: bool,
    pub keys: &'static [(f32, Pose)],
}

/// The start of every built-in animation's id.
pub const BUILT_IN: &str = "playlibre://";

/// How built-in ids started before the rename to PlayLibre. Games saved back
/// then still use it, so it means the same as [`BUILT_IN`].
pub const BUILT_IN_BEFORE: &str = "libroxel://";

/// The name after the start of a built-in id (`playlibre://wave` or the
/// older `libroxel://wave` both give `wave`), or `None` for any other id.
/// Shared by built-in animations, pictures and sounds.
pub fn built_in_name(id: &str) -> Option<&str> {
    id.strip_prefix(BUILT_IN).or_else(|| id.strip_prefix(BUILT_IN_BEFORE))
}

const fn limb(swing: f32, out: f32) -> Limb {
    Limb { swing, out }
}

const fn arms(left: Limb, right: Limb) -> Pose {
    Pose {
        left_arm: left,
        right_arm: right,
        left_leg: limb(0.0, 0.0),
        right_leg: limb(0.0, 0.0),
        nod: 0.0,
        turn: 0.0,
        bob: 0.0,
    }
}

const REST: Pose = arms(limb(0.0, 0.0), limb(0.0, 0.0));

const WAVE_UP: Pose = arms(limb(0.0, 0.0), limb(0.0, 150.0));
const WAVE_OVER: Pose = arms(limb(0.0, 0.0), limb(0.0, 120.0));
const CHEER_UP: Pose = Pose { bob: 0.4, ..arms(limb(0.0, 160.0), limb(0.0, 160.0)) };
const CHEER_DOWN: Pose = arms(limb(0.0, 140.0), limb(0.0, 140.0));
const POINT: Pose = arms(limb(0.0, 0.0), limb(90.0, 0.0));
const NOD_DOWN: Pose = Pose { nod: 20.0, ..REST };
const NOD_UP: Pose = Pose { nod: -5.0, ..REST };
const SHAKE_LEFT: Pose = Pose { turn: 25.0, ..REST };
const SHAKE_RIGHT: Pose = Pose { turn: -25.0, ..REST };
const JACKS_OUT: Pose = Pose {
    left_arm: limb(0.0, 160.0),
    right_arm: limb(0.0, 160.0),
    left_leg: limb(0.0, 20.0),
    right_leg: limb(0.0, 20.0),
    nod: 0.0,
    turn: 0.0,
    bob: 0.3,
};
const DANCE_A: Pose = Pose {
    left_arm: limb(30.0, 100.0),
    right_arm: limb(-20.0, 30.0),
    left_leg: limb(25.0, 0.0),
    right_leg: limb(0.0, 0.0),
    nod: 0.0,
    turn: 15.0,
    bob: 0.25,
};
const DANCE_B: Pose = Pose {
    left_arm: limb(-20.0, 30.0),
    right_arm: limb(30.0, 100.0),
    left_leg: limb(0.0, 0.0),
    right_leg: limb(25.0, 0.0),
    nod: 0.0,
    turn: -15.0,
    bob: 0.25,
};

const JUMP_UP: Pose = arms(limb(170.0, 0.0), limb(170.0, 0.0));
const FALL: Pose = arms(limb(150.0, 25.0), limb(150.0, 25.0));
const CLIMB_A: Pose = Pose {
    left_arm: limb(160.0, 0.0),
    right_arm: limb(110.0, 0.0),
    left_leg: limb(40.0, 0.0),
    right_leg: limb(0.0, 0.0),
    nod: 0.0,
    turn: 0.0,
    bob: 0.0,
};
const CLIMB_B: Pose = Pose {
    left_arm: limb(110.0, 0.0),
    right_arm: limb(160.0, 0.0),
    left_leg: limb(0.0, 0.0),
    right_leg: limb(40.0, 0.0),
    nod: 0.0,
    turn: 0.0,
    bob: 0.0,
};
const HOLD: Pose = arms(limb(0.0, 0.0), limb(90.0, 0.0));
const LAUGH_BACK: Pose = Pose { nod: -18.0, bob: 0.1, ..arms(limb(10.0, 10.0), limb(10.0, 10.0)) };

/// Every built-in animation.
pub const CLIPS: &[Clip] = &[
    Clip {
        name: "wave",
        length: 2.0,
        looped: false,
        legs: false,
        keys: &[
            (0.0, REST),
            (0.3, WAVE_UP),
            (0.6, WAVE_OVER),
            (0.9, WAVE_UP),
            (1.2, WAVE_OVER),
            (1.5, WAVE_UP),
            (2.0, REST),
        ],
    },
    Clip {
        name: "cheer",
        length: 1.6,
        looped: false,
        legs: false,
        keys: &[
            (0.0, REST),
            (0.3, CHEER_UP),
            (0.6, CHEER_DOWN),
            (0.9, CHEER_UP),
            (1.2, CHEER_DOWN),
            (1.6, REST),
        ],
    },
    Clip {
        name: "point",
        length: 1.5,
        looped: false,
        legs: false,
        keys: &[(0.0, REST), (0.25, POINT), (1.2, POINT), (1.5, REST)],
    },
    Clip {
        name: "nod",
        length: 1.2,
        looped: false,
        legs: false,
        keys: &[(0.0, REST), (0.3, NOD_DOWN), (0.6, NOD_UP), (0.9, NOD_DOWN), (1.2, REST)],
    },
    Clip {
        name: "shake",
        length: 1.2,
        looped: false,
        legs: false,
        keys: &[(0.0, REST), (0.3, SHAKE_LEFT), (0.6, SHAKE_RIGHT), (0.9, SHAKE_LEFT), (1.2, REST)],
    },
    Clip {
        name: "jacks",
        length: 0.8,
        looped: true,
        legs: true,
        keys: &[(0.0, REST), (0.4, JACKS_OUT), (0.8, REST)],
    },
    Clip {
        name: "dance",
        length: 1.2,
        looped: true,
        legs: true,
        keys: &[(0.0, DANCE_A), (0.6, DANCE_B), (1.2, DANCE_A)],
    },
    // Arms thrown up for a jump, held there.
    Clip {
        name: "jump",
        length: 0.3,
        looped: false,
        legs: false,
        keys: &[(0.0, REST), (0.15, JUMP_UP), (0.3, JUMP_UP)],
    },
    Clip { name: "fall", length: 0.3, looped: false, legs: false, keys: &[(0.0, JUMP_UP), (0.3, FALL)] },
    // Hand over hand, a knee up on each reach.
    Clip {
        name: "climb",
        length: 1.0,
        looped: true,
        legs: true,
        keys: &[(0.0, CLIMB_A), (0.5, CLIMB_B), (1.0, CLIMB_A)],
    },
    // The right arm held out in front, as for carrying a tool.
    Clip { name: "hold", length: 1.0, looped: true, legs: false, keys: &[(0.0, HOLD), (1.0, HOLD)] },
    Clip {
        name: "laugh",
        length: 1.2,
        looped: false,
        legs: false,
        keys: &[(0.0, REST), (0.3, LAUGH_BACK), (0.5, REST), (0.8, LAUGH_BACK), (1.2, REST)],
    },
];

/// The built-in clip an animation id names, like `playlibre://wave`.
pub fn clip(id: &str) -> Option<&'static Clip> {
    let name = built_in_name(id)?;
    CLIPS.iter().find(|c| c.name == name)
}

fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn mix_limb(a: Limb, b: Limb, t: f32) -> Limb {
    limb(mix(a.swing, b.swing, t), mix(a.out, b.out, t))
}

/// Blends two poses: `t` 0 is `a`, 1 is `b`.
pub fn mix_pose(a: Pose, b: Pose, t: f32) -> Pose {
    Pose {
        left_arm: mix_limb(a.left_arm, b.left_arm, t),
        right_arm: mix_limb(a.right_arm, b.right_arm, t),
        left_leg: mix_limb(a.left_leg, b.left_leg, t),
        right_leg: mix_limb(a.right_leg, b.right_leg, t),
        nod: mix(a.nod, b.nod, t),
        turn: mix(a.turn, b.turn, t),
        bob: mix(a.bob, b.bob, t),
    }
}

impl Clip {
    /// The pose `time` seconds in (wrapped round when looped, held at the
    /// end when not).
    pub fn sample(&self, time: f32) -> Pose {
        sample_keys(self.keys, self.length, self.looped, time, true)
    }
}

/// The pose `time` seconds into the keys of a clip `length` long. With
/// `ease`, moves between keys start and stop gently; without, they go
/// straight from one key to the next (for clips with many close keys).
pub fn sample_keys(keys: &[(f32, Pose)], length: f32, looped: bool, time: f32, ease: bool) -> Pose {
    let time = if looped && length > 0.0 { time.rem_euclid(length) } else { time.clamp(0.0, length) };
    let Some(first) = keys.first() else { return REST };
    if time <= first.0 {
        return first.1;
    }
    for pair in keys.windows(2) {
        let ((t0, a), (t1, b)) = (pair[0], pair[1]);
        if time <= t1 {
            let t = if t1 > t0 { (time - t0) / (t1 - t0) } else { 1.0 };
            // Ease in and out, so moves don't start and stop with a jolt.
            let t = if ease { t * t * (3.0 - 2.0 * t) } else { t };
            return mix_pose(a, b, t);
        }
    }
    keys.last().map_or(REST, |k| k.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clips_are_found_by_id_and_blend_between_keys() {
        let wave = clip("playlibre://wave").unwrap();
        assert!(clip("playlibre://moonwalk").is_none());
        assert!(clip("wave").is_none());
        assert_eq!(wave.sample(0.0), REST);
        assert_eq!(wave.sample(0.3), WAVE_UP);
        let between = wave.sample(0.15);
        assert!(between.right_arm.out > 10.0 && between.right_arm.out < 140.0);
        // Not looped: it holds the end.
        assert_eq!(wave.sample(5.0), REST);
        // Looped: it wraps round.
        let dance = clip("playlibre://dance").unwrap();
        assert!(std::ptr::eq(clip("libroxel://dance").unwrap(), dance), "games saved before the rename");
        assert_eq!(dance.sample(0.6), dance.sample(1.8));
        for c in CLIPS {
            assert!(c.keys.windows(2).all(|w| w[0].0 <= w[1].0), "{}", c.name);
            assert!((c.keys.last().unwrap().0 - c.length).abs() < 1e-6, "{}", c.name);
        }
    }
}
