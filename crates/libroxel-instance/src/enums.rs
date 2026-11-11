//! Enumerated property values, such as a part's shape or material.
//!
//! Like classes and properties, these are internal names. The scripting
//! layer decides what scripts call them; the file format uses
//! [`EnumValue::file_name`], which must never change.

/// The geometry of a part. Size is the bounding box in every case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PartShape {
    /// A box that fills its size.
    #[default]
    Block,
    /// A sphere whose diameter is the smallest side of its size.
    Ball,
    /// A cylinder lying along the part's X axis.
    Cylinder,
    /// A ramp: full height at the back (+Z), sloping down to the front (-Z).
    Wedge,
}

/// What a part's surface looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Material {
    #[default]
    Plastic,
    SmoothPlastic,
    /// Glows in its own colour.
    Neon,
    Metal,
    /// Shiny and see-through.
    Glass,
    Wood,
    Brick,
    Grass,
}

/// The shape of an animation curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EasingStyle {
    Linear,
    Sine,
    #[default]
    Quad,
    Cubic,
    Quart,
    Quint,
    Exponential,
    Circular,
    Back,
    Bounce,
    Elastic,
}

/// Which end of an animation curve the easing applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EasingDirection {
    In,
    #[default]
    Out,
    InOut,
}

impl EasingStyle {
    /// The eased-in curve: 0 at 0, 1 at 1.
    fn ease_in(self, t: f32) -> f32 {
        use std::f32::consts::PI;
        match self {
            EasingStyle::Linear => t,
            EasingStyle::Sine => 1.0 - (t * PI / 2.0).cos(),
            EasingStyle::Quad => t * t,
            EasingStyle::Cubic => t * t * t,
            EasingStyle::Quart => t.powi(4),
            EasingStyle::Quint => t.powi(5),
            EasingStyle::Exponential => {
                if t <= 0.0 {
                    0.0
                } else {
                    2f32.powf(10.0 * t - 10.0)
                }
            }
            EasingStyle::Circular => 1.0 - (1.0 - t * t).max(0.0).sqrt(),
            EasingStyle::Back => {
                let c = 1.70158;
                (c + 1.0) * t * t * t - c * t * t
            }
            EasingStyle::Bounce => 1.0 - bounce_out(1.0 - t),
            EasingStyle::Elastic => {
                if t <= 0.0 || t >= 1.0 {
                    t
                } else {
                    -(2f32.powf(10.0 * t - 10.0)) * ((t * 10.0 - 10.75) * (2.0 * PI / 3.0)).sin()
                }
            }
        }
    }

    /// Progress (0 to 1) after a fraction `t` of the time, eased in `direction`.
    pub fn ease(self, direction: EasingDirection, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match direction {
            EasingDirection::In => self.ease_in(t),
            EasingDirection::Out => 1.0 - self.ease_in(1.0 - t),
            EasingDirection::InOut => {
                if t < 0.5 {
                    self.ease_in(t * 2.0) / 2.0
                } else {
                    1.0 - self.ease_in((1.0 - t) * 2.0) / 2.0
                }
            }
        }
    }
}

fn bounce_out(t: f32) -> f32 {
    let (n, d) = (7.5625, 2.75);
    if t < 1.0 / d {
        n * t * t
    } else if t < 2.0 / d {
        let t = t - 1.5 / d;
        n * t * t + 0.75
    } else if t < 2.5 / d {
        let t = t - 2.25 / d;
        n * t * t + 0.9375
    } else {
        let t = t - 2.625 / d;
        n * t * t + 0.984375
    }
}

/// Which enumeration a value belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnumKind {
    Shape,
    Material,
    EasingStyle,
    EasingDirection,
}

/// A value from one of the enumerations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnumValue {
    Shape(PartShape),
    Material(Material),
    EasingStyle(EasingStyle),
    EasingDirection(EasingDirection),
}

impl EnumKind {
    pub const ALL: &[EnumKind] =
        &[EnumKind::Shape, EnumKind::Material, EnumKind::EasingStyle, EnumKind::EasingDirection];

    /// Every value of this enumeration, in a stable order.
    pub fn items(self) -> &'static [EnumValue] {
        use EnumValue as E;
        match self {
            EnumKind::Shape => &[
                E::Shape(PartShape::Block),
                E::Shape(PartShape::Ball),
                E::Shape(PartShape::Cylinder),
                E::Shape(PartShape::Wedge),
            ],
            EnumKind::Material => &[
                E::Material(Material::Plastic),
                E::Material(Material::SmoothPlastic),
                E::Material(Material::Neon),
                E::Material(Material::Metal),
                E::Material(Material::Glass),
                E::Material(Material::Wood),
                E::Material(Material::Brick),
                E::Material(Material::Grass),
            ],
            EnumKind::EasingStyle => &[
                E::EasingStyle(EasingStyle::Linear),
                E::EasingStyle(EasingStyle::Sine),
                E::EasingStyle(EasingStyle::Quad),
                E::EasingStyle(EasingStyle::Cubic),
                E::EasingStyle(EasingStyle::Quart),
                E::EasingStyle(EasingStyle::Quint),
                E::EasingStyle(EasingStyle::Exponential),
                E::EasingStyle(EasingStyle::Circular),
                E::EasingStyle(EasingStyle::Back),
                E::EasingStyle(EasingStyle::Bounce),
                E::EasingStyle(EasingStyle::Elastic),
            ],
            EnumKind::EasingDirection => &[
                E::EasingDirection(EasingDirection::In),
                E::EasingDirection(EasingDirection::Out),
                E::EasingDirection(EasingDirection::InOut),
            ],
        }
    }

    /// The value kind name used in type checks (see [`crate::PropertyValue::kind_name`]).
    pub fn kind_name(self) -> &'static str {
        match self {
            EnumKind::Shape => "enum:shape",
            EnumKind::Material => "enum:material",
            EnumKind::EasingStyle => "enum:easing_style",
            EnumKind::EasingDirection => "enum:easing_direction",
        }
    }

    /// Looks a value up by its file name.
    pub fn from_file_name(self, name: &str) -> Option<EnumValue> {
        self.items().iter().copied().find(|v| v.file_name() == name)
    }
}

impl EnumValue {
    pub fn kind(self) -> EnumKind {
        match self {
            EnumValue::Shape(_) => EnumKind::Shape,
            EnumValue::Material(_) => EnumKind::Material,
            EnumValue::EasingStyle(_) => EnumKind::EasingStyle,
            EnumValue::EasingDirection(_) => EnumKind::EasingDirection,
        }
    }

    /// The position of this value in [`EnumKind::items`].
    pub fn index(self) -> usize {
        self.kind().items().iter().position(|v| *v == self).expect("every value is listed")
    }

    /// A stable name for files. Never shown to scripts; never change these.
    pub fn file_name(self) -> &'static str {
        match self {
            EnumValue::Shape(s) => match s {
                PartShape::Block => "block",
                PartShape::Ball => "ball",
                PartShape::Cylinder => "cylinder",
                PartShape::Wedge => "wedge",
            },
            EnumValue::Material(m) => match m {
                Material::Plastic => "plastic",
                Material::SmoothPlastic => "smooth_plastic",
                Material::Neon => "neon",
                Material::Metal => "metal",
                Material::Glass => "glass",
                Material::Wood => "wood",
                Material::Brick => "brick",
                Material::Grass => "grass",
            },
            EnumValue::EasingStyle(e) => match e {
                EasingStyle::Linear => "linear",
                EasingStyle::Sine => "sine",
                EasingStyle::Quad => "quad",
                EasingStyle::Cubic => "cubic",
                EasingStyle::Quart => "quart",
                EasingStyle::Quint => "quint",
                EasingStyle::Exponential => "exponential",
                EasingStyle::Circular => "circular",
                EasingStyle::Back => "back",
                EasingStyle::Bounce => "bounce",
                EasingStyle::Elastic => "elastic",
            },
            EnumValue::EasingDirection(d) => match d {
                EasingDirection::In => "in",
                EasingDirection::Out => "out",
                EasingDirection::InOut => "in_out",
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_curves_start_at_zero_and_end_at_one() {
        for &item in EnumKind::EasingStyle.items() {
            let EnumValue::EasingStyle(style) = item else { unreachable!() };
            for &dir in &[EasingDirection::In, EasingDirection::Out, EasingDirection::InOut] {
                assert!(style.ease(dir, 0.0).abs() < 1e-3, "{style:?} {dir:?}");
                assert!((style.ease(dir, 1.0) - 1.0).abs() < 1e-3, "{style:?} {dir:?}");
            }
        }
        assert!((EasingStyle::Linear.ease(EasingDirection::In, 0.25) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn file_names_round_trip() {
        for &kind in EnumKind::ALL {
            for (i, &item) in kind.items().iter().enumerate() {
                assert_eq!(item.kind(), kind);
                assert_eq!(item.index(), i);
                assert_eq!(kind.from_file_name(item.file_name()), Some(item));
            }
        }
    }
}
