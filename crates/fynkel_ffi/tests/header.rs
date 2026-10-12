//! `include/fynkel.h` is what cbindgen makes of the source right now, so the
//! header (and the Dart bindings generated from it) can't drift from the
//! Rust functions, and `include/abi.lock` says which ABI version it was
//! made with, so a change without a new version fails here.
//! `FYNKEL_BLESS=1` rewrites both.

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
    fynkel_cabi::check_header(
        &crate_dir.join("include/fynkel.h"),
        &String::from_utf8(generated).unwrap(),
        fynkel_ffi::FY_ABI_VERSION,
        std::env::var("FYNKEL_BLESS").as_deref() == Ok("1"),
        "run `FYNKEL_BLESS=1 cargo test -p fynkel_ffi --test header`, then regenerate the Dart bindings (packages/fynkel: dart run tool/ffigen.dart)",
    );
}
