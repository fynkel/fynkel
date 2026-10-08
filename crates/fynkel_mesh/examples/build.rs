//! Times building meshes (stage 2 step 2): the built-in shapes, and a
//! grid like a terrain made from code, with and without what MeshData::new
//! computes.
//!
//! `cargo run --release -p fynkel_mesh --example build`
//!
//! Each line is the best of 5 runs.

use std::time::{Duration, Instant};

use fynkel_mesh::{MeshData, MeshParts};

/// Makes one mesh.
type Build<'a> = Box<dyn FnMut() -> MeshData + 'a>;

fn best(mut f: impl FnMut() -> MeshData) -> (Duration, usize) {
    let mut best = Duration::MAX;
    let mut vertices = 0;
    for _ in 0..5 {
        let t = Instant::now();
        let mesh = std::hint::black_box(f());
        best = best.min(t.elapsed());
        vertices = mesh.vertices().len();
    }
    (best, vertices)
}

fn main() {
    // A 256 × 256 grid of quads: 66,049 shared vertices, 131,072 triangles.
    let n = 256u32;
    let mut positions = vec![];
    let mut uvs = vec![];
    for z in 0..=n {
        for x in 0..=n {
            let (u, v) = (x as f32 / n as f32, z as f32 / n as f32);
            positions.push([u * 100.0, (u * 20.0).sin() * (v * 13.0).cos(), v * 100.0]);
            uvs.push([u * 10.0, v * 10.0]);
        }
    }
    let normals: Vec<[f32; 3]> = vec![[0.0, 1.0, 0.0]; positions.len()];
    let mut indices = vec![];
    for z in 0..n {
        for x in 0..n {
            let a = z * (n + 1) + x;
            let b = a + n + 1;
            indices.extend([a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    let grid = MeshParts { positions: &positions, indices: &indices, ..MeshParts::default() };

    let lines: [(&str, Build); 7] = [
        ("cube", Box::new(MeshData::cube)),
        ("sphere 48 × 24", Box::new(|| MeshData::sphere(48, 24))),
        ("plane", Box::new(MeshData::plane)),
        ("grid: positions only", Box::new(|| MeshData::new(grid).unwrap())),
        ("grid: + normals", Box::new(|| MeshData::new(MeshParts { normals: Some(&normals), ..grid }).unwrap())),
        ("grid: + UVs (tangents)", Box::new(|| MeshData::new(MeshParts { uvs: Some(&uvs), ..grid }).unwrap())),
        (
            "grid: + normals + UVs",
            Box::new(|| MeshData::new(MeshParts { normals: Some(&normals), uvs: Some(&uvs), ..grid }).unwrap()),
        ),
    ];
    for (name, f) in lines {
        let (time, vertices) = best(f);
        println!("{name:<24} {:>9.3} ms   {vertices} vertices", time.as_secs_f64() * 1e3);
    }
}
