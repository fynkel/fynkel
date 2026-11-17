import 'dart:async';
import 'dart:ffi';

import 'package:ffi/ffi.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/src/bindings.g.dart';
import 'package:test/test.dart';

/// A square glowing [glow] times white, a fifth of a 64 × 64 view across,
/// on black, seen at [quality] by a camera with [bloom].
({World world, HeadlessView view, Camera camera}) glowing(double glow, Quality quality) {
  final world = World()
    ..quality = quality
    ..sky = null;
  final camera = Camera(clearColor: Color.black);
  world.add(
    GameObject(
      transform: Transform(position: Vec3(0, 0, 5)),
      components: [camera],
    ),
  );
  world.add(
    GameObject(
      transform: Transform(rotation: Quat.axisAngle(Vec3.right, 90.deg), scale: Vec3.all(1.2)),
      components: [
        MeshRenderer(Mesh.plane(), Material(color: Color.black, emissive: Color.white, emissiveIntensity: glow)),
      ],
    ),
  );
  final view = HeadlessView(world, width: 64, height: 64);
  addTearDown(view.dispose);
  addTearDown(world.dispose);
  return (world: world, view: view, camera: camera);
}

/// The red of the pixel 3 to the left of the square, in the middle row.
int beside(HeadlessView view) => view.read(view.frame()).rgba[(32 * 64 + 23) * 4];

/// What [body] prints.
List<String> printed(void Function() body) {
  final lines = <String>[];
  runZoned(body, zoneSpecification: ZoneSpecification(print: (_, _, _, line) => lines.add(line)));
  return lines;
}

void main() {
  test('a camera has no bloom or ambient occlusion unless given them', () {
    final camera = Camera();
    expect((camera.bloom, camera.ambientOcclusion), (null, null));
    final glowing = Camera(bloom: const Bloom(), ambientOcclusion: const AmbientOcclusion());
    expect(glowing.bloom, const Bloom(strength: 1, threshold: 1));
    expect(glowing.ambientOcclusion, const AmbientOcclusion(intensity: 1, radius: 0.5));
    expect(const Bloom(strength: 0.5).toString(), 'Bloom(strength: 0.5, threshold: 1.0)');
    expect(const AmbientOcclusion(radius: 2).toString(), 'AmbientOcclusion(intensity: 1.0, radius: 2.0)');
    expect(const Bloom(strength: 2).copyWith(threshold: 3), const Bloom(strength: 2, threshold: 3));
  });

  test('light brighter than white glows when the camera asks and the quality draws bloom', () {
    for (final quality in [Quality.medium, Quality.high]) {
      final (:view, :camera, world: _) = glowing(8, quality);
      expect(beside(view), 0, reason: 'none until the camera asks ($quality)');
      camera.bloom = const Bloom();
      expect(beside(view), greaterThan(20), reason: '$quality');
      camera.bloom = const Bloom(strength: 3);
      expect(beside(view), greaterThan(40), reason: 'more with more strength ($quality)');
    }
    final (:view, :camera, world: _) = glowing(8, Quality.low);
    camera.bloom = const Bloom();
    expect(beside(view), 0, reason: 'low draws none');
  });

  test("a game's own quality decides: bloom turned on at low glows, turned off at high doesn't", () {
    final (:view, :camera, :world) = glowing(8, Quality.low.copyWith(name: 'mine', bloom: true));
    camera.bloom = const Bloom();
    expect(beside(view), greaterThan(20));
    world.quality = Quality.high.copyWith(bloom: false);
    expect(beside(view), 0);
  });

  test('what the engine refuses throws and changes nothing', () {
    final (:view, :camera, :world) = glowing(8, Quality.high);
    camera.bloom = const Bloom();
    expect(() => camera.bloom = const Bloom(strength: -1), throwsArgumentError);
    expect(camera.bloom, const Bloom());
    expect(() => camera.ambientOcclusion = const AmbientOcclusion(radius: double.nan), throwsArgumentError);
    expect(camera.ambientOcclusion, isNull);
    for (final bad in [
      Quality.high.copyWith(shadowCascades: 0),
      Quality.high.copyWith(shadowResolution: 1000),
      Quality.high.copyWith(spotShadows: 5),
      Quality.high.copyWith(shadowDistance: 0),
      Quality.high.copyWith(renderScale: 0),
      Quality.high.copyWith(renderScale: 2.5),
      Quality.high.copyWith(antiAliasing: 1),
      Quality.high.copyWith(antiAliasing: 8),
    ]) {
      expect(() => world.quality = bad, throwsArgumentError, reason: '$bad');
      expect(world.quality, Quality.high);
    }
    world.quality = Quality.high.copyWith(shadows: false, shadowCascades: 0);
    expect(beside(view), greaterThan(20), reason: 'still glowing');
    for (final antiAliasing in [0, 2, 4]) {
      world.quality = Quality.high.copyWith(antiAliasing: antiAliasing);
      expect(beside(view), greaterThan(20), reason: 'glowing with antiAliasing $antiAliasing');
    }
  });

  test('the levels are what the engine has, and copies compare by value', () {
    expect(Quality.levels.map((q) => q.name), ['low', 'medium', 'high']);
    expect(const Quality(name: 'medium'), Quality.medium, reason: 'the default is medium');
    expect(Quality.low.copyWith(), Quality.low);
    expect(Quality.low.copyWith(bloom: true), isNot(Quality.low));
    expect(Quality.low.copyWith(renderScale: 1), isNot(Quality.low));
    expect(Quality.levels.map((q) => (q.renderScale, q.antiAliasing)), [(0.75, 4), (1, 4), (1, 4)]);
    expect(Quality.low.copyWith(bloom: true).hashCode, Quality.low.copyWith(bloom: true).hashCode);
    expect(
      (Quality.low.ambientOcclusion, Quality.medium.ambientOcclusion, Quality.high.ambientOcclusion),
      (false, false, true),
    );
    expect(Quality.levels, contains(Quality.device));
    // A phone's shadows sharp enough to read: one 1024 map spread over 40 m
    // gave walls saw-toothed shadows (playtest game 7b).
    expect((Quality.low.shadowDistance, Quality.low.shadowResolution), (20, 1024));
    expect(Quality.low.shadowSoftness, ShadowSoftness.soft);
    // Written twice, here and in the renderer: the same.
    for (final (i, level) in Quality.levels.indexed) {
      using((arena) {
        final q = arena<FyQuality>();
        expect(fy_quality_preset(FY_QUALITY_LOW + i, q), FyStatus.FY_STATUS_OK);
        final e = q.ref;
        final engine = Quality(
          name: level.name,
          hdr: e.hdr,
          bloom: e.bloom,
          ambientOcclusion: e.ambient_occlusion,
          lampHighlights: e.lamp_highlights,
          shadows: e.shadows,
          shadowCascades: e.shadow_cascades,
          shadowResolution: e.shadow_resolution,
          shadowSoftness: ShadowSoftness.values[e.shadow_softness],
          shadowDistance: e.shadow_distance,
          spotShadows: e.spot_shadows,
          spotShadowResolution: e.spot_shadow_resolution,
          antiAliasing: e.samples == 1 ? 0 : e.samples,
          softParticles: e.soft_particles,
          // The views' to draw at, not the engine's.
          renderScale: level.renderScale,
        );
        expect(engine, level);
      });
    }
    expect(Quality.low.toString(), contains('low'));
  });

  test('a world nothing draws says nothing about effects it would leave out (a test at low stays quiet)', () {
    final world = World()..quality = Quality.low.copyWith(name: 'quiet');
    addTearDown(world.dispose);
    var lines = printed(() => world.add(GameObject(components: [Camera(bloom: const Bloom())])));
    expect(lines, isEmpty, reason: 'no view draws it');
    lines = printed(() => addTearDown(HeadlessView(world, width: 8, height: 8).dispose));
    expect(lines, hasLength(1), reason: 'once a view draws it, it says');
  });

  test('a debug build says once when the quality drawn leaves out what a camera asks for', () {
    final (:view, :camera, :world) = glowing(8, Quality.low.copyWith(name: 'cheap'));
    var lines = printed(() {
      camera.bloom = const Bloom();
      camera.bloom = const Bloom(strength: 2);
      camera.ambientOcclusion = const AmbientOcclusion();
    });
    expect(lines, hasLength(2));
    expect(lines[0], contains('asks for bloom, but the quality drawn (cheap)'));
    expect(lines[1], contains('ambientOcclusion: true'));
    lines = printed(() => world.quality = Quality.medium.copyWith(bloom: false));
    expect(lines, isEmpty, reason: 'once each');
    view.frame();

    // The device's level, once a view draws the world.
    final other = World();
    addTearDown(other.dispose);
    final quiet = Camera(bloom: const Bloom(), ambientOcclusion: const AmbientOcclusion());
    lines = printed(() => other.add(GameObject(components: [quiet])));
    expect(lines, isEmpty, reason: 'nothing draws it yet');
    lines = printed(() => addTearDown(HeadlessView(other, width: 8, height: 8).dispose));
    expect(lines, Quality.device.ambientOcclusion ? isEmpty : hasLength(1));
    lines = printed(() => other.quality = Quality.high);
    expect(lines, isEmpty, reason: 'high draws both');
  });

  test('a quality that leaves an effect out says so once in a run, not once a world (a test file at low)', () {
    final quality = Quality.low.copyWith(name: 'each test');
    final lines = printed(() {
      for (var i = 0; i < 3; i++) {
        final (:view, :camera, :world) = glowing(8, quality);
        camera.bloom = const Bloom();
        view.dispose();
        world.dispose();
      }
    });
    expect(lines, hasLength(1), reason: '$lines');
  });
}
