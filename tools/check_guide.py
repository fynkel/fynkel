#!/usr/bin/env python3
"""Checks that docs/writing-a-game.md's code compiles, and runs it.

Every ```dart block goes into a throwaway app, `my_game`, as the guide
says: a block whose first line is a path comment (`// lib/hud.dart`) is
that file; the rest go together into lib/game.dart, which imports
package:fynkel and ValueNotifier. Then `flutter analyze` and `flutter test`
there: the guide's own tests, and test/guide_runs_test.dart, made here,
which adds each game object and behaviour the guide writes (those made
with no arguments) to a world of its own with a camera, runs it for a
second and expects no errors, and tools/guide_checks_test.dart, which
checks that some of them do what the guide says. Run it from anywhere:
python3 tools/check_guide.py [--keep]
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
    files['test/guide_runs_test.dart'] = runs_test(files)
    files['test/guide_checks_test.dart'] = (ROOT / 'tools' / 'guide_checks_test.dart').read_text()
    for path, text in files.items():
        (app / path).parent.mkdir(parents=True, exist_ok=True)
        (app / path).write_text(text)
    assets(files, app)
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
  fynkel_physics:
    path: {ROOT / 'packages' / 'fynkel_physics'}
  fynkel_audio:
    path: {ROOT / 'packages' / 'fynkel_audio'}
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


# Where a stand-in for an asset the guide loads is looked for: the same
# path, then the same name, then any file of its kind.
STAND_INS = [ROOT / 'examples', ROOT / 'tests' / 'assets', ROOT / 'crates' / 'fynkel_audio' / 'tests' / 'fixtures']


def assets(files: dict[str, str], app: pathlib.Path) -> None:
    """The app's assets/ for the runs test: the guide's own shaders (its
    ```wgsl blocks that start with their path), and for every other name
    lib/ loads (`assets.audio('bark.ogg')`), a file of the repository's in
    its place."""
    for block in re.findall(r'```wgsl\n(.*?)```', GUIDE.read_text(), re.S):
        path = re.match(r'// (assets/[\w/.]+)', block)
        if path:
            (app / path.group(1)).parent.mkdir(parents=True, exist_ok=True)
            (app / path.group(1)).write_text(block)
    everything = [f for root in STAND_INS for f in root.rglob('*.*') if '/build/' not in str(f) and f.is_file()]
    loaded = set()
    for path, text in files.items():
        if path.startswith('lib/'):
            # A name made from a number ('level_$number', 'level_${number + 1}')
            # stands for the first few.
            for name in re.findall(r"assets\.\w+\(\[?'([^']+)'", text):
                loaded.update(re.sub(r'\$(\{[^}]*\}|\w+)', str(n), name) for n in (1, 2, 3))
    for name in sorted(loaded):
        target = app / 'assets' / name
        if target.exists():
            continue
        suffix = pathlib.PurePath(name).suffix
        found = ([f for f in everything if str(f).endswith('/' + name)]
                 or [f for f in everything if f.name == pathlib.PurePath(name).name]
                 or [f for f in everything if f.suffix == suffix])
        if found:
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(min(found, key=lambda f: (len(f.parts), str(f))), target)


def runs_test(files: dict[str, str]) -> str:
    """test/guide_runs_test.dart: a test for each class in lib/ that is a
    GameObject or a Behaviour, directly or through another of the guide's,
    and is made with no arguments."""
    classes = {}  # (file, name) -> (base, body)
    for path, text in files.items():
        if not path.startswith('lib/'):
            continue
        for m in re.finditer(r'^class (\w+)(?:<[^>]*>)? extends (\w+)[^{]*\{(.*?)^\}', text, re.S | re.M):
            classes[(path, m.group(1))] = (m.group(2), m.group(3))

    def kind(path: str, name: str, seen: int = 0) -> str | None:
        base = classes[(path, name)][0]
        if base in ('GameObject', 'Behaviour'):
            return base
        for (other, other_name) in classes:
            if other_name == base and seen < 8:
                return kind(other, other_name, seen + 1)
        return None

    def no_arguments(name: str, body: str) -> bool:
        made = re.search(r'^  (?:const |factory )?' + name + r'\(', body, re.M)
        if not made:
            return not re.search(r'^  (?:const |factory )?' + name + r'\.', body, re.M)
        depth, at = 1, made.end()
        while depth:
            depth += {'(': 1, ')': -1}.get(body[at], 0)
            at += 1
        params = body[made.end():at - 1].strip()
        return params == '' or (params[0] in '{[' and 'required' not in params)

    # Functions that make a game object with no arguments (`ball()`): they
    # put a behaviour with what it needs, and run instead of it alone.
    functions = [(path, m.group(1)) for path, text in files.items() if path.startswith('lib/')
                 for m in re.finditer(r'^GameObject ([a-z]\w*)\(\)', text, re.M)]
    made_by_functions = {(path, name) for path, text in files.items() if path.startswith('lib/')
                         for name in re.findall(r'\b([A-Z]\w*)\(\)', ' '.join(
                             re.findall(r'^GameObject [a-z]\w*\(\).*?^\S', text, re.S | re.M)))}
    imports, tests, prefixes = [], [], {}

    def prefix(path: str) -> str:
        if path not in prefixes:
            prefixes[path] = f'f{len(prefixes)}'
            imports.append(f"import 'package:my_game/{path[4:]}' as {prefixes[path]};")
        return prefixes[path]

    def run(label: str, made: str, camera: str = '') -> None:
        tests.append(f"""  test('{label} runs for a second', () async {{
    final world = World(source: (name) => File('assets/$name').readAsBytes())..input.simulate.resize(800, 600);
    addTearDown(world.dispose);
    final errors = <ObjectError>[];
    world.errors.listen(errors.add);
    world.add(GameObject(transform: Transform(position: Vec3(0, 3, 10)), components: [Camera(){camera}]));
    {made}
    world.run(1);
    await pumpEventQueue();
    expect(errors.map((e) => '${{e.error}}'), isEmpty);
  }});""")

    for (path, name), (base, body) in sorted(classes.items()):
        what = kind(path, name)
        if what is None or name.startswith('_') or not no_arguments(name, body):
            continue
        if (path, name) in made_by_functions:
            continue
        p = prefix(path)
        if what == 'GameObject':
            run(f'{path}: {name}', f'world.add({p}.{name}());')
        elif re.search(r'get<Camera>\(\)!', body):
            run(f'{path}: {name}, on the camera', '', camera=f', {p}.{name}()')
        else:
            run(f'{path}: {name}', f'world.add(GameObject(components: [{p}.{name}()]));')
    for path, name in sorted(functions):
        run(f'{path}: {name}()', f'world.add({prefix(path)}.{name}());')
    return '\n'.join([
        '// Made by tools/check_guide.py: each of the guide\'s game objects and',
        '// behaviours, run for a second.',
        "import 'dart:io';",
        '',
        "import 'package:flutter_test/flutter_test.dart';",
        "import 'package:fynkel/fynkel.dart';",
        "import 'package:fynkel/testing.dart';",
        *imports,
        '',
        'void main() {',
        *tests,
        '}',
        '',
    ])


if __name__ == '__main__':
    sys.exit(main())
