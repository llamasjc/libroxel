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

/// A 2D vector, used for things like a GUI object's anchor point.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2::new(0.0, 0.0);

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// A position or size on screen: a fraction of the parent (`scale`) plus pixels (`offset`), per axis.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct UDim2 {
    pub x_scale: f32,
    pub x_offset: f32,
    pub y_scale: f32,
    pub y_offset: f32,
}

impl UDim2 {
    pub const fn new(x_scale: f32, x_offset: f32, y_scale: f32, y_offset: f32) -> Self {
        Self { x_scale, x_offset, y_scale, y_offset }
    }

    /// Resolves to pixels inside a parent of `parent` pixels.
    pub fn resolve(self, parent: Vec2) -> Vec2 {
        Vec2::new(self.x_scale * parent.x + self.x_offset, self.y_scale * parent.y + self.y_offset)
    }

    pub fn lerp(self, other: UDim2, t: f32) -> UDim2 {
        let l = |a: f32, b: f32| a + (b - a) * t;
        UDim2::new(
            l(self.x_scale, other.x_scale),
            l(self.x_offset, other.x_offset),
            l(self.y_scale, other.y_scale),
            l(self.y_offset, other.y_offset),
        )
    }
}

impl Color3 {
    pub fn lerp(self, other: Color3, t: f32) -> Color3 {
        let l = |a: f32, b: f32| a + (b - a) * t;
        Color3::new(l(self.r, other.r), l(self.g, other.g), l(self.b, other.b))
    }
}

/// A rotation as a 3x3 matrix, stored as its three columns (the rotated X, Y and Z axes).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rotation {
    pub x: Vec3,
    pub y: Vec3,
    pub z: Vec3,
}

impl Default for Rotation {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Rotation {
    pub const IDENTITY: Rotation =
        Rotation { x: Vec3::new(1.0, 0.0, 0.0), y: Vec3::new(0.0, 1.0, 0.0), z: Vec3::new(0.0, 0.0, 1.0) };

    pub fn about_x(a: f32) -> Rotation {
        let (s, c) = a.sin_cos();
        Rotation { x: Vec3::new(1.0, 0.0, 0.0), y: Vec3::new(0.0, c, s), z: Vec3::new(0.0, -s, c) }
    }

    pub fn about_y(a: f32) -> Rotation {
        let (s, c) = a.sin_cos();
        Rotation { x: Vec3::new(c, 0.0, -s), y: Vec3::new(0.0, 1.0, 0.0), z: Vec3::new(s, 0.0, c) }
    }

    pub fn about_z(a: f32) -> Rotation {
        let (s, c) = a.sin_cos();
        Rotation { x: Vec3::new(c, s, 0.0), y: Vec3::new(-s, c, 0.0), z: Vec3::new(0.0, 0.0, 1.0) }
    }

    /// Rotation around an arbitrary unit axis.
    pub fn about_axis(axis: Vec3, a: f32) -> Rotation {
        let axis = axis.normalize_or_zero();
        let (s, c) = a.sin_cos();
        let t = 1.0 - c;
        let (x, y, z) = (axis.x, axis.y, axis.z);
        Rotation {
            x: Vec3::new(t * x * x + c, t * x * y + s * z, t * x * z - s * y),
            y: Vec3::new(t * x * y - s * z, t * y * y + c, t * y * z + s * x),
            z: Vec3::new(t * x * z + s * y, t * y * z - s * x, t * z * z + c),
        }
    }

    /// From an orientation in degrees, applied around Y, then X, then Z.
    pub fn from_orientation(degrees: Vec3) -> Rotation {
        Rotation::about_y(degrees.y.to_radians())
            * Rotation::about_x(degrees.x.to_radians())
            * Rotation::about_z(degrees.z.to_radians())
    }

    /// Back to an orientation in degrees (Y, then X, then Z), rounded to remove float noise.
    pub fn to_orientation(self) -> Vec3 {
        // Rows of the matrix, from its columns.
        let m12 = self.z.y; // row 1, column 2
        let x = (-m12).clamp(-1.0, 1.0).asin();
        let (y, z) = if m12.abs() < 0.9999 {
            (self.z.x.atan2(self.z.z), self.x.y.atan2(self.y.y))
        } else {
            // Looking straight up or down: fold Z into Y.
            ((-self.x.z).atan2(self.x.x), 0.0)
        };
        let round = |r: f32| {
            let d = (r.to_degrees() * 1000.0).round() / 1000.0;
            if d == 0.0 { 0.0 } else { d }
        };
        Vec3::new(round(x), round(y), round(z))
    }

    pub fn transpose(self) -> Rotation {
        Rotation {
            x: Vec3::new(self.x.x, self.y.x, self.z.x),
            y: Vec3::new(self.x.y, self.y.y, self.z.y),
            z: Vec3::new(self.x.z, self.y.z, self.z.z),
        }
    }

    pub fn apply(self, v: Vec3) -> Vec3 {
        self.x * v.x + self.y * v.y + self.z * v.z
    }

    /// As a unit quaternion `(x, y, z, w)`.
    pub fn to_quat(self) -> [f32; 4] {
        let (m00, m11, m22) = (self.x.x, self.y.y, self.z.z);
        let trace = m00 + m11 + m22;
        let q = if trace > 0.0 {
            let s = (trace + 1.0).sqrt() * 2.0;
            [(self.y.z - self.z.y) / s, (self.z.x - self.x.z) / s, (self.x.y - self.y.x) / s, 0.25 * s]
        } else if m00 > m11 && m00 > m22 {
            let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
            [0.25 * s, (self.y.x + self.x.y) / s, (self.z.x + self.x.z) / s, (self.y.z - self.z.y) / s]
        } else if m11 > m22 {
            let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
            [(self.y.x + self.x.y) / s, 0.25 * s, (self.z.y + self.y.z) / s, (self.z.x - self.x.z) / s]
        } else {
            let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
            [(self.z.x + self.x.z) / s, (self.z.y + self.y.z) / s, 0.25 * s, (self.x.y - self.y.x) / s]
        };
        let len = q.iter().map(|c| c * c).sum::<f32>().sqrt();
        q.map(|c| c / len)
    }

    pub fn from_quat(q: [f32; 4]) -> Rotation {
        let [x, y, z, w] = q;
        Rotation {
            x: Vec3::new(1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + w * z), 2.0 * (x * z - w * y)),
            y: Vec3::new(2.0 * (x * y - w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + w * x)),
            z: Vec3::new(2.0 * (x * z + w * y), 2.0 * (y * z - w * x), 1.0 - 2.0 * (x * x + y * y)),
        }
    }

    /// Spherical interpolation towards `other`.
    pub fn slerp(self, other: Rotation, t: f32) -> Rotation {
        let a = self.to_quat();
        let mut b = other.to_quat();
        let mut dot: f32 = a.iter().zip(&b).map(|(p, q)| p * q).sum();
        if dot < 0.0 {
            b = b.map(|c| -c);
            dot = -dot;
        }
        let q = if dot > 0.9995 {
            let mut q = [0.0; 4];
            for i in 0..4 {
                q[i] = a[i] + (b[i] - a[i]) * t;
            }
            let len = q.iter().map(|c| c * c).sum::<f32>().sqrt();
            q.map(|c| c / len)
        } else {
            let theta = dot.acos();
            let (wa, wb) = (((1.0 - t) * theta).sin() / theta.sin(), (t * theta).sin() / theta.sin());
            let mut q = [0.0; 4];
            for i in 0..4 {
                q[i] = a[i] * wa + b[i] * wb;
            }
            q
        };
        Rotation::from_quat(q)
    }
}

impl Mul for Rotation {
    type Output = Rotation;
    fn mul(self, o: Rotation) -> Rotation {
        Rotation { x: self.apply(o.x), y: self.apply(o.y), z: self.apply(o.z) }
    }
}

/// A position plus a rotation: where something is and which way it faces.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CFrame {
    pub pos: Vec3,
    pub rot: Rotation,
}

impl CFrame {
    pub const IDENTITY: CFrame = CFrame { pos: Vec3::ZERO, rot: Rotation::IDENTITY };

    pub fn new(pos: Vec3, rot: Rotation) -> Self {
        Self { pos, rot }
    }

    /// At `pos`, facing `target` (forward is -Z), with up as close to +Y as possible.
    pub fn look_at(pos: Vec3, target: Vec3) -> CFrame {
        let forward = (target - pos).normalize_or_zero();
        if forward == Vec3::ZERO {
            return CFrame::new(pos, Rotation::IDENTITY);
        }
        let mut right = forward.cross(Vec3::new(0.0, 1.0, 0.0)).normalize_or_zero();
        if right == Vec3::ZERO {
            right = Vec3::new(1.0, 0.0, 0.0);
        }
        let up = right.cross(forward);
        CFrame::new(pos, Rotation { x: right, y: up, z: -forward })
    }

    pub fn inverse(self) -> CFrame {
        let rot = self.rot.transpose();
        CFrame::new(-rot.apply(self.pos), rot)
    }

    /// A point in this frame's space, in world space.
    pub fn point_to_world(self, p: Vec3) -> Vec3 {
        self.pos + self.rot.apply(p)
    }

    pub fn lerp(self, other: CFrame, t: f32) -> CFrame {
        CFrame::new(self.pos.lerp(other.pos, t), self.rot.slerp(other.rot, t))
    }
}

impl Mul for CFrame {
    type Output = CFrame;
    fn mul(self, o: CFrame) -> CFrame {
        CFrame::new(self.point_to_world(o.pos), self.rot * o.rot)
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

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-3
    }

    #[test]
    fn orientation_round_trips() {
        for d in [[0.0, 90.0, 0.0], [30.0, 45.0, 10.0], [-20.0, 170.0, 5.0], [0.0, 0.0, 90.0]] {
            let v = Vec3::new(d[0], d[1], d[2]);
            assert!(close(Rotation::from_orientation(v).to_orientation(), v), "{v:?}");
        }
        // A quarter turn around Y points the X axis at -Z.
        let r = Rotation::from_orientation(Vec3::new(0.0, 90.0, 0.0));
        assert!(close(r.x, Vec3::new(0.0, 0.0, -1.0)));
    }

    #[test]
    fn cframes_compose_and_invert() {
        let a = CFrame::new(Vec3::new(1.0, 2.0, 3.0), Rotation::about_y(0.7));
        let b = CFrame::new(Vec3::new(-4.0, 0.5, 2.0), Rotation::about_x(-0.3));
        let ab = a * b;
        let back = a.inverse() * ab;
        assert!(close(back.pos, b.pos));
        assert!(close(back.rot.x, b.rot.x) && close(back.rot.z, b.rot.z));
        let look = CFrame::look_at(Vec3::ZERO, Vec3::new(0.0, 0.0, -5.0));
        assert!(close(look.rot.z, Vec3::new(0.0, 0.0, 1.0)));
        let half = Rotation::IDENTITY.slerp(Rotation::about_y(1.0), 0.5);
        assert!(close(half.x, Rotation::about_y(0.5).x));
    }

    #[test]
    fn colour_from_bytes() {
        assert_eq!(Color3::from_rgb8(255, 0, 255), Color3::new(1.0, 0.0, 1.0));
    }
}
