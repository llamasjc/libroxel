//! What a part is made of, for physics: how heavy it is for its size, how
//! much it grips, and how much it bounces. Each material has its own, as
//! Roblox documents them, and a part's `CustomPhysicalProperties` can set
//! its own instead.

use crate::{EnumValue, InstanceId, InstanceTree, Material, Property, PropertyValue};

/// The extra a part keeps its custom physical properties in: five numbers
/// (density, friction, elasticity, friction weight, elasticity weight).
/// Not the property's own name, which would show scripts the raw text.
pub const CUSTOM: &str = "libroxel.physical";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Physical {
    /// Mass for each cubic stud.
    pub density: f32,
    /// 0 slides like ice, 2 grips completely.
    pub friction: f32,
    /// 0 doesn't bounce, 1 bounces back as high as it fell.
    pub elasticity: f32,
    /// How much this part's friction counts against what it touches.
    pub friction_weight: f32,
    /// How much this part's elasticity counts against what it touches.
    pub elasticity_weight: f32,
}

impl Physical {
    const fn new(density: f32, friction: f32, elasticity: f32) -> Self {
        Physical { density, friction, elasticity, friction_weight: 1.0, elasticity_weight: 1.0 }
    }

    /// What `material` is made of.
    pub fn of_material(material: Material) -> Physical {
        use Material as M;
        match material {
            M::Plastic | M::Air | M::Water => Physical::new(0.7, 0.3, 0.5),
            M::SmoothPlastic => Physical::new(0.7, 0.2, 0.5),
            M::Neon => Physical::new(0.7, 0.3, 0.2),
            M::Wood | M::WoodPlanks => Physical::new(0.35, 0.48, 0.2),
            M::Marble => Physical::new(2.563, 0.2, 0.17),
            M::Basalt => Physical::new(2.691, 0.7, 0.15),
            M::Slate | M::Granite => Physical::new(2.691, 0.4, 0.2),
            M::CrackedLava => Physical::new(2.691, 0.65, 0.15),
            M::Concrete => Physical::new(2.403, 0.7, 0.2),
            M::Limestone | M::Pavement => Physical::new(2.691, 0.5, 0.2),
            M::Brick => Physical::new(1.922, 0.8, 0.15),
            M::Pebble => Physical::new(2.403, 0.4, 0.17),
            M::Cobblestone | M::Rock => Physical::new(2.691, 0.5, 0.17),
            M::Sandstone => Physical::new(2.691, 0.5, 0.15),
            M::CorrodedMetal => Physical::new(7.85, 0.7, 0.2),
            M::DiamondPlate => Physical::new(7.85, 0.35, 0.25),
            M::Foil => Physical::new(2.7, 0.4, 0.25),
            M::Metal => Physical::new(7.85, 0.4, 0.25),
            M::Grass | M::LeafyGrass => Physical::new(0.9, 0.4, 0.1),
            M::Ground => Physical::new(0.9, 0.45, 0.1),
            M::Sand => Physical::new(1.6, 0.5, 0.05),
            M::Fabric => Physical::new(0.7, 0.35, 0.05),
            M::Snow => Physical::new(0.9, 0.3, 0.03),
            M::Mud => Physical::new(0.9, 0.3, 0.07),
            M::Asphalt => Physical::new(2.36, 0.8, 0.2),
            M::Salt => Physical::new(2.16, 0.5, 0.05),
            M::Ice => Physical::new(0.919, 0.02, 0.15),
            M::Glacier => Physical::new(0.919, 0.05, 0.15),
            M::Glass | M::ForceField => Physical::new(2.4, 0.25, 0.2),
        }
    }

    /// The five numbers, in the order `PhysicalProperties.new` takes them.
    pub fn to_array(self) -> [f32; 5] {
        [self.density, self.friction, self.elasticity, self.friction_weight, self.elasticity_weight]
    }

    /// From the five numbers, kept to the ranges Roblox allows.
    pub fn from_array(n: [f32; 5]) -> Physical {
        Physical {
            density: n[0].clamp(0.01, 100.0),
            friction: n[1].clamp(0.0, 2.0),
            elasticity: n[2].clamp(0.0, 1.0),
            friction_weight: n[3].clamp(0.0, 100.0),
            elasticity_weight: n[4].clamp(0.0, 100.0),
        }
    }
}

/// A part's custom physical properties, if it has them.
pub fn custom(tree: &InstanceTree, id: InstanceId) -> Option<Physical> {
    let Ok(Some(PropertyValue::String(text))) = tree.get_extra(id, CUSTOM) else { return None };
    let mut n = [0.0f32; 5];
    let mut parts = text.split_whitespace();
    for slot in &mut n {
        *slot = parts.next()?.parse().ok()?;
    }
    Some(Physical::from_array(n))
}

/// Gives a part custom physical properties, or takes them away with `None`.
pub fn set_custom(
    tree: &mut InstanceTree,
    id: InstanceId,
    physical: Option<Physical>,
) -> crate::tree::Result<()> {
    let value = physical.map(|p| {
        let n = p.to_array();
        PropertyValue::String(format!("{} {} {} {} {}", n[0], n[1], n[2], n[3], n[4]))
    });
    tree.set_extra(id, CUSTOM, value)
}

/// What a part is made of: its custom physical properties, or else its material's.
pub fn of_part(tree: &InstanceTree, id: InstanceId) -> Physical {
    custom(tree, id).unwrap_or_else(|| {
        let material = match tree.get(id, Property::Material) {
            Ok(PropertyValue::Enum(EnumValue::Material(m))) => m,
            _ => Material::Plastic,
        };
        Physical::of_material(material)
    })
}

/// A part's mass: its size times its density.
pub fn mass(tree: &InstanceTree, id: InstanceId) -> f32 {
    let s = tree.get_vec3(id, Property::Size).unwrap_or_default();
    (s.x * s.y * s.z * of_part(tree, id).density).max(0.01)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClassKind, Vec3};

    #[test]
    fn parts_weigh_and_grip_by_material_unless_set_otherwise() {
        let mut tree = InstanceTree::new();
        let part = tree.create(ClassKind::Part, "Block").unwrap();
        tree.set(part, Property::Size, PropertyValue::Vec3(Vec3::new(2.0, 1.0, 4.0))).unwrap();
        // Plastic, as before materials counted.
        assert!((mass(&tree, part) - 8.0 * 0.7).abs() < 1e-4);
        tree.set(part, Property::Material, PropertyValue::Enum(EnumValue::Material(Material::Ice))).unwrap();
        assert!(of_part(&tree, part).friction < 0.05, "ice is slippery");
        let rubber = Physical {
            density: 1.0,
            friction: 1.5,
            elasticity: 0.9,
            friction_weight: 1.0,
            elasticity_weight: 1.0,
        };
        set_custom(&mut tree, part, Some(rubber)).unwrap();
        assert_eq!(of_part(&tree, part), rubber);
        assert!((mass(&tree, part) - 8.0).abs() < 1e-4);
        set_custom(&mut tree, part, None).unwrap();
        assert!(of_part(&tree, part).friction < 0.05);
        // Out of Roblox's ranges: kept inside them.
        assert_eq!(Physical::from_array([0.0, 9.0, -1.0, 1.0, 1.0]).friction, 2.0);
    }
}
