//! The editor's terrain tools: a brush that adds, digs or paints cells
//! where you click, and a hill maker.

use libroxel_instance::voxels::Voxels;
use libroxel_instance::{Material, Vec3};

/// What the terrain brush does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrushMode {
    Add,
    Dig,
    Paint,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Brush {
    pub mode: BrushMode,
    pub material: Material,
    /// Across, in studs.
    pub size: f32,
}

impl Default for Brush {
    fn default() -> Self {
        Self { mode: BrushMode::Add, material: Material::Grass, size: 6.0 }
    }
}

/// Uses the brush where a ray met the terrain (or something else) at
/// `point`, on a surface facing `normal`. Returns how many cells changed.
pub fn apply_brush(voxels: &mut Voxels, brush: Brush, point: Vec3, normal: Vec3) -> usize {
    let radius = (brush.size / 2.0).max(0.5);
    match brush.mode {
        // Grow out of the surface, or dig into it.
        BrushMode::Add => voxels.fill_ball(point + normal * (radius * 0.4), radius, Some(brush.material)),
        BrushMode::Dig => voxels.fill_ball(point - normal * (radius * 0.4), radius, None),
        BrushMode::Paint => voxels.paint_ball(point, radius, brush.material),
    }
}

/// A smooth bumpy value from 0 to 1 for a point on the ground.
fn bumps(x: f32, z: f32, seed: u32) -> f32 {
    let hash = |ix: i32, iz: i32| {
        let mut n = (ix as u32).wrapping_mul(374_761_393)
            ^ (iz as u32).wrapping_mul(668_265_263)
            ^ seed.wrapping_mul(2_246_822_519);
        n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
        (n ^ (n >> 16)) as f32 / u32::MAX as f32
    };
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    let (fx, fz) = (x.floor(), z.floor());
    let (tx, tz) = (smooth(x - fx), smooth(z - fz));
    let (ix, iz) = (fx as i32, fz as i32);
    let top = hash(ix, iz) + (hash(ix + 1, iz) - hash(ix, iz)) * tx;
    let bottom = hash(ix, iz + 1) + (hash(ix + 1, iz + 1) - hash(ix, iz + 1)) * tx;
    top + (bottom - top) * tz
}

/// Builds a hill on the ground at height `base`: grass on top, rock under
/// it, a sandy edge, and snow on the highest bits.
pub fn hill(voxels: &mut Voxels, centre: Vec3, radius: f32, height: f32, seed: u32) {
    let r = radius.ceil() as i32;
    let (cx, cz) = (centre.x.floor() as i32, centre.z.floor() as i32);
    let base = centre.y.floor() as i32;
    let mut cells = Vec::new();
    for z in cz - r..=cz + r {
        for x in cx - r..=cx + r {
            let (dx, dz) = (x as f32 + 0.5 - centre.x, z as f32 + 0.5 - centre.z);
            let d = (dx * dx + dz * dz).sqrt() / radius;
            // A wobbly edge, so it isn't a perfect circle.
            let edge = 0.85 + 0.3 * bumps(x as f32 / 9.0, z as f32 / 9.0, seed + 7);
            if d > edge {
                continue;
            }
            let fall = 1.0 - (d / edge);
            let shape = fall * fall * (3.0 - 2.0 * fall);
            let lumps = bumps(x as f32 / 6.0, z as f32 / 6.0, seed) * 0.35 + 0.65;
            let h = ((height * shape * lumps).round() as i32).max(1);
            for y in 0..h {
                let depth = h - 1 - y;
                let material = if h <= 2 {
                    Material::Sand
                } else if depth >= 3 {
                    Material::Rock
                } else if depth == 0 && h as f32 > height * 0.8 {
                    Material::Snow
                } else {
                    Material::Grass
                };
                cells.push(((x, base + y, z), Some(material)));
            }
        }
    }
    voxels.set_cells(cells);
}

/// A square pool of water `depth` deep on the ground at `centre`, `2 * half`
/// cells across inside, with a rim of rock steps to climb in by.
pub fn pool(voxels: &mut Voxels, centre: Vec3, half: i32, depth: i32) {
    let (cx, base, cz) = (centre.x.floor() as i32, centre.y.floor() as i32, centre.z.floor() as i32);
    let outer = half + depth;
    let mut cells = Vec::new();
    for z in -outer..outer {
        for x in -outer..outer {
            // How many rings out from the water this column is (0 = water).
            let ring = (x.max(-x - 1) - half + 1).max(z.max(-z - 1) - half + 1).max(0);
            let (material, height) =
                if ring == 0 { (Material::Water, depth) } else { (Material::Rock, depth - ring + 1) };
            for y in 0..height {
                cells.push(((cx + x, base + y, cz + z), Some(material)));
            }
        }
    }
    voxels.set_cells(cells);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pool_has_water_inside_a_stepped_rim() {
        let mut v = Voxels::new();
        pool(&mut v, Vec3::new(0.0, 0.0, 0.0), 3, 4);
        assert!(v.is_water(0, 0, 0) && v.is_water(-3, 3, 2) && !v.is_water(0, 4, 0));
        // The rim steps down from the water's edge.
        let height = |x: i32| (0..10).filter(|y| v.is_solid(x, *y, 0)).count();
        assert_eq!([height(3), height(4), height(5), height(6), height(7)], [4, 3, 2, 1, 0]);
        assert_eq!([height(-4), height(-7), height(-8)], [4, 1, 0]);
    }

    #[test]
    fn a_hill_is_highest_in_the_middle_with_sand_round_the_edge() {
        let mut v = Voxels::new();
        hill(&mut v, Vec3::new(0.0, 0.0, 0.0), 20.0, 10.0, 3);
        let top = |x: i32, z: i32| (0..40).rev().find(|y| v.is_solid(x, *y, z));
        let middle = top(0, 0).unwrap();
        assert!(middle >= 6, "{middle}");
        assert!(top(30, 30).is_none());
        // Somewhere along a line out from the middle, the top turns to sand.
        let edge = (0..30).rev().find_map(|x| top(x, 0).map(|y| (x, y))).unwrap();
        assert_eq!(v.get(edge.0, edge.1, 0), Some(Material::Sand));
        assert_eq!(v.get(0, 0, 0), Some(Material::Rock));
    }

    #[test]
    fn brushes_add_dig_and_paint() {
        let mut v = Voxels::new();
        let up = Vec3::new(0.0, 1.0, 0.0);
        let mut brush = Brush { size: 4.0, ..Default::default() };
        assert!(apply_brush(&mut v, brush, Vec3::new(0.0, 0.0, 0.0), up) > 0);
        assert!(v.is_solid(0, 1, 0));
        brush.mode = BrushMode::Paint;
        brush.material = Material::Snow;
        apply_brush(&mut v, brush, Vec3::new(0.0, 1.0, 0.0), up);
        assert_eq!(v.get(0, 1, 0), Some(Material::Snow));
        brush.mode = BrushMode::Dig;
        apply_brush(&mut v, brush, Vec3::new(0.0, 3.0, 0.0), up);
        assert!(!v.is_solid(0, 2, 0));
    }
}
