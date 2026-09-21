# fynkel_editor

The Fynkel Editor (blueprint §19.2e, ADR-0016, ADR-0018): a Flutter macOS
desktop app over `package:fynkel_authoring`. The editor holds no project
truth of its own and never runs game code; `docs/editor.md` is the
reference.

```sh
flutter pub get
flutter analyze --fatal-infos
flutter test                       # includes test/spike/docking_spike_test.dart, the docking spike
```

Use Flutter's own `dart`/`flutter` binaries. Formatting is `dart format`
at 120 columns (`analysis_options.yaml`).
