// Hardening G8: package:fynkel/testing.dart, what each example's tests
// wrote for themselves.

import 'dart:async';
import 'dart:io';
import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

void main() {
  late World world;
  setUp(() => world = World()..input.simulate.resize(320, 200));
  tearDown(() => world.dispose());

  test('run runs that many seconds of frames, 60 a second unless told', () {
    var frames = 0;
    world.add(_Counter(() => frames++));
    world.run(0.5);
    expect(frames, 30);
    expect(world.time.sinceStart, closeTo(0.5, 1e-9));
    world.run(1, fps: 10);
    expect(frames, 40);
  });

  test('tap taps there on the next frame, a new finger each time', () {
    final taps = <ViewPoint>[];
    world.input.onTap.listen((finger) => taps.add(finger.position));
    world.tap(10, 20);
    world.run(1 / 60);
    world.tap(30, 40);
    world.run(1 / 60);
    expect(taps, [world.viewPoint(10, 20), world.viewPoint(30, 40)]);
    expect(world.viewPoint(10, 20), const ViewPoint(10, 20, width: 320, height: 200));
  });

  test('without a view size, tap and viewPoint say how to give one', () {
    final bare = World();
    addTearDown(bare.dispose);
    expect(() => bare.tap(1, 1), throwsA(isStateError.having((e) => e.message, 'message', contains('resize'))));
    expect(() => bare.viewPoint(1, 1), throwsStateError);
  });
  // Playtest game 8: an onLoad that awaits a routine goes on only in a
  // frame, so `await game.loaded` with none run waited for ever.
  group('untilLoaded', () {
    test('loads a game whose onLoad awaits a routine, running frames as the app would', () async {
      final game = world.add(_Fades());
      await world.untilLoaded(game);
      expect(game.faded, isTrue);
      // The fade's 0.4 s ran as frames, and not many more.
      expect(world.time.sinceStart, inInclusiveRange(0.4, 0.5));
    });

    test('a game that loads without frames is ready as soon as it has, a frame each 5 ms meanwhile', () async {
      final game = world.add(_Plain());
      await world.untilLoaded(game);
      expect(game.child, isNotNull);
      // 20 ms of reading: about 4 frames, as the app would have drawn.
      expect(world.time.frame, lessThan(10));
    });

    test('throws what the load threw, and a TimeoutException for a load that never ends', () async {
      world.errors.listen((_) {}); // the failed load is the world's error too
      await expectLater(world.untilLoaded(world.add(_Broken())), throwsA(isA<StateError>()));
      await expectLater(
        world.untilLoaded(world.add(_Forever()), timeout: const Duration(milliseconds: 200)),
        throwsA(isA<TimeoutException>().having((e) => e.message, 'message', contains('_Forever'))),
      );
    });
  });

  // Stage 14's GT11: the golden check was copied into the lab; a game had none.
  group('expectImage', () {
    late Directory dir;
    setUp(() => dir = Directory.systemTemp.createTempSync('fynkel_image'));
    tearDown(() => dir.deleteSync(recursive: true));

    Uint8List picture(int width, int height, int Function(int x, int y) red) {
      final rgba = Uint8List(width * height * 4);
      for (var y = 0; y < height; y++) {
        for (var x = 0; x < width; x++) {
          rgba
            ..[(y * width + x) * 4] = red(x, y)
            ..[(y * width + x) * 4 + 1] = (x * 7 + y * 3) & 0xFF
            ..[(y * width + x) * 4 + 3] = 255;
        }
      }
      return rgba;
    }

    test('a picture matches itself saved, and small or rare differences pass', () {
      final rgba = picture(40, 30, (x, y) => (x * 5 + y * 11) & 0xFF);
      final path = '${dir.path}/a.png';
      expectImage(path, rgba, width: 40, height: 30, bless: true);
      expectImage(path, rgba, width: 40, height: 30);
      // 2/255 everywhere, and one pixel far off: still the same picture.
      final near = Uint8List.fromList(rgba);
      for (var i = 0; i < near.length; i += 4) {
        near[i + 1] = near[i + 1] < 250 ? near[i + 1] + 2 : near[i + 1] - 2;
      }
      near[0] ^= 0x80;
      expectImage(path, near, width: 40, height: 30);
    });

    test('a different picture fails, saying how much, and leaves it and the difference to look at', () {
      final rgba = picture(40, 30, (x, y) => 100);
      final path = '${dir.path}/b.png';
      expectImage(path, rgba, width: 40, height: 30, bless: true);
      final other = picture(40, 30, (x, y) => x < 20 ? 100 : 140);
      expect(
        () => expectImage(path, other, width: 40, height: 30, failures: '${dir.path}/failures'),
        throwsA(
          isA<ImageDiffers>().having((e) => e.message, 'message', allOf(contains('600 of 1200'), contains('40'))),
        ),
      );
      expect(File('${dir.path}/failures/b.actual.png').existsSync(), isTrue);
      expect(File('${dir.path}/failures/b.diff.png').existsSync(), isTrue);
      expect(
        () => expectImage(path, picture(20, 30, (x, y) => 100), width: 20, height: 30),
        throwsA(isA<ImageDiffers>().having((e) => e.message, 'message', contains('40 × 30'))),
      );
    });

    test('with no picture saved yet, it says how to save one; FYNKEL_BLESS=1 saves it', () {
      final rgba = picture(8, 8, (x, y) => x * 30);
      final path = '${dir.path}/new/c.png';
      expect(
        () => expectImage(path, rgba, width: 8, height: 8),
        throwsA(isA<ImageDiffers>().having((e) => e.message, 'message', contains('FYNKEL_BLESS=1'))),
      );
      expectImage(path, rgba, width: 8, height: 8, bless: true);
      expectImage(path, rgba, width: 8, height: 8);
      expect(File(path).lengthSync(), lessThan(8 * 8 * 4), reason: 'saved compressed');
    });

    test("a view's frame is checked as it draws: expectFrame", () {
      final drawn = World()
        ..sky = null
        ..add(GameObject(components: [Camera(clearColor: const Color.rgb(0.2, 0.4, 0.6))]));
      addTearDown(drawn.dispose);
      final HeadlessView view;
      try {
        view = HeadlessView(drawn, width: 32, height: 16);
      } on FynkelException {
        markTestSkipped('no GPU');
        return;
      }
      addTearDown(view.dispose);
      final path = '${dir.path}/frame.png';
      view.expectFrame(path, bless: true);
      view.expectFrame(path);
      drawn.camera!.clearColor = const Color.rgb(0.9, 0.1, 0.1);
      expect(() => view.expectFrame(path, failures: dir.path), throwsA(isA<ImageDiffers>()));
    });
  });
}

class _Counter extends GameObject {
  _Counter(this.tick);

  final void Function() tick;

  @override
  void onUpdate() {
    super.onUpdate();
    tick();
  }
}

/// Fades in before it's ready, as a game with levels does.
class _Fades extends GameObject {
  var faded = false;

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    await startRoutine(() async {
      await wait(0.4);
      faded = true;
    });
  }
}

/// Reads a file, as it were, and adds what it read.
class _Plain extends GameObject {
  GameObject? child;

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    await Future<void>.delayed(const Duration(milliseconds: 20));
    child = add(GameObject());
  }
}

class _Broken extends GameObject {
  @override
  Future<void> onLoad() async {
    await super.onLoad();
    await Future<void>.delayed(const Duration(milliseconds: 1));
    throw StateError('no level file');
  }
}

class _Forever extends GameObject {
  @override
  Future<void> onLoad() async {
    await super.onLoad();
    await Completer<void>().future;
  }
}
