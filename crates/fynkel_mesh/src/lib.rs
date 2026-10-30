//! Triangle meshes on the CPU (stage 2 plan §3.2): the vertex layout, the
//! built-in shapes, and [`MeshData::new`], which checks a mesh made from
//! code or a file and fills in what it leaves out. [`MeshData::change`]
//! changes one while the game runs (stage 13).
//!
//! Conventions, glTF's:
//! - Front faces wind counter-clockwise seen from outside, and normals
//!   point outwards.
//! - Texture coordinates have (0, 0) at the image's top-left corner, with
//!   v growing downwards.
//! - A tangent points where u grows. Its `w` is +1 or -1 so that
//!   `cross(normal, tangent) * w` is the bitangent, which points up the
//!   image (where v shrinks), as normal maps expect.
//!
//! Nothing here knows about the GPU or the world.

use std::collections::HashMap;
use std::fmt;
use std::ops::Range;

use glam::{Vec3, Vec4};

mod ears;
mod made;
mod outline;
mod shapes;
mod sweep;
mod text;

pub use outline::fill;
pub use sweep::{PathPoint, Sweep};
pub use text::{Font, TextAlign, TextShape, TextStyle};

/// One vertex, as the GPU reads it: 52 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    /// Unit length.
    pub normal: [f32; 3],
    /// xyz: unit length, at right angles to the normal. w: +1 or -1, the
    /// bitangent's side (see the module docs).
    pub tangent: [f32; 4],
    pub uv: [f32; 2],
    /// sRGB and alpha, 0 to 255 (Unity's `Color32`): multiplied into the
    /// material's colour. White for a mesh that gives none.
    pub color: [u8; 4],
}

const _: () = assert!(std::mem::size_of::<Vertex>() == 52);

/// The colour a vertex has when its mesh gives none.
pub const WHITE: [u8; 4] = [255; 4];

/// What bends a vertex with a skeleton, as the GPU reads it: 16 bytes,
/// beside its [`Vertex`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SkinVertex {
    /// Up to four joints, as indices into the skin's list of joints.
    pub joints: [u16; 4],
    /// How much each moves the vertex, out of 65535: they add up to it.
    pub weights: [u16; 4],
}

const _: () = assert!(std::mem::size_of::<SkinVertex>() == 16);

/// A checked triangle list: every index points at a vertex, and every
/// vertex has a unit normal and tangent.
#[derive(Debug, Default, PartialEq)]
pub struct MeshData {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    /// One per vertex for a skinned mesh, none for another.
    skin: Vec<SkinVertex>,
    /// Each submesh's range of `indices`, one after another from 0 to the
    /// end; one for a mesh that doesn't split.
    submeshes: Vec<Range<u32>>,
    /// The smallest and largest corner of the box around every vertex.
    bounds: (Vec3, Vec3),
    /// Changed often: the GPU's copy has room to grow and is written in
    /// place ([`MeshData::mark_dynamic`]).
    dynamic: bool,
    /// Which setting of the indices it has, so a copy into a mesh that has
    /// the same ones already leaves them.
    indices_made: Made,
}

/// A number for each time a mesh's indices are set, never the same twice;
/// two meshes are equal whatever theirs are.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Made(u64);

impl Made {
    pub(crate) fn next() -> Self {
        static LAST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        Made(LAST.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1)
    }
}

impl PartialEq for Made {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Clone for MeshData {
    fn clone(&self) -> Self {
        MeshData {
            vertices: self.vertices.clone(),
            indices: self.indices.clone(),
            skin: self.skin.clone(),
            submeshes: self.submeshes.clone(),
            bounds: self.bounds,
            dynamic: self.dynamic,
            indices_made: self.indices_made,
        }
    }

    /// Into the room it already has, so a copy kept for the next change
    /// allocates nothing.
    fn clone_from(&mut self, source: &Self) {
        self.vertices.clone_from(&source.vertices);
        if self.indices_made.0 == 0 || self.indices_made.0 != source.indices_made.0 {
            self.indices.clone_from(&source.indices);
            self.indices_made = source.indices_made;
        }
        self.skin.clone_from(&source.skin);
        self.submeshes.clone_from(&source.submeshes);
        self.bounds = source.bounds;
        self.dynamic = source.dynamic;
    }
}

/// What [`MeshData::new`] takes. Only positions and indices are required.
#[derive(Clone, Copy, Debug, Default)]
pub struct MeshParts<'a> {
    pub positions: &'a [[f32; 3]],
    /// When left out, each vertex gets the average of the faces around it,
    /// weighted by their angle at the vertex. So a vertex shared by
    /// triangles is smooth, and a corner that should be sharp needs a
    /// vertex per face.
    pub normals: Option<&'a [[f32; 3]]>,
    /// When left out, they're generated with MikkTSpace from the UVs (as
    /// glTF requires). A vertex whose triangles need different tangents,
    /// such as one on the line where a mirrored texture meets itself, is
    /// split in two. Without UVs, any direction at right angles to the
    /// normal is used.
    pub tangents: Option<&'a [[f32; 4]]>,
    /// (0, 0) when left out.
    pub uvs: Option<&'a [[f32; 2]]>,
    /// White when left out.
    pub colors: Option<&'a [[u8; 4]]>,
    /// Three per triangle.
    pub indices: &'a [u32],
    /// Where each submesh after the first starts in `indices`, in order:
    /// empty for one submesh.
    pub submeshes: &'a [u32],
    /// For a mesh a skeleton bends: each vertex's four joints, and how
    /// much each moves it. Weights are 0 or more; they're scaled to add up
    /// to 1, and a vertex with none follows its first joint alone.
    #[allow(clippy::type_complexity)]
    pub skin: Option<(&'a [[u16; 4]], &'a [[f32; 4]])>,
}

/// Why [`MeshData::new`] refused a mesh.
#[derive(Clone, Debug, PartialEq)]
pub enum MeshError {
    NoTriangles,
    /// The index count isn't a multiple of 3.
    NotTriangles(usize),
    TooManyVertices(usize),
    /// `normals`, `tangents` or `uvs` doesn't have one item per position.
    WrongLength {
        what: &'static str,
        len: usize,
        positions: usize,
    },
    IndexOutOfRange {
        at: usize,
        index: u32,
        vertices: usize,
    },
    NotFinite {
        what: &'static str,
        at: usize,
    },
    /// A normal or a tangent's xyz is too short to have a direction.
    ZeroLength {
        what: &'static str,
        at: usize,
    },
    /// A tangent's w is neither 1 nor -1.
    TangentSign {
        at: usize,
        w: f32,
    },
    /// A skin weight is below 0.
    NegativeWeight {
        at: usize,
    },
    /// A submesh starts before the one before it, past the end, or in the
    /// middle of a triangle.
    BadSubmesh {
        at: usize,
        start: u32,
    },
    /// A skinned mesh can't be changed (stage 13 §5).
    Skinned,
    /// An outline or profile to make a mesh from that can't be one.
    Outline(String),
}

impl fmt::Display for MeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MeshError::NoTriangles => write!(f, "a mesh needs at least one triangle"),
            MeshError::NotTriangles(n) => {
                write!(f, "indices come in threes, one per triangle corner, but there are {n}")
            }
            MeshError::TooManyVertices(n) => write!(f, "{n} vertices is more than 32-bit indices can reach"),
            MeshError::WrongLength { what, len, positions } => {
                write!(f, "{what} has {len} items, but there are {positions} positions")
            }
            MeshError::IndexOutOfRange { at, index, vertices } => {
                write!(f, "index {at} is {index}, but there are only {vertices} vertices")
            }
            MeshError::NotFinite { what, at } => write!(f, "{what} {at} isn't finite"),
            MeshError::ZeroLength { what, at } => write!(f, "{what} {at} has no direction (zero length)"),
            MeshError::TangentSign { at, w } => write!(f, "tangent {at} has w = {w}; it must be 1 or -1"),
            MeshError::NegativeWeight { at } => write!(f, "vertex {at} has a skin weight below 0"),
            MeshError::BadSubmesh { at, start } => write!(
                f,
                "submesh {} starts at index {start}: each must start on a triangle, in order, within the indices",
                at + 1
            ),
            MeshError::Skinned => write!(f, "a skinned mesh can't be changed"),
            MeshError::Outline(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for MeshError {}

/// The normal a vertex gets when no face around it has an area.
const FALLBACK_NORMAL: Vec3 = Vec3::Y;

impl MeshData {
    /// Checks `parts` and fills in the normals, tangents and UVs it leaves
    /// out (see [`MeshParts`]). The result may have more vertices than
    /// `parts`, when generated tangents split some.
    pub fn new(parts: MeshParts<'_>) -> Result<Self, MeshError> {
        let MeshParts { positions, normals, tangents, uvs, colors, indices, submeshes, skin } = parts;
        let n = positions.len();
        if indices.is_empty() {
            return Err(MeshError::NoTriangles);
        }
        if !indices.len().is_multiple_of(3) {
            return Err(MeshError::NotTriangles(indices.len()));
        }
        if u32::try_from(n).is_err() {
            return Err(MeshError::TooManyVertices(n));
        }
        for (what, len) in [
            ("normals", normals.map(<[_]>::len)),
            ("tangents", tangents.map(<[_]>::len)),
            ("uvs", uvs.map(<[_]>::len)),
            ("colors", colors.map(<[_]>::len)),
            ("joints", skin.map(|(j, _)| j.len())),
            ("weights", skin.map(|(_, w)| w.len())),
        ] {
            if let Some(len) = len.filter(|&len| len != n) {
                return Err(MeshError::WrongLength { what, len, positions: n });
            }
        }
        in_range(indices, n)?;
        let submeshes = submesh_ranges(indices.len(), submeshes)?;
        finite("position", positions.iter().flatten().copied(), 3)?;
        finite("uv", uvs.unwrap_or_default().iter().flatten().copied(), 2)?;
        let skin = match skin {
            Some((joints, weights)) => {
                finite("weight", weights.iter().flatten().copied(), 4)?;
                if let Some(at) = weights.iter().flatten().position(|&w| w < 0.0) {
                    return Err(MeshError::NegativeWeight { at: at / 4 });
                }
                joints.iter().zip(weights).map(|(&joints, &weights)| skin_vertex(joints, weights)).collect()
            }
            None => Vec::new(),
        };

        let normals: Vec<Vec3> = match normals {
            Some(given) => {
                finite("normal", given.iter().flatten().copied(), 3)?;
                given
                    .iter()
                    .enumerate()
                    .map(|(at, &v)| Vec3::from(v).try_normalize().ok_or(MeshError::ZeroLength { what: "normal", at }))
                    .collect::<Result<_, _>>()?
            }
            None => angle_weighted_normals(positions, indices),
        };
        let uv = |i: usize| uvs.map_or([0.0; 2], |uvs| uvs[i]);
        let color = |i: usize| colors.map_or(WHITE, |colors| colors[i]);
        let mut mesh = Self {
            vertices: (0..n)
                .map(|i| Vertex {
                    position: positions[i],
                    normal: normals[i].into(),
                    tangent: [0.0; 4],
                    uv: uv(i),
                    color: color(i),
                })
                .collect(),
            indices: indices.to_vec(),
            skin,
            submeshes,
            bounds: bounds(positions.iter().copied()),
            dynamic: false,
            indices_made: Made::next(),
        };

        match (tangents, uvs) {
            (Some(given), _) => {
                finite("tangent", given.iter().flatten().copied(), 4)?;
                for (at, (vertex, &t)) in mesh.vertices.iter_mut().zip(given).enumerate() {
                    let xyz = Vec3::from_slice(&t[..3])
                        .try_normalize()
                        .ok_or(MeshError::ZeroLength { what: "tangent", at })?;
                    if t[3].abs() != 1.0 {
                        return Err(MeshError::TangentSign { at, w: t[3] });
                    }
                    vertex.tangent = xyz.extend(t[3]).into();
                }
            }
            (None, Some(_)) => mesh.generate_tangents(),
            (None, None) => {
                for vertex in &mut mesh.vertices {
                    vertex.tangent = any_tangent(Vec3::from(vertex.normal)).into();
                }
            }
        }
        Ok(mesh)
    }

    pub fn vertices(&self) -> &[Vertex] {
        &self.vertices
    }

    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// Each submesh's range of [`MeshData::indices`]: one for a mesh that
    /// doesn't split.
    pub fn submeshes(&self) -> &[Range<u32>] {
        &self.submeshes
    }

    /// Each vertex's joints and weights, for a mesh a skeleton bends.
    pub fn skin(&self) -> Option<&[SkinVertex]> {
        (!self.skin.is_empty()).then_some(&self.skin[..])
    }

    /// The smallest and largest corner of the axis-aligned box around
    /// every vertex.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        self.bounds
    }

    /// MikkTSpace gives each triangle corner a tangent; corners that share
    /// a vertex but got different tangents get a vertex each.
    fn generate_tangents(&mut self) {
        let mut corners = Corners { mesh: self, tangents: vec![None; self.indices.len()] };
        bevy_mikktspace::generate_tangents(&mut corners).expect("generating tangents can't fail");
        let tangents = corners.tangents;

        // The tangent each vertex has so far, and the copies made for others.
        let mut assigned: Vec<Option<[f32; 4]>> = vec![None; self.vertices.len()];
        let mut copies: HashMap<(u32, [u32; 4]), u32> = HashMap::new();
        for (corner, tangent) in tangents.into_iter().enumerate() {
            let index = self.indices[corner];
            let vertex = self.vertices[index as usize];
            // MikkTSpace leaves out corners of degenerate triangles with no
            // neighbour to borrow from.
            let tangent = tangent.unwrap_or_else(|| any_tangent(Vec3::from(vertex.normal)).into());
            match assigned[index as usize] {
                None => {
                    assigned[index as usize] = Some(tangent);
                    self.vertices[index as usize].tangent = tangent;
                }
                Some(t) if t == tangent => {}
                Some(_) => {
                    let next = self.vertices.len() as u32;
                    let copy = *copies.entry((index, tangent.map(f32::to_bits))).or_insert(next);
                    if copy == next {
                        self.vertices.push(Vertex { tangent, ..vertex });
                        if !self.skin.is_empty() {
                            self.skin.push(self.skin[index as usize]);
                        }
                    }
                    self.indices[corner] = copy;
                }
            }
        }
        // Vertices no triangle uses still need a valid tangent.
        for (vertex, assigned) in self.vertices.iter_mut().zip(&assigned) {
            if assigned.is_none() {
                vertex.tangent = any_tangent(Vec3::from(vertex.normal)).into();
            }
        }
    }
}

/// What [`MeshData::change`] replaces: each channel given replaces that
/// channel, and the rest stay (stage 13 §3.1).
#[derive(Clone, Copy, Debug, Default)]
pub struct MeshChange<'a> {
    /// A different count from the mesh's resets every channel not given
    /// with it: normals and tangents are worked out again, UVs go to
    /// (0, 0) and colours to white.
    pub positions: Option<&'a [[f32; 3]]>,
    pub normals: Option<&'a [[f32; 3]]>,
    pub tangents: Option<&'a [[f32; 4]]>,
    pub uvs: Option<&'a [[f32; 2]]>,
    pub colors: Option<&'a [[u8; 4]]>,
    /// Every submesh's triangles, one after another, and where each after
    /// the first starts (as [`MeshParts::submeshes`]).
    pub indices: Option<(&'a [u32], &'a [u32])>,
    /// Each vertex's normal again from the faces around it, weighted by
    /// their area (Unity's `RecalculateNormals`): quicker than
    /// [`MeshData::new`]'s weighting by angle, for a mesh that changes each
    /// frame.
    pub recalculate_normals: bool,
    /// Each vertex's tangent again from its UVs, without splitting any
    /// vertex (Unity's `RecalculateTangents`): quicker than MikkTSpace.
    pub recalculate_tangents: bool,
}

impl MeshData {
    /// A mesh with nothing in it yet, to [`change`](MeshData::change): it
    /// draws nothing.
    pub fn empty() -> Self {
        Self { submeshes: std::iter::once(0..0).collect(), ..Self::default() }
    }

    /// Replaces what `change` gives, in place. The mesh is checked as
    /// [`MeshData::new`] checks one, but may have no triangles; a refused
    /// change leaves it as it was, with a message saying why. A skinned
    /// mesh is refused.
    pub fn change(&mut self, change: MeshChange<'_>) -> Result<(), MeshError> {
        if !self.skin.is_empty() {
            return Err(MeshError::Skinned);
        }
        let MeshChange { positions, normals, tangents, uvs, colors, indices, .. } = change;
        let n = positions.map_or(self.vertices.len(), <[_]>::len);
        if u32::try_from(n).is_err() {
            return Err(MeshError::TooManyVertices(n));
        }
        for (what, len) in [
            ("normals", normals.map(<[_]>::len)),
            ("tangents", tangents.map(<[_]>::len)),
            ("uvs", uvs.map(<[_]>::len)),
            ("colors", colors.map(<[_]>::len)),
        ] {
            if let Some(len) = len.filter(|&len| len != n) {
                return Err(MeshError::WrongLength { what, len, positions: n });
            }
        }
        if let Some((indices, _)) = indices
            && !indices.len().is_multiple_of(3)
        {
            return Err(MeshError::NotTriangles(indices.len()));
        }
        // The triangles it has already point at its vertices: they're
        // looked at again only when new, or the count changes.
        if indices.is_some() || n != self.vertices.len() {
            in_range(indices.map_or(&self.indices[..], |(i, _)| i), n)?;
        }
        let submeshes = match indices {
            Some((indices, starts)) => Some(submesh_ranges(indices.len(), starts)?),
            None => None,
        };
        let position_bounds = positions.map(checked_bounds).transpose()?;
        finite("normal", normals.unwrap_or_default().iter().flatten().copied(), 3)?;
        finite("tangent", tangents.unwrap_or_default().iter().flatten().copied(), 4)?;
        finite("uv", uvs.unwrap_or_default().iter().flatten().copied(), 2)?;
        for (at, &v) in normals.unwrap_or_default().iter().enumerate() {
            if Vec3::from(v).length_squared() < 1e-12 {
                return Err(MeshError::ZeroLength { what: "normal", at });
            }
        }
        for (at, &t) in tangents.unwrap_or_default().iter().enumerate() {
            if Vec3::from_slice(&t[..3]).length_squared() < 1e-12 {
                return Err(MeshError::ZeroLength { what: "tangent", at });
            }
            if t[3].abs() != 1.0 {
                return Err(MeshError::TangentSign { at, w: t[3] });
            }
        }

        // Checked: nothing below fails.
        let resized = n != self.vertices.len();
        let blank = Vertex {
            position: [0.0; 3],
            normal: FALLBACK_NORMAL.into(),
            tangent: [1.0, 0.0, 0.0, 1.0],
            uv: [0.0; 2],
            color: WHITE,
        };
        if resized {
            self.vertices.clear();
            self.vertices.resize(n, blank);
        }
        if let Some(positions) = positions {
            for (v, &p) in self.vertices.iter_mut().zip(positions) {
                v.position = p;
            }
            self.bounds = position_bounds.unwrap_or_default();
        }
        if let Some(normals) = normals {
            for (v, &m) in self.vertices.iter_mut().zip(normals) {
                v.normal = Vec3::from(m).normalize().into();
            }
        }
        if let Some(tangents) = tangents {
            for (v, &t) in self.vertices.iter_mut().zip(tangents) {
                v.tangent = Vec3::from_slice(&t[..3]).normalize().extend(t[3]).into();
            }
        }
        if let Some(uvs) = uvs {
            for (v, &uv) in self.vertices.iter_mut().zip(uvs) {
                v.uv = uv;
            }
        }
        if let Some(colors) = colors {
            for (v, &c) in self.vertices.iter_mut().zip(colors) {
                v.color = c;
            }
        }
        if let (Some((indices, _)), Some(submeshes)) = (indices, submeshes) {
            self.indices.clear();
            self.indices.extend_from_slice(indices);
            self.indices_made = Made::next();
            self.submeshes = submeshes;
        }
        if change.recalculate_normals || (resized && normals.is_none()) {
            self.recalculate_normals();
        }
        if change.recalculate_tangents || (resized && tangents.is_none()) {
            self.recalculate_tangents();
        }
        Ok(())
    }

    /// Says the mesh changes often, as Unity's `MarkDynamic`: each view
    /// keeps its vertices and triangles on the GPU with room to grow, half
    /// again, and writes a change into them where they are. Its outline
    /// is pushed out along its own normals, not along ones averaged where
    /// corners split.
    pub fn mark_dynamic(&mut self) {
        self.dynamic = true;
    }

    pub fn is_dynamic(&self) -> bool {
        self.dynamic
    }

    /// Each vertex's normal from the faces around it, weighted by their
    /// area. A vertex no face with an area touches faces +Y.
    pub fn recalculate_normals(&mut self) {
        // Summed in the normals themselves, so nothing is allocated.
        for v in &mut self.vertices {
            v.normal = [0.0; 3];
        }
        let vertices = &mut self.vertices;
        for tri in self.indices.chunks_exact(3) {
            let p = [0, 1, 2].map(|k| Vec3::from(vertices[tri[k] as usize].position));
            // Twice the triangle's area long, so bigger faces count more.
            let face = (p[1] - p[0]).cross(p[2] - p[0]);
            for &i in tri {
                let n = &mut vertices[i as usize].normal;
                *n = (Vec3::from(*n) + face).into();
            }
        }
        for v in vertices {
            v.normal = Vec3::from(v.normal).try_normalize().unwrap_or(FALLBACK_NORMAL).into();
        }
    }

    /// Each vertex's tangent from its UVs (Lengyel's method): along u, at
    /// right angles to the normal, w for the side the image's up is on.
    /// No vertex is split, so a mirrored seam averages; MikkTSpace
    /// ([`MeshData::new`]) splits it.
    pub fn recalculate_tangents(&mut self) {
        // Each vertex's sums along u and along v, kept on the thread so a
        // mesh changed each frame allocates nothing.
        thread_local! {
            static SUMS: std::cell::RefCell<Vec<(Vec3, Vec3)>> = std::cell::RefCell::default();
        }
        SUMS.with_borrow_mut(|sums| {
            sums.clear();
            sums.resize(self.vertices.len(), (Vec3::ZERO, Vec3::ZERO));
            for tri in self.indices.chunks_exact(3) {
                let v = [0, 1, 2].map(|k| self.vertices[tri[k] as usize]);
                let (e1, e2) = (
                    Vec3::from(v[1].position) - Vec3::from(v[0].position),
                    Vec3::from(v[2].position) - Vec3::from(v[0].position),
                );
                let (du1, dv1) = (v[1].uv[0] - v[0].uv[0], v[1].uv[1] - v[0].uv[1]);
                let (du2, dv2) = (v[2].uv[0] - v[0].uv[0], v[2].uv[1] - v[0].uv[1]);
                let det = du1 * dv2 - du2 * dv1;
                if det.abs() < 1e-12 {
                    continue;
                }
                let u = (e1 * dv2 - e2 * dv1) / det;
                let w = (e2 * du1 - e1 * du2) / det;
                for &i in tri {
                    sums[i as usize].0 += u;
                    sums[i as usize].1 += w;
                }
            }
            for (vertex, &(u, v)) in self.vertices.iter_mut().zip(sums.iter()) {
                let n = Vec3::from(vertex.normal);
                let Some(t) = (u - n * n.dot(u)).try_normalize() else {
                    vertex.tangent = any_tangent(n).into();
                    continue;
                };
                // v grows down the image; the bitangent points up it.
                let w = if n.cross(t).dot(v) > 0.0 { -1.0 } else { 1.0 };
                vertex.tangent = t.extend(w).into();
            }
        });
    }
}

/// Fails on the first index past the last of `n` vertices.
fn in_range(indices: &[u32], n: usize) -> Result<(), MeshError> {
    match indices.iter().enumerate().find(|&(_, &i)| i as usize >= n) {
        Some((at, &index)) => Err(MeshError::IndexOutOfRange { at, index, vertices: n }),
        None => Ok(()),
    }
}

/// The submeshes' ranges of `count` indices, from where each after the
/// first starts.
pub(crate) fn submesh_ranges(count: usize, starts: &[u32]) -> Result<Vec<Range<u32>>, MeshError> {
    let count = count as u32;
    let mut ranges = Vec::with_capacity(starts.len() + 1);
    let mut from = 0;
    for (at, &start) in starts.iter().enumerate() {
        if start < from || start > count || !start.is_multiple_of(3) {
            return Err(MeshError::BadSubmesh { at, start });
        }
        ranges.push(from..start);
        from = start;
    }
    ranges.push(from..count);
    Ok(ranges)
}

/// The smallest and largest corner of the box around `positions`; the
/// origin for none.
/// The box around `positions`, refusing one that isn't finite: one pass
/// for both.
fn checked_bounds(positions: &[[f32; 3]]) -> Result<(Vec3, Vec3), MeshError> {
    let (mut lo, mut hi) = (Vec3::INFINITY, Vec3::NEG_INFINITY);
    for (at, &p) in positions.iter().enumerate() {
        let p = Vec3::from(p);
        if !p.is_finite() {
            return Err(MeshError::NotFinite { what: "position", at });
        }
        (lo, hi) = (lo.min(p), hi.max(p));
    }
    Ok(if lo.x > hi.x { (Vec3::ZERO, Vec3::ZERO) } else { (lo, hi) })
}

pub(crate) fn bounds(positions: impl Iterator<Item = [f32; 3]>) -> (Vec3, Vec3) {
    let (lo, hi) = positions
        .fold((Vec3::INFINITY, Vec3::NEG_INFINITY), |(lo, hi), p| (lo.min(Vec3::from(p)), hi.max(Vec3::from(p))));
    if lo.x > hi.x { (Vec3::ZERO, Vec3::ZERO) } else { (lo, hi) }
}

/// A vertex's joints and weights, the weights scaled to add up to 65535
/// exactly (the largest takes what rounding leaves). With no weight at
/// all, its first joint moves it alone.
fn skin_vertex(joints: [u16; 4], weights: [f32; 4]) -> SkinVertex {
    let sum: f32 = weights.iter().sum();
    if sum <= 0.0 {
        return SkinVertex { joints, weights: [u16::MAX, 0, 0, 0] };
    }
    let mut quantized = weights.map(|w| (w / sum * 65535.0).round() as i32);
    let largest = (0..4).max_by(|&a, &b| weights[a].total_cmp(&weights[b])).expect("four");
    quantized[largest] += 65535 - quantized.iter().sum::<i32>();
    // A joint that doesn't move the vertex is left at 0, so the GPU reads
    // no further than the joints used.
    let joints = std::array::from_fn(|i| if quantized[i] == 0 { 0 } else { joints[i] });
    SkinVertex { joints, weights: quantized.map(|w| w.clamp(0, 65535) as u16) }
}

/// Fails on the first value that isn't finite, naming the item it's in.
fn finite(what: &'static str, values: impl Iterator<Item = f32>, per_item: usize) -> Result<(), MeshError> {
    match values.enumerate().find(|(_, v)| !v.is_finite()) {
        Some((i, _)) => Err(MeshError::NotFinite { what, at: i / per_item }),
        None => Ok(()),
    }
}

/// Each vertex's normal: the faces around it, each weighted by its angle
/// at the vertex. Unlike weighting by area, this doesn't depend on how a
/// flat polygon was cut into triangles.
fn angle_weighted_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<Vec3> {
    let mut sums = vec![Vec3::ZERO; positions.len()];
    for tri in indices.chunks_exact(3) {
        let p = [0, 1, 2].map(|k| Vec3::from(positions[tri[k] as usize]));
        let Some(face) = (p[1] - p[0]).cross(p[2] - p[0]).try_normalize() else { continue };
        for k in 0..3 {
            let (a, b) = (p[(k + 1) % 3] - p[k], p[(k + 2) % 3] - p[k]);
            sums[tri[k] as usize] += face * a.angle_between(b);
        }
    }
    sums.into_iter().map(|s| s.try_normalize().unwrap_or(FALLBACK_NORMAL)).collect()
}

/// A unit tangent at right angles to `normal`, for a vertex with nothing
/// to say which way u goes.
fn any_tangent(normal: Vec3) -> Vec4 {
    normal.any_orthonormal_vector().extend(1.0)
}

/// The mesh as MikkTSpace reads it: one face per triangle.
struct Corners<'a> {
    mesh: &'a MeshData,
    tangents: Vec<Option<[f32; 4]>>,
}

impl Corners<'_> {
    fn vertex(&self, face: usize, vert: usize) -> &Vertex {
        &self.mesh.vertices[self.mesh.indices[face * 3 + vert] as usize]
    }
}

impl bevy_mikktspace::Geometry for Corners<'_> {
    fn num_faces(&self) -> usize {
        self.mesh.indices.len() / 3
    }

    fn num_vertices_of_face(&self, _face: usize) -> usize {
        3
    }

    fn position(&self, face: usize, vert: usize) -> [f32; 3] {
        self.vertex(face, vert).position
    }

    fn normal(&self, face: usize, vert: usize) -> [f32; 3] {
        self.vertex(face, vert).normal
    }

    // MikkTSpace's bitangent follows growing v. Flipping v makes it point
    // up the image instead, as the w convention above needs.
    fn tex_coord(&self, face: usize, vert: usize) -> [f32; 2] {
        let [u, v] = self.vertex(face, vert).uv;
        [u, 1.0 - v]
    }

    fn set_tangent(&mut self, tangent_space: Option<bevy_mikktspace::TangentSpace>, face: usize, vert: usize) {
        self.tangents[face * 3 + vert] = tangent_space.map(|t| t.tangent_encoded());
    }
}
