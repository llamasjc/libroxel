//! Terrain methods: `FillBlock`, `FillBall`, `FillCylinder`, `FillRegion`,
//! `FillWedge`, `ReplaceMaterial`, `ReadVoxels`, `WriteVoxels` and `Clear`.
//! Filling with `Enum.Material.Air` digs cells out.

use libroxel_instance::{ClassKind, EnumKind, EnumValue, Material};
use mlua::{UserDataMethods, Value};

use crate::bindings::{LuaInstance, as_enum, as_number, as_vector3, host, require_class, tree_err};
use crate::names::terrain;
use crate::types::as_cframe;

fn err(message: impl Into<String>) -> mlua::Error {
    mlua::Error::runtime(message.into())
}

/// The material to fill with, `None` for air.
fn fill_material(value: &Value, method: &str) -> mlua::Result<Option<Material>> {
    match as_enum(EnumKind::Material, value) {
        Some(EnumValue::Material(Material::Air)) => Ok(None),
        Some(EnumValue::Material(m)) => {
            if libroxel_instance::voxels::TERRAIN_MATERIALS.contains(&m) {
                Ok(Some(m))
            } else {
                Err(err(format!("{method}: terrain can't be made of that material")))
            }
        }
        _ => Err(err(format!("{method} expects a material"))),
    }
}

/// The corners of a `Region3` (a table with a CFrame and a Size).
fn region(value: &Value, method: &str) -> mlua::Result<(libroxel_instance::Vec3, libroxel_instance::Vec3)> {
    let bad = || err(format!("{method} expects a Region3"));
    let Value::Table(t) = value else { return Err(bad()) };
    let cf = as_cframe(&t.get::<Value>("CFrame")?).ok_or_else(bad)?;
    let size = as_vector3(&t.get::<Value>("Size")?).ok_or_else(bad)?;
    Ok((cf.pos - size / 2.0, cf.pos + size / 2.0))
}

/// Cells a region covers, at most this many on a side so a script can't
/// ask for the whole world.
const MOST_CELLS: i32 = 512;

fn cell_range(lo: f32, hi: f32) -> std::ops::Range<i32> {
    let lo = lo.round() as i32;
    lo..(hi.round() as i32).min(lo.saturating_add(MOST_CELLS))
}

fn material_value(lua: &mlua::Lua, material: Option<Material>) -> mlua::Result<Value> {
    let m = material.unwrap_or(Material::Air);
    crate::bindings::property_to_lua(lua, libroxel_instance::PropertyValue::Enum(EnumValue::Material(m)))
}

/// A grid of `size` blocks, each `resolution` studs, as the reference platform's
/// three-deep tables with a `Size` field.
fn grid(lua: &mlua::Lua, size: [usize; 3]) -> mlua::Result<mlua::Table> {
    let t = lua.create_table()?;
    let n = |v: usize| v as f32;
    t.set(
        "Size",
        crate::bindings::LuaVector3(libroxel_instance::Vec3::new(n(size[0]), n(size[1]), n(size[2]))),
    )?;
    Ok(t)
}

/// Turns down a box too big (or endless) for one fill, rather than keep
/// the game busy for minutes.
fn check_size(size: libroxel_instance::Vec3, method: &str) -> mlua::Result<()> {
    let cells = f64::from(size.x.abs()) * f64::from(size.y.abs()) * f64::from(size.z.abs());
    if cells.is_finite() && cells <= libroxel_instance::voxels::MOST_EDIT_CELLS as f64 {
        Ok(())
    } else {
        Err(err(format!(
            "{method}: the region is too big (at most {} cells at once)",
            libroxel_instance::voxels::MOST_EDIT_CELLS
        )))
    }
}

/// The most blocks one `ReadVoxels` or `WriteVoxels` call covers: at a
/// resolution of 4, 64 million cells.
const MOST_BLOCKS: usize = 1 << 20;

/// Only a resolution of 4 is used by real games; anything else is read as 4 at
/// most, so a huge one can't make a call loop for ever.
fn voxel_resolution(value: &Value) -> i32 {
    as_number(value).filter(|r| r.is_finite()).unwrap_or(4.0).clamp(1.0, 4.0).round() as i32
}

fn edit(
    lua: &mlua::Lua,
    terrain: libroxel_instance::InstanceId,
    f: impl FnOnce(&mut libroxel_instance::voxels::Voxels) -> usize,
) -> mlua::Result<()> {
    let host = host(lua);
    let mut tree = host.tree.borrow_mut();
    tree.edit_voxels(terrain, f).map_err(tree_err)?;
    Ok(())
}

pub(crate) fn add_instance_methods<M: UserDataMethods<LuaInstance>>(m: &mut M) {
    m.add_method(terrain::FILL_BLOCK, |lua, this, (cframe, size, material): (Value, Value, Value)| {
        require_class(lua, this.0, ClassKind::Terrain, terrain::FILL_BLOCK)?;
        let cf = as_cframe(&cframe).ok_or_else(|| err("FillBlock expects a CFrame first"))?;
        let size = as_vector3(&size).ok_or_else(|| err("FillBlock expects a Vector3 size"))?;
        let material = fill_material(&material, terrain::FILL_BLOCK)?;
        check_size(size, terrain::FILL_BLOCK)?;
        let axes = [cf.rot.x, cf.rot.y, cf.rot.z];
        edit(lua, this.0, |v| v.fill_block(cf.pos, axes, size, material))
    });
    m.add_method(terrain::FILL_BALL, |lua, this, (centre, radius, material): (Value, Value, Value)| {
        require_class(lua, this.0, ClassKind::Terrain, terrain::FILL_BALL)?;
        let centre = as_vector3(&centre).ok_or_else(|| err("FillBall expects a Vector3 centre"))?;
        let radius = as_number(&radius).ok_or_else(|| err("FillBall expects a radius"))?;
        let material = fill_material(&material, terrain::FILL_BALL)?;
        edit(lua, this.0, |v| v.fill_ball(centre, radius.min(512.0), material))
    });
    m.add_method(
        terrain::FILL_CYLINDER,
        |lua, this, (cframe, height, radius, material): (Value, Value, Value, Value)| {
            require_class(lua, this.0, ClassKind::Terrain, terrain::FILL_CYLINDER)?;
            let cf = as_cframe(&cframe).ok_or_else(|| err("FillCylinder expects a CFrame first"))?;
            let height = as_number(&height).ok_or_else(|| err("FillCylinder expects a height"))?;
            let radius = as_number(&radius).ok_or_else(|| err("FillCylinder expects a radius"))?;
            let material = fill_material(&material, terrain::FILL_CYLINDER)?;
            let axes = [cf.rot.x, cf.rot.y, cf.rot.z];
            edit(lua, this.0, |v| {
                v.fill_cylinder(cf.pos, axes, height.min(1024.0), radius.min(512.0), material)
            })
        },
    );
    m.add_method("FillRegion", |lua, this, (r, _resolution, material): (Value, Value, Value)| {
        require_class(lua, this.0, ClassKind::Terrain, "FillRegion")?;
        let (lo, hi) = region(&r, "FillRegion")?;
        let material = fill_material(&material, "FillRegion")?;
        check_size(hi - lo, "FillRegion")?;
        let v = libroxel_instance::Vec3::new;
        let axes = [v(1.0, 0.0, 0.0), v(0.0, 1.0, 0.0), v(0.0, 0.0, 1.0)];
        edit(lua, this.0, |v| v.fill_block((lo + hi) / 2.0, axes, hi - lo, material))
    });
    m.add_method("FillWedge", |lua, this, (cframe, size, material): (Value, Value, Value)| {
        require_class(lua, this.0, ClassKind::Terrain, "FillWedge")?;
        let cf = as_cframe(&cframe).ok_or_else(|| err("FillWedge expects a CFrame first"))?;
        let size = as_vector3(&size).ok_or_else(|| err("FillWedge expects a Vector3 size"))?;
        let material = fill_material(&material, "FillWedge")?;
        check_size(size, "FillWedge")?;
        let axes = [cf.rot.x, cf.rot.y, cf.rot.z];
        edit(lua, this.0, |v| v.fill_wedge(cf.pos, axes, size, material))
    });
    m.add_method("ReplaceMaterial", |lua, this, (r, _resolution, from, to): (Value, Value, Value, Value)| {
        require_class(lua, this.0, ClassKind::Terrain, "ReplaceMaterial")?;
        let (lo, hi) = region(&r, "ReplaceMaterial")?;
        let from = fill_material(&from, "ReplaceMaterial")?;
        let to = fill_material(&to, "ReplaceMaterial")?;
        let (xs, ys, zs) = (cell_range(lo.x, hi.x), cell_range(lo.y, hi.y), cell_range(lo.z, hi.z));
        if xs.is_empty() || ys.is_empty() || zs.is_empty() {
            return Ok(());
        }
        let min = (xs.start, ys.start, zs.start);
        let max = (xs.end - 1, ys.end - 1, zs.end - 1);
        edit(lua, this.0, |v| v.replace(min, max, from, to))
    });
    m.add_method("ReadVoxels", |lua, this, (r, resolution): (Value, Value)| {
        require_class(lua, this.0, ClassKind::Terrain, "ReadVoxels")?;
        let (lo, hi) = region(&r, "ReadVoxels")?;
        let res = voxel_resolution(&resolution);
        let count = |a: f32, b: f32| (((b - a) / res as f32).round().max(0.0) as usize).min(256);
        let size = [count(lo.x, hi.x), count(lo.y, hi.y), count(lo.z, hi.z)];
        if size[0] * size[1] * size[2] > MOST_BLOCKS {
            return Err(err(format!("ReadVoxels: the region is too big (at most {MOST_BLOCKS} blocks)")));
        }
        let materials = grid(lua, size)?;
        let occupancy = grid(lua, size)?;
        let host = host(lua);
        let tree = host.tree.borrow();
        let voxels = tree.voxels(this.0).map_err(tree_err)?;
        let start = [lo.x.round() as i32, lo.y.round() as i32, lo.z.round() as i32];
        for i in 0..size[0] {
            let (mx, ox) = (lua.create_table()?, lua.create_table()?);
            for j in 0..size[1] {
                let (my, oy) = (lua.create_table()?, lua.create_table()?);
                for k in 0..size[2] {
                    // The block's most common material, and how full it is.
                    let mut counts: Vec<(Material, u32)> = Vec::new();
                    let mut solid = 0;
                    for dy in 0..res {
                        for dz in 0..res {
                            for dx in 0..res {
                                let x = start[0] + i as i32 * res + dx;
                                let y = start[1] + j as i32 * res + dy;
                                let z = start[2] + k as i32 * res + dz;
                                if let Some(m) = voxels.get(x, y, z) {
                                    solid += 1;
                                    match counts.iter_mut().find(|c| c.0 == m) {
                                        Some(c) => c.1 += 1,
                                        None => counts.push((m, 1)),
                                    }
                                }
                            }
                        }
                    }
                    let most = counts.iter().max_by_key(|c| c.1).map(|c| c.0);
                    my.raw_set(k + 1, material_value(lua, most)?)?;
                    oy.raw_set(k + 1, f64::from(solid) / f64::from(res * res * res))?;
                }
                mx.raw_set(j + 1, my)?;
                ox.raw_set(j + 1, oy)?;
            }
            materials.raw_set(i + 1, mx)?;
            occupancy.raw_set(i + 1, ox)?;
        }
        Ok((materials, occupancy))
    });
    m.add_method(
        "WriteVoxels",
        |lua,
         this,
         (r, resolution, materials, occupancy): (Value, Value, mlua::Table, Option<mlua::Table>)| {
            require_class(lua, this.0, ClassKind::Terrain, "WriteVoxels")?;
            let (lo, _) = region(&r, "WriteVoxels")?;
            let res = voxel_resolution(&resolution);
            let start = [lo.x.round() as i32, lo.y.round() as i32, lo.z.round() as i32];
            let mut cells = Vec::new();
            let mut blocks = 0;
            for (i, plane) in materials.sequence_values::<mlua::Table>().enumerate().take(256) {
                for (j, row) in plane?.sequence_values::<mlua::Table>().enumerate().take(256) {
                    for (k, m) in row?.sequence_values::<Value>().enumerate().take(256) {
                        blocks += 1;
                        if blocks > MOST_BLOCKS {
                            return Err(err(format!(
                                "WriteVoxels: too many blocks (at most {MOST_BLOCKS} at once)"
                            )));
                        }
                        let material = fill_material(&m?, "WriteVoxels")?;
                        let full = match &occupancy {
                            Some(o) => o
                                .get::<mlua::Table>(i + 1)
                                .and_then(|p| p.get::<mlua::Table>(j + 1))
                                .and_then(|r| r.get::<f32>(k + 1))
                                .unwrap_or(1.0),
                            None => 1.0,
                        };
                        // A part-full block fills from the bottom.
                        let layers = (full.clamp(0.0, 1.0) * res as f32).round() as i32;
                        for dy in 0..res {
                            let filled = material.is_some() && dy < layers;
                            for dz in 0..res {
                                for dx in 0..res {
                                    let at = (
                                        start[0] + i as i32 * res + dx,
                                        start[1] + j as i32 * res + dy,
                                        start[2] + k as i32 * res + dz,
                                    );
                                    cells.push((at, if filled { material } else { None }));
                                }
                            }
                        }
                    }
                }
            }
            edit(lua, this.0, |v| {
                let n = cells.len();
                v.set_cells(cells);
                n
            })
        },
    );
    m.add_method("SetMaterialColor", |lua, this, (material, colour): (Value, Value)| {
        require_class(lua, this.0, ClassKind::Terrain, "SetMaterialColor")?;
        let Some(m) = fill_material(&material, "SetMaterialColor")? else { return Ok(()) };
        let c = match &colour {
            Value::UserData(u) => u.borrow::<crate::bindings::LuaColor3>().ok().map(|c| c.0),
            _ => None,
        }
        .ok_or_else(|| err("SetMaterialColor expects a Color3"))?;
        let host = host(lua);
        let mut tree = host.tree.borrow_mut();
        let key = format!("MaterialColor.{m:?}");
        tree.set_extra(this.0, &key, Some(libroxel_instance::PropertyValue::Color3(c))).map_err(tree_err)
    });
    m.add_method("GetMaterialColor", |lua, this, material: Value| {
        require_class(lua, this.0, ClassKind::Terrain, "GetMaterialColor")?;
        let m = fill_material(&material, "GetMaterialColor")?.unwrap_or(Material::Grass);
        let kept = host(lua).tree.borrow().get_extra(this.0, &format!("MaterialColor.{m:?}")).ok().flatten();
        let c = match kept {
            Some(libroxel_instance::PropertyValue::Color3(c)) => c,
            _ => {
                use std::str::FromStr;
                let d = rbx_types::TerrainMaterials::from_str(&format!("{m:?}"))
                    .map(|t| t.default_color())
                    .unwrap_or(rbx_types::Color3uint8::new(163, 162, 165));
                libroxel_instance::Color3::from_rgb8(d.r, d.g, d.b)
            }
        };
        Ok(crate::bindings::LuaColor3(c))
    });
    m.add_method("CopyRegion", |lua, this, r: Value| {
        require_class(lua, this.0, ClassKind::Terrain, "CopyRegion")?;
        let (lo, hi) = region(&r, "CopyRegion")?;
        let lo = [lo.x.round() as i32 * CELL, lo.y.round() as i32 * CELL, lo.z.round() as i32 * CELL];
        let hi = [
            (hi.x.round() as i32 + 1) * CELL,
            (hi.y.round() as i32 + 1) * CELL,
            (hi.z.round() as i32 + 1) * CELL,
        ];
        let host = host(lua);
        let tree = host.tree.borrow();
        let voxels = tree.voxels(this.0).map_err(tree_err)?;
        let inside = |(x, y, z): (i32, i32, i32)| {
            x >= lo[0] && x < hi[0] && y >= lo[1] && y < hi[1] && z >= lo[2] && z < hi[2]
        };
        let cells = voxels
            .chunks()
            .flat_map(|c| voxels.cells_in_chunk(c))
            .filter(|(at, _)| inside(*at))
            .map(|((x, y, z), m)| ((x - lo[0], y - lo[1], z - lo[2]), m))
            .collect();
        Ok(TerrainRegion { size: [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]], cells })
    });
    m.add_method(
        "PasteRegion",
        |lua, this, (copied, corner, paste_empty): (mlua::AnyUserData, Value, Option<bool>)| {
            require_class(lua, this.0, ClassKind::Terrain, "PasteRegion")?;
            let copied =
                copied.borrow::<TerrainRegion>().map_err(|_| err("PasteRegion expects a TerrainRegion"))?;
            let corner =
                as_vector3(&corner).ok_or_else(|| err("PasteRegion expects a Vector3int16 corner"))?;
            let at = [
                corner.x.round() as i32 * CELL,
                corner.y.round() as i32 * CELL,
                corner.z.round() as i32 * CELL,
            ];
            let size = copied.size;
            let cells: Vec<_> = copied
                .cells
                .iter()
                .map(|((x, y, z), m)| ((x + at[0], y + at[1], z + at[2]), Some(*m)))
                .collect();
            edit(lua, this.0, |v| {
                if paste_empty.unwrap_or(true) {
                    let inside = |(x, y, z): (i32, i32, i32)| {
                        x >= at[0]
                            && x < at[0] + size[0]
                            && y >= at[1]
                            && y < at[1] + size[1]
                            && z >= at[2]
                            && z < at[2] + size[2]
                    };
                    let chunks: Vec<_> = v.chunks().collect();
                    let gone: Vec<_> = chunks
                        .into_iter()
                        .flat_map(|c| v.cells_in_chunk(c))
                        .filter(|(p, _)| inside(*p))
                        .map(|(p, _)| (p, None))
                        .collect();
                    v.set_cells(gone);
                }
                let n = cells.len();
                v.set_cells(cells);
                n
            })
        },
    );
    m.add_function(terrain::CLEAR, |lua, (obj, rest): (mlua::AnyUserData, mlua::MultiValue)| {
        let id = obj.borrow::<LuaInstance>()?.0;
        let class = host(lua).tree.borrow().class(id).map_err(tree_err)?;
        if class != ClassKind::Terrain {
            // Trails, particle emitters and others have a Clear of their own.
            let Some(Value::Function(f)) =
                crate::lenient::index(lua, id, class, obj.clone(), terrain::CLEAR)?
            else {
                return Err(crate::bindings::not_a_member(lua, id, terrain::CLEAR));
            };
            let mut args = rest;
            args.push_front(Value::UserData(obj));
            return f.call::<()>(args);
        }
        edit(lua, id, |v| {
            v.clear();
            0
        })
    });
}

const CELL: i32 = 4;

pub(crate) struct TerrainRegion {
    size: [i32; 3],
    cells: Vec<((i32, i32, i32), Material)>,
}

impl mlua::UserData for TerrainRegion {
    fn add_fields<F: mlua::UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("SizeInCells", |_, this| {
            let s = this.size.map(|v| (v / CELL) as f32);
            Ok(crate::bindings::LuaVector3(libroxel_instance::Vec3::new(s[0], s[1], s[2])))
        });
        fields.add_field_method_get("IsSmooth", |_, _| Ok(true));
        fields.add_field_method_get("ClassName", |_, _| Ok("TerrainRegion"));
        fields.add_meta_field(mlua::MetaMethod::Type, "Instance");
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("ConvertToSmooth", |_, _, ()| Ok(()));
        methods.add_method("IsA", |_, _, name: String| Ok(name == "TerrainRegion" || name == "Instance"));
    }
}
