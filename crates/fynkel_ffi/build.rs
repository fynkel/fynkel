//! Flutter and Dart write the library's path into it, and a long one (a game
//! deep in folders) needs room, which Apple's linker leaves only when asked.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_VENDOR").is_ok_and(|vendor| vendor == "apple") {
        println!("cargo::rustc-link-arg-cdylib=-Wl,-headerpad_max_install_names");
    }
}
