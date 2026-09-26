// The Shader dropdown's rules (ADR-0031 § 7): same-named values kept when
// they fit, the old shader's own fields removed, the alpha mode allowed.
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:fynkel_editor/src/inspector/asset_edits.dart';
import 'package:fynkel_editor/src/inspector/shader_switch.dart';

ShaderInfo shader(
  String id,
  List<Map<String, Object?>> properties, {
  List<String> alpha = const ['opaque', 'mask', 'blend'],
}) => ShaderInfo.fromJson({'shader': id, 'name': 'Tests/$id', 'ok': true, 'alpha': alpha, 'properties': properties});

final lit = ShaderInfo.fromJson({'shader': pbrShader, 'name': 'Lit', 'builtin': true, 'ok': true});

void main() {
  const texture = {'asset': 'c0000000-0000-4000-8000-000000000002'};
  final water = shader(
    'water',
    [
      {'name': 'baseColor', 'type': 'color3'},
      {'name': 'roughness', 'type': 'float'},
      {'name': 'flow', 'type': 'vec2'},
      {'name': 'base_color', 'type': 'texture'},
    ],
    alpha: ['blend'],
  );

  test('from Lit: shared names carried over, fitted, the Lit fields removed', () {
    final edit = switchShader({
      'shader': pbrShader,
      'base_color': [0.1, 0.2, 0.3, 0.5],
      'roughness': 0.25,
      'metallic': 1,
      'base_color_texture': texture,
    }, water);
    expect(edit['/shader'], {'asset': 'water'});
    expect(edit['/parameters'], {
      'baseColor': [0.1, 0.2, 0.3],
      'roughness': 0.25,
    });
    expect(edit['/textures'], {
      'base_color': {'texture': texture},
    });
    for (final f in ['/base_color', '/roughness', '/metallic', '/base_color_texture']) {
      expect(identical(edit[f], unset), isTrue, reason: f);
    }
    expect(edit['/alpha_mode'], 'blend', reason: 'opaque is not allowed');
  });

  test('back to Lit: the values become its fields, parameters and textures go', () {
    final edit = switchShader({
      'shader': {'asset': 'water'},
      'alpha_mode': 'blend',
      'parameters': {
        'baseColor': [1, 0, 0],
        'flow': [1, 2],
      },
      'textures': {
        'base_color': {'texture': texture},
      },
    }, lit);
    expect(edit['/shader'], pbrShader);
    expect(edit['/base_color'], [1, 0, 0, 1]);
    expect(edit['/base_color_texture'], texture);
    expect(identical(edit['/parameters'], unset), isTrue);
    expect(identical(edit['/textures'], unset), isTrue);
    expect(edit.containsKey('/alpha_mode'), isFalse, reason: 'Lit allows blend');
  });

  test('a value that does not fit the new type is dropped', () {
    final other = shader('other', [
      {'name': 'flow', 'type': 'float'},
      {'name': 'on', 'type': 'bool'},
      {'name': 'count', 'type': 'int'},
    ]);
    final edit = switchShader({
      'shader': {'asset': 'water'},
      'parameters': {
        'flow': [1, 2],
        'on': 1,
        'count': 2.6,
      },
    }, other);
    expect(edit['/parameters'], {'on': true, 'count': 3});
  });

  test('sameName and fitValue', () {
    expect(sameName('base_color'), sameName('baseColor'));
    expect(fitValue([1, 2], 3), isNull);
    expect(fitValue(0.5, 1), 0.5);
    expect(fitValue(true, 1), 1);
  });
}
