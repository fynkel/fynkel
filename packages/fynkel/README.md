# package:fynkel

Flutter-independent Dart gameplay API over the Fynkel native runtime
(ABI v1, `crates/fynkel_native`). See `docs/design/dart-sdk.md`.

## Native runtime (ADR-0015)

The runtime reaches a Dart or Flutter app as the code asset
`package:fynkel/fynkel_native`, provided by `hook/build.dart`. Every binding
is a `@Native` external on that asset; nothing calls `DynamicLibrary.open`.
The hook resolves the library in this order:

1. `hooks.user_defines.fynkel.native_lib` — use this library as-is.
2. Fynkel source checkout (`packages/fynkel` inside the Cargo workspace) —
   `cargo build --release -p fynkel_platform_apple` (macOS) or
   `-p fynkel_native` (other targets), cross-compiling with `--target` when
   the requested architecture is not the host's.
3. Prebuilt artifact for the target named in `fynkel.lock.json`, downloaded
   from the release store, SHA-256 verified, cached under the shared hook
   output directory.
4. `cargo build` from source vendored next to the package when a Rust
   toolchain exists and the artifact is unpublished or
   `build_from_source: true`.

Otherwise it fails with a diagnostic naming the fix (install Rust, set
`native_lib`, or pin a published runtime version).

Configure in the app's `pubspec.yaml`:

```yaml
hooks:
  user_defines:
    fynkel:
      lock_file: fynkel.lock.json   # project runtime pin; default: the package's
      build_from_source: true       # skip the download
      native_lib: path/to/libfynkel_platform_apple.dylib
      features: [authoring]         # cargo features of the runtime — the editor only, never a game
```

`features` opts into cargo features of the native crates; the hook always
builds with `--no-default-features` and adds exactly what is listed, so a
game's runtime carries none of them. The only known feature is `authoring`
(the editor's `fynkel_authoring_*` ABI, `package:fynkel_authoring`); a
requested feature also skips the prebuilt download, since the published
runtime is a game's.

Hooks run with a stripped environment (only `PATH`, `HOME` and a few tool
variables survive), which is why these are user-defines rather than
environment variables.

`fynkel.lock.json` pins `runtime_version`, `abi_version`, the release URL
template and per-target artifacts (`<os>[-simulator]-<arch>` → file +
sha256). Until release CI publishes artifacts every entry is unpublished
(`sha256: null`), so the hook builds from source.
