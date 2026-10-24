//! Shapes with settings (stage 13 §3.2), and meshes joined into one. Each
//! is wound and textured as the built-in shapes are; where a shape has flat
//! faces (a box, a grid) its UVs are in metres, times `uv_per_metre`, so a
//! texture tiles at the same size whatever the shape's size.

use std::f32::consts::TAU;

use glam::{Mat3, Mat4, Vec2, Vec3};

use crate::shapes::Shape;
use crate::{MeshData, MeshError, Vertex};

impl MeshData {
    /// A box of `size` centred on the origin, each face cut into a grid of
    /// `segments` along X, Y and Z (at least 1 each), with its own flat
    /// normal. Each face shows its texture as [`MeshData::cube`]'s does,
    /// `uv_per_metre` times a metre.
    pub fn box_shape(size: Vec3, segments: [u32; 3], uv_per_metre: f32) -> Self {
        let segments = segments.map(|n| n.max(1));
        let axis = |v: Vec3| {
            if v.x != 0.0 {
                0
            } else if v.y != 0.0 {
                1
            } else {
                2
            }
        };
        let mut s = Shape::default();
        // The cube's faces: normal, image-right, image-up.
        for (normal, right, up) in [
            (Vec3::X, Vec3::NEG_Z, Vec3::Y),
            (Vec3::NEG_X, Vec3::Z, Vec3::Y),
            (Vec3::Y, Vec3::X, Vec3::NEG_Z),
            (Vec3::NEG_Y, Vec3::X, Vec3::Z),
            (Vec3::Z, Vec3::X, Vec3::Y),
            (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y),
        ] {
            let (r, u) = (axis(right), axis(up));
            let centre = normal * size * 0.5;
            let extent = Vec2::new(size[r], size[u]);
            s.face(centre, normal, right, up, extent, [segments[r], segments[u]], uv_per_metre);
        }
        s.build()
    }

    /// A flat grid of `size` (along X and Z) facing +Y, centred on the
    /// origin, cut into `segments` squares along X and Z: the ground of a
    /// game, or a mesh to bend. Seen from above, image-right is +X and
    /// image-up -Z, as [`MeshData::plane`]'s.
    pub fn grid(size: Vec2, segments: [u32; 2], uv_per_metre: f32) -> Self {
        let mut s = Shape::default();
        s.face(Vec3::ZERO, Vec3::Y, Vec3::X, Vec3::NEG_Z, size, segments.map(|n| n.max(1)), uv_per_metre);
        s.build()
    }

    /// A sphere of `radius`, textured as [`MeshData::sphere`].
    pub fn sphere_sized(radius: f32, segments: u32, rings: u32) -> Self {
        let mut sphere = Self::sphere(segments, rings);
        sphere.scale(radius / 0.5);
        sphere
    }

    /// A cone along Y centred on the origin, `height` high, its point at
    /// the top and its base of `radius`, with `segments` slices around and
    /// a flat base unless `cap` is false. The side's texture wraps around
    /// once from +Z, its top at the point.
    pub fn cone(radius: f32, height: f32, segments: u32, cap: bool) -> Self {
        assert!(segments >= 3, "a cone needs at least 3 segments");
        let half = height * 0.5;
        let mut s = Shape::default();
        // Outwards at right angles to the slope: out by the height, up by
        // the radius.
        let normal = |phi: f32| Vec3::new(phi.sin() * height, radius, phi.cos() * height).normalize();
        let along = |phi: f32| Vec3::new(phi.cos(), 0.0, -phi.sin());
        // The point: one vertex per slice, each with its slice's middle
        // normal, so the shading doesn't pinch.
        for segment in 0..segments {
            let u = (segment as f32 + 0.5) / segments as f32;
            let phi = TAU * u;
            s.push(Vec3::Y * half, normal(phi), along(phi), [u, 0.0]);
        }
        let ring = segments;
        for segment in 0..=segments {
            let u = segment as f32 / segments as f32;
            let phi = TAU * u;
            s.push(Vec3::new(phi.sin() * radius, -half, phi.cos() * radius), normal(phi), along(phi), [u, 1.0]);
        }
        for segment in 0..segments {
            let b = ring + segment;
            s.indices.extend([segment, b, b + 1]);
        }
        if cap {
            s.cap(Vec3::NEG_Y * half, Vec3::NEG_Y, radius, segments);
        }
        s.build()
    }

    /// A ring around Y in the XZ plane: its middle `radius` from the
    /// centre, its tube of `tube` radius; `segments` slices around Y and
    /// `sides` around the tube. The texture wraps once each way: u around
    /// Y from +Z, v around the tube from its outer edge, downwards.
    pub fn torus(radius: f32, tube: f32, segments: u32, sides: u32) -> Self {
        assert!(segments >= 3 && sides >= 3, "a torus needs at least 3 segments and 3 sides");
        let mut s = Shape::default();
        for side in 0..=sides {
            let v = side as f32 / sides as f32;
            // Downwards from the outer edge, as a sphere's rings go down.
            let theta = -TAU * v;
            for segment in 0..=segments {
                let u = segment as f32 / segments as f32;
                let phi = TAU * u;
                let out = Vec3::new(phi.sin(), 0.0, phi.cos());
                let n = out * theta.cos() + Vec3::Y * theta.sin();
                s.push(out * radius + n * tube, n, Vec3::new(phi.cos(), 0.0, -phi.sin()), [u, v]);
            }
        }
        let row = segments + 1;
        for side in 0..sides {
            for segment in 0..segments {
                let a = side * row + segment;
                let b = a + row;
                s.indices.extend([a, b, a + 1, a + 1, b, b + 1]);
            }
        }
        s.build()
    }

    /// A flat round disc of `radius` facing +Y, with `segments` around; a
    /// ring when `inner` is more than 0. Seen from above, the texture is
    /// as [`MeshData::plane`]'s, across the whole disc.
    pub fn disc(radius: f32, inner: f32, segments: u32) -> Self {
        assert!(segments >= 3, "a disc needs at least 3 segments");
        let mut s = Shape::default();
        if inner <= 0.0 {
            s.cap(Vec3::ZERO, Vec3::Y, radius, segments);
            return s.build();
        }
        let uv = |p: Vec3| [0.5 + p.x / (2.0 * radius), 0.5 + p.z / (2.0 * radius)];
        for segment in 0..segments {
            let phi = TAU * segment as f32 / segments as f32;
            let out = Vec3::new(phi.sin(), 0.0, phi.cos());
            for r in [radius, inner] {
                s.push(out * r, Vec3::Y, Vec3::X, uv(out * r));
            }
        }
        for segment in 0..segments {
            let next = (segment + 1) % segments;
            let (outer, inner) = (segment * 2, segment * 2 + 1);
            let (next_outer, next_inner) = (next * 2, next * 2 + 1);
            s.indices.extend([inner, outer, next_outer, inner, next_outer, next_inner]);
        }
        s.build()
    }

    /// `meshes` as one, each placed by its matrix (Unity's
    /// `CombineMeshes`): many things drawn with one draw for each
    /// material instead of one each. Submesh n of each goes into submesh n
    /// of the result, so a town of houses with walls (0) and roofs (1)
    /// is two draws. A matrix that mirrors turns its mesh's triangles
    /// round, so they still face out. A skinned mesh can't be combined.
    pub fn combine(meshes: &[(&MeshData, Mat4)]) -> Result<Self, MeshError> {
        if meshes.iter().any(|(m, _)| !m.skin.is_empty()) {
            return Err(MeshError::Skinned);
        }
        let vertices: usize = meshes.iter().map(|(m, _)| m.vertices.len()).sum();
        if u32::try_from(vertices).is_err() {
            return Err(MeshError::TooManyVertices(vertices));
        }
        let parts = meshes.iter().map(|(m, _)| m.submeshes.len()).max().unwrap_or(0);
        let mut out = MeshData::empty();
        out.vertices.reserve(vertices);
        // Each mesh's vertices start here in the result.
        let mut starts = Vec::with_capacity(meshes.len());
        for &(mesh, matrix) in meshes {
            starts.push(out.vertices.len() as u32);
            let normals = Mat3::from_mat4(matrix).inverse().transpose();
            let turn = Mat3::from_mat4(matrix).determinant() < 0.0;
            for v in &mesh.vertices {
                let normal = (normals * Vec3::from(v.normal)).try_normalize().unwrap_or(Vec3::Y);
                let along = matrix.transform_vector3(Vec3::from_slice(&v.tangent[..3]));
                let along =
                    (along - normal * normal.dot(along)).try_normalize().unwrap_or(normal.any_orthonormal_vector());
                let w = if turn { -v.tangent[3] } else { v.tangent[3] };
                out.vertices.push(Vertex {
                    position: matrix.transform_point3(Vec3::from(v.position)).into(),
                    normal: normal.into(),
                    tangent: along.extend(w).into(),
                    uv: v.uv,
                    color: v.color,
                });
            }
        }
        let mut submesh_starts = Vec::with_capacity(parts);
        for part in 0..parts {
            if part > 0 {
                submesh_starts.push(out.indices.len() as u32);
            }
            for (&(mesh, matrix), &start) in meshes.iter().zip(&starts) {
                let Some(range) = mesh.submeshes.get(part) else { continue };
                let turn = Mat3::from_mat4(matrix).determinant() < 0.0;
                for tri in mesh.indices[range.start as usize..range.end as usize].chunks_exact(3) {
                    let [a, b, c] = [tri[0] + start, tri[1] + start, tri[2] + start];
                    out.indices.extend(if turn { [a, c, b] } else { [a, b, c] });
                }
            }
        }
        if out.indices.is_empty() {
            return Err(MeshError::NoTriangles);
        }
        out.submeshes = crate::submesh_ranges(out.indices.len(), &submesh_starts)?;
        out.bounds = crate::bounds(out.vertices.iter().map(|v| v.position));
        out.indices_made = crate::Made::next();
        Ok(out)
    }

    /// Every position `by` times as far from the origin.
    fn scale(&mut self, by: f32) {
        for v in &mut self.vertices {
            v.position = (Vec3::from(v.position) * by).into();
        }
        self.bounds = (self.bounds.0 * by, self.bounds.1 * by);
    }
}

impl Shape {
    /// A flat face at `centre`, `extent` along image-right and image-up,
    /// cut into `segments` along each, its texture `uv_per_metre` times a
    /// metre from its top-left corner.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn face(
        &mut self,
        centre: Vec3,
        normal: Vec3,
        right: Vec3,
        up: Vec3,
        extent: Vec2,
        segments: [u32; 2],
        uv_per_metre: f32,
    ) {
        let base = self.positions.len() as u32;
        let [across, down] = segments;
        for j in 0..=down {
            let t = j as f32 / down as f32; // 0 at the bottom
            for i in 0..=across {
                let s = i as f32 / across as f32; // 0 at the left
                let p = centre + right * (s - 0.5) * extent.x + up * (t - 0.5) * extent.y;
                let uv = [s * extent.x * uv_per_metre, (1.0 - t) * extent.y * uv_per_metre];
                self.push(p, normal, right, uv);
            }
        }
        let row = across + 1;
        for j in 0..down {
            for i in 0..across {
                let a = base + j * row + i; // bottom left
                let (b, c, d) = (a + 1, a + 1 + row, a + row);
                self.indices.extend([a, b, c, a, c, d]);
            }
        }
    }
}
