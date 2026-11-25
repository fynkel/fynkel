---
title: "Saving"
description: "Keep progress and settings between runs."
---

What's kept between runs is plain data, never objects: the
level reached, the best score, the settings (`Controls` has `toJson` and
`Controls.fromJson`, [A controls page](/flutter/rebinding/)). Write it as JSON to a file in the app's own
folder, which the app finds with the `path_provider` package
(`getApplicationSupportDirectory()`, a folder that may not be there yet)
and hands to the game:

```dart
// lib/save.dart
import 'dart:convert';
import 'dart:io';

/// What the game keeps between runs.
class Save {
  Save({this.level = 1, this.best = 0});

  /// Which shape of save this game writes: a later game that changes it
  /// reads an older one by this.
  static const version = 1;

  int level;
  int best;

  Map<String, Object?> toJson() => {'version': version, 'level': level, 'best': best};

  /// Reads [file]; a new save when there's none, or it can't be read
  /// (an older game's, or cut short).
  static Save read(File file) {
    try {
      final json = jsonDecode(file.readAsStringSync()) as Map<String, Object?>;
      return Save(level: json['level'] as int? ?? 1, best: json['best'] as int? ?? 0);
    } catch (_) {
      return Save();
    }
  }

  /// Writes it whole: to a file beside [file] first, on the disk before
  /// it's renamed over it, so a crash or a power cut halfway leaves the
  /// last save, not half of this one.
  void write(File file) {
    file.parent.createSync(recursive: true);
    File('${file.path}.new')
      ..writeAsStringSync(jsonEncode(toJson()), flush: true)
      ..renameSync(file.path);
  }
}
```

The app finds the folder before the game starts, so `main` is `async`,
and Flutter must be ready before `path_provider` is asked
(`WidgetsFlutterBinding.ensureInitialized()`). Add `path_provider` to
the app's dependencies (`flutter pub add path_provider`):

```dart
// lib/main_with_save.dart
import 'dart:io';

import 'package:flutter/widgets.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:path_provider/path_provider.dart';

import 'levels.dart' show Levels;
import 'save.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  final folder = await getApplicationSupportDirectory();
  final file = File('${folder.path}/save.json');
  final save = Save.read(file);
  runApp(FynkelApp(() => Levels(first: save.level)));
}
```

Save when something worth keeping happens (a level done) and in
`onApplicationPause` ([The frame and the lifecycle](/basics/lifecycle/)), as
the app may be closed while it's away, not every frame: a write is a few
milliseconds on a phone.

A few settings and nothing else can go in `shared_preferences` instead, as
[examples/pocket_hole](https://github.com/fynkel/fynkel/blob/main/examples/pocket_hole)'s level does: simpler, but its writes are queued, so one
the app is killed in the middle of may be lost; the file above says what
happens then.
