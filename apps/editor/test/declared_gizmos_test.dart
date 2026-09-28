// Declared gizmos (item 9h): a field's `gizmo` hint parsed from the
// schema's x-fynkel, what each shape draws in the entity's space or the
// world's, and what a drag of its handle sets.
import 'dart:math' as math;

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/hierarchy/transform_math.dart' show identityMatrix;
import 'package:fynkel_editor/src/inspector/field_spec.dart';
import 'package:fynkel_editor/src/scene_view/component_handles.dart';
import 'package:fynkel_editor/src/scene_view/declared_gizmos.dart';
import 'package:fynkel_editor/src/scene_view/gizmo.dart';
import 'package:fynkel_editor/src/scene_view/scene_math.dart';

/// A behaviour's schema as the generator writes it: every shape once.
Map<String, Object?> schema() {
  Map<String, Object?> number(Map<String, Object?> gizmo, {num? min, Object? def, String type = 'number'}) => {
    'type': type,
    'minimum': ?min,
    'default': ?def,
    'x-fynkel': {'type': type, 'min': ?min, 'gizmo': gizmo},
  };
  Map<String, Object?> vec3(Map<String, Object?> gizmo, {Object? def}) => {
    'type': 'array',
    'items': {'type': 'number'},
    'minItems': 3,
    'maxItems': 3,
    'default': ?def,
    'x-fynkel': {'type': 'vec3', 'gizmo': gizmo},
  };
  return {
    'type': 'object',
    'properties': {
      'reach': number({'shape': 'radius', 'when': 'always'}, min: 0.1, def: 2.0),
      'count': number({'shape': 'radius'}, type: 'integer'),
      'extent': vec3({'shape': 'box'}),
      'spot': vec3({'shape': 'point'}),
      'push': vec3({'shape': 'direction', 'space': 'world', 'color': 0xFFFF0000}),
      'length': number({'shape': 'cone', 'angle': 'spread'}),
      'spread': {
        'type': 'number',
        'maximum': 1.0,
        'x-fynkel': {'type': 'number', 'kind': 'angle', 'max': 1.0},
      },
      'target': {
        'type': 'string',
        'x-fynkel': {
          'type': 'string',
          'kind': 'entity',
          'gizmo': {'shape': 'lineTo'},
        },
      },
      'waypoints': {
        'type': 'array',
        'items': {
          'type': 'array',
          'items': {'type': 'number'},
          'minItems': 3,
          'maxItems': 3,
        },
        'x-fynkel': {
          'type': 'vec3List',
          'gizmo': {'shape': 'path'},
        },
      },
      'plain': {'type': 'number'},
      'odd': {
        'type': 'number',
        'x-fynkel': {
          'type': 'number',
          'gizmo': {'shape': 'spiral'},
        },
      },
    },
  };
}

final spec = parseComponentSpec('game.Every', schema());
final gizmos = DeclaredGizmos.of(spec)!;

ComponentContext contextOf(Map<String, Object?> data, {List<double> world = identityMatrix}) {
  final cam = EditorCamera.initial()..distance = 30;
  return ComponentContext(world: world, data: data, vp: ViewProjection(cam, 1000, 800), aspect: 1.25);
}

List<double> placed(Vec3 t, {double scale = 1}) => [scale, 0, 0, 0, 0, scale, 0, 0, 0, 0, scale, 0, t[0], t[1], t[2], 1];

/// Turned a quarter about +Y (local +X points to world −Z).
List<double> turned(Vec3 t) => [0, 0, -1, 0, 0, 1, 0, 0, 1, 0, 0, 0, t[0], t[1], t[2], 1];

Offset pixel(ComponentContext c, Vec3 p) {
  final r = c.vp.project(p)!;
  return Offset(r[0], r[1]);
}

Handle handleOf(ComponentContext c, String id) => gizmos.handles(c).singleWhere((h) => h.id == id);

FieldEdit dragBy(ComponentContext c, String id, Vec3 delta) {
  final h = handleOf(c, id);
  final from = pixel(c, h.grab), to = pixel(c, vAdd(h.grab, delta));
  final v = h.constraint.solve(
    DragInput(
      startPx: from,
      px: to,
      startRay: c.vp.rayAt(from.dx, from.dy),
      ray: c.vp.rayAt(to.dx, to.dy),
      pivotPx: pixel(c, c.origin),
      cameraForward: c.vp.camera.forward,
    ),
  );
  return gizmos.edit(h, v!, c)!;
}

Matcher near(List<num> v, [double tolerance = 1e-6]) =>
    pairwiseCompare<num, num>(v, (a, b) => (a - b).abs() < tolerance, 'within $tolerance');

List<double> vector(FieldEdit e, String field) => [for (final v in e.fields[field] as List) (v as num).toDouble()];

void main() {
  test('the hints parse; an unknown shape or a field without one has none', () {
    expect(gizmos.type, 'game.Every');
    expect(gizmos.fields.map((f) => f.name), [
      'reach', 'count', 'extent', 'spot', 'push', 'length', 'target', 'waypoints',
    ]);
    final reach = spec.field('reach')!.gizmo!;
    expect((reach.shape, reach.always, reach.world, reach.color), ('radius', true, false, null));
    final push = spec.field('push')!.gizmo!;
    expect((push.world, push.color), (true, 0xFFFF0000));
    expect(spec.field('length')!.gizmo!.angle, 'spread');
    expect(spec.field('target')!.gizmo!.draggable, isFalse);
    expect(spec.field('plain')!.gizmo, isNull);
    expect(spec.field('odd')!.gizmo, isNull);
    expect(spec.field('waypoints')!.kind, FieldKind.array);
    expect(spec.field('waypoints')!.item!.kind, FieldKind.vec3);
    expect(gizmos.anyAlways, isTrue);
    expect(gizmos.supports(const {}), isTrue);
    expect(DeclaredGizmos.of(parseComponentSpec('game.None', {'type': 'object', 'properties': {}})), isNull);
    final onlyLine = DeclaredGizmos(gizmos.type, [spec.field('target')!]);
    expect(onlyLine.supports(const {}), isFalse, reason: 'a lineTo alone has nothing to drag');
  });

  test('unselected, only the always-on ones draw; a lineTo reaches its target by name', () {
    final c = contextOf({'target': 'Gate'}, world: placed([1, 0, 0]));
    expect(gizmos.lines(c, selected: false).length, 3, reason: 'the reach sphere\'s three circles');
    final all = gizmos.lines(c, selected: true, find: (name) => name == 'Gate' ? const [5, 0, 0] : null);
    final line = all.where((l) => l.points.length == 2 && vDistance(l.points.last, const [5, 0, 0]) < 1e-9);
    expect(line.single.points.first, [1, 0, 0]);
    expect(gizmos.lines(contextOf({'target': 'Nobody'}), selected: true, find: (_) => null).length, lessThan(all.length));
  });

  test('a lineTo reaches an object reference by its entity id (ADR-0035)', () {
    final c = contextOf({
      'target': {'entity': 'e1'},
    }, world: placed([1, 0, 0]));
    final all = gizmos.lines(c, selected: true, findEntity: (id) => id == 'e1' ? const [0, 0, 7] : null);
    final line = all.where((l) => l.points.length == 2 && vDistance(l.points.last, const [0, 0, 7]) < 1e-9);
    expect(line.single.points.first, [1, 0, 0]);
    final none = gizmos.lines(contextOf({'target': null}), selected: true, findEntity: (_) => const [0, 0, 7]);
    expect(none.length, lessThan(all.length), reason: 'None draws no line');
  });

  test('a radius: the schema default when the data has none, scaled with the entity, clamped to its minimum', () {
    final c = contextOf(const {}, world: placed([0, 0, 0], scale: 2));
    final h = handleOf(c, 'reach.+x');
    expect(h.grab, near(const [4, 0, 0]), reason: 'default 2 m on a ×2 entity');
    final e = dragBy(c, 'reach.+x', const [2, 0, 0]);
    expect(e.fields['reach'] as num, closeTo(3, 1e-6));
    expect(e.readout, startsWith('Reach 3'));
    expect(dragBy(c, 'reach.+x', const [-10, 0, 0]).fields['reach'], 0.1, reason: 'the field\'s min');
    final count = dragBy(contextOf({'count': 3}), 'count.+x', const [1.4, 0, 0]);
    expect(count.fields['count'], 4, reason: 'an integer field stays whole');
  });

  test('a box grows about its centre along the turned entity\'s axis', () {
    final c = contextOf({
      'extent': [2.0, 1.0, 1.0],
    }, world: turned([0, 0, 0]));
    // Local +X is world −Z: the +x face sits at z = −1.
    expect(handleOf(c, 'extent.+x').grab, near(const [0, 0, -1]));
    final e = dragBy(c, 'extent.+x', const [0, 0, -0.5]);
    expect(vector(e, 'extent'), near(const [3, 1, 1]), reason: 'both faces move: the size grows by twice the drag');
  });

  test('a point moves in the view plane and is written back in the entity\'s space', () {
    final c = contextOf({
      'spot': [1.0, 0.0, 0.0],
    }, world: placed([10, 0, 0], scale: 2));
    expect(handleOf(c, 'spot.at').grab, near(const [12, 0, 0]));
    // Up the screen by 2 m (the view plane's up): 1 m in the ×2 entity.
    final up = c.camera[1];
    final e = dragBy(c, 'spot.at', vScale(up, 2));
    expect(vector(e, 'spot'), near(vAdd(const [1, 0, 0], up), 1e-4));
  });

  test('a world-space direction ignores the entity\'s turn: its tip is dragged', () {
    final c = contextOf({
      'push': [0.0, 0.0, -3.0],
    }, world: turned([1, 0, 0]));
    expect(handleOf(c, 'push.tip').grab, near(const [1, 0, -3]));
    final up = c.camera[1];
    final e = dragBy(c, 'push.tip', up);
    expect(vector(e, 'push'), near(vAdd(const [0, 0, -3], up), 1e-4));
    expect(e.readout, startsWith('Push ('));
  });

  test('a cone: its length along −Z and its angle from the rim, within the angle field\'s bounds', () {
    final c = contextOf({'length': 4.0, 'spread': 0.3});
    expect(handleOf(c, 'length.length').grab, near(const [0, 0, -4]));
    expect(dragBy(c, 'length.length', const [0, 0, -1]).fields['length'] as num, closeTo(5, 1e-6));
    final r0 = 4 * math.tan(0.3);
    expect(handleOf(c, 'length.angle.+x').grab, near([r0, 0, -4]));
    final wider = dragBy(c, 'length.angle.+x', const [0.5, 0, 0]);
    expect(wider.fields['spread'] as num, closeTo(math.atan2(r0 + 0.5, 4), 1e-6));
    expect(wider.readout, endsWith('°'));
    expect(dragBy(c, 'length.angle.+y', const [0, 50, 0]).fields['spread'], 1.0, reason: 'the angle field\'s max');
  });

  test('a path: a dot per point, each dragged on its own', () {
    final c = contextOf({
      'waypoints': [
        [0.0, 0.0, 0.0],
        [2.0, 0.0, 0.0],
      ],
    }, world: placed([0, 1, 0]));
    final hs = gizmos.handles(c).where((h) => h.id.startsWith('waypoints.')).toList();
    expect(hs.map((h) => h.id), ['waypoints.0', 'waypoints.1']);
    expect(hs[1].grab, near(const [2, 1, 0]));
    final up = c.camera[1];
    final e = dragBy(c, 'waypoints.1', up);
    final points = e.fields['waypoints'] as List;
    expect(points[0], [0, 0, 0]);
    expect([for (final v in points[1] as List) v as num], near(vAdd(const [2, 0, 0], up), 1e-4));
  });
}
