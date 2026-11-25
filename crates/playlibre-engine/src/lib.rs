//! PlayLibre's game loop.
//!
//! The pure parts ([`physics`], [`character`], [`session`]) work on an
//! [`InstanceTree`](playlibre_instance::InstanceTree) and are tested without
//! a window. The [`view`] module plugs them into Bevy: it mirrors parts onto
//! entities, draws them, and provides the play and fly cameras.

pub mod camera;
pub mod character;
pub mod chat;
mod clusters;
pub mod coedit;
pub mod collision_shapes;
pub mod devices;
pub mod effects;
pub mod hinge;
pub mod mesh_files;
pub mod movers;
pub mod net;
pub mod npc;
pub mod physics;
pub mod pictures;
mod rigid;
pub mod seat;
pub mod session;
pub mod sounds;
mod terrain_view;
pub mod view;
pub mod view_effects;
pub mod voice;
pub mod weld;

pub use character::{Character, CharacterInput};
pub use session::{Blast, FrameTimes, LogEntry, LogLevel, PlaySession};

/// `x` held between `lo` and `hi`, never panicking as `clamp` does: limits
/// come from games (a NaN, or a lower limit above the upper one), and a game
/// mustn't stop over them. A NaN limit holds nothing back on that side, and
/// limits the wrong way round are swapped.
pub(crate) fn clamp_lenient<T: num_like::Float>(x: T, lo: T, hi: T) -> T {
    let (lo, hi) = if lo > hi { (hi, lo) } else { (lo, hi) };
    let x = if !lo.is_nan() && x < lo { lo } else { x };
    if !hi.is_nan() && x > hi { hi } else { x }
}

mod num_like {
    pub trait Float: Copy + PartialOrd {
        fn is_nan(self) -> bool;
    }
    impl Float for f32 {
        fn is_nan(self) -> bool {
            f32::is_nan(self)
        }
    }
    impl Float for f64 {
        fn is_nan(self) -> bool {
            f64::is_nan(self)
        }
    }
}
