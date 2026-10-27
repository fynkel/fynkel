//! Meshes made from shapes or from data.

use std::sync::Arc;

use fynkel_world::{MeshData, MeshParts};

use crate::error::{Error, FyStatus, guard};

use super::*;

// --- Meshes and materials ----------------------------------------------------

/// A mesh on its own, before any world has it. Opaque to C. Made with
/// `fy_mesh_new` or `fy_mesh_new_shape` on any thread, freed with
/// `fy_mesh_free`. Worlds that add it share its data, so freeing it
/// doesn't take it from them.
#[derive(Debug)]
pub struct FyMesh(pub(super) Arc<MeshData>, pub(super) Vec<Arc<MeshData>>);

/// How many copies a mesh that changes keeps from before, for its next
/// changes to be written into: a world, and frames on their way to the
/// GPU, hold the last few.
const SPARE_MESHES: usize = 4;

impl FyMesh {
    pub(super) fn new(data: Arc<MeshData>) -> Self {
        FyMesh(data, Vec::new())
    }

    /// Its data, to change. While another holds it (a world, a frame on its
    /// way to the GPU) the data is copied first: into a copy from before
    /// that nothing holds any longer, so a mesh changed each frame
    /// allocates nothing once it has a few, or else into a new one.
    pub(super) fn own(&mut self) -> &mut MeshData {
        if Arc::get_mut(&mut self.0).is_none() {
            let free = self.1.iter_mut().position(|spare| Arc::get_mut(spare).is_some());
            let next = match free {
                Some(i) => {
                    let mut spare = self.1.swap_remove(i);
                    Arc::get_mut(&mut spare).expect("nothing else holds it").clone_from(&self.0);
                    spare
                }
                None => Arc::new((*self.0).clone()),
            };
            let held = std::mem::replace(&mut self.0, next);
            if self.1.len() == SPARE_MESHES {
                self.1.remove(0);
            }
            self.1.push(held);
        }
        Arc::get_mut(&mut self.0).expect("only this mesh holds it")
    }
}

/// Writes a new mesh handle to `out`.
///
/// # Safety
///
/// `out` is null or writable.
pub(super) unsafe fn new_mesh(data: MeshData, out: *mut *mut FyMesh) -> Result<(), Error> {
    if out.is_null() {
        return Err(Error::null("out"));
    }
    // SAFETY: not null, and the caller promises it's writable.
    unsafe { out.write(Box::into_raw(Box::new(FyMesh::new(Arc::new(data))))) };
    Ok(())
}

/// Makes a built-in shape (`FY_MESH_CUBE`, `FY_MESH_SPHERE`,
/// `FY_MESH_PLANE`, `FY_MESH_CAPSULE`, `FY_MESH_CYLINDER` or
/// `FY_MESH_QUAD`) and writes it to `out`.
///
/// # Safety
///
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_new_shape(shape: u32, out: *mut *mut FyMesh) -> FyStatus {
    guard(|| {
        let data = match shape {
            FY_MESH_CUBE => MeshData::cube(),
            // 720 triangles, as light as Unity's sphere (768): 2,208 (48 ×
            // 24) held the Note10+ at 23 fps with 1,000 of them.
            FY_MESH_SPHERE => MeshData::sphere(24, 16),
            FY_MESH_PLANE => MeshData::plane(),
            FY_MESH_CAPSULE => MeshData::capsule(24, 6),
            FY_MESH_CYLINDER => MeshData::cylinder(24),
            FY_MESH_QUAD => MeshData::quad(),
            other => {
                return Err(Error::new(
                    FyStatus::InvalidValue,
                    format!(
                        "mesh shape {other} doesn't exist (0 = cube, 1 = sphere, 2 = plane, 3 = capsule, 4 = \
                         cylinder, 5 = quad)"
                    ),
                ));
            }
        };
        // SAFETY: per this function's contract.
        unsafe { new_mesh(data, out) }
    })
}

/// `count` items of `N` values at `p`, or `None` when `p` is null.
///
/// # Safety
///
/// `p` is null or valid for `count * N` reads.
pub(super) unsafe fn items<'a, T, const N: usize>(
    p: *const T,
    count: usize,
    what: &str,
) -> Result<Option<&'a [[T; N]]>, Error> {
    if p.is_null() {
        return Ok(None);
    }
    let len = count.checked_mul(N).ok_or_else(|| Error::new(FyStatus::InvalidValue, format!("too many {what}")))?;
    // SAFETY: not null; the caller promises len readable items, and [T; N]
    // has T's alignment.
    let flat = unsafe { std::slice::from_raw_parts(p, len) };
    Ok(Some(flat.as_chunks::<N>().0))
}

/// Makes a mesh from arrays and writes it to `out`. Only positions and
/// indices are required:
/// - Missing normals are the average of the faces around each vertex,
///   weighted by angle, so shared vertices are smooth and sharp corners
///   need a vertex per face.
/// - Missing tangents are generated with MikkTSpace from the UVs; a vertex
///   whose faces need different tangents (a mirrored texture's seam) is
///   split.
/// - Missing UVs are (0, 0).
///
/// A broken mesh (indices out of range, not a multiple of 3, values that
/// aren't finite, arrays of the wrong length) is refused with
/// `FY_STATUS_INVALID_VALUE` and a message that says which item. The arrays
/// are copied and can be freed after the call.
///
/// Generating tangents takes about 1 µs per triangle, so a big mesh with UVs
/// is worth making off the UI thread. This touches no world and can run on
/// any thread.
///
/// # Safety
///
/// `mesh` is null or readable, and its non-null arrays are valid for the
/// counts it gives; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_new(mesh: *const FyMeshData, out: *mut *mut FyMesh) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let m = unsafe { read(mesh, "mesh") }?;
        let n = m.vertex_count;
        // SAFETY: per this function's contract, for each array.
        let parts = unsafe {
            MeshParts {
                positions: items::<f32, 3>(m.positions, n, "positions")?.ok_or_else(|| Error::null("positions"))?,
                normals: items::<f32, 3>(m.normals, n, "normals")?,
                tangents: items::<f32, 4>(m.tangents, n, "tangents")?,
                uvs: items::<f32, 2>(m.uvs, n, "uvs")?,
                colors: items::<u8, 4>(m.colors, n, "colors")?,
                indices: items::<u32, 1>(m.indices, m.index_count, "indices")?
                    .ok_or_else(|| Error::null("indices"))?
                    .as_flattened(),
                submeshes: items::<u32, 1>(m.submesh_starts, m.submesh_start_count, "submesh_starts")?
                    .unwrap_or_default()
                    .as_flattened(),
                skin: None,
            }
        };
        let data = MeshData::new(parts).map_err(|e| Error::new(FyStatus::InvalidValue, e.to_string()))?;
        // SAFETY: per this function's contract.
        unsafe { new_mesh(data, out) }
    })
}

/// Makes a mesh with nothing in it, to change (`fy_mesh_set_positions` and
/// the rest), and writes it to `out`. It draws nothing until it has
/// triangles.
///
/// # Safety
///
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_new_empty(out: *mut *mut FyMesh) -> FyStatus {
    // SAFETY: per this function's contract.
    guard(|| unsafe { new_mesh(MeshData::empty(), out) })
}
