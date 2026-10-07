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
}
