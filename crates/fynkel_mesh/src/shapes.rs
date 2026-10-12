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

    /// A capsule along Y centred on the origin, 2 high from end to end and
    /// of radius 0.5, as Unity's (and a default `CapsuleCollider`): a
    /// cylinder 1 high with a half-sphere on each end. `segments` slices
    /// around, `rings` bands in each half-sphere. The texture wraps around
    /// once from +Z, its top at the top end; each part takes the share of
    /// its height that its length is of the outline.
    pub fn capsule(segments: u32, rings: u32) -> Self {
        assert!(segments >= 3 && rings >= 1, "a capsule needs at least 3 segments and 1 ring");
        let (radius, half) = (0.5f32, 0.5f32);
        // From the top pole down: each row's angle from +Y on its
        // half-sphere and its centre's height, the top half-sphere's
        // equator and the bottom's both present, so the side is one band.
        let quarter = std::f32::consts::FRAC_PI_2;
        let mut rows: Vec<(f32, f32)> = Vec::new();
        for ring in 0..=rings {
            rows.push((quarter * ring as f32 / rings as f32, half));
        }
        for ring in 0..=rings {
            rows.push((quarter + quarter * ring as f32 / rings as f32, -half));
        }
        // V by length along the outline: the arcs are a quarter turn each.
        let arc = quarter * radius;
        let total = 2.0 * arc + 2.0 * half;
        let mut s = Shape::default();
        for (i, &(theta, centre)) in rows.iter().enumerate() {
            let along =
                if i <= rings as usize { theta * radius } else { arc + 2.0 * half + (theta - quarter) * radius };
            let v = along / total;
            for segment in 0..=segments {
                let u = segment as f32 / segments as f32;
                let phi = std::f32::consts::TAU * u;
                let n = Vec3::new(theta.sin() * phi.sin(), theta.cos(), theta.sin() * phi.cos());
                let tangent = Vec3::new(phi.cos(), 0.0, -phi.sin());
                s.push(n * radius + Vec3::Y * centre, n, tangent, [u, v]);
            }
        }
        let row = segments + 1;
        let bands = rows.len() as u32 - 1;
        for band in 0..bands {
            for segment in 0..segments {
                let a = band * row + segment;
                let b = a + row;
                if band != 0 {
                    s.indices.extend([a, b, a + 1]);
                }
                if band != bands - 1 {
                    s.indices.extend([a + 1, b, b + 1]);
                }
            }
        }
        s.build()
    }

    /// A cylinder along Y centred on the origin, 2 high and of radius 0.5,
    /// as Unity's, with `segments` slices around. The side's texture wraps
    /// around once from +Z, its top at the top; each cap shows the texture
    /// as the plane does from outside, with its own flat normal.
    pub fn cylinder(segments: u32) -> Self {
        assert!(segments >= 3, "a cylinder needs at least 3 segments");
        let (radius, half) = (0.5f32, 1.0f32);
        let mut s = Shape::default();
        for (y, v) in [(half, 0.0), (-half, 1.0)] {
            for segment in 0..=segments {
                let u = segment as f32 / segments as f32;
                let phi = std::f32::consts::TAU * u;
                let n = Vec3::new(phi.sin(), 0.0, phi.cos());
                let tangent = Vec3::new(phi.cos(), 0.0, -phi.sin());
                s.push(n * radius + Vec3::Y * y, n, tangent, [u, v]);
            }
        }
        let row = segments + 1;
        for segment in 0..segments {
            let (a, b) = (segment, segment + row);
            s.indices.extend([a, b, a + 1, a + 1, b, b + 1]);
        }
        // The caps: a centre and a ring each. Seen from outside, image-right
        // is +X on both; image-up is -Z on the top, +Z on the bottom, as the
        // cube's.
        for (normal, up) in [(Vec3::Y, Vec3::NEG_Z), (Vec3::NEG_Y, Vec3::Z)] {
            let base = s.positions.len() as u32;
            let uv = |p: Vec3| [0.5 + p.dot(Vec3::X) / (2.0 * radius), 0.5 - p.dot(up) / (2.0 * radius)];
            s.push(normal * half, normal, Vec3::X, [0.5, 0.5]);
            for segment in 0..segments {
                let phi = std::f32::consts::TAU * segment as f32 / segments as f32;
                let p = Vec3::new(phi.sin(), 0.0, phi.cos()) * radius;
                s.push(p + normal * half, normal, Vec3::X, uv(p));
            }
            for segment in 0..segments {
                let (a, b) = (base + 1 + segment, base + 1 + (segment + 1) % segments);
                // Counter-clockwise seen from outside: from +Z towards +X
                // is counter-clockwise seen from +Y, and clockwise from -Y.
                if normal == Vec3::Y {
                    s.indices.extend([base, a, b]);
                } else {
                    s.indices.extend([base, b, a]);
                }
            }
        }
        s.build()
    }

    /// A 1×1 quad in XY centred on the origin, facing +Z: towards a camera
    /// that looks along its forward (-Z) at it, as Unity's quad faces its
    /// camera. Image-right is +X and image-up +Y.
    pub fn quad() -> Self {
        let mut s = Shape::default();
        for (position, uv) in
            [([-0.5, -0.5], [0.0, 1.0]), ([0.5, -0.5], [1.0, 1.0]), ([0.5, 0.5], [1.0, 0.0]), ([-0.5, 0.5], [0.0, 0.0])]
        {
            s.push(Vec3::new(position[0], position[1], 0.0), Vec3::Z, Vec3::X, uv);
        }
        s.indices.extend([0, 1, 2, 0, 2, 3]);
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
