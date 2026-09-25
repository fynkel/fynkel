// Code first (ADR-0027 §8): data assets written from Dart by
// `ProjectWriter.writeData` — the instance then knows its asset, so a
// scene or data asset written afterwards names it; one never written is
// refused. Over a copy of the fixture project with the `test.Weapon` type,
// validated and cooked by the `fynkel` CLI, loaded by the real native
// library.

import 'dart:convert';
import 'dart:io';

import 'package:fynkel/authoring.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

import '../support/weapons.dart';

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late String project;
  late NativeLibrary lib;

  setUpAll(() => lib = NativeLibrary.open());
  tearDownAll(() => lib.dispose());

  setUp(() {
    project = Directory.systemTemp.createTempSync('fynkel-code-first-data-').path;
    Process.runSync('cp', ['-R', '$fixture/.', project]);
    final stale = Directory('$project/.fynkel');
    if (stale.existsSync()) stale.deleteSync(recursive: true);
    writeWeaponSchemas(project);
    addTearDown(() => Directory(project).deleteSync(recursive: true));
  });

  ProjectWriter writer() => ProjectWriter(project, registerBehaviours: registerWeapons);

  Map<String, Object?> read(String rel) =>
      jsonDecode(File('$project/assets/$rel').readAsStringSync()) as Map<String, Object?>;

  test('written data assets validate, and a written scene names them', () {
    final w = writer();
    final sword = Weapon()..damage = 3;
    final great = Weapon()..damage = 8;
    final s = w.writeData('data/sword', sword);
    expect(sword.asset, s.ref);
    expect(sword.dataType, same(weaponType));
    // Two that name each other: one, the other, then the first again.
    w.writeData('data/great', great..upgrade = sword, name: 'Great Sword');
    expect(w.writeData('data/sword', sword..upgrade = great).changed, isTrue);
    expect(read('data/sword.data.json'), {
      r'$schema': 'https://fynkel.dev/schemas/data.schema.json',
      'formatVersion': 1,
      'id': s.ref.asset,
      'name': 'sword',
      'type': 'test.Weapon',
      'version': 1,
      'data': {
        'damage': 3,
        'upgrade': {'asset': great.asset!.asset},
      },
    });
    expect(read('data/great.data.json')['name'], 'Great Sword');
    final scene = w.writeScene(
      'scenes/arena',
      SceneSpec('Arena', [
        ObjectSpec('Knight', behaviours: [
          Wielder()
            ..weapon = sword
            ..spares = [great, sword],
        ]),
      ]),
    );

    final r = runFynkel(['validate', '--project', project, '--json']);
    expect(r.exitCode, 0, reason: '${r.stdout}${r.stderr}');
    cookProject(project);
    final world = World.create(library: lib)..registry.registerData(weaponType)..registry.register(wielderType);
    addTearDown(world.dispose);
    world.loadProject(project);
    world.onLoadDiagnostics = (d) => fail('load diagnostics: $d');
    world.loadScene(scene.ref);
    final knight = world.find('Knight')!.behaviour<Wielder>()!;
    expect(knight.weapon!.damage, 3);
    expect(knight.weapon!.upgrade!.damage, 8);
    expect(knight.weapon!.upgrade!.upgrade, same(knight.weapon));
    expect(knight.spares.map((x) => x.damage), [8, 3]);

    // Written again: the same ids and bytes; unchanged, the file untouched.
    final before = [for (final f in ['sword', 'great']) File('$project/assets/data/$f.data.json').readAsStringSync()];
    final again = writer();
    final sword2 = Weapon()..damage = 3;
    final great2 = Weapon()..damage = 8;
    again.writeData('data/sword', sword2);
    expect(sword2.asset, sword.asset);
    expect(again.writeData('data/great', great2..upgrade = sword2, name: 'Great Sword').changed, isFalse);
    again.writeData('data/sword', sword2..upgrade = great2);
    expect(again.writeData('data/sword', sword2).changed, isFalse);
    expect([for (final f in ['sword', 'great']) File('$project/assets/data/$f.data.json').readAsStringSync()], before);
  });

  test('an instance never written, or of no registered type, is refused', () {
    final w = writer();
    final loose = Weapon()..damage = 2;
    expect(() => w.writeScene('scenes/x', SceneSpec('X', [ObjectSpec('A', behaviours: [Wielder()..weapon = loose])])),
        throwsArgumentError);
    expect(() => w.writeScene('scenes/x', SceneSpec('X', [ObjectSpec('A', behaviours: [Wielder()..spares = [loose]])])),
        throwsArgumentError);
    expect(() => w.writeData('data/y', Weapon()..upgrade = loose), throwsArgumentError);
    expect(() => ProjectWriter(project).writeData('data/z', Weapon()), throwsArgumentError, reason: 'not registered');
    expect(File('$project/assets/scenes/x.scene.json').existsSync(), isFalse);
    expect(Directory('$project/assets/data').existsSync(), isFalse);
  });
}
