//! Terrain from a place file: the `SmoothGrid` blob a terrain saves its
//! voxels in, turned into Libroxel's one-stud terrain cells.
//!
//! The blob starts with a version byte (1) and the base-2 log of a chunk's
//! size (5, so chunks of 32 × 32 × 32 voxels, each voxel 4 studs across).
//! Then come chunks until the blob ends. Each starts with how far it is
//! from the one before in whole chunks (three big-endian `i32`s, stored
//! with their bytes interleaved), then its voxels, run-length encoded and
//! ordered by Y, then Z, then X. A voxel is a flag byte (material in the
//! low six bits; bit 6: an occupancy byte follows; bit 7: a count byte
//! follows, where a count of 0 means a water-occupancy byte follows
//! instead and any other count repeats the voxel that many more times).

use std::collections::HashMap;

use libroxel_instance::Material;

/// Studs along each side of one voxel in the file.
pub const VOXEL: i32 = 4;
const CHUNK_LOG2: u8 = 5;
const CHUNK: i32 = 1 << CHUNK_LOG2;
const CHUNK_VOXELS: i32 = CHUNK * CHUNK * CHUNK;

/// Past this many filled one-stud cells, the rest of the terrain is left out.
pub const MAX_CELLS: usize = 48_000_000;

/// The file's terrain materials, by their number in a voxel.
pub const MATERIALS: &[&str] = &[
    "Air",
    "Water",
    "Grass",
    "Slate",
    "Concrete",
    "Brick",
    "Sand",
    "WoodPlanks",
    "Rock",
    "Glacier",
    "Snow",
    "Sandstone",
    "Mud",
    "Basalt",
    "Ground",
    "CrackedLava",
    "Asphalt",
    "Cobblestone",
    "Ice",
    "LeafyGrass",
    "Salt",
    "Limestone",
    "Pavement",
];

/// The Libroxel terrain material closest to a file's material number.
pub fn material(id: u8) -> Option<Material> {
    Some(match MATERIALS.get(id as usize).copied()? {
        "Air" => return None,
        "Water" => Material::Water,
        "Grass" | "LeafyGrass" => Material::Grass,
        "Sand" | "Sandstone" | "Salt" => Material::Sand,
        "Snow" | "Glacier" => Material::Snow,
        "Brick" => Material::Brick,
        "WoodPlanks" | "Mud" | "Ground" => Material::Wood,
        "Ice" => Material::Glass,
        "Concrete" | "Asphalt" | "Pavement" => Material::SmoothPlastic,
        // Slate, Rock, Basalt, Limestone, Cobblestone, CrackedLava.
        _ => Material::Rock,
    })
}

/// A one-stud cell and what fills it.
pub type Cell = ((i32, i32, i32), Material);

/// One filled voxel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Voxel {
    /// The material's number (see [`MATERIALS`]).
    pub material: u8,
    /// How full of the material it is, 0 to 1.
    pub solid: f32,
    /// How full of water the rest of it is, 0 to 1.
    pub water: f32,
}

/// Why a terrain blob couldn't be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GridError {
    /// Not version 1 with 32-voxel chunks.
    Header(u8, u8),
    /// The blob stopped partway through a chunk.
    Truncated,
    /// A material number past the known ones.
    Material(u8),
}

impl std::fmt::Display for GridError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GridError::Header(v, s) => write!(f, "unknown terrain format (version {v}, chunk size {s})"),
            GridError::Truncated => write!(f, "the terrain data stops partway through"),
            GridError::Material(m) => write!(f, "unknown terrain material {m}"),
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Result<u8, GridError> {
        let b = *self.bytes.get(self.at).ok_or(GridError::Truncated)?;
        self.at += 1;
        Ok(b)
    }

    fn done(&self) -> bool {
        self.at >= self.bytes.len()
    }
}

/// Every filled voxel in a `SmoothGrid` blob, by voxel position (each
/// voxel covers `VOXEL` studs from `position * VOXEL`).
pub fn decode(bytes: &[u8]) -> Result<HashMap<(i32, i32, i32), Voxel>, GridError> {
    let mut r = Reader { bytes, at: 0 };
    let (version, size) = (r.byte()?, r.byte()?);
    if version != 1 || size != CHUNK_LOG2 {
        return Err(GridError::Header(version, size));
    }
    let mut voxels = HashMap::new();
    let mut chunk = [0i32; 3];
    while !r.done() {
        // Three big-endian i32s with their bytes interleaved.
        let mut raw = [[0u8; 4]; 3];
        for byte in 0..4 {
            for axis in &mut raw {
                axis[byte] = r.byte()?;
            }
        }
        for (c, b) in chunk.iter_mut().zip(raw) {
            *c = c.wrapping_add(i32::from_be_bytes(b));
        }
        let base = (chunk[0] * CHUNK, chunk[1] * CHUNK, chunk[2] * CHUNK);
        let mut index = 0;
        while index < CHUNK_VOXELS {
            let flag = r.byte()?;
            let id = flag & 0x3F;
            if id as usize >= MATERIALS.len() {
                return Err(GridError::Material(id));
            }
            // Without an occupancy byte a voxel is completely full (or, for air, empty).
            let solid = if flag & 0x40 != 0 { f32::from(r.byte()?) / 255.0 } else { 1.0 };
            let (mut repeats, mut water) = (0u32, 0.0);
            if flag & 0x80 != 0 {
                match r.byte()? {
                    0 => water = f32::from(r.byte()?) / 255.0,
                    n => repeats = u32::from(n),
                }
            }
            for _ in 0..=repeats {
                if index >= CHUNK_VOXELS {
                    return Err(GridError::Truncated);
                }
                if id != 0 || water > 0.0 {
                    let (x, y, z) = (index % CHUNK, index / (CHUNK * CHUNK), (index / CHUNK) % CHUNK);
                    let solid = if id == 0 { 0.0 } else { solid };
                    voxels.insert((base.0 + x, base.1 + y, base.2 + z), Voxel { material: id, solid, water });
                }
                index += 1;
            }
        }
    }
    Ok(voxels)
}

/// One-stud cells for decoded voxels: a voxel fills its 4 × 4 × 4 studs,
/// or for a surface voxel (air above) only as many layers from the bottom
/// as it is full; water fills the layers above that. Stops at
/// [`MAX_CELLS`]; the second value says whether it had to.
pub fn cells(voxels: &HashMap<(i32, i32, i32), Voxel>) -> (Vec<Cell>, bool) {
    let mut out = Vec::new();
    let mut keys: Vec<_> = voxels.keys().copied().collect();
    keys.sort_unstable_by_key(|&(x, y, z)| (y, z, x));
    let layers = |fill: f32| ((fill * VOXEL as f32).round() as i32).clamp(0, VOXEL);
    for (x, y, z) in keys {
        let v = voxels[&(x, y, z)];
        let covered = voxels.get(&(x, y + 1, z)).is_some_and(|a| a.solid >= 0.5 || a.water >= 0.5);
        let (mut solid, mut wet) =
            if covered { (if v.solid >= 0.5 { VOXEL } else { 0 }, 0) } else { (layers(v.solid), 0) };
        let material = material(v.material);
        if material == Some(Material::Water) {
            (solid, wet) = (0, if covered { VOXEL } else { layers(v.solid.max(v.water)).max(1) });
        } else if v.water > 0.0 {
            wet = if covered { VOXEL - solid } else { (layers(v.solid + v.water) - solid).max(0) };
        }
        let material = material.filter(|m| *m != Material::Water);
        for dy in 0..solid + wet {
            let fill = if dy < solid { material } else { Some(Material::Water) };
            let Some(fill) = fill else { continue };
            for dz in 0..VOXEL {
                for dx in 0..VOXEL {
                    out.push(((x * VOXEL + dx, y * VOXEL + dy, z * VOXEL + dz), fill));
                }
            }
        }
        if out.len() >= MAX_CELLS {
            return (out, true);
        }
    }
    (out, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two chunks side by side, each with a water voxel and a pavement
    /// voxel at its first corner and air elsewhere: the reference blob
    /// from rbx-dom's SmoothGrid tests (MIT licensed), shortened by
    /// building the air runs here.
    fn reference() -> Vec<u8> {
        let mut blob = vec![0x01, 0x05];
        for offset in [[0u8; 12], [0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0]] {
            blob.extend(offset);
            blob.extend([0x01, 0x16]);
            // 32766 voxels of air: 127 runs of 256, then 254.
            for _ in 0..127 {
                blob.extend([0x80, 0xFF]);
            }
            blob.extend([0x80, 0xFD]);
        }
        blob
    }

    #[test]
    fn the_reference_blob_decodes() {
        let voxels = decode(&reference()).unwrap();
        assert_eq!(voxels.len(), 4);
        assert_eq!(voxels[&(0, 0, 0)], Voxel { material: 1, solid: 1.0, water: 0.0 });
        assert_eq!(voxels[&(1, 0, 0)].material, 22);
        assert_eq!(voxels[&(32, 0, 0)].material, 1);
        assert_eq!(voxels[&(33, 0, 0)].material, 22);
    }

    #[test]
    fn bad_blobs_are_refused() {
        assert_eq!(decode(&[2, 5]).unwrap_err(), GridError::Header(2, 5));
        let mut short = reference();
        short.truncate(40);
        assert_eq!(decode(&short).unwrap_err(), GridError::Truncated);
        // A header alone is an empty terrain.
        assert!(decode(&[1, 5]).unwrap().is_empty());
    }

    #[test]
    fn occupancy_and_runs() {
        // One chunk at chunk (0, -1, 0): rock half full, then a run of
        // three full grass voxels, a voxel of half water, then air.
        let mut blob = vec![0x01, 0x05];
        let minus_one = (-1i32).to_be_bytes();
        for byte in minus_one {
            blob.extend([0, byte, 0]);
        }
        blob.extend([0x48, 0x80, 0x82, 0x02, 0x80, 0x00, 0x80]);
        let left = CHUNK_VOXELS - 5;
        let mut left = left as u32;
        while left > 0 {
            let n = left.min(256);
            blob.extend([0x80, (n - 1) as u8]);
            left -= n;
        }
        let voxels = decode(&blob).unwrap();
        assert_eq!(voxels.len(), 5);
        let rock = voxels[&(0, -32, 0)];
        assert_eq!(rock.material, 8);
        assert!((rock.solid - 128.0 / 255.0).abs() < 1e-6);
        assert_eq!(voxels[&(3, -32, 0)].material, 2);
        let water = voxels[&(4, -32, 0)];
        assert_eq!((water.material, water.solid), (0, 0.0));
        assert!((water.water - 0.5).abs() < 0.01);

        let (cells, cut) = cells(&voxels);
        assert!(!cut);
        let at = |p: (i32, i32, i32)| cells.iter().find(|(c, _)| *c == p).map(|(_, m)| *m);
        // Half-full rock: the bottom two of its four layers.
        assert_eq!(at((0, -128, 0)), Some(Material::Rock));
        assert_eq!(at((0, -127, 3)), Some(Material::Rock));
        assert_eq!(at((0, -126, 0)), None);
        // Full grass: all four layers.
        assert_eq!(at((4, -125, 0)), Some(Material::Grass));
        // Half water: two layers of water.
        assert_eq!(at((16, -127, 0)), Some(Material::Water));
        assert_eq!(at((16, -126, 0)), None);
        assert_eq!(cells.len(), 2 * 16 + 3 * 64 + 2 * 16);
    }

    #[test]
    fn every_file_material_has_a_libroxel_one() {
        assert_eq!(material(0), None);
        for id in 1..MATERIALS.len() as u8 {
            assert!(material(id).is_some(), "{}", MATERIALS[id as usize]);
        }
        assert_eq!(material(1), Some(Material::Water));
        assert_eq!(material(19), Some(Material::Grass));
    }
}
