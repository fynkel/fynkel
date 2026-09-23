// Component handles (item 9g): a light's range and angles, a collider's
// size, a camera's field of view — each handler's handles, and what a
// drag along them sets.
import 'dart:math' as math;

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/hierarchy/transform_math.dart' show identityMatrix;
import 'package:fynkel_editor/src/scene_view/component_handles.dart';
import 'package:fynkel_editor/src/scene_view/gizmo.dart';
import 'package:fynkel_editor/src/scene_view/scene_math.dart';

ComponentContext contextOf(Map<String, Object?> data, {List<double> world = identityMatrix, EditorCamera? camera}) {
  final cam = camera ?? (EditorCamera.initial()..distance = 30);
  return ComponentContext(world: world, data: data, vp: ViewProjection(cam, 1000, 800), aspect: 1.25);
}

Offset pixel(ComponentContext c, Vec3 p) {
  final r = c.vp.project(p)!;
  return Offset(r[0], r[1]);
}

Handle handleOf(ComponentHandles h, ComponentContext c, String id) => h.handles(c).singleWhere((x) => x.id == id);

/// Drags handle [id] from its grab point to the pixel of the grab point
/// moved by [delta] in the world.
FieldEdit dragBy(ComponentHandles h, ComponentContext c, String id, Vec3 delta) {
  final handle = handleOf(h, c, id);
  final from = pixel(c, handle.grab), to = pixel(c, vAdd(handle.grab, delta));
  final value = handle.constraint.solve(
    DragInput(
      startPx: from,
      px: to,
      startRay: c.vp.rayAt(from.dx, from.dy),
      ray: c.vp.rayAt(to.dx, to.dy),
      pivotPx: pixel(c, c.origin),
      cameraForward: c.vp.camera.forward,
    ),
  );
  return h.edit(handle, value!, c)!;
}

double number(FieldEdit e, String field) => (e.fields[field] as num).toDouble();
List<double> vector(FieldEdit e, String field) => [for (final v in e.fields[field] as List) (v as num).toDouble()];

Matcher near(List<num> v) => pairwiseCompare<num, num>(v, (a, b) => (a - b).abs() < 1e-6, 'within 1e-6');

List<double> translated(Vec3 t, {double scale = 1}) => [scale, 0, 0, 0, 0, scale, 0, 0, 0, 0, scale, 0, t[0], t[1], t[2], 1];

void main() {
  group('the registry', () {
    test('the built-in types, what they support, and a registered one', () {
      for (final type in ['fynkel.PointLight', 'fynkel.SpotLight', 'fynkel.Collider', 'fynkel.Camera']) {
        expect(ComponentHandlers.of(type)?.type, type);
      }
      expect(ComponentHandlers.of('fynkel.DirectionalLight'), isNull, reason: 'nothing to drag');
      expect(ComponentHandlers.supports('fynkel.Collider', {'shape': 'box'}), isTrue);
      expect(ComponentHandlers.supports('fynkel.Collider', {'shape': 'mesh'}), isFalse);
      expect(ComponentHandlers.supports('fynkel.Transform', {}), isFalse);
      expect(() => ComponentHandlers.register(const PointLightHandles()), throwsArgumentError);
      expect(() => ComponentHandlers.unregister('fynkel.Camera'), throwsArgumentError);
      ComponentHandlers.register(const _Radius());
      addTearDown(() => ComponentHandlers.unregister('game.Radius'));
      expect(ComponentHandlers.supports('game.Radius', {}), isTrue);
    });
  });

  group('a point light', () {
    test('a dot at its range on each axis, dragged out and in', () {
      final c = contextOf({'range': 10.0}, world: translated([1, 2, 3]));
      final hs = const PointLightHandles().handles(c);
      expect(hs.length, greaterThanOrEqualTo(4), reason: 'an axis pointing at the camera drops its dots');
      for (final h in hs) {
        expect(vDistance(h.grab, [1, 2, 3]), closeTo(10, 1e-9));
        final hits = hitTestHandles([for (final x in hs) ProjectedHandle(x, c.vp)], pixel(c, h.grab));
        expect(hits?.id, h.id);
      }
      expect(hs.first.shape.guides, hasLength(3), reason: 'the three circles, drawn while dragging');
      expect(number(dragBy(const PointLightHandles(), c, 'range.+x', const [2, 0, 0]), 'range'), closeTo(12, 1e-6));
      final shrunk = dragBy(const PointLightHandles(), c, 'range.+x', const [-15, 0, 0]);
      expect(number(shrunk, 'range'), 0.001, reason: 'never zero: the runtime refuses it');
      expect(shrunk.readout, startsWith('range '));
    });
  });

  group('a spot light', () {
    // Pointing down −Z from the origin, range 10.
    Map<String, Object?> data({double outer = 0.5, double inner = 0.3}) => {
      'range': 10.0,
      'outer_angle': outer,
      'inner_angle': inner,
    };

    test('its range at the cone\'s end, its angles on the two circles', () {
      final c = contextOf(data());
      final hs = const SpotLightHandles().handles(c);
      expect(hs.first.id, 'range');
      expect(hs.first.grab, [0, 0, -10]);
      final outer = handleOf(const SpotLightHandles(), c, 'outer.+x');
      expect(outer.grab[0], closeTo(10 * math.tan(0.5), 1e-9));
      expect(number(dragBy(const SpotLightHandles(), c, 'range', const [0, 0, -3]), 'range'), closeTo(13, 1e-6));
    });

    test('the outer angle widens; narrowed past the inner one it takes it along; the inner stays inside', () {
      final c = contextOf(data());
      final r0 = 10 * math.tan(0.5);
      final wider = dragBy(const SpotLightHandles(), c, 'outer.+x', const [1, 0, 0]);
      expect(number(wider, 'outer_angle'), closeTo(math.atan2(r0 + 1, 10), 1e-6));
      expect(wider.fields.containsKey('inner_angle'), isFalse);
      expect(wider.readout, endsWith('°'));
      final narrow = dragBy(const SpotLightHandles(), c, 'outer.+x', [-(r0 - 1), 0, 0]);
      final angle = number(narrow, 'outer_angle');
      expect(angle, closeTo(math.atan2(1, 10), 1e-6));
      expect(number(narrow, 'inner_angle'), angle);
      final inner = dragBy(const SpotLightHandles(), c, 'inner.+y', const [0, 20, 0]);
      expect(number(inner, 'inner_angle'), 0.5, reason: 'at most the outer angle');
    });
  });

  group('a collider', () {
    test('a box face moves and the opposite one stays, under the entity\'s scale', () {
      final world = translated([0, 1, 0], scale: 2);
      final c = contextOf({
        'shape': 'box',
        'size': [0.5, 0.5, 0.5],
        'offset': [0.0, 0.0, 0.0],
      }, world: world);
      final hs = const ColliderHandles().handles(c);
      expect(hs.map((h) => h.id), containsAll(['size.+x', 'size.-x', 'size.+y']));
      expect(handleOf(const ColliderHandles(), c, 'size.+x').grab, [1, 1, 0]);
      final e = dragBy(const ColliderHandles(), c, 'size.+x', const [1, 0, 0]);
      // One world metre on a ×2 entity: the extent grows by 0.5 locally,
      // the half extent by 0.25, the centre by 0.25.
      expect(vector(e, 'size')[0], closeTo(0.75, 1e-6));
      expect(vector(e, 'size').sublist(1), [0.5, 0.5]);
      expect(vector(e, 'offset')[0], closeTo(0.25, 1e-6));
      final minX = transformPoint(world, [vector(e, 'offset')[0] - vector(e, 'size')[0], 0, 0]);
      expect(minX[0], closeTo(-1, 1e-6), reason: 'the −X face stays where it was');
      expect(e.readout, 'x 3 m');
      final negative = dragBy(const ColliderHandles(), c, 'size.-x', const [-1, 0, 0]);
      expect(vector(negative, 'size')[0], closeTo(0.75, 1e-6));
      expect(vector(negative, 'offset')[0], closeTo(-0.25, 1e-6));
      final flat = dragBy(const ColliderHandles(), c, 'size.+x', const [-5, 0, 0]);
      expect(vector(flat, 'size')[0], 0.001, reason: 'never zero');
    });

    test('a sphere\'s radius, scaled by the entity\'s largest axis', () {
      final c = contextOf({
        'shape': 'sphere',
        'size': [0.5, 0.5, 0.5],
      }, world: translated([0, 0, 0], scale: 2));
      expect(vLength(handleOf(const ColliderHandles(), c, 'radius.+x').grab), closeTo(1, 1e-9));
      final e = dragBy(const ColliderHandles(), c, 'radius.+x', const [1, 0, 0]);
      expect(vector(e, 'size'), near(const [1, 0.5, 0.5]));
    });

    test('a capsule\'s radius and half height about its centre', () {
      final c = contextOf({
        'shape': 'capsule',
        'size': [0.5, 1.0, 0.5],
      });
      expect(handleOf(const ColliderHandles(), c, 'height.+y').grab, [0, 1.5, 0]);
      final taller = dragBy(const ColliderHandles(), c, 'height.+y', const [0, 0.5, 0]);
      expect(vector(taller, 'size'), near(const [0.5, 1.5, 0.5]));
      final wider = dragBy(const ColliderHandles(), c, 'radius.+x', const [0.25, 0, 0]);
      expect(vector(wider, 'size'), near(const [0.75, 1, 0.5]));
      final flat = dragBy(const ColliderHandles(), c, 'height.+y', const [0, -3, 0]);
      expect(vector(flat, 'size')[1], 0, reason: 'a sphere, the runtime takes zero');
    });

    test('a mesh collider has nothing to drag', () {
      expect(const ColliderHandles().supports({'shape': 'mesh'}), isFalse);
      expect(const ColliderHandles().handles(contextOf({'shape': 'mesh'})), isEmpty);
    });
  });

  group('a camera', () {
    // At the origin looking down −Z; the dots at near + 4 m.
    test('its field of view from the frustum\'s top and bottom', () {
      final c = contextOf({'projection': 'perspective', 'fov_y': 1.0, 'near': 0.1, 'far': 100.0});
      final top = handleOf(const CameraHandles(), c, 'fov.+y');
      expect(top.grab[1], closeTo(4.1 * math.tan(0.5), 1e-9));
      expect(top.grab[2], closeTo(-4.1, 1e-9));
      expect(top.shape.guides, hasLength(6), reason: 'near and far rectangles and four edges');
      final wider = dragBy(const CameraHandles(), c, 'fov.+y', const [0, 1, 0]);
      expect(number(wider, 'fov_y'), closeTo(2 * math.atan2(4.1 * math.tan(0.5) + 1, 4.1), 1e-6));
      expect(wider.readout, startsWith('field of view'));
      final shut = dragBy(const CameraHandles(), c, 'fov.-y', const [0, 10, 0]);
      expect(number(shut, 'fov_y'), CameraHandles.minFov, reason: 'the runtime refuses zero');
    });

    test('an orthographic camera\'s height', () {
      final c = contextOf({'projection': 'orthographic', 'ortho_height': 10.0, 'near': 0.1, 'far': 100.0});
      expect(const CameraHandles().handles(c).map((h) => h.id), ['height.+y', 'height.-y']);
      final e = dragBy(const CameraHandles(), c, 'height.+y', const [0, 1, 0]);
      expect(number(e, 'ortho_height'), closeTo(12, 1e-6));
    });
  });
}

/// A game component's radius, as a project would register it.
final class _Radius extends ComponentHandles {
  const _Radius();
  @override
  String get type => 'game.Radius';
  @override
  List<Handle> handles(ComponentContext c) => const [];
  @override
  FieldEdit? edit(Handle handle, DragValue value, ComponentContext c) => FieldEdit({'radius': value.scalar}, '');
}
