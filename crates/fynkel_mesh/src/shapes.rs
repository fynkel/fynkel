//! The built-in shapes, with UVs that show a texture the right way round
//! from outside. Their tangents are worked out exactly here rather than
//! generated: MikkTSpace takes about 3 ms for the sphere, and gives the
//! same directions (a test checks).

use glam::Vec3;

use crate::{MeshData, MeshParts};

impl MeshData {
    /// A 1×1×1 cube centred on the origin, with 24 vertices so each face
    /// has its own flat normal. Each face shows the whole texture, upright
    /// on the four sides; the top's image-up is -Z, the bottom's +Z.
    pub fn cube() -> Self {
        // Each face: its normal, and the directions of image-right and
        // image-up on it, chosen so right × up = normal (which makes the
        // winding counter-clockwise).
        let faces = [
            (Vec3::X, Vec3::NEG_Z, Vec3::Y),
            (Vec3::NEG_X, Vec3::Z, Vec3::Y),
            (Vec3::Y, Vec3::X, Vec3::NEG_Z),
            (Vec3::NEG_Y, Vec3::X, Vec3::Z),
            (Vec3::Z, Vec3::X, Vec3::Y),
            (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y),
        ];
        let mut s = Shape::default();
        for (normal, right, up) in faces {
            let base = s.positions.len() as u32;
            for (su, sv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                s.push((normal + right * su + up * sv) * 0.5, normal, right, [(su + 1.0) * 0.5, (1.0 - sv) * 0.5]);
            }
            s.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        s.build()
    }

    /// A sphere of radius 0.5 centred on the origin: `rings` bands from
    /// pole to pole, `segments` slices around. The texture wraps around
    /// once, starting and ending at +Z (a seam of doubled vertices), with
    /// its top at the north pole (+Y).
    pub fn sphere(segments: u32, rings: u32) -> Self {
        assert!(segments >= 3 && rings >= 2, "a sphere needs at least 3 segments and 2 rings");
        let mut s = Shape::default();
        for ring in 0..=rings {
            let v = ring as f32 / rings as f32;
            let theta = std::f32::consts::PI * v; // 0 at +Y
            for segment in 0..=segments {
                let u = segment as f32 / segments as f32;
                // Growing towards +X seen from +Z, so the texture reads
                // left to right from outside.
                let phi = std::f32::consts::TAU * u;
                let n = Vec3::new(theta.sin() * phi.sin(), theta.cos(), theta.sin() * phi.cos());
                // Along the line of latitude, where phi (and u) grows. Also
                // defined at the poles.
                let tangent = Vec3::new(phi.cos(), 0.0, -phi.sin());
                s.push(n * 0.5, n, tangent, [u, v]);
            }
        }
        let row = segments + 1;
        for ring in 0..rings {
            for segment in 0..segments {
                let a = ring * row + segment; // upper left
                let b = a + row; // lower left
                // Skip the triangles that collapse to a line at each pole.
                if ring != 0 {
                    s.indices.extend([a, b, a + 1]);
                }
                if ring != rings - 1 {
                    s.indices.extend([a + 1, b, b + 1]);
                }
            }
        }
        s.build()
    }

    /// A 1×1 plane in XZ centred on the origin, facing +Y. Seen from
    /// above, image-right is +X and image-up is -Z.
    pub fn plane() -> Self {
        let mut s = Shape::default();
        for (position, uv) in
            [([-0.5, 0.5], [0.0, 1.0]), ([0.5, 0.5], [1.0, 1.0]), ([0.5, -0.5], [1.0, 0.0]), ([-0.5, -0.5], [0.0, 0.0])]
        {
            s.push(Vec3::new(position[0], 0.0, position[1]), Vec3::Y, Vec3::X, uv);
        }
        s.indices.extend([0, 1, 2, 0, 2, 3]);
        s.build()
    }
}

/// A shape's arrays, filled vertex by vertex. Every tangent's w is +1: the
/// shapes show their texture unmirrored.
#[derive(Default)]
struct Shape {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    tangents: Vec<[f32; 4]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

impl Shape {
    fn push(&mut self, position: Vec3, normal: Vec3, tangent: Vec3, uv: [f32; 2]) {
        self.positions.push(position.into());
        self.normals.push(normal.into());
        self.tangents.push(tangent.extend(1.0).into());
        self.uvs.push(uv);
    }

    fn build(&self) -> MeshData {
        MeshData::new(MeshParts {
            positions: &self.positions,
            normals: Some(&self.normals),
            tangents: Some(&self.tangents),
            uvs: Some(&self.uvs),
            indices: &self.indices,
        })
        .expect("the built-in shapes are valid")
    }
}
