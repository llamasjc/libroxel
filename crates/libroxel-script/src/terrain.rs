//! Terrain methods: `FillBlock`, `FillBall`, `FillCylinder` and `Clear`.
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
    m.add_method(terrain::CLEAR, |lua, this, ()| {
        require_class(lua, this.0, ClassKind::Terrain, terrain::CLEAR)?;
        edit(lua, this.0, |v| {
            v.clear();
            0
        })
    });
}
