//! What the editor's session and the cook cost on a large project
//! (roadmap Phase 1 item 3; `benchmarks/scale/generate.py` writes one):
//!
//! ```text
//! cargo run --release -p fynkel_authoring --example scale -- DIR [session|cook|cook-warm]
//! ```
//!
//! `session` (the default) opens the project, then rescans one material,
//! one texture and the materials folder the way the editor's watcher
//! does; `cook` empties the cache and cooks twice (cold, then nothing to
//! do); `cook-warm` cooks once over the cache as it is. Run each under
//! `/usr/bin/time -l` for the peak memory.

use std::path::PathBuf;
use std::time::Instant;

use fynkel_authoring::OpenProject;

fn ms(t: Instant) -> String {
    format!("{:.1} ms", t.elapsed().as_secs_f64() * 1000.0)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(
        args.next()
            .expect("usage: scale DIR [session|cook|cook-warm]"),
    );
    let mode = args.next().unwrap_or_else(|| "session".into());
    let t = Instant::now();
    let mut project = OpenProject::open(&dir).expect("the project opens");
    println!(
        "open: {} ({} assets, {} references)",
        ms(t),
        project.assets.len(),
        project.references.len()
    );
    match mode.as_str() {
        "session" => {
            let first = |sub: &str| -> PathBuf {
                let mut files: Vec<PathBuf> = std::fs::read_dir(dir.join("assets").join(sub))
                    .expect("the folder")
                    .map(|e| e.unwrap().path())
                    .collect();
                files.sort();
                files[0].clone()
            };
            for (what, path) in [
                ("one material", first("materials")),
                ("one texture", first("textures")),
                ("the materials folder", dir.join("assets/materials")),
            ] {
                let t = Instant::now();
                project.rescan(&[path]);
                println!("rescan {what}: {}", ms(t));
            }
        }
        "cook" => {
            let _ = std::fs::remove_dir_all(project.cache_dir());
            cook(&project, "cold");
            cook(&project, "warm");
        }
        "cook-warm" => cook(&project, "warm"),
        other => panic!("unknown mode {other}"),
    }
}

fn cook(project: &OpenProject, what: &str) {
    let t = Instant::now();
    let report = project.cook();
    let cooked = report
        .assets
        .iter()
        .filter(|a| a.status == fynkel_import::Status::Cooked)
        .count();
    assert!(!report.has_errors(), "{:?}", report.diagnostics.first());
    println!("cook {what}: {} ({cooked} cooked)", ms(t));
}
