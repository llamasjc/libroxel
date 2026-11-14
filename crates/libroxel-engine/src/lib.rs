//! Libroxel's game loop.
//!
//! The pure parts ([`physics`], [`character`], [`session`]) work on an
//! [`InstanceTree`](libroxel_instance::InstanceTree) and are tested without
//! a window. The [`view`] module plugs them into Bevy: it mirrors parts onto
//! entities, draws them, and provides the play and fly cameras.

pub mod camera;
pub mod character;
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
pub mod weld;

pub use character::{Character, CharacterInput};
pub use session::{Blast, LogEntry, LogLevel, PlaySession};
