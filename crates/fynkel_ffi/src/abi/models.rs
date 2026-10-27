//! Models: loading them, reading what is in them, and spawning them.

use std::sync::Arc;

use fynkel_world::{Material, ModelData, TextureId};

use crate::error::{Error, FyStatus, guard};

use super::*;

// --- Models ------------------------------------------------------------------

/// A model read from a `.glb`, before any world has it. Opaque to C. Made
/// with `fy_model_load` on any thread, freed with `fy_model_free`. Its
/// meshes and textures are shared with the handles `fy_model_mesh` and
/// `fy_model_texture` give, and with every world that spawns it.
#[derive(Debug)]
pub struct FyModel(Arc<ModelData>);

/// How many of each thing a model has, from `fy_model_counts`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FyModelCounts {
    pub meshes: usize,
    pub textures: usize,
    pub materials: usize,
    /// Lines saying what was skipped (`fy_model_warning`).
    pub warnings: usize,
    /// Animations (`fy_model_clip`).
    pub clips: usize,
}

/// Reads a `.glb` (binary glTF 2.0) of `len` bytes and writes the model to
/// `out`. Its images are decoded and its meshes' tangents generated here,
/// so a big model takes a while: call it off the UI thread.
///
/// What can't be drawn yet (skins, see-through materials, ...)
/// is skipped with a warning (`fy_model_warning`). A file that can't be
/// read at all, or needs a glTF extension the engine doesn't know, is
/// refused (`FY_STATUS_INVALID_VALUE`) with a message saying why.
///
/// # Safety
///
/// `bytes` is valid for `len` bytes (it may be null when `len` is 0);
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_model_load(bytes: *const u8, len: usize, out: *mut *mut FyModel) -> FyStatus {
    guard(|| {
        if out.is_null() {
            return Err(Error::null("out"));
        }
        let bytes = if len == 0 {
            &[][..]
        } else {
            // SAFETY: per this function's contract.
            unsafe { std::slice::from_raw_parts(handle(bytes, "bytes")?, len) }
        };
        let model = ModelData::from_glb(bytes).map_err(|e| Error::new(FyStatus::InvalidValue, e.to_string()))?;
        // SAFETY: checked non-null; the caller promises it's writable.
        unsafe { out.write(Box::into_raw(Box::new(FyModel(Arc::new(model))))) };
        Ok(())
    })
}

/// Frees a model handle. Mesh and texture handles taken from it, and
/// worlds that spawned it, keep their share. Null is ignored.
///
/// # Safety
///
/// `model` is null or live, not used again after this.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_model_free(model: *mut FyModel) {
    if !model.is_null() {
        // A panic here leaks the model rather than abort (fy_world_free).
        let _ = guard(|| {
            // SAFETY: made by Box::into_raw in fy_model_load; the caller gives
            // it up.
            drop(unsafe { Box::from_raw(model) });
            Ok(())
        });
    }
}

/// # Safety
///
/// `model` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_model_counts(model: *const FyModel, out: *mut FyModelCounts) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let model = &unsafe { handle(model, "model") }?.0;
        let counts = FyModelCounts {
            meshes: model.meshes().len(),
            textures: model.images().len(),
            materials: model.materials().len(),
            warnings: model.warnings().len(),
            clips: model.clips().len(),
        };
        // SAFETY: per this function's contract.
        unsafe { write(out, counts, "out") }
    })
}

/// `items[index]`, or an error naming what there are fewer of.
fn nth<'a, T>(items: &'a [T], index: usize, what: &str) -> Result<&'a T, Error> {
    items.get(index).ok_or_else(|| {
        Error::new(FyStatus::InvalidValue, format!("the model has {} {what}; there's no {what} {index}", items.len()))
    })
}

/// Writes a new handle to the model's mesh `index` to `out`, sharing its
/// data.
///
/// # Safety
///
/// `model` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_model_mesh(model: *const FyModel, index: usize, out: *mut *mut FyMesh) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let model = &unsafe { handle(model, "model") }?.0;
        let mesh = Arc::clone(nth(model.meshes(), index, "meshes")?);
        // SAFETY: per this function's contract.
        unsafe { write(out, Box::into_raw(Box::new(FyMesh::new(mesh))), "out") }
    })
}

/// Writes a new handle to the model's image `index` to `out`, sharing its
/// pixels.
///
/// # Safety
///
/// `model` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_model_texture(model: *const FyModel, index: usize, out: *mut *mut FyTexture) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let model = &unsafe { handle(model, "model") }?.0;
        let image = Arc::clone(nth(model.images(), index, "textures")?);
        // SAFETY: per this function's contract.
        unsafe { write(out, Box::into_raw(Box::new(FyTexture(image))), "out") }
    })
}

/// Writes the model's material `index` to `out`, its colours converted to
/// sRGB. Each texture field is the model's texture index plus 1 (0 for
/// none), not a world's id: the caller maps them to its own.
///
/// # Safety
///
/// `model` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_model_material(model: *const FyModel, index: usize, out: *mut FyMaterial) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let model = &unsafe { handle(model, "model") }?.0;
        let data = nth(model.materials(), index, "materials")?;
        let numbers: Vec<TextureId> =
            (1..=model.images().len() as u64).map(|n| TextureId::from_raw(n).expect("from 1")).collect();
        let material: FyMaterial = Material::from_model(data, &numbers).into();
        // SAFETY: per this function's contract.
        unsafe { write(out, material, "out") }
    })
}

/// The name the file gives material `index` ("" for none), as UTF-8
/// without a terminating zero: its length goes to `out_len`, and it's
/// copied into `buf` only when it fits in `cap`.
///
/// # Safety
///
/// `model` is null or live; `buf` is null or valid for `cap` writes;
/// `out_len` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_model_material_name(
    model: *const FyModel,
    index: usize,
    buf: *mut u8,
    cap: usize,
    out_len: *mut usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let model = &unsafe { handle(model, "model") }?.0;
        let name = &nth(model.materials(), index, "materials")?.name;
        // SAFETY: per this function's contract.
        unsafe { write_all(name.bytes(), buf, cap, out_len) }
    })
}

/// Warning `index`: one line saying what was skipped, as UTF-8 like
/// `fy_model_material_name`.
///
/// # Safety
///
/// `model` is null or live; `buf` is null or valid for `cap` writes;
/// `out_len` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_model_warning(
    model: *const FyModel,
    index: usize,
    buf: *mut u8,
    cap: usize,
    out_len: *mut usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let model = &unsafe { handle(model, "model") }?.0;
        let warning = nth(model.warnings(), index, "warnings")?;
        // SAFETY: per this function's contract.
        unsafe { write_all(warning.bytes(), buf, cap, out_len) }
    })
}

/// Clip `index`'s name, as UTF-8 like `fy_model_material_name`, and its
/// length in seconds to `out_length`.
///
/// # Safety
///
/// `model` is null or live; `buf` is null or valid for `cap` writes;
/// `out_len` and `out_length` are null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_model_clip(
    model: *const FyModel,
    index: usize,
    buf: *mut u8,
    cap: usize,
    out_len: *mut usize,
    out_length: *mut f32,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let model = &unsafe { handle(model, "model") }?.0;
        let clip = nth(model.clips(), index, "clips")?;
        // SAFETY: per this function's contract.
        unsafe {
            write(out_length, clip.length, "out_length")?;
            write_all(clip.name.bytes(), buf, cap, out_len)
        }
    })
}

/// `count` ids at `p`, each turned into a typed id by `id`.
///
/// # Safety
///
/// `p` is null or valid for `count` reads (it may be null when `count` is
/// 0).
unsafe fn ids<T>(p: *const u64, count: usize, what: &str, id: fn(u64) -> Result<T, Error>) -> Result<Vec<T>, Error> {
    if count == 0 {
        return Ok(Vec::new());
    }
    // SAFETY: per this function's contract.
    let raw = unsafe { std::slice::from_raw_parts(handle(p, what)?, count) };
    raw.iter().map(|&r| id(r)).collect()
}

/// Spawns the model: a root object at `transform`, named after the model's
/// scene, with one object per node below it, named and placed as in the
/// file. A node drawing one primitive gets a mesh renderer; one drawing
/// several gets an unnamed child for each. Writes the root's id to `out`.
///
/// `meshes` and `materials` are this world's ids for the model's meshes
/// (`fy_mesh_add` of each `fy_model_mesh`) and materials (`fy_material_add`
/// of each `fy_model_material`, its textures mapped), in the model's
/// order. Spawning again with the same ids shares everything. On error
/// nothing is spawned.
///
/// # Safety
///
/// `world` and `model` are null or live; `meshes` and `materials` are
/// valid for their counts; `transform` is null or readable; `out` is null
/// or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_model_spawn(
    world: *const FyWorld,
    model: *const FyModel,
    meshes: *const u64,
    mesh_count: usize,
    materials: *const u64,
    material_count: usize,
    transform: *const FyTransform,
    out: *mut u64,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, model, transform) =
            unsafe { (handle(world, "world")?, &handle(model, "model")?.0, read(transform, "transform")?) };
        // SAFETY: per this function's contract.
        let (meshes, materials) = unsafe {
            (ids(meshes, mesh_count, "meshes", mesh)?, ids(materials, material_count, "materials", material)?)
        };
        let root = world.lock()?.spawn_model(model, &meshes, &materials, transform.to_component()?)?;
        // SAFETY: per this function's contract.
        unsafe { write(out, root.raw(), "out") }
    })
}
