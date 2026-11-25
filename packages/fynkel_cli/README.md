# fynkel_cli

The `fynkel` command, for [Fynkel](https://pub.dev/packages/fynkel), code-first 3D
games in Flutter.

```sh
dart pub global activate fynkel_cli
fynkel create my_game
cd my_game
flutter run -d macos
```

- `fynkel create <folder>` makes a Flutter app set up for Fynkel, with a first
  game and its test. `--physics` and `--audio` add those packages, `--org` sets
  the app's id, and `--platforms` which of macos, ios and android it is for.
- `fynkel doctor` checks this machine, and the game in the folder it is run in,
  and says what to do about what is missing.

If the shell doesn't find `fynkel` after the first line, add
`$HOME/.pub-cache/bin` to your PATH.
