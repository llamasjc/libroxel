//! Small value types stored in instance properties.
//!
//! These are deliberately plain so the tree has no dependency on a maths
//! library; the engine converts them to its own types at the boundary.

use std::ops::{Add, Div, Mul, Neg, Sub};

/// A 3D vector of `f32`s.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3::new(0.0, 0.0, 0.0);
    pub const ONE: Vec3 = Vec3::new(1.0, 1.0, 1.0);

    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    pub fn dot(self, other: Vec3) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    pub fn cross(self, other: Vec3) -> Vec3 {
        Vec3::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }

    /// The unit vector in the same direction, or zero for a zero vector.
    pub fn normalize_or_zero(self) -> Vec3 {
        let len = self.length();
        if len > 0.0 { self / len } else { Vec3::ZERO }
    }

    pub fn lerp(self, other: Vec3, t: f32) -> Vec3 {
        self + (other - self) * t
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl Mul for Vec3 {
    type Output = Vec3;
    fn mul(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x * o.x, self.y * o.y, self.z * o.z)
    }
}

impl Mul<f32> for Vec3 {
    type Output = Vec3;
    fn mul(self, s: f32) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }
}

impl Div for Vec3 {
    type Output = Vec3;
    fn div(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x / o.x, self.y / o.y, self.z / o.z)
    }
}

impl Div<f32> for Vec3 {
    type Output = Vec3;
    fn div(self, s: f32) -> Vec3 {
        Vec3::new(self.x / s, self.y / s, self.z / s)
    }
}

impl Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        Vec3::new(-self.x, -self.y, -self.z)
    }
}

/// An RGB colour with components in `0.0..=1.0` (sRGB space).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color3 {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl Color3 {
    pub const WHITE: Color3 = Color3::new(1.0, 1.0, 1.0);
    /// The default colour of a new part: a neutral mid grey.
    pub const MEDIUM_GREY: Color3 = Color3::new(0.64, 0.64, 0.64);

    pub const fn new(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b }
    }

    /// Builds a colour from 8-bit channels.
    pub fn from_rgb8(r: u8, g: u8, b: u8) -> Self {
        Self::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
    }
}

impl Default for Color3 {
    fn default() -> Self {
        Self::WHITE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_arithmetic() {
        let a = Vec3::new(1.0, 2.0, 3.0);
        let b = Vec3::new(4.0, 5.0, 6.0);
        assert_eq!(a + b, Vec3::new(5.0, 7.0, 9.0));
        assert_eq!(b - a, Vec3::new(3.0, 3.0, 3.0));
        assert_eq!(a * 2.0, Vec3::new(2.0, 4.0, 6.0));
        assert_eq!(-a, Vec3::new(-1.0, -2.0, -3.0));
        assert_eq!(a.dot(b), 32.0);
        assert_eq!(Vec3::new(1.0, 0.0, 0.0).cross(Vec3::new(0.0, 1.0, 0.0)), Vec3::new(0.0, 0.0, 1.0));
        assert_eq!(Vec3::new(3.0, 4.0, 0.0).length(), 5.0);
        assert_eq!(Vec3::ZERO.normalize_or_zero(), Vec3::ZERO);
    }

    #[test]
    fn colour_from_bytes() {
        assert_eq!(Color3::from_rgb8(255, 0, 255), Color3::new(1.0, 0.0, 1.0));
    }
}
