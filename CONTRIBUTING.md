# Contributing

Fynkel is a 0.x preview. The most useful thing you can send is a report of
what went wrong, or what you couldn't work out from the guide.

## Reporting a bug

[Open an issue](https://github.com/fynkel/fynkel/issues) with:

- what you did, as little code as shows it (a `flutter test` that fails is
  best: [Testing a game](https://fynkel.com/making/testing/) runs one without a window);
- what you expected and what happened, with the whole error;
- what `fynkel doctor` prints, and the device if it's on one.

A security problem goes another way: [SECURITY.md](SECURITY.md).

## Changing something

For more than a small fix, open an issue first, so you don't write what
can't be merged. Fynkel stays code-first (no editor, no scene files),
keeps Unity's names where Unity has one, and runs on macOS, iOS and
Android.

1. Build from a clone, on a Mac: the README's "From this repository".
2. Change it, with a test that fails without the change.
3. Run what your change touches:
   - Rust: `cargo fmt --all --check`,
     `cargo clippy --workspace --all-targets -- -D warnings`,
     `cargo test --workspace`
   - Dart, in the package: `dart format -l 120 .`,
     `dart analyze --fatal-infos`, `dart test`
     (`flutter analyze` and `flutter test` in `fynkel_flutter`,
     `fynkel_measure` and the examples)
   - The guide's code: `python3 tools/check_guide.py`
   - A C header or `lib/src/bindings.g.dart`: `dart run tool/ffigen.dart`
4. Open a pull request that says what changed and why. The same checks run
   on it.

What you send is under the [MIT licence](LICENSE), as Fynkel is. Art and
sound must be under a licence that allows it, written in the `CREDITS.md`
beside it.
