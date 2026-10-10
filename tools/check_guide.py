#!/usr/bin/env python3
"""Checks that docs/writing-a-game.md's code compiles, and runs its test.

Every ```dart block goes into a throwaway app, `my_game`, as the guide
says: a block whose first line is a path comment (`// lib/hud.dart`) is
that file; the rest go together into lib/game.dart, which imports
package:fynkel and ValueNotifier. Then `flutter analyze` and `flutter test`
there. Run it from anywhere: python3 tools/check_guide.py [--keep]
"""

import pathlib
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
GUIDE = ROOT / 'docs' / 'writing-a-game.md'

HEADER = """import 'package:flutter/foundation.dart' show ValueNotifier;
import 'package:fynkel/fynkel.dart';

"""


def main() -> int:
    keep = '--keep' in sys.argv
    blocks = re.findall(r'```dart\n(.*?)```', GUIDE.read_text(), re.S)
    app = pathlib.Path(tempfile.mkdtemp(prefix='my_game_'))
    files: dict[str, str] = {}
    shared = []
    for block in blocks:
        first = block.split('\n', 1)[0]
        path = re.fullmatch(r'// ((?:lib|test)/[\w/]+\.dart)', first)
        if path:
            files[path.group(1)] = block
        else:
            shared.append(block)
    files['lib/game.dart'] = HEADER + '\n'.join(shared)
    for path, text in files.items():
        (app / path).parent.mkdir(parents=True, exist_ok=True)
        (app / path).write_text(text)
    (app / 'pubspec.yaml').write_text(f"""name: my_game
publish_to: none
environment:
  sdk: ^3.13.0
dependencies:
  flutter:
    sdk: flutter
  fynkel:
    path: {ROOT / 'packages' / 'fynkel'}
  fynkel_flutter:
    path: {ROOT / 'packages' / 'fynkel_flutter'}
dev_dependencies:
  flutter_test:
    sdk: flutter
""")
    print(f'{len(blocks)} blocks, {len(files)} files, in {app}')
    try:
        for command in (['flutter', 'pub', 'get'], ['flutter', 'analyze', '--fatal-infos'], ['flutter', 'test']):
            if subprocess.run(command, cwd=app).returncode != 0:
                keep = True
                print(f'failed: {" ".join(command)} (kept {app})')
                return 1
        return 0
    finally:
        if not keep:
            shutil.rmtree(app)


if __name__ == '__main__':
    sys.exit(main())
