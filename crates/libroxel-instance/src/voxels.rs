//! Terrain voxels: a grid of one-stud cubes, each empty (air) or filled
//! with a material. Cell `(x, y, z)` covers the box from `(x, y, z)` to
//! `(x + 1, y + 1, z + 1)` in world studs.
//!
//! Cells are kept in chunks of 16 × 16 × 16. Saved and sent as text: each
//! chunk is `cx,cy,cz:` followed by runs of `code*count`, chunks joined by
//! `;`, where a code is a material's place in [`TERRAIN_MATERIALS`] plus one.

use std::collections::BTreeMap;

use crate::{Material, Vec3};

/// The size of a chunk along each side, in cells.
pub const CHUNK: i32 = 16;
const CHUNK_CELLS: usize = (CHUNK * CHUNK * CHUNK) as usize;

/// Materials terrain cells can be, in the order of their codes (code 0 is air).
pub const TERRAIN_MATERIALS: &[Material] = &[
    Material::Grass,
    Material::Sand,
    Material::Rock,
    Material::Snow,
    Material::Brick,
    Material::Wood,
    Material::Plastic,
    Material::SmoothPlastic,
    Material::Metal,
    Material::Neon,
    Material::Glass,
    // New materials go at the end, so saved codes keep their meaning.
    Material::Water,
];

/// The code a material is stored as; 0 for air, or anything terrain can't be.
fn code_of(material: Material) -> u8 {
    TERRAIN_MATERIALS.iter().position(|m| *m == material).map_or(0, |i| i as u8 + 1)
}

fn material_of(code: u8) -> Option<Material> {
    code.checked_sub(1).and_then(|i| TERRAIN_MATERIALS.get(i as usize)).copied()
}

/// Every cell of terrain.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Voxels {
    chunks: BTreeMap<(i32, i32, i32), Box<[u8; CHUNK_CELLS]>>,
    /// Goes up with every change, so viewers know when to look again.
    version: u64,
}

fn split(v: i32) -> (i32, usize) {
    (v.div_euclid(CHUNK), v.rem_euclid(CHUNK) as usize)
}

fn index(x: usize, y: usize, z: usize) -> usize {
    (y * CHUNK as usize + z) * CHUNK as usize + x
}

/// The cell a point is in.
pub fn cell_at(p: Vec3) -> (i32, i32, i32) {
    (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32)
}

impl Voxels {
    pub fn new() -> Self {
        Self::default()
    }

    /// Changes with every edit.
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// What fills a cell, or `None` for air.
    pub fn get(&self, x: i32, y: i32, z: i32) -> Option<Material> {
        let ((cx, lx), (cy, ly), (cz, lz)) = (split(x), split(y), split(z));
        let chunk = self.chunks.get(&(cx, cy, cz))?;
        material_of(chunk[index(lx, ly, lz)])
    }

    /// Whether a cell is filled with something that isn't water.
    pub fn is_solid(&self, x: i32, y: i32, z: i32) -> bool {
        self.get(x, y, z).is_some_and(|m| m != Material::Water)
    }

    /// Whether a cell is water.
    pub fn is_water(&self, x: i32, y: i32, z: i32) -> bool {
        self.get(x, y, z) == Some(Material::Water)
    }

    /// Whether the point `p` is in water.
    pub fn water_at(&self, p: Vec3) -> bool {
        let (x, y, z) = cell_at(p);
        self.is_water(x, y, z)
    }

    fn put(&mut self, x: i32, y: i32, z: i32, code: u8) {
        let ((cx, lx), (cy, ly), (cz, lz)) = (split(x), split(y), split(z));
        if code == 0 {
            if let Some(chunk) = self.chunks.get_mut(&(cx, cy, cz)) {
                chunk[index(lx, ly, lz)] = 0;
            }
        } else {
            let chunk = self.chunks.entry((cx, cy, cz)).or_insert_with(|| Box::new([0; CHUNK_CELLS]));
            chunk[index(lx, ly, lz)] = code;
        }
    }

    fn tidy(&mut self) {
        self.chunks.retain(|_, c| c.iter().any(|v| *v != 0));
        self.version += 1;
    }

    /// Fills one cell (`None` empties it).
    pub fn set(&mut self, x: i32, y: i32, z: i32, material: Option<Material>) {
        self.put(x, y, z, material.map_or(0, code_of));
        self.tidy();
    }

    /// Fills every cell whose middle is inside `test`, within the cells
    /// from `min` to `max` (inclusive). Returns how many cells changed.
    fn fill_where(
        &mut self,
        min: (i32, i32, i32),
        max: (i32, i32, i32),
        material: Option<Material>,
        test: impl Fn(Vec3) -> bool,
    ) -> usize {
        let code = material.map_or(0, code_of);
        let mut changed = 0;
        for y in min.1..=max.1 {
            for z in min.2..=max.2 {
                for x in min.0..=max.0 {
                    let middle = Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
                    if test(middle) && self.get(x, y, z).map_or(0, code_of) != code {
                        self.put(x, y, z, code);
                        changed += 1;
                    }
                }
            }
        }
        if changed > 0 {
            self.tidy();
        }
        changed
    }

    /// Fills a box, turned by `rotation` (its three axes), centred on
    /// `centre`, `size` studs big. Air empties it.
    pub fn fill_block(
        &mut self,
        centre: Vec3,
        axes: [Vec3; 3],
        size: Vec3,
        material: Option<Material>,
    ) -> usize {
        let half = size * 0.5;
        // The box's reach along each world axis.
        let reach = Vec3::new(
            (axes[0].x * half.x).abs() + (axes[1].x * half.y).abs() + (axes[2].x * half.z).abs(),
            (axes[0].y * half.x).abs() + (axes[1].y * half.y).abs() + (axes[2].y * half.z).abs(),
            (axes[0].z * half.x).abs() + (axes[1].z * half.y).abs() + (axes[2].z * half.z).abs(),
        );
        let (min, max) = (cell_at(centre - reach), cell_at(centre + reach));
        self.fill_where(min, max, material, |p| {
            let d = p - centre;
            d.dot(axes[0]).abs() <= half.x && d.dot(axes[1]).abs() <= half.y && d.dot(axes[2]).abs() <= half.z
        })
    }

    /// Fills an upright (along `axes[1]`) cylinder `height` tall. Air empties it.
    pub fn fill_cylinder(
        &mut self,
        centre: Vec3,
        axes: [Vec3; 3],
        height: f32,
        radius: f32,
        material: Option<Material>,
    ) -> usize {
        let reach = height.max(radius * 2.0);
        let r = Vec3::new(reach, reach, reach);
        let (min, max) = (cell_at(centre - r), cell_at(centre + r));
        self.fill_where(min, max, material, |p| {
            let d = p - centre;
            let along = d.dot(axes[1]);
            let off = d - axes[1] * along;
            along.abs() <= height / 2.0 && off.length() <= radius
        })
    }

    /// Fills a ball. Air empties it.
    pub fn fill_ball(&mut self, centre: Vec3, radius: f32, material: Option<Material>) -> usize {
        let r = Vec3::new(radius, radius, radius);
        let (min, max) = (cell_at(centre - r), cell_at(centre + r));
        self.fill_where(min, max, material, |p| (p - centre).length() <= radius)
    }

    /// Sets many cells at once (`None` empties one).
    pub fn set_cells(&mut self, cells: impl IntoIterator<Item = ((i32, i32, i32), Option<Material>)>) {
        for ((x, y, z), material) in cells {
            self.put(x, y, z, material.map_or(0, code_of));
        }
        self.tidy();
    }

    /// Changes the material of the filled cells in a ball, leaving air alone.
    pub fn paint_ball(&mut self, centre: Vec3, radius: f32, material: Material) -> usize {
        let r = Vec3::new(radius, radius, radius);
        let (min, max) = (cell_at(centre - r), cell_at(centre + r));
        let code = code_of(material);
        let mut changed = Vec::new();
        for y in min.1..=max.1 {
            for z in min.2..=max.2 {
                for x in min.0..=max.0 {
                    let middle = Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
                    if (middle - centre).length() <= radius
                        && self.get(x, y, z).is_some_and(|m| code_of(m) != code)
                    {
                        changed.push(((x, y, z), Some(material)));
                    }
                }
            }
        }
        let n = changed.len();
        if n > 0 {
            self.set_cells(changed);
        }
        n
    }

    /// Empties every cell.
    pub fn clear(&mut self) {
        if !self.chunks.is_empty() {
            self.chunks.clear();
            self.version += 1;
        }
    }

    /// The chunks that have anything in them, by chunk position.
    pub fn chunks(&self) -> impl Iterator<Item = (i32, i32, i32)> + '_ {
        self.chunks.keys().copied()
    }

    /// A number that changes when a chunk's cells do (0 for an empty chunk).
    pub fn chunk_fingerprint(&self, chunk: (i32, i32, i32)) -> u64 {
        use std::hash::{Hash, Hasher};
        let Some(cells) = self.chunks.get(&chunk) else { return 0 };
        let mut h = std::collections::hash_map::DefaultHasher::new();
        cells[..].hash(&mut h);
        h.finish() | 1
    }

    /// Every filled cell of one chunk, as world cell positions.
    pub fn cells_in_chunk(&self, chunk: (i32, i32, i32)) -> Vec<((i32, i32, i32), Material)> {
        let Some(cells) = self.chunks.get(&chunk) else { return Vec::new() };
        let mut out = Vec::new();
        for y in 0..CHUNK as usize {
            for z in 0..CHUNK as usize {
                for x in 0..CHUNK as usize {
                    if let Some(m) = material_of(cells[index(x, y, z)]) {
                        let at = (
                            chunk.0 * CHUNK + x as i32,
                            chunk.1 * CHUNK + y as i32,
                            chunk.2 * CHUNK + z as i32,
                        );
                        out.push((at, m));
                    }
                }
            }
        }
        out
    }

    /// Filled cells inside the box from `min` to `max` (world studs), as
    /// boxes: runs of cells along X merged into one.
    pub fn boxes_in(&self, min: Vec3, max: Vec3) -> Vec<(Vec3, Vec3)> {
        let (lo, hi) = (cell_at(min), cell_at(max));
        let mut out = Vec::new();
        for y in lo.1..=hi.1 {
            for z in lo.2..=hi.2 {
                let mut run: Option<i32> = None;
                for x in lo.0..=hi.0 + 1 {
                    let solid = x <= hi.0 && self.is_solid(x, y, z);
                    match (solid, run) {
                        (true, None) => run = Some(x),
                        (false, Some(start)) => {
                            out.push((
                                Vec3::new(start as f32, y as f32, z as f32),
                                Vec3::new(x as f32, y as f32 + 1.0, z as f32 + 1.0),
                            ));
                            run = None;
                        }
                        _ => {}
                    }
                }
            }
        }
        out
    }

    /// The first filled cell a ray from `origin` along `dir` (a unit vector)
    /// meets within `reach` studs: the distance, the face's outward normal and
    /// the cell. A ray starting inside a filled cell doesn't hit it.
    pub fn raycast(&self, origin: Vec3, dir: Vec3, reach: f32) -> Option<(f32, Vec3, (i32, i32, i32))> {
        if self.chunks.is_empty() {
            return None;
        }
        let mut cell = cell_at(origin);
        let step = |d: f32| {
            if d > 0.0 {
                1
            } else if d < 0.0 {
                -1
            } else {
                0
            }
        };
        let (sx, sy, sz) = (step(dir.x), step(dir.y), step(dir.z));
        let next = |p: f32, c: i32, s: i32, d: f32| {
            if s == 0 { f32::INFINITY } else { ((c + (s > 0) as i32) as f32 - p) / d }
        };
        let delta = |d: f32| if d == 0.0 { f32::INFINITY } else { 1.0 / d.abs() };
        let mut t_max = Vec3::new(
            next(origin.x, cell.0, sx, dir.x),
            next(origin.y, cell.1, sy, dir.y),
            next(origin.z, cell.2, sz, dir.z),
        );
        let t_delta = Vec3::new(delta(dir.x), delta(dir.y), delta(dir.z));
        loop {
            let (t, normal) = if t_max.x <= t_max.y && t_max.x <= t_max.z {
                cell.0 += sx;
                let t = t_max.x;
                t_max.x += t_delta.x;
                (t, Vec3::new(-sx as f32, 0.0, 0.0))
            } else if t_max.y <= t_max.z {
                cell.1 += sy;
                let t = t_max.y;
                t_max.y += t_delta.y;
                (t, Vec3::new(0.0, -sy as f32, 0.0))
            } else {
                cell.2 += sz;
                let t = t_max.z;
                t_max.z += t_delta.z;
                (t, Vec3::new(0.0, 0.0, -sz as f32))
            };
            if t > reach || !t.is_finite() {
                return None;
            }
            if self.is_solid(cell.0, cell.1, cell.2) {
                return Some((t, normal, cell));
            }
        }
    }

    /// As text, for saving and sending.
    pub fn encode(&self) -> String {
        let mut out = String::new();
        for (&(cx, cy, cz), cells) in &self.chunks {
            if !out.is_empty() {
                out.push(';');
            }
            out.push_str(&format!("{cx},{cy},{cz}:"));
            let mut i = 0;
            let mut first = true;
            while i < CHUNK_CELLS {
                let code = cells[i];
                let mut n = 1;
                while i + n < CHUNK_CELLS && cells[i + n] == code {
                    n += 1;
                }
                if !first {
                    out.push(',');
                }
                first = false;
                if n == 1 {
                    out.push_str(&code.to_string());
                } else {
                    out.push_str(&format!("{code}*{n}"));
                }
                i += n;
            }
        }
        out
    }

    /// Reads [`encode`](Self::encode)'s text. Anything malformed is skipped.
    pub fn decode(text: &str) -> Voxels {
        let mut voxels = Voxels::default();
        for chunk in text.split(';').filter(|c| !c.trim().is_empty()) {
            let Some((at, runs)) = chunk.split_once(':') else { continue };
            let at: Vec<i32> = at.split(',').filter_map(|v| v.trim().parse().ok()).collect();
            let [cx, cy, cz] = at[..] else { continue };
            let mut cells = Box::new([0u8; CHUNK_CELLS]);
            let mut i = 0;
            for run in runs.split(',') {
                let (code, n) = match run.split_once('*') {
                    Some((c, n)) => (c.trim().parse::<u8>().ok(), n.trim().parse::<usize>().ok()),
                    None => (run.trim().parse::<u8>().ok(), Some(1)),
                };
                let (Some(code), Some(n)) = (code, n) else { break };
                let code = if material_of(code).is_some() { code } else { 0 };
                let end = (i + n).min(CHUNK_CELLS);
                cells[i..end].fill(code);
                i = end;
            }
            if cells.iter().any(|c| *c != 0) {
                voxels.chunks.insert((cx, cy, cz), cells);
            }
        }
        voxels.version = 1;
        voxels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AXES: [Vec3; 3] = [Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)];

    #[test]
    fn blocks_and_balls_fill_cells_and_air_empties_them() {
        let mut v = Voxels::new();
        let n =
            v.fill_block(Vec3::new(0.0, -2.0, 0.0), AXES, Vec3::new(8.0, 4.0, 8.0), Some(Material::Grass));
        assert_eq!(n, 8 * 4 * 8);
        assert_eq!(v.get(-4, -4, -4), Some(Material::Grass));
        assert_eq!(v.get(3, -1, 3), Some(Material::Grass));
        assert_eq!(v.get(4, -1, 0), None);
        assert_eq!(v.get(0, 0, 0), None);
        let dug = v.fill_ball(Vec3::new(0.0, 0.0, 0.0), 2.0, None);
        assert!(dug > 0);
        assert_eq!(v.get(0, -1, 0), None);
        assert_eq!(v.get(-4, -4, -4), Some(Material::Grass));
        v.clear();
        assert!(v.is_empty());
    }

    #[test]
    fn text_round_trips() {
        let mut v = Voxels::new();
        v.fill_block(Vec3::new(10.0, 0.0, -30.0), AXES, Vec3::new(20.0, 2.0, 6.0), Some(Material::Sand));
        v.fill_ball(Vec3::new(10.0, 3.0, -30.0), 3.0, Some(Material::Rock));
        let text = v.encode();
        let back = Voxels::decode(&text);
        assert_eq!(back.chunks, v.chunks);
        assert!(text.len() < 2000, "{}", text.len());
        assert!(Voxels::decode("nonsense;1,2:3").is_empty());
    }

    #[test]
    fn rays_stop_at_the_first_filled_cell() {
        let mut v = Voxels::new();
        v.fill_block(Vec3::new(0.0, -0.5, 0.0), AXES, Vec3::new(10.0, 1.0, 10.0), Some(Material::Grass));
        let (t, normal, cell) =
            v.raycast(Vec3::new(0.3, 5.0, 0.2), Vec3::new(0.0, -1.0, 0.0), 100.0).unwrap();
        assert!((t - 5.0).abs() < 1e-4);
        assert_eq!(normal, Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(cell, (0, -1, 0));
        assert!(v.raycast(Vec3::new(0.3, 5.0, 0.2), Vec3::new(0.0, 1.0, 0.0), 100.0).is_none());
        assert!(v.raycast(Vec3::new(0.3, 5.0, 0.2), Vec3::new(0.0, -1.0, 0.0), 4.0).is_none());
        let side = Vec3::new(-1.0, -1.0, 0.0).normalize_or_zero();
        let (_, normal, _) = v.raycast(Vec3::new(10.0, 4.0, 0.5), side, 100.0).unwrap();
        assert_eq!(normal, Vec3::new(1.0, 0.0, 0.0));
    }

    #[test]
    fn merged_boxes_cover_the_filled_cells() {
        let mut v = Voxels::new();
        v.fill_block(Vec3::new(0.0, 0.5, 0.5), AXES, Vec3::new(6.0, 1.0, 1.0), Some(Material::Rock));
        let boxes = v.boxes_in(Vec3::new(-10.0, 0.0, 0.0), Vec3::new(10.0, 0.5, 0.5));
        assert_eq!(boxes, vec![(Vec3::new(-3.0, 0.0, 0.0), Vec3::new(3.0, 1.0, 1.0))]);
    }
}
