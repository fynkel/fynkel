# Optional native combat package

Run the supported, statically linked extension route:

```sh
cargo run --release -p fynkel_example_native_combat --example headless --locked
cargo test --release -p fynkel_example_native_combat --locked
```

The package defines Health and Damage outside the engine catalogue. The host depends on this crate and calls `world.install_extension(&Combat)` before loading scenes or starting gameplay. A host that embeds the C bridge can install on a core `World` before constructing `FynkelWorld::with_core(world)`. Install native codecs before matching Dart metadata registration; the matching metadata adopts the native storage and validation.

This is a Rust source API, linked into the host, using its engine revision, Cargo.lock and Rust toolchain. Extension API version 1 and runtime ABI 5 must match the manifest; these checks do not turn Rust types into a stable binary ABI. Rebuild the host when changing native package code. There is no runtime discovery, dynamic library loader or automatic Dart package-to-native linking in this route.

The workspace dependency disables core default features. This example needs no physics, animation or audio. Packages declare required modules in their manifest and system descriptions; unavailable requirements reject installation, and disabled requirements prevent execution. A failed installation publishes neither component registrations nor systems. The editor rejects native extension installation before calling package code; it reads generated component schemas as metadata instead.

Ordinary game development uses `@FynkelComponent`, GameObject data access and Dart systems; see `../registered_gameplay`. Rust is optional. Native codecs are trusted, pure value codecs: do not put services, shared mutable state or effects in component records. Native managed object-reference codecs are deliberately rejected; runtime-defined component layouts provide managed references.
