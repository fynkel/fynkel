//! Shapes with settings, outlines, combining and sweeping; changing and reading
//! meshes; meshes and materials in a world.

use std::cell::RefCell;
use std::sync::Arc;

use fynkel_world::{MeshData, Vec2, Vec3};

use crate::error::{Error, FyStatus, guard};

use super::*;

// --- Shapes with settings, outlines, combining (stage 13 phase 2) ------------

/// The most segments a shape takes along any one direction.
const MAX_SEGMENTS: u32 = 1024;

/// `value` if it's finite and more than 0.
fn length(value: f32, what: &str) -> Result<f32, Error> {
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err(Error::new(FyStatus::InvalidValue, format!("{what} is {value}; it must be more than 0")))
    }
}

/// `value` if it's finite and 0 or more.
fn at_least_0(value: f32, what: &str) -> Result<f32, Error> {
    if value.is_finite() && value >= 0.0 {
        Ok(value)
    } else {
        Err(Error::new(FyStatus::InvalidValue, format!("{what} is {value}; it must be 0 or more")))
    }
}

/// `n` if it's from `least` to [`MAX_SEGMENTS`].
fn segment_count(n: u32, least: u32, what: &str) -> Result<u32, Error> {
    if (least..=MAX_SEGMENTS).contains(&n) {
        Ok(n)
    } else {
        Err(Error::new(FyStatus::InvalidValue, format!("{what} is {n}; it must be from {least} to {MAX_SEGMENTS}")))
    }
}

/// Makes a box of `size` centred on the origin, each face cut into a grid
/// of `segments_x`, `segments_y` and `segments_z` squares along X, Y and Z
/// (1 or more each), its UVs in metres times `uv_per_metre`, and writes it
/// to `out`.
///
/// # Safety
///
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_new_box(
    size: FyVec3,
    segments_x: u32,
    segments_y: u32,
    segments_z: u32,
    uv_per_metre: f32,
    out: *mut *mut FyMesh,
) -> FyStatus {
    guard(|| {
        let size = Vec3::new(length(size.x, "size.x")?, length(size.y, "size.y")?, length(size.z, "size.z")?);
        let counts = [
            segment_count(segments_x, 1, "segments_x")?,
            segment_count(segments_y, 1, "segments_y")?,
            segment_count(segments_z, 1, "segments_z")?,
        ];
        let data = MeshData::box_shape(size, counts, at_least_0(uv_per_metre, "uv_per_metre")?);
        // SAFETY: per this function's contract.
        unsafe { new_mesh(data, out) }
    })
}

/// Makes a flat grid `width` along X by `depth` along Z facing +Y,
/// centred on the origin, cut into `segments_x` by `segments_z` squares,
/// its UVs in metres times `uv_per_metre`, and writes it to `out`.
///
/// # Safety
///
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_new_grid(
    width: f32,
    depth: f32,
    segments_x: u32,
    segments_z: u32,
    uv_per_metre: f32,
    out: *mut *mut FyMesh,
) -> FyStatus {
    guard(|| {
        let size = Vec2::new(length(width, "width")?, length(depth, "depth")?);
        let counts = [segment_count(segments_x, 1, "segments_x")?, segment_count(segments_z, 1, "segments_z")?];
        let data = MeshData::grid(size, counts, at_least_0(uv_per_metre, "uv_per_metre")?);
        // SAFETY: per this function's contract.
        unsafe { new_mesh(data, out) }
    })
}

/// Makes a sphere of `radius` with `segments` slices around (3 or more)
/// and `rings` bands from pole to pole (2 or more), and writes it to
/// `out`.
///
/// # Safety
///
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_new_sphere(radius: f32, segments: u32, rings: u32, out: *mut *mut FyMesh) -> FyStatus {
    guard(|| {
        let data = MeshData::sphere_sized(
            length(radius, "radius")?,
            segment_count(segments, 3, "segments")?,
            segment_count(rings, 2, "rings")?,
        );
        // SAFETY: per this function's contract.
        unsafe { new_mesh(data, out) }
    })
}

/// Makes a capsule along Y, `height` from end to end (less than twice the
/// radius is a sphere) and of `radius`, with `segments` slices around (3
/// or more) and `rings` bands in each round end (1 or more), and writes it
/// to `out`.
///
/// # Safety
///
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_new_capsule(
    radius: f32,
    height: f32,
    segments: u32,
    rings: u32,
    out: *mut *mut FyMesh,
) -> FyStatus {
    guard(|| {
        let data = MeshData::capsule_sized(
            length(radius, "radius")?,
            at_least_0(height, "height")?,
            segment_count(segments, 3, "segments")?,
            segment_count(rings, 1, "rings")?,
        );
        // SAFETY: per this function's contract.
        unsafe { new_mesh(data, out) }
    })
}

/// Makes a cylinder along Y, `height` high and of `radius`, with
/// `segments` slices around (3 or more), flat ends unless `caps` is
/// false, and writes it to `out`.
///
/// # Safety
///
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_new_cylinder(
    radius: f32,
    height: f32,
    segments: u32,
    caps: bool,
    out: *mut *mut FyMesh,
) -> FyStatus {
    guard(|| {
        let data = MeshData::cylinder_sized(
            length(radius, "radius")?,
            length(height, "height")?,
            segment_count(segments, 3, "segments")?,
            caps,
        );
        // SAFETY: per this function's contract.
        unsafe { new_mesh(data, out) }
    })
}

/// Makes a cone along Y, `height` high with its point at the top and a
/// base of `radius`, with `segments` slices around (3 or more), a flat
/// base unless `cap` is false, and writes it to `out`.
///
/// # Safety
///
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_new_cone(
    radius: f32,
    height: f32,
    segments: u32,
    cap: bool,
    out: *mut *mut FyMesh,
) -> FyStatus {
    guard(|| {
        let data = MeshData::cone(
            length(radius, "radius")?,
            length(height, "height")?,
            segment_count(segments, 3, "segments")?,
            cap,
        );
        // SAFETY: per this function's contract.
        unsafe { new_mesh(data, out) }
    })
}

/// Makes a ring around Y, its middle `radius` from the centre and its
/// tube of radius `tube`, with `segments` slices around Y and `sides`
/// around the tube (3 or more each), and writes it to `out`.
///
/// # Safety
///
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_new_torus(
    radius: f32,
    tube: f32,
    segments: u32,
    sides: u32,
    out: *mut *mut FyMesh,
) -> FyStatus {
    guard(|| {
        let data = MeshData::torus(
            length(radius, "radius")?,
            length(tube, "tube")?,
            segment_count(segments, 3, "segments")?,
            segment_count(sides, 3, "sides")?,
        );
        // SAFETY: per this function's contract.
        unsafe { new_mesh(data, out) }
    })
}

/// Makes a flat disc of `radius` facing +Y with `segments` around (3 or
/// more), a ring when `inner` (less than `radius`) is more than 0, and
/// writes it to `out`.
///
/// # Safety
///
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_new_disc(radius: f32, inner: f32, segments: u32, out: *mut *mut FyMesh) -> FyStatus {
    guard(|| {
        let radius = length(radius, "radius")?;
        let inner = at_least_0(inner, "inner")?;
        if inner >= radius {
            return Err(Error::new(FyStatus::InvalidValue, format!("inner is {inner}; it must be less than {radius}")));
        }
        let data = MeshData::disc(radius, inner, segment_count(segments, 3, "segments")?);
        // SAFETY: per this function's contract.
        unsafe { new_mesh(data, out) }
    })
}

/// The loops of `points`: `loop_count` of them, each `loop_sizes[k]`
/// points long, one after another.
///
/// # Safety
///
/// `points` is valid for `point_count` items and `loop_sizes` for
/// `loop_count`.
unsafe fn loops<'a>(
    points: *const FyVec2,
    point_count: usize,
    loop_sizes: *const u32,
    loop_count: usize,
) -> Result<Vec<&'a [Vec2]>, Error> {
    let mut rings = Vec::with_capacity(loop_count);
    // SAFETY: per this function's contract.
    unsafe { each_loop(points, point_count, loop_sizes, loop_count, |ring| rings.push(ring)) }?;
    Ok(rings)
}

/// [`loops`] handed to `each` one at a time, allocating nothing.
///
/// # Safety
///
/// As [`loops`].
unsafe fn each_loop<'a>(
    points: *const FyVec2,
    point_count: usize,
    loop_sizes: *const u32,
    loop_count: usize,
    mut each: impl FnMut(&'a [Vec2]),
) -> Result<(), Error> {
    if points.is_null() {
        return Err(Error::null("points"));
    }
    if loop_sizes.is_null() || loop_count == 0 {
        return Err(Error::null("loop_sizes"));
    }
    // SAFETY: not null, and the caller promises the counts; FyVec2 is two
    // f32s, as Vec2.
    let (points, sizes) = unsafe {
        (
            std::slice::from_raw_parts(points.cast::<Vec2>(), point_count),
            std::slice::from_raw_parts(loop_sizes, loop_count),
        )
    };
    let total: usize = sizes.iter().map(|&n| n as usize).sum();
    if total != point_count {
        return Err(Error::new(
            FyStatus::InvalidValue,
            format!("the loops have {total} points between them, but there are {point_count}"),
        ));
    }
    let mut from = 0;
    for &n in sizes {
        each(&points[from..from + n as usize]);
        from += n as usize;
    }
    Ok(())
}

const _: () = assert!(std::mem::size_of::<FyVec2>() == std::mem::size_of::<Vec2>());

/// Makes a slab `depth` thick along Z from an outline less its holes, and
/// writes it to `out`. The `points` are the outline's, then each hole's;
/// `loop_sizes` says how many each has, the outline first (each 3 or
/// more). Either way round is fine. The front faces +Z; the ends show the
/// texture flat, `uv_per_metre` times a metre. Corners sharper than 40°
/// are sharp, the rest smooth.
///
/// # Safety
///
/// `points` is valid for `point_count` items and `loop_sizes` for
/// `loop_count`; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_extrude(
    points: *const FyVec2,
    point_count: usize,
    loop_sizes: *const u32,
    loop_count: usize,
    depth: f32,
    uv_per_metre: f32,
    out: *mut *mut FyMesh,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let rings = unsafe { loops(points, point_count, loop_sizes, loop_count) }?;
        let data = MeshData::extrude(
            rings[0],
            &rings[1..],
            length(depth, "depth")?,
            at_least_0(uv_per_metre, "uv_per_metre")?,
        )
        .map_err(|e| Error::new(FyStatus::InvalidValue, e.to_string()))?;
        // SAFETY: per this function's contract.
        unsafe { new_mesh(data, out) }
    })
}

/// Makes a mesh by turning `profile` around Y with `sides` slices (3 or
/// more), as on a lathe, and writes it to `out`. Each point is (distance
/// from Y, height); the outside is on the right of the way the profile
/// goes. Corners sharper than 40° are sharp, the rest smooth.
///
/// # Safety
///
/// `profile` is valid for `count` items; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_lathe(
    profile: *const FyVec2,
    count: usize,
    sides: u32,
    out: *mut *mut FyMesh,
) -> FyStatus {
    guard(|| {
        let size = [count as u32];
        // SAFETY: per this function's contract.
        let rings = unsafe { loops(profile, count, size.as_ptr(), 1) }?;
        if let Some(at) = rings[0].iter().position(|p| p.x < 0.0) {
            return Err(Error::new(
                FyStatus::InvalidValue,
                format!("profile point {at} is left of the axis (x below 0)"),
            ));
        }
        let data = MeshData::lathe(rings[0], segment_count(sides, 3, "sides")?)
            .map_err(|e| Error::new(FyStatus::InvalidValue, e.to_string()))?;
        // SAFETY: per this function's contract.
        unsafe { new_mesh(data, out) }
    })
}

/// Makes one mesh of `count` meshes, each placed by its transform, and
/// writes it to `out` (Unity's `CombineMeshes`). Submesh n of each goes
/// into submesh n of the result, so it's one draw per material. A
/// transform that mirrors turns its triangles round. A skinned mesh, or
/// one with no triangles, is refused.
///
/// # Safety
///
/// `meshes` and `transforms` are valid for `count` items, each mesh live;
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_combine(
    meshes: *const *const FyMesh,
    transforms: *const FyTransform,
    count: usize,
    out: *mut *mut FyMesh,
) -> FyStatus {
    guard(|| {
        if meshes.is_null() {
            return Err(Error::null("meshes"));
        }
        if transforms.is_null() {
            return Err(Error::null("transforms"));
        }
        // SAFETY: not null; the caller promises count items.
        let (meshes, transforms) =
            unsafe { (std::slice::from_raw_parts(meshes, count), std::slice::from_raw_parts(transforms, count)) };
        let mut parts = Vec::with_capacity(count);
        for (k, (&mesh, &t)) in meshes.iter().zip(transforms).enumerate() {
            // SAFETY: the caller promises each is live.
            let mesh = unsafe { mesh.as_ref() }.ok_or_else(|| Error::null(&format!("meshes[{k}]")))?;
            parts.push((&*mesh.0, t.to_component()?.matrix()));
        }
        let data = MeshData::combine(&parts).map_err(|e| Error::new(FyStatus::InvalidValue, e.to_string()))?;
        // SAFETY: per this function's contract.
        unsafe { new_mesh(data, out) }
    })
}

/// Loops an outline and its holes have up to which [`with_sweep`] lists
/// them on the stack.
const FEW_LOOPS: usize = 16;

/// The sweep of a path of `path_count` points, ten floats each, and an
/// outline's loops, as `fy_mesh_sweep` takes them, handed to `f`. The path
/// is read into room kept on the thread, so a rope swept each frame
/// allocates nothing (stage 14's WO8).
///
/// # Safety
///
/// As `fy_mesh_sweep`.
#[allow(clippy::too_many_arguments)]
unsafe fn with_sweep<T>(
    path: *const f32,
    path_count: usize,
    points: *const f32,
    point_count: usize,
    loop_sizes: *const u32,
    loop_count: usize,
    caps: bool,
    uv_per_metre: f32,
    f: impl FnOnce(&fynkel_world::Sweep<'_>) -> Result<T, Error>,
) -> Result<T, Error> {
    thread_local! {
        static PATH: RefCell<Vec<fynkel_world::PathPoint>> = RefCell::default();
    }
    // SAFETY: per this function's contract.
    let floats = unsafe { items::<f32, 10>(path, path_count, "path") }?.ok_or_else(|| Error::null("path"))?;
    let uv_per_metre = at_least_0(uv_per_metre, "uv_per_metre")?;
    let points = points.cast::<FyVec2>();
    let (mut few, many): ([&[Vec2]; FEW_LOOPS], Vec<&[Vec2]>);
    let rings: &[&[Vec2]] = if loop_count <= FEW_LOOPS {
        few = [&[]; FEW_LOOPS];
        let mut k = 0;
        // SAFETY: per this function's contract; two floats are a FyVec2.
        unsafe {
            each_loop(points, point_count, loop_sizes, loop_count, |ring| {
                few[k] = ring;
                k += 1;
            })
        }?;
        &few[..loop_count]
    } else {
        // SAFETY: as above.
        many = unsafe { loops(points, point_count, loop_sizes, loop_count) }?;
        &many
    };
    PATH.with_borrow_mut(|path| {
        path.clear();
        path.extend(floats.iter().map(|p| fynkel_world::PathPoint {
            position: Vec3::from_slice(&p[0..3]),
            forward: Vec3::from_slice(&p[3..6]),
            up: Vec3::from_slice(&p[6..9]),
            distance: p[9],
        }));
        f(&fynkel_world::Sweep { path, outline: rings[0], holes: &rings[1..], caps, uv_per_metre })
    })
}

/// Makes a mesh of an outline swept along a path (a road, a rope, a pipe)
/// and writes it to `out`. The path is `path_count` points (2 or more),
/// ten floats each: its position, the way it goes (any length but 0),
/// which way is up across it, and how far along it is in metres. The
/// outline is drawn across the path at each, its X to the right and Y
/// up; `points` are its points, two floats each, then each hole's, and
/// `loop_sizes` says how many each has, the outline first (each 3 or
/// more, either way round). `caps` fills the ends; a path that closes
/// (its last point where its first is) has none. The texture repeats
/// `uv_per_metre` times a metre around the outline and along the path.
/// Corners of the outline sharper than 40° are sharp, the rest smooth.
///
/// # Safety
///
/// `path` is valid for `path_count * 10` floats, `points` for
/// `point_count * 2` and `loop_sizes` for `loop_count` items; `out` is
/// null or writable.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn fy_mesh_sweep(
    path: *const f32,
    path_count: usize,
    points: *const f32,
    point_count: usize,
    loop_sizes: *const u32,
    loop_count: usize,
    caps: bool,
    uv_per_metre: f32,
    out: *mut *mut FyMesh,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let data = unsafe {
            with_sweep(path, path_count, points, point_count, loop_sizes, loop_count, caps, uv_per_metre, |sweep| {
                MeshData::sweep(sweep).map_err(|e| Error::new(FyStatus::InvalidValue, e.to_string()))
            })
        }?;
        // SAFETY: per this function's contract.
        unsafe { new_mesh(data, out) }
    })
}

/// Sweeps into `mesh` in place of what it had, as `fy_mesh_sweep` makes
/// one: a rope swept again each frame. Worlds that have it keep what they
/// had until `fy_mesh_update`; when the triangles come out as they were,
/// they're kept. A skinned mesh is refused.
///
/// # Safety
///
/// `mesh` is null or live, used on no other thread meanwhile; the rest as
/// `fy_mesh_sweep`.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn fy_mesh_set_sweep(
    mesh: *mut FyMesh,
    path: *const f32,
    path_count: usize,
    points: *const f32,
    point_count: usize,
    loop_sizes: *const u32,
    loop_count: usize,
    caps: bool,
    uv_per_metre: f32,
) -> FyStatus {
    guard(|| {
        // SAFETY: the caller promises it's null or live, and only theirs.
        let mesh = unsafe { mesh.as_mut() }.ok_or_else(|| Error::null("mesh"))?;
        // SAFETY: per this function's contract.
        unsafe {
            with_sweep(path, path_count, points, point_count, loop_sizes, loop_count, caps, uv_per_metre, |sweep| {
                mesh.own().sweep_into(sweep).map_err(|e| Error::new(FyStatus::InvalidValue, e.to_string()))
            })
        }
    })
}

/// Changes `mesh`'s own data, as `change` says. Worlds that have it keep
/// what they had until `fy_mesh_update`. The data is copied when another
/// holds it (a world, a frame on its way to the GPU), so the first change
/// after an update copies the mesh and the rest change it in place; the
/// copy goes into one from before when nothing holds that any longer.
///
/// # Safety
///
/// `mesh` is null or live, and not used on another thread meanwhile.
unsafe fn change_mesh(mesh: *mut FyMesh, change: fynkel_world::MeshChange<'_>) -> Result<(), Error> {
    // SAFETY: the caller promises it's null or live, and only theirs.
    let mesh = unsafe { mesh.as_mut() }.ok_or_else(|| Error::null("mesh"))?;
    mesh.own().change(change).map_err(|e| Error::new(FyStatus::InvalidValue, e.to_string()))
}

/// Sets the vertices' positions, three floats each (x, y, z). A different
/// count from the mesh's resets every other channel: normals and tangents
/// are worked out again, UVs go to (0, 0) and colours to white; then its
/// triangles must not point past the last vertex, so set those first when
/// there are fewer. The array is copied.
///
/// # Safety
///
/// `mesh` is null or live, used on no other thread meanwhile;
/// `positions` is valid for `count * 3` floats (or null with `count` 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_set_positions(mesh: *mut FyMesh, positions: *const f32, count: usize) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let positions = unsafe { items::<f32, 3>(positions, count, "positions") }?.unwrap_or_default();
        let change = fynkel_world::MeshChange { positions: Some(positions), ..Default::default() };
        // SAFETY: per this function's contract.
        unsafe { change_mesh(mesh, change) }
    })
}

/// Sets the vertices' normals, three floats each, one per vertex. Each is
/// made unit length; none may be zero length.
///
/// # Safety
///
/// As `fy_mesh_set_positions`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_set_normals(mesh: *mut FyMesh, normals: *const f32, count: usize) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let normals = unsafe { items::<f32, 3>(normals, count, "normals") }?.unwrap_or_default();
        // SAFETY: per this function's contract.
        unsafe { change_mesh(mesh, fynkel_world::MeshChange { normals: Some(normals), ..Default::default() }) }
    })
}

/// Sets the vertices' tangents, four floats each (a direction, then w = 1
/// or -1), one per vertex.
///
/// # Safety
///
/// As `fy_mesh_set_positions`, for `count * 4` floats.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_set_tangents(mesh: *mut FyMesh, tangents: *const f32, count: usize) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let tangents = unsafe { items::<f32, 4>(tangents, count, "tangents") }?.unwrap_or_default();
        // SAFETY: per this function's contract.
        unsafe { change_mesh(mesh, fynkel_world::MeshChange { tangents: Some(tangents), ..Default::default() }) }
    })
}

/// Sets the vertices' texture coordinates, two floats each, one per vertex.
///
/// # Safety
///
/// As `fy_mesh_set_positions`, for `count * 2` floats.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_set_uvs(mesh: *mut FyMesh, uvs: *const f32, count: usize) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let uvs = unsafe { items::<f32, 2>(uvs, count, "uvs") }?.unwrap_or_default();
        // SAFETY: per this function's contract.
        unsafe { change_mesh(mesh, fynkel_world::MeshChange { uvs: Some(uvs), ..Default::default() }) }
    })
}

/// Sets the vertices' colours, four bytes each (sRGB red, green and blue,
/// then alpha), one per vertex. They multiply the material's colour.
///
/// # Safety
///
/// As `fy_mesh_set_positions`, for `count * 4` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_set_colors(mesh: *mut FyMesh, colors: *const u8, count: usize) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let colors = unsafe { items::<u8, 4>(colors, count, "colors") }?.unwrap_or_default();
        // SAFETY: per this function's contract.
        unsafe { change_mesh(mesh, fynkel_world::MeshChange { colors: Some(colors), ..Default::default() }) }
    })
}

/// Sets the triangles: three indices each, counter-clockwise seen from the
/// front, every submesh's one after another, and where each submesh after
/// the first starts (as `FyMeshData`'s). None is allowed: the mesh then
/// draws nothing.
///
/// # Safety
///
/// As `fy_mesh_set_positions`; `indices` is valid for `index_count` u32s
/// and `starts` for `start_count` (each null with its count 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_set_triangles(
    mesh: *mut FyMesh,
    indices: *const u32,
    index_count: usize,
    starts: *const u32,
    start_count: usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (indices, starts) = unsafe {
            (
                items::<u32, 1>(indices, index_count, "indices")?.unwrap_or_default().as_flattened(),
                items::<u32, 1>(starts, start_count, "starts")?.unwrap_or_default().as_flattened(),
            )
        };
        let change = fynkel_world::MeshChange { indices: Some((indices, starts)), ..Default::default() };
        // SAFETY: per this function's contract.
        unsafe { change_mesh(mesh, change) }
    })
}

/// Works out the mesh's normals (`FY_MESH_NORMALS`), from the faces around
/// each vertex weighted by their area, and its tangents
/// (`FY_MESH_TANGENTS`), from its UVs without splitting a vertex. Normals
/// first, when both.
///
/// # Safety
///
/// `mesh` is null or live, used on no other thread meanwhile.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_recalculate(mesh: *mut FyMesh, what: u32) -> FyStatus {
    guard(|| {
        let change = fynkel_world::MeshChange {
            recalculate_normals: what & FY_MESH_NORMALS != 0,
            recalculate_tangents: what & FY_MESH_TANGENTS != 0,
            ..Default::default()
        };
        // SAFETY: per this function's contract.
        unsafe { change_mesh(mesh, change) }
    })
}

/// Says the mesh changes often (Unity's `MarkDynamic`): each view keeps it
/// on the GPU with room to grow and writes changes where it is.
///
/// # Safety
///
/// `mesh` is null or live, used on no other thread meanwhile.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_mark_dynamic(mesh: *mut FyMesh) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let mesh = unsafe { mesh.as_mut() }.ok_or_else(|| Error::null("mesh"))?;
        if !mesh.0.is_dynamic() {
            mesh.own().mark_dynamic();
        }
        Ok(())
    })
}

/// Gives the world `mesh` as it is now under `id`, which it was added as
/// (`fy_mesh_add`): its objects are drawn with it, and each view uploads it
/// again, from the next frame. Nothing is copied.
///
/// # Safety
///
/// `world` and `mesh` are null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_update(world: *const FyWorld, id: u64, mesh: *const FyMesh) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, data) = unsafe { (handle(world, "world")?, handle(mesh, "mesh")?) };
        Ok(world.lock()?.set_mesh(self::mesh(id)?, Arc::clone(&data.0))?)
    })
}

/// Copies the mesh's normals (3 floats each), tangents (4), UVs (2) and
/// colours (4 bytes) to the arrays given, each for `vertex_count`
/// vertices, which must be `fy_mesh_counts`' number. A null array is
/// skipped.
///
/// # Safety
///
/// `mesh` is null or live; each array is null or writable for its items.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_read_vertices(
    mesh: *const FyMesh,
    normals: *mut f32,
    tangents: *mut f32,
    uvs: *mut f32,
    colors: *mut u8,
    vertex_count: usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let mesh = unsafe { handle(mesh, "mesh") }?;
        let vertices = mesh.0.vertices();
        if vertex_count != vertices.len() {
            return Err(Error::new(
                FyStatus::WrongSize,
                format!("the mesh has {} vertices, not {vertex_count}", vertices.len()),
            ));
        }
        /// Each vertex's `N` values from `get`, to `out` if it isn't null.
        ///
        /// # Safety
        ///
        /// `out` is null or writable for `vertices.len() * N` values.
        unsafe fn copy<T: Copy, const N: usize>(
            out: *mut T,
            vertices: &[fynkel_world::Vertex],
            get: impl Fn(&fynkel_world::Vertex) -> [T; N],
        ) {
            if out.is_null() {
                return;
            }
            // SAFETY: not null, and writable for this many per the contract.
            let out = unsafe { std::slice::from_raw_parts_mut(out, vertices.len() * N) };
            for (out, v) in out.chunks_exact_mut(N).zip(vertices) {
                out.copy_from_slice(&get(v));
            }
        }
        // SAFETY: per this function's contract.
        unsafe {
            copy(normals, vertices, |v| v.normal);
            copy(tangents, vertices, |v| v.tangent);
            copy(uvs, vertices, |v| v.uv);
            copy(colors, vertices, |v| v.color);
        }
        Ok(())
    })
}

/// Writes how many submeshes the mesh has: 1 when it doesn't split.
///
/// # Safety
///
/// `mesh` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_submesh_count(mesh: *const FyMesh, out: *mut usize) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let mesh = unsafe { handle(mesh, "mesh") }?;
        // SAFETY: per this function's contract.
        unsafe { write(out, mesh.0.submeshes().len(), "out") }
    })
}

/// Writes where each submesh starts in the indices, and how many it has,
/// for `count` submeshes, which must be `fy_mesh_submesh_count`'s.
///
/// # Safety
///
/// `mesh` is null or live; `starts` and `counts` are each null or writable
/// for `count` u32s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_read_submeshes(
    mesh: *const FyMesh,
    starts: *mut u32,
    counts: *mut u32,
    count: usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let mesh = unsafe { handle(mesh, "mesh") }?;
        let submeshes = mesh.0.submeshes();
        if count != submeshes.len() {
            return Err(Error::new(
                FyStatus::WrongSize,
                format!("the mesh has {} submeshes, not {count}", submeshes.len()),
            ));
        }
        for (i, range) in submeshes.iter().enumerate() {
            // SAFETY: each is null or writable for count u32s per the contract.
            unsafe {
                if !starts.is_null() {
                    starts.add(i).write(range.start);
                }
                if !counts.is_null() {
                    counts.add(i).write(range.end - range.start);
                }
            }
        }
        Ok(())
    })
}

/// Frees a mesh handle. Worlds that added it keep their share. Null is
/// ignored.
///
/// # Safety
///
/// `mesh` is null or live, not used again after this.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_free(mesh: *mut FyMesh) {
    if !mesh.is_null() {
        // A panic here leaks the mesh rather than abort (fy_world_free).
        let _ = guard(|| {
            // SAFETY: made by Box::into_raw in new_mesh; the caller gives it up.
            drop(unsafe { Box::from_raw(mesh) });
            Ok(())
        });
    }
}

/// Writes how many vertices and indices the mesh has. There can be more
/// vertices than were given, when generated tangents split some.
///
/// # Safety
///
/// `mesh` is null or live; the outs are null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_counts(
    mesh: *const FyMesh,
    out_vertices: *mut usize,
    out_indices: *mut usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let mesh = unsafe { handle(mesh, "mesh") }?;
        // SAFETY: per this function's contract.
        unsafe {
            write(out_vertices, mesh.0.vertices().len(), "out_vertices")?;
            write(out_indices, mesh.0.indices().len(), "out_indices")
        }
    })
}

/// Copies the mesh's vertex positions, three floats each, to `positions`
/// and its indices to `indices`: `vertex_count` and `index_count` must be
/// `fy_mesh_counts`' numbers. A null array (with its count 0) is skipped.
///
/// # Safety
///
/// `mesh` is null or live; `positions` is null or writable for
/// `vertex_count * 3` floats, `indices` for `index_count` u32s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_read(
    mesh: *const FyMesh,
    positions: *mut f32,
    vertex_count: usize,
    indices: *mut u32,
    index_count: usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let mesh = unsafe { handle(mesh, "mesh") }?;
        let (vertices, mesh_indices) = (mesh.0.vertices(), mesh.0.indices());
        if !positions.is_null() {
            if vertex_count != vertices.len() {
                return Err(Error::new(
                    FyStatus::WrongSize,
                    format!("the mesh has {} vertices, not {vertex_count}", vertices.len()),
                ));
            }
            // SAFETY: not null, and writable for vertex_count * 3 floats per the contract.
            let out = unsafe { std::slice::from_raw_parts_mut(positions, vertex_count * 3) };
            for (out, vertex) in out.chunks_exact_mut(3).zip(vertices) {
                out.copy_from_slice(&vertex.position);
            }
        }
        if !indices.is_null() {
            if index_count != mesh_indices.len() {
                return Err(Error::new(
                    FyStatus::WrongSize,
                    format!("the mesh has {} indices, not {index_count}", mesh_indices.len()),
                ));
            }
            // SAFETY: not null, and writable for index_count u32s per the contract.
            unsafe { std::slice::from_raw_parts_mut(indices, index_count) }.copy_from_slice(mesh_indices);
        }
        Ok(())
    })
}

/// Gives the world a share of `mesh` (nothing is copied) and writes the
/// world's id for it to `out`. Adding the same handle twice gives two ids.
///
/// # Safety
///
/// `world` and `mesh` are null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_add(world: *const FyWorld, mesh: *const FyMesh, out: *mut u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, mesh) = unsafe { (handle(world, "world")?, handle(mesh, "mesh")?) };
        let id = world.lock()?.add_mesh(Arc::clone(&mesh.0));
        // SAFETY: per this function's contract.
        unsafe { write(out, id.raw(), "out") }
    })
}

/// Removes a mesh. Refused while an object uses it.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_mesh_remove(world: *const FyWorld, id: u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        Ok(world.lock()?.remove_mesh(mesh(id)?)?)
    })
}

/// Gives the submeshes of `object`'s mesh after the first a material each
/// (its `MeshRenderer`'s material draws the first), as Unity's
/// `Renderer.materials`: `count` ids at `materials`, each a material the
/// world has. A submesh without one isn't drawn. None (null with 0) takes
/// them away.
///
/// # Safety
///
/// `world` is null or live; `materials` is valid for `count` u64s (or null
/// with `count` 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_submesh_materials_set(
    world: *const FyWorld,
    object: u64,
    materials: *const u64,
    count: usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        // SAFETY: per this function's contract.
        let ids = unsafe { items::<u64, 1>(materials, count, "materials") }?.unwrap_or_default();
        let ids = ids.iter().map(|&[id]| material(id)).collect::<Result<Vec<_>, _>>()?;
        Ok(world.lock()?.set_submesh_materials(self::object(object)?, &ids)?)
    })
}

/// Adds a material and writes its id to `out`.
///
/// # Safety
///
/// `world` is null or live; `value` is null or readable; `out` is null or
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_material_add(world: *const FyWorld, value: *const FyMaterial, out: *mut u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, value) = unsafe { (handle(world, "world")?, read(value, "value")?) };
        let id = world.lock()?.add_material(value.to_material()?)?;
        // SAFETY: per this function's contract.
        unsafe { write(out, id.raw(), "out") }
    })
}

/// Changes a material. Every object using it changes with it.
///
/// # Safety
///
/// `world` is null or live; `value` is null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_material_set(world: *const FyWorld, id: u64, value: *const FyMaterial) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let (world, value) = unsafe { (handle(world, "world")?, read(value, "value")?) };
        Ok(world.lock()?.set_material(material(id)?, value.to_material()?)?)
    })
}

/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_material_get(world: *const FyWorld, id: u64, out: *mut FyMaterial) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let value = world.lock()?.material(material(id)?)?;
        // SAFETY: per this function's contract.
        unsafe { write(out, value.into(), "out") }
    })
}

/// Removes a material. Refused while an object uses it.
///
/// # Safety
///
/// `world` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_material_remove(world: *const FyWorld, id: u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        Ok(world.lock()?.remove_material(material(id)?)?)
    })
}

/// Writes `len` bytes into a material's values from `offset`, where its
/// shader's `Values` puts the one being set (`fy_shader_describe`). Past
/// their end is `FY_STATUS_WRONG_SIZE`.
///
/// # Safety
///
/// `world` is null or live; `bytes` is valid for `len` reads (it may be
/// null when `len` is 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_material_values_set(
    world: *const FyWorld,
    id: u64,
    offset: usize,
    bytes: *const u8,
    len: usize,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let world = unsafe { handle(world, "world") }?;
        let bytes = if len == 0 {
            &[][..]
        } else {
            // SAFETY: per this function's contract.
            unsafe { std::slice::from_raw_parts(handle(bytes, "bytes")?, len) }
        };
        Ok(world.lock()?.set_material_values(material(id)?, offset, bytes)?)
    })
}
