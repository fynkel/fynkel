// Unity parity: shader assets in the editor (ADR-0031 § 7), driven through
// the whole shell over a real project — Create ▸ Shader, the shader's page,
// Create Material, and the material Inspector's Shader dropdown and fields.
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

import 'harness.dart';

const rusted = 'c0000000-0000-4000-8000-000000000020';
const broken = 'c0000000-0000-4000-8000-000000000901';

void main() {
  Map<String, Object?> read(Editor e, String rel) =>
      jsonDecode(File('${e.dir.path}/assets/$rel').readAsStringSync()) as Map<String, Object?>;

  Future<void> reveal(WidgetTester tester, Finder f) async {
    await Scrollable.ensureVisible(tester.element(f), alignment: 0.5);
    await tester.pumpAndSettle();
  }

  testWidgets('Create ▸ Shader, its page, Create Material, and the Shader dropdown keeping values', (tester) async {
    final e = await Editor.open(tester, tag: 'shader');
    final browser = e.shell.browser!;
    browser.setFolder('materials');
    await tester.pumpAndSettle();

    // Create ▸ New Lit Shader: one file from the template, with its sidecar.
    await e.click(find.byKey(const Key('project.new')));
    await e.click(find.byKey(const Key('project.new.shader.lit')));
    await e.press(LogicalKeyboardKey.escape); // keep the name
    final shaderFile = File('${e.dir.path}/assets/materials/New Lit Shader.shader');
    expect(shaderFile.existsSync(), isTrue);
    expect(File('${shaderFile.path}.meta.json').existsSync(), isTrue);
    final shaderId = browser.selection.single;

    // Its page: what the header says, Compiled, the properties.
    await e.click(find.byKey(Key('project.asset.$shaderId')));
    expect(find.text('Custom/New Lit Shader'), findsWidgets);
    expect(
      find.descendant(of: find.byKey(const Key('shader.status')), matching: find.text('Compiled')),
      findsOneWidget,
    );
    for (final name in ['base_color', 'base_map', 'metallic', 'roughness']) {
      expect(find.byKey(Key('shader.property.$name')), findsOneWidget, reason: name);
    }

    // Create Material: beside it, drawn with it.
    await e.click(find.byKey(const Key('shader.createMaterial')));
    await e.press(LogicalKeyboardKey.escape);
    const materialRel = 'materials/New Lit Shader Material.material.json';
    expect(read(e, materialRel)['shader'], {'asset': shaderId});
    final materialId = browser.selection.single;
    await e.click(find.byKey(Key('project.asset.$materialId')));
    expect(
      find.descendant(of: find.byKey(const Key('material.shader')), matching: find.text('Custom/New Lit Shader')),
      findsOneWidget,
    );

    // A property's field: typed in, saved at once.
    final metallic = find.descendant(
      of: find.byKey(const Key('material.param.metallic')),
      matching: find.byKey(const ValueKey('number.text')),
    );
    await reveal(tester, metallic);
    await e.click(metallic);
    await e.type('0.7');
    await e.submit();
    expect((read(e, materialRel)['parameters'] as Map)['metallic'], 0.7);
    expect(find.byKey(const Key('material.param.base_map')), findsOneWidget, reason: 'a texture field');

    // The dropdown on the built-in Lit: the shared value kept, as its field.
    await reveal(tester, find.byKey(const Key('material.shader.button')));
    await e.click(find.byKey(const Key('material.shader.button')));
    await e.click(find.byKey(const Key('material.shader.fynkel.pbr@1')));
    var saved = read(e, materialRel);
    expect(saved['shader'], 'fynkel.pbr@1');
    expect(saved['metallic'], 0.7);
    expect(saved.containsKey('parameters'), isFalse);
    expect(find.byKey(const Key('material.base_color_texture')), findsOneWidget, reason: 'the Lit fields');

    // And back through the Custom submenu: still 0.7.
    await e.click(find.byKey(const Key('material.shader.button')));
    await e.click(find.text('Custom').last);
    await e.click(find.byKey(Key('material.shader.$shaderId')));
    saved = read(e, materialRel);
    expect(saved['shader'], {'asset': shaderId});
    expect((saved['parameters'] as Map)['metallic'], 0.7);
    expect(saved.containsKey('metallic'), isFalse);

    // ⌘Z in the Inspector undoes the switch.
    await e.press(LogicalKeyboardKey.keyZ, command: true);
    expect(read(e, materialRel)['shader'], 'fynkel.pbr@1');
  });

  testWidgets('a shader that does not compile: its errors with their lines, and its materials say why', (tester) async {
    final e = await Editor.open(
      tester,
      tag: 'shader-broken',
      prepare: (dir) {
        File('${dir.path}/assets/materials/broken.shader').writeAsStringSync(
          'shader "Tests/Broken" {\n    properties {\n        amount float = 1\n    }\n}\n\n'
          'fn surface(in: SurfaceInput) -> Surface {\n    var s = default_surface();\n    s.base_color = material.missing;\n    return s;\n}\n',
        );
        File('${dir.path}/assets/materials/broken.shader.meta.json').writeAsStringSync(
          '{"\$schema":"https://fynkel.dev/schemas/meta.schema.json","formatVersion":2,"id":"$broken",'
          '"name":"broken.shader","importer":"fynkel.shader","importerVersion":1,"settings":{},"subAssets":{}}',
        );
        final rustedFile = File('${dir.path}/assets/materials/rusted.material.json');
        final m = jsonDecode(rustedFile.readAsStringSync()) as Map<String, Object?>;
        m
          ..removeWhere((k, _) => !const {r'$schema', 'formatVersion', 'id', 'name'}.contains(k))
          ..['shader'] = {'asset': broken};
        rustedFile.writeAsStringSync(jsonEncode(m));
      },
    );
    e.shell.browser!.setFolder('materials');
    await tester.pumpAndSettle();

    await e.click(find.byKey(const Key('project.asset.$broken')));
    expect(find.textContaining('1 error'), findsOneWidget);
    expect(find.descendant(of: find.byKey(const Key('shader.error.0')), matching: find.text('line 9')), findsOneWidget);
    expect(find.byKey(const Key('shader.createMaterial')), findsOneWidget);

    await e.click(find.byKey(const Key('project.asset.$rusted')));
    expect(find.byKey(const Key('material.shaderProblem')), findsOneWidget);
    expect(
      find.descendant(of: find.byKey(const Key('material.shader')), matching: find.text('broken.shader (errors)')),
      findsOneWidget,
    );
  });
}
