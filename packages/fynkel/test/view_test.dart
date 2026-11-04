import 'dart:ffi';
import 'dart:io';

import 'package:ffi/ffi.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/platform.dart';
import 'package:fynkel/src/bindings.g.dart';
import 'package:test/test.dart';

import 'render_test.dart' show stage1;

@Native<Void Function(Pointer<Void>)>(symbol: 'CFRelease')
external void _cfRelease(Pointer<Void> object);

void main() {
  test('a view lets go of the images it draws through, and its next frame makes them again', () async {
    final scene = stage1();
    final world = scene.world;
    addTearDown(world.dispose);
    world.add(
      GameObject(
        transform: Transform(rotation: Quat.lookRotation(Vec3(0.6, -1, -0.5))),
        components: [Light.sun(shadows: true)],
      ),
    );
    scene.cube.get<MeshRenderer>()!.castShadows = true;
    final view = HeadlessView(world, width: 64, height: 64);
    addTearDown(view.dispose);

    final before = view.read(view.frame());
    final held = view.stats.shadowBytes;
    expect(held, greaterThan(0));

    view.releaseImages();
    while (view.stats.shadowBytes != 0) {
      await Future<void>.delayed(const Duration(milliseconds: 1));
    }
    expect(view.stats.toString(), contains('shadowBytes=0 '));

    final after = view.read(view.frame());
    expect(view.stats.shadowBytes, held);
    expect(after.rgba, before.rgba);
  });

  test('a resized headless view draws, and reads back, at the new size', () {
    final scene = stage1();
    final view = HeadlessView(scene.world, width: 32, height: 32);
    addTearDown(view.dispose);
    addTearDown(scene.world.dispose);

    view.read(view.frame());
    view.resize(48, 16);
    expect((view.width, view.height), (48, 16));
    final frame = view.read(view.frame());
    expect(frame.rgba, hasLength(48 * 16 * 4));
    final i = (8 * 48 + 24) * 4;
    expect(frame.rgba[i], greaterThan(frame.rgba[i + 2]), reason: 'the orange cube in the middle');
  });

  test("a view draws from its own camera, and black while it's gone", () {
    final scene = stage1();
    final world = scene.world;
    final view = HeadlessView(world, width: 8, height: 8);
    addTearDown(view.dispose);
    addTearDown(world.dispose);
    List<int> corner() => view.read(view.frame()).rgba.sublist(0, 4);
    final main = corner();

    final red = Camera(clearColor: const Color.rgb(1, 0, 0));
    final behind = GameObject(
      transform: Transform(position: Vec3(0, 0, -4)),
      components: [red],
    );
    // Not in the world yet: black, not the main camera's picture.
    view.camera = red;
    expect(corner(), [0, 0, 0, 255]);
    world.add(behind);
    expect(corner(), [255, 0, 0, 255]);
    expect(world.camera, isNot(red), reason: 'the main camera is still the first');

    view.camera = null;
    expect(corner(), main);
    view.camera = red;
    behind.destroy();
    expect(corner(), [0, 0, 0, 255]);
  });

  test('a camera whose near is not below its far is refused when drawn, naming it', () {
    final scene = stage1();
    final world = scene.world;
    final view = HeadlessView(world, width: 8, height: 8);
    addTearDown(view.dispose);
    addTearDown(world.dispose);
    final camera = world.camera!..gameObject.name = 'eye';

    // Further out in two steps: past far for a moment, which is fine
    // until it draws.
    camera.near = 2000;
    expect(
      view.frame,
      throwsA(isA<ArgumentError>().having((e) => e.message, 'message', allOf(contains("'eye'"), contains('near')))),
    );
    camera.far = 5000;
    expect(view.read(view.frame()).rgba, hasLength(8 * 8 * 4));
  });

  test('stats count the frames a view drew', () {
    final scene = stage1();
    final view = HeadlessView(scene.world, width: 8, height: 8);
    addTearDown(view.dispose);
    addTearDown(scene.world.dispose);

    final before = view.stats;
    for (var i = 0; i < 4; i++) {
      view.read(view.frame());
    }
    final window = view.stats - before;
    expect((window.posted, window.rendered, window.skipped, window.presented), (4, 4, 0, 0));
    view.dispose();
    expect(() => view.stats, throwsStateError);
  });

  test("a view's frames are timed on the GPU, each part on its own, only when asked", () {
    final scene = stage1();
    final view = HeadlessView(scene.world, width: 8, height: 8);
    addTearDown(view.dispose);
    addTearDown(scene.world.dispose);
    addTearDown(() => WorldView.timeGpu = false);

    view.read(view.frame());
    expect(view.stats.gpu, const GpuTimes(), reason: 'nothing timed unless asked');
    WorldView.timeGpu = true;
    final before = view.stats;
    for (var i = 0; i < 30 && (view.stats - before).gpu.frames < 3; i++) {
      view.read(view.frame());
    }
    final gpu = (view.stats - before).gpu;
    if (Platform.isMacOS) expect(gpu.frames, greaterThanOrEqualTo(3), reason: "a Mac's GPU times its frames");
    if (gpu.frames > 0) {
      expect(gpu.scene + gpu.finish, greaterThan(Duration.zero), reason: '$gpu');
      expect(gpu + gpu - gpu, gpu);
      if (Platform.isMacOS) expect(gpu.untimed, lessThan(gpu.frames), reason: "the passes' clocks are read");
      // As the measure tool reads it: key=value pairs, split on spaces.
      for (final pair in (view.stats - before).toString().split(' ')) {
        expect(pair.split('='), hasLength(2), reason: pair);
      }
      expect(
        gpu.total,
        gpu.shadows + gpu.skyLight + gpu.ambientOcclusion + gpu.scene + gpu.bloom + gpu.finish + gpu.effects,
      );
    }
  });

  test('an Apple view draws frames that Flutter could show', () {
    final scene = stage1();
    final view = AppleView(scene.world, width: 64, height: 32);
    addTearDown(view.dispose);
    addTearDown(scene.world.dispose);
    expect(view.address, isNot(0));
    expect(AppleView.acquireAddress, isNot(0));
    expect(AppleView.setFrameReadyAddress, isNot(0));

    // What the Swift plugin does on Flutter's raster thread.
    final surface = malloc<Pointer<Void>>();
    addTearDown(() => malloc.free(surface));
    final pointer = Pointer<FyView>.fromAddress(view.address);
    view.frame();
    final deadline = DateTime.now().add(const Duration(seconds: 5));
    var flags = 0;
    while (flags & FY_APPLE_SHOWN == 0 && DateTime.now().isBefore(deadline)) {
      flags = fy_apple_acquire(pointer, surface);
    }
    expect(flags & FY_APPLE_SHOWN, FY_APPLE_SHOWN, reason: 'the frame finished within 5 s');
    _cfRelease(surface.value);
    expect(view.stats.presented, 1);
  }, skip: Platform.isMacOS || Platform.isIOS ? false : 'Apple only');

  test('an Apple view is refused off Apple', () {
    final world = World();
    addTearDown(world.dispose);
    expect(
      () => AppleView(world, width: 8, height: 8),
      throwsA(isA<FynkelException>().having((e) => e.status, 'status', FynkelStatus.unsupported)),
    );
  }, skip: Platform.isMacOS || Platform.isIOS ? 'not on Apple' : false);

  test('an Android view is refused off Android', () {
    final world = World();
    addTearDown(world.dispose);
    expect(
      () => AndroidSurfaceView(world, width: 8, height: 8),
      throwsA(isA<FynkelException>().having((e) => e.status, 'status', FynkelStatus.unsupported)),
    );
  }, skip: Platform.isAndroid ? 'not on Android' : false);
}
