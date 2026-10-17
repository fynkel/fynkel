//! Triangle meshes on the CPU (stage 2 plan §3.2): the vertex layout, the
//! built-in shapes, and [`MeshData::new`], which checks a mesh made from
//! code or a file and fills in what it leaves out.
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

use glam::{Vec3, Vec4};

mod shapes;

/// One vertex, as the GPU reads it: 48 bytes.
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
}

const _: () = assert!(std::mem::size_of::<Vertex>() == 48);

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
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshData {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    /// One per vertex for a skinned mesh, none for another.
    skin: Vec<SkinVertex>,
    /// The smallest and largest corner of the box around every vertex.
    bounds: (Vec3, Vec3),
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
    /// Three per triangle.
    pub indices: &'a [u32],
    /// For a mesh a skeleton bends: each vertex's four joints, and how
    /// much each moves it. Weights are 0 or more; they're scaled to add up
    /// to 1, and a vertex with none follows its first joint alone.
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
        let MeshParts { positions, normals, tangents, uvs, indices, skin } = parts;
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
            ("joints", skin.map(|(j, _)| j.len())),
            ("weights", skin.map(|(_, w)| w.len())),
        ] {
            if let Some(len) = len.filter(|&len| len != n) {
                return Err(MeshError::WrongLength { what, len, positions: n });
            }
        }
        if let Some((at, &index)) = indices.iter().enumerate().find(|&(_, &i)| i as usize >= n) {
            return Err(MeshError::IndexOutOfRange { at, index, vertices: n });
        }
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
        let mut mesh = Self {
            vertices: (0..n)
                .map(|i| Vertex { position: positions[i], normal: normals[i].into(), tangent: [0.0; 4], uv: uv(i) })
                .collect(),
            indices: indices.to_vec(),
            skin,
            bounds: positions.iter().fold((Vec3::INFINITY, Vec3::NEG_INFINITY), |(lo, hi), &p| {
                (lo.min(Vec3::from(p)), hi.max(Vec3::from(p)))
            }),
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
