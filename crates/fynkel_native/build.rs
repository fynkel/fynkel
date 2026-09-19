//! Apple dylibs get a padded Mach-O header: the Flutter native-assets build
//! rewrites the install name to the app's absolute path with
//! `install_name_tool`, which fails when the new name does not fit in the
//! original load commands (deeply nested project directories).

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_VENDOR").as_deref() == Ok("apple") {
        println!("cargo:rustc-link-arg=-Wl,-headerpad_max_install_names");
    }
}
