//! Mesh data on the CPU, and the built-in shapes.
//!
//! Front faces wind counter-clockwise seen from outside, and normals point
//! outwards.

use glam::Vec3;

/// One vertex: position and normal.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
}

/// A triangle list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshData {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl MeshData {
    /// A 1×1×1 cube centred on the origin, with 24 vertices so each face
    /// has its own flat normal.
    pub fn cube() -> Self {
        let mut mesh = Self::default();
        // Each face: its normal, and two axes spanning it, chosen so that
        // u × v = normal (which makes the winding counter-clockwise).
        let faces = [
            (Vec3::X, Vec3::NEG_Z, Vec3::Y),
            (Vec3::NEG_X, Vec3::Z, Vec3::Y),
            (Vec3::Y, Vec3::X, Vec3::NEG_Z),
            (Vec3::NEG_Y, Vec3::X, Vec3::Z),
            (Vec3::Z, Vec3::X, Vec3::Y),
            (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y),
        ];
        for (normal, u, v) in faces {
            let base = mesh.vertices.len() as u32;
            for (su, sv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let p = (normal + u * su + v * sv) * 0.5;
                mesh.vertices.push(Vertex { position: p.into(), normal: normal.into() });
            }
            mesh.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        mesh
    }

    /// A sphere of radius 0.5 centred on the origin: `rings` bands from
    /// pole to pole, `segments` slices around.
    pub fn sphere(segments: u32, rings: u32) -> Self {
        assert!(segments >= 3 && rings >= 2, "a sphere needs at least 3 segments and 2 rings");
        let mut mesh = Self::default();
        for ring in 0..=rings {
            let theta = std::f32::consts::PI * ring as f32 / rings as f32; // 0 at +Y
            for segment in 0..=segments {
                let phi = std::f32::consts::TAU * segment as f32 / segments as f32;
                let n = Vec3::new(theta.sin() * phi.sin(), theta.cos(), theta.sin() * phi.cos());
                mesh.vertices.push(Vertex { position: (n * 0.5).into(), normal: n.into() });
            }
        }
        let row = segments + 1;
        for ring in 0..rings {
            for segment in 0..segments {
                let a = ring * row + segment; // upper left
                let b = a + row; // lower left
                // Skip the triangles that collapse to a line at each pole.
                if ring != 0 {
                    mesh.indices.extend([a, b, a + 1]);
                }
                if ring != rings - 1 {
                    mesh.indices.extend([a + 1, b, b + 1]);
                }
            }
        }
        mesh
    }

    /// A 1×1 plane in XZ centred on the origin, facing +Y.
    pub fn plane() -> Self {
        let n = [0.0, 1.0, 0.0];
        let vertices = [[-0.5, 0.0, 0.5], [0.5, 0.0, 0.5], [0.5, 0.0, -0.5], [-0.5, 0.0, -0.5]]
            .map(|position| Vertex { position, normal: n })
            .to_vec();
        Self { vertices, indices: vec![0, 1, 2, 0, 2, 3] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every triangle winds counter-clockwise seen from where its normal
    /// points, and every normal is unit length.
    fn check(mesh: &MeshData) {
        assert_eq!(mesh.indices.len() % 3, 0);
        for v in &mesh.vertices {
            assert!((Vec3::from(v.normal).length() - 1.0).abs() < 1e-5);
        }
        for tri in mesh.indices.chunks(3) {
            let [a, b, c] = [tri[0], tri[1], tri[2]].map(|i| mesh.vertices[i as usize]);
            let face = (Vec3::from(b.position) - Vec3::from(a.position))
                .cross(Vec3::from(c.position) - Vec3::from(a.position));
            assert!(face.length() > 1e-7, "degenerate triangle {tri:?}");
            let avg_normal = Vec3::from(a.normal) + Vec3::from(b.normal) + Vec3::from(c.normal);
            assert!(face.dot(avg_normal) > 0.0, "triangle {tri:?} winds clockwise");
        }
    }

    #[test]
    fn cube_has_flat_faces_wound_outwards() {
        let cube = MeshData::cube();
        assert_eq!((cube.vertices.len(), cube.indices.len()), (24, 36));
        check(&cube);
        for v in &cube.vertices {
            assert!(v.position.iter().all(|c| c.abs() == 0.5));
        }
    }

    #[test]
    fn sphere_is_wound_outwards_with_radius_half() {
        let sphere = MeshData::sphere(32, 16);
        check(&sphere);
        for v in &sphere.vertices {
            assert!((Vec3::from(v.position).length() - 0.5).abs() < 1e-6);
        }
    }

    #[test]
    fn plane_faces_up() {
        check(&MeshData::plane());
    }
}
