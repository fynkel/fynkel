//! `include/fynkel.h` is what cbindgen makes of the source right now, so the
//! header (and the Dart bindings generated from it) can't drift from the
//! Rust functions. `FYNKEL_BLESS=1` rewrites it.

use std::path::Path;

#[test]
fn the_header_is_up_to_date() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let config = cbindgen::Config::from_file(crate_dir.join("cbindgen.toml")).expect("cbindgen.toml");
    let mut generated = Vec::new();
    cbindgen::Builder::new()
        .with_crate(crate_dir)
        .with_config(config)
        .generate()
        .expect("cbindgen understands the crate")
        .write(&mut generated);
    let generated = String::from_utf8(generated).unwrap();

    let path = crate_dir.join("include/fynkel.h");
    if std::env::var("FYNKEL_BLESS").as_deref() == Ok("1") {
        std::fs::write(&path, &generated).unwrap();
        return;
    }
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        current == generated,
        "{} is out of date; run `FYNKEL_BLESS=1 cargo test -p fynkel_ffi --test header`, then regenerate the Dart \
         bindings (packages/fynkel: dart run ffigen)",
        path.display()
    );
}
