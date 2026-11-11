//! Libroxel's game loop.
//!
//! The pure parts ([`physics`], [`character`], [`session`]) work on an
//! [`InstanceTree`](libroxel_instance::InstanceTree) and are tested without
//! a window. The [`view`] module plugs them into Bevy: it mirrors parts onto
//! entities, draws them, and provides the play and fly cameras.

pub mod character;
pub mod npc;
pub mod physics;
pub mod seat;
pub mod session;
pub mod view;
pub mod weld;

pub use character::{Character, CharacterInput};
pub use session::{Blast, LogEntry, LogLevel, PlaySession};
