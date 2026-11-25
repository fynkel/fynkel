# Releasing Fynkel

All the packages share one version and go out together, from one commit of
main. A version on pub.dev can't be taken back, only followed by another.

1. **The version.** Set it in the five packages' `pubspec.yaml`
   (`fynkel`, `fynkel_flutter`, `fynkel_physics`, `fynkel_audio`,
   `fynkel_cli`), in `fynkelVersion` (`packages/fynkel_cli/lib/fynkel_cli.dart`)
   and in `Cargo.toml`, and write the `CHANGELOG.md`s. Merge it, run the
   long checks on main (`gh workflow run nightly.yml`), and wait for them.
2. **The libraries**, on a Mac with the NDK, from that commit:
   `cd packages/fynkel && dart run tool/prebuilt.dart`. They are in
   `target/prebuilt/v<version>/`.
3. **The packages**: `dart run tool/release.dart`. They are in
   `fynkel-release-packages/` in the system's temporary folder (it says
   where), and pub has said what it thinks of each.
   Nothing may be wrong.
4. **Try them** before anything is public: `python3 tools/rehearse.py` stands
   in for pub.dev and the GitHub release, and says what to type. Make a game
   as a new user does, and build it for macOS, an iPhone and an Android phone.
5. **The GitHub release** `v<version>` of that commit, with every library,
   `SHA256SUMS` and no more: `gh release create`. Files of a release that is
   out are never changed: the packages hold their hashes.
6. **pub.dev**, in this order, each once the one before is there:
   `flutter pub publish` in each one's folder there: `fynkel`,
   `fynkel_flutter`, `fynkel_physics`, `fynkel_audio`, `fynkel_cli`.
7. **The website**, and a new game from nothing: `dart pub global activate
   fynkel_cli`, `fynkel create`, `flutter run`.
