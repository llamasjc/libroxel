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
    Material::Slate,
    Material::Concrete,
    Material::Cobblestone,
    Material::Ice,
    Material::WoodPlanks,
    Material::Mud,
    Material::Ground,
    Material::Basalt,
    Material::Asphalt,
    Material::LeafyGrass,
    Material::Salt,
    Material::Limestone,
    Material::Pavement,
    Material::Sandstone,
    Material::Glacier,
    Material::CrackedLava,
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
    /// Each chunk's fingerprint, worked out when it changes (viewers ask
    /// for every chunk's after each edit; hashing them all each time cost
    /// more than the edits on a big map).
    prints: BTreeMap<(i32, i32, i32), u64>,
    /// Goes up with every change, so viewers know when to look again.
    version: u64,
}

fn print_of(cells: &[u8; CHUNK_CELLS]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    cells[..].hash(&mut h);
    h.finish() | 1
}

fn split(v: i32) -> (i32, usize) {
    (v.div_euclid(CHUNK), v.rem_euclid(CHUNK) as usize)
}

fn index(x: usize, y: usize, z: usize) -> usize {
    (y * CHUNK as usize + z) * CHUNK as usize + x
}

/// The most cells one fill or replace looks at: a flat 4096 by 4096 stud
/// map 16 studs deep. Bigger (or endless, from a NaN or infinite size)
/// would keep the game busy for minutes and take gigabytes.
pub const MOST_EDIT_CELLS: u64 = 1 << 28;

/// Whether the cells from `min` to `max` (inclusive) are few enough for one edit.
pub fn fits_one_edit(min: (i32, i32, i32), max: (i32, i32, i32)) -> bool {
    let span = |lo: i32, hi: i32| (i64::from(hi) - i64::from(lo) + 1).max(0) as u64;
    span(min.0, max.0)
        .checked_mul(span(min.1, max.1))
        .and_then(|n| n.checked_mul(span(min.2, max.2)))
        .is_some_and(|n| n <= MOST_EDIT_CELLS)
}

/// The furthest a ray walks through terrain, as for parts.
const MOST_RAY_REACH: f32 = 1.0e6;

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

    /// Drops the given chunks if they ended up empty. (Only the chunks an
    /// edit touched: looking over every chunk of a big map on each edit
    /// cost more than the edit.)
    fn tidy(&mut self, touched: impl IntoIterator<Item = (i32, i32, i32)>) {
        for key in touched {
            match self.chunks.get(&key) {
                Some(c) if c.iter().all(|v| *v == 0) => {
                    self.chunks.remove(&key);
                    self.prints.remove(&key);
                }
                Some(c) => {
                    self.prints.insert(key, print_of(c));
                }
                None => {
                    self.prints.remove(&key);
                }
            }
        }
    }

    /// Fills one cell (`None` empties it).
    pub fn set(&mut self, x: i32, y: i32, z: i32, material: Option<Material>) {
        self.put(x, y, z, material.map_or(0, code_of));
        self.tidy([(x.div_euclid(CHUNK), y.div_euclid(CHUNK), z.div_euclid(CHUNK))]);
        self.version += 1;
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
        self.change_where(min, max, code != 0, |p, now| {
            (now != code && test(Vec3::new(p.0 as f32 + 0.5, p.1 as f32 + 0.5, p.2 as f32 + 0.5)))
                .then_some(code)
        })
    }

    /// Turns every cell of `from` in the cells from `min` to `max`
    /// (inclusive) into `to` (`None` is air). Returns how many changed.
    pub fn replace(
        &mut self,
        min: (i32, i32, i32),
        max: (i32, i32, i32),
        from: Option<Material>,
        to: Option<Material>,
    ) -> usize {
        let (from, to) = (from.map_or(0, code_of), to.map_or(0, code_of));
        if from == to {
            return 0;
        }
        self.change_where(min, max, from == 0, |_, now| (now == from).then_some(to))
    }

    /// Looks at every cell from `min` to `max` (inclusive) and sets the
    /// code `change` gives (from its position and current code), a chunk at
    /// a time. Chunks not there yet are looked at only when `fill_empty`.
    fn change_where(
        &mut self,
        min: (i32, i32, i32),
        max: (i32, i32, i32),
        fill_empty: bool,
        change: impl Fn((i32, i32, i32), u8) -> Option<u8>,
    ) -> usize {
        if !fits_one_edit(min, max) {
            return 0;
        }
        let mut changed = 0;
        let mut touched = Vec::new();
        // A chunk at a time, straight into its cells.
        let (lo, hi) = (
            (min.0.div_euclid(CHUNK), min.1.div_euclid(CHUNK), min.2.div_euclid(CHUNK)),
            (max.0.div_euclid(CHUNK), max.1.div_euclid(CHUNK), max.2.div_euclid(CHUNK)),
        );
        for cy in lo.1..=hi.1 {
            for cz in lo.2..=hi.2 {
                for cx in lo.0..=hi.0 {
                    let key = (cx, cy, cz);
                    let existed = self.chunks.contains_key(&key);
                    if !fill_empty && !existed {
                        continue;
                    }
                    let base = (cx * CHUNK, cy * CHUNK, cz * CHUNK);
                    let span = |lo: i32, hi: i32, start: i32| lo.max(start)..=hi.min(start + CHUNK - 1);
                    let chunk = self.chunks.entry(key).or_insert_with(|| Box::new([0; CHUNK_CELLS]));
                    let mut any = false;
                    for y in span(min.1, max.1, base.1) {
                        for z in span(min.2, max.2, base.2) {
                            for x in span(min.0, max.0, base.0) {
                                let at = index(
                                    (x - base.0) as usize,
                                    (y - base.1) as usize,
                                    (z - base.2) as usize,
                                );
                                if let Some(code) = change((x, y, z), chunk[at])
                                    && code != chunk[at]
                                {
                                    chunk[at] = code;
                                    changed += 1;
                                    any = true;
                                }
                            }
                        }
                    }
                    if any || !existed {
                        touched.push(key);
                    }
                }
            }
        }
        self.tidy(touched);
        if changed > 0 {
            self.version += 1;
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

    /// Fills a wedge the shape of a wedge part: `size` studs, turned by
    /// `rotation` (its three axes), sloping from its top at the back (+Z)
    /// down to its front (-Z). Air empties it.
    pub fn fill_wedge(
        &mut self,
        centre: Vec3,
        axes: [Vec3; 3],
        size: Vec3,
        material: Option<Material>,
    ) -> usize {
        let half = size * 0.5;
        let reach = half.length();
        let r = Vec3::new(reach, reach, reach);
        let (min, max) = (cell_at(centre - r), cell_at(centre + r));
        self.fill_where(min, max, material, |p| {
            let d = p - centre;
            let (x, y, z) = (d.dot(axes[0]), d.dot(axes[1]), d.dot(axes[2]));
            let inside = x.abs() <= half.x && y.abs() <= half.y && z.abs() <= half.z;
            // How high the slope is here, 0 at the front to 1 at the back.
            inside && (y + half.y) / size.y.max(1e-3) <= (z + half.z) / size.z.max(1e-3)
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
        let mut touched = std::collections::BTreeSet::new();
        for ((x, y, z), material) in cells {
            self.put(x, y, z, material.map_or(0, code_of));
            touched.insert((x.div_euclid(CHUNK), y.div_euclid(CHUNK), z.div_euclid(CHUNK)));
        }
        self.tidy(touched);
        self.version += 1;
    }

    /// Changes the material of the filled cells in a ball, leaving air alone.
    pub fn paint_ball(&mut self, centre: Vec3, radius: f32, material: Material) -> usize {
        let r = Vec3::new(radius, radius, radius);
        let (min, max) = (cell_at(centre - r), cell_at(centre + r));
        if !fits_one_edit(min, max) {
            return 0;
        }
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
            self.prints.clear();
            self.version += 1;
        }
    }

    /// The chunks that have anything in them, by chunk position.
    pub fn chunks(&self) -> impl Iterator<Item = (i32, i32, i32)> + '_ {
        self.chunks.keys().copied()
    }

    /// A number that changes when a chunk's cells do (0 for an empty chunk).
    pub fn chunk_fingerprint(&self, chunk: (i32, i32, i32)) -> u64 {
        match self.prints.get(&chunk) {
            Some(print) => *print,
            None => self.chunks.get(&chunk).map_or(0, |cells| print_of(cells)),
        }
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
        if self.chunks.is_empty() || ![origin.x, origin.y, origin.z].iter().all(|v| v.is_finite()) {
            return None;
        }
        let reach = reach.min(MOST_RAY_REACH);
        // Past the last chunk on the way out there is nothing to hit.
        let (lo, hi) = self.chunks.keys().fold(((i32::MAX, i32::MAX, i32::MAX), (i32::MIN, i32::MIN, i32::MIN)), |(lo, hi), c| {
            ((lo.0.min(c.0), lo.1.min(c.1), lo.2.min(c.2)), (hi.0.max(c.0), hi.1.max(c.1), hi.2.max(c.2)))
        });
        let (lo, hi) = (
            (lo.0 * CHUNK, lo.1 * CHUNK, lo.2 * CHUNK),
            (hi.0 * CHUNK + CHUNK - 1, hi.1 * CHUNK + CHUNK - 1, hi.2 * CHUNK + CHUNK - 1),
        );
        let leaving = |c: i32, s: i32, lo: i32, hi: i32| (s > 0 && c > hi) || (s < 0 && c < lo);
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
            if t > reach
                || !t.is_finite()
                || leaving(cell.0, sx, lo.0, hi.0)
                || leaving(cell.1, sy, lo.1, hi.1)
                || leaving(cell.2, sz, lo.2, hi.2)
            {
                return None;
            }
            if self.is_solid(cell.0, cell.1, cell.2) {
                return Some((t, normal, cell));
            }
        }
    }

    /// As text, for saving and sending.
    pub fn encode(&self) -> String {
        self.encode_chunks(self.chunks.keys().copied())
    }

    /// [`encode`](Self::encode) for only some chunks; an empty one is sent
    /// as all air.
    pub fn encode_chunks(&self, chunks: impl IntoIterator<Item = (i32, i32, i32)>) -> String {
        const AIR: [u8; CHUNK_CELLS] = [0; CHUNK_CELLS];
        let mut out = String::new();
        for (cx, cy, cz) in chunks {
            let cells = self.chunks.get(&(cx, cy, cz)).map_or(&AIR, |c| &**c);
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

    /// Replaces the chunks in [`encode_chunks`](Self::encode_chunks)'s text,
    /// leaving the rest.
    pub fn patch(&mut self, text: &str) {
        let mut parts = Self::decode(text);
        let named = text.split(';').filter_map(|chunk| {
            let at: Vec<i32> =
                chunk.split_once(':')?.0.split(',').filter_map(|v| v.trim().parse().ok()).collect();
            let [cx, cy, cz] = at[..] else { return None };
            Some((cx, cy, cz))
        });
        for key in named {
            match parts.chunks.remove(&key) {
                Some(cells) => {
                    self.prints.insert(key, print_of(&cells));
                    self.chunks.insert(key, cells);
                }
                None => {
                    self.chunks.remove(&key);
                    self.prints.remove(&key);
                }
            }
        }
        self.version += 1;
    }

    /// Reads [`encode`](Self::encode)'s text. Anything malformed is skipped.
    pub fn decode(text: &str) -> Voxels {
        let mut voxels = Voxels::default();
        for chunk in text.split(';').filter(|c| !c.trim().is_empty()) {
            let Some((at, runs)) = chunk.split_once(':') else { continue };
            let at: Vec<i32> = at.split(',').filter_map(|v| v.trim().parse().ok()).collect();
            let [cx, cy, cz] = at[..] else { continue };
            let mut cells = Box::new([0u8; CHUNK_CELLS]);
            let mut i: usize = 0;
            for run in runs.split(',') {
                let (code, n) = match run.split_once('*') {
                    Some((c, n)) => (c.trim().parse::<u8>().ok(), n.trim().parse::<usize>().ok()),
                    None => (run.trim().parse::<u8>().ok(), Some(1)),
                };
                let (Some(code), Some(n)) = (code, n) else { break };
                let code = if material_of(code).is_some() { code } else { 0 };
                let end = i.saturating_add(n).min(CHUNK_CELLS);
                cells[i..end].fill(code);
                i = end;
            }
            if cells.iter().any(|c| *c != 0) {
                voxels.prints.insert((cx, cy, cz), print_of(&cells));
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
    fn a_patch_replaces_only_its_chunks() {
        let mut host = Voxels::new();
        host.set(1, 1, 1, Some(Material::Grass));
        host.set(40, 1, 1, Some(Material::Rock));
        let mut copy = Voxels::new();
        copy.patch(&host.encode_chunks([(0, 0, 0)]));
        assert_eq!(copy.get(1, 1, 1), Some(Material::Grass));
        assert_eq!(copy.get(40, 1, 1), None);
        copy.patch(&host.encode_chunks([(2, 0, 0)]));
        assert_eq!(copy.get(40, 1, 1), Some(Material::Rock));
        copy.patch(&Voxels::new().encode_chunks([(0, 0, 0)]));
        assert_eq!(copy.get(1, 1, 1), None);
        assert_eq!(copy.get(40, 1, 1), Some(Material::Rock));
    }

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
    fn damaged_text_never_panics() {
        // A run long enough to wrap around once added up.
        let text = format!("0,0,0:1*5,2*{}", usize::MAX);
        let v = Voxels::decode(&text);
        assert_eq!(v.get(0, 0, 0), Some(material_of(1).unwrap()));
        let mut v = Voxels::new();
        v.patch(&text);
        assert!(Voxels::decode("1,2,3:;;:;,,,:*").is_empty());
    }

    #[test]
    fn endless_fills_are_turned_down() {
        let mut v = Voxels::new();
        for size in [Vec3::new(f32::INFINITY, 1.0, 1.0), Vec3::new(1e9, 1e9, 1e9), Vec3::new(f32::NAN, 4.0, 4.0)] {
            let began = std::time::Instant::now();
            v.fill_block(Vec3::new(0.0, 0.0, 0.0), AXES, size, Some(Material::Grass));
            v.fill_wedge(Vec3::new(0.0, 0.0, 0.0), AXES, size, Some(Material::Grass));
            assert!(began.elapsed().as_secs() < 2, "{size:?}");
        }
        assert_eq!(v.paint_ball(Vec3::new(0.0, 0.0, 0.0), f32::INFINITY, Material::Rock), 0);
        assert_eq!(v.fill_ball(Vec3::new(0.0, 0.0, 0.0), 1e9, None), 0);
        assert!(fits_one_edit((0, 0, 0), (4095, 15, 4095)));
        assert!(!fits_one_edit((i32::MIN, 0, 0), (i32::MAX, 0, 0)));
    }

    #[test]
    fn a_ray_past_the_terrain_stops_at_once() {
        let mut v = Voxels::new();
        v.set(0, 0, 0, Some(Material::Grass));
        let began = std::time::Instant::now();
        assert!(v.raycast(Vec3::new(0.5, 5.0, 0.5), Vec3::new(1.0, 0.0, 0.0), f32::INFINITY).is_none());
        assert!(v.raycast(Vec3::new(0.5, 5.0, 0.5), Vec3::new(0.0, 1.0, 0.0), 1e30).is_none());
        assert!(v.raycast(Vec3::new(f32::NAN, 5.0, 0.5), Vec3::new(0.0, -1.0, 0.0), 10.0).is_none());
        assert!(began.elapsed().as_millis() < 500);
        assert!(v.raycast(Vec3::new(0.5, 5.0, 0.5), Vec3::new(0.0, -1.0, 0.0), f32::INFINITY).is_some());
    }

    #[test]
    fn merged_boxes_cover_the_filled_cells() {
        let mut v = Voxels::new();
        v.fill_block(Vec3::new(0.0, 0.5, 0.5), AXES, Vec3::new(6.0, 1.0, 1.0), Some(Material::Rock));
        let boxes = v.boxes_in(Vec3::new(-10.0, 0.0, 0.0), Vec3::new(10.0, 0.5, 0.5));
        assert_eq!(boxes, vec![(Vec3::new(-3.0, 0.0, 0.0), Vec3::new(3.0, 1.0, 1.0))]);
    }
}
