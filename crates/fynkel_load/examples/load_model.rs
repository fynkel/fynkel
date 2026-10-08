//! Times reading the sample models (stage 2 step 4): what `Model.load`
//! does on its helper isolate.
//!
//! `cargo run --release -p fynkel_load --example load_model`
//!
//! Each line is the best of 5 runs.

use std::time::{Duration, Instant};

use fynkel_load::ModelData;

fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/assets/gltf");
    for name in [
        "Box.glb",
        "BoxTextured.glb",
        "CesiumMilkTruck.glb",
        "Fox.glb",
        "AlphaBlendModeTest.glb",
        "NormalTangentMirrorTest.glb",
        "tower.glb",
    ] {
        let bytes = std::fs::read(dir.join(name)).unwrap();
        let best: Duration = (0..5)
            .map(|_| {
                let t = Instant::now();
                std::hint::black_box(ModelData::from_glb(&bytes).unwrap());
                t.elapsed()
            })
            .min()
            .unwrap();
        let model = ModelData::from_glb(&bytes).unwrap();
        let pixels: u64 = model.images().iter().map(|i| u64::from(i.width()) * u64::from(i.height())).sum();
        println!(
            "{name:28} {:>5} KiB  {:>3} nodes  {:>2} meshes  {:>5.1} Mpx  {:>6.1} ms",
            bytes.len() / 1024,
            model.nodes().len(),
            model.meshes().len(),
            pixels as f64 / 1e6,
            best.as_secs_f64() * 1000.0
        );
    }
}
