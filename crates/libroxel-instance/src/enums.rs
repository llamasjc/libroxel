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

/// Which enumeration a value belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnumKind {
    Shape,
    Material,
}

/// A value from one of the enumerations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnumValue {
    Shape(PartShape),
    Material(Material),
}

impl EnumKind {
    pub const ALL: &[EnumKind] = &[EnumKind::Shape, EnumKind::Material];

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
        }
    }

    /// The value kind name used in type checks (see [`crate::PropertyValue::kind_name`]).
    pub fn kind_name(self) -> &'static str {
        match self {
            EnumKind::Shape => "enum:shape",
            EnumKind::Material => "enum:material",
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
