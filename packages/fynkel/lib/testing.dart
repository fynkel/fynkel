/// For a game's tests: run its world for a while, tap it, and name a point
/// in its view. `HeadlessView` draws it, `view.png()` saves a frame, and
/// `view.expectFrame('test/frames/start.png')` checks one against a saved
/// picture.
///
/// ```dart
/// final world = World()..input.simulate.resize(320, 200);
/// final game = world.add(MyGame());
/// world.run(2);
/// world.tap(160, 100);
/// world.run(0.1);
/// ```
library;

import 'dart:io';
import 'dart:typed_data';

import 'fynkel.dart';
import 'src/png.dart';

/// [run], [tap] and [viewPoint] on a [World], for tests.
extension WorldTesting on World {
  /// Runs [seconds] of frames, [fps] a second: `world.update(1 / fps)`
  /// that many times.
  void run(double seconds, {int fps = 60}) {
    final frames = (seconds * fps).round();
    for (var i = 0; i < frames; i++) {
      update(1 / fps);
    }
  }

  /// A tap at ([x], [y]) in the view's logical pixels, on the next frame:
  /// a finger down and up again, a new one each time. Throws a
  /// [StateError] while the input has no view size.
  void tap(double x, double y) {
    _sized('tap');
    final id = _fingers[this] = (_fingers[this] ?? 1 << 20) + 1;
    input.simulate
      ..fingerDown(id, x, y)
      ..fingerUp(id, x, y);
  }

  /// ([x], [y]) in the view, as the input gives a finger's place: for
  /// `camera.screenPointToRay` and the like. Throws a [StateError] while
  /// the input has no view size.
  ViewPoint viewPoint(double x, double y) {
    _sized('viewPoint');
    return ViewPoint(x, y, width: input.width, height: input.height);
  }

  void _sized(String what) {
    if (input.width > 0 && input.height > 0) return;
    throw StateError('$what needs a view size: give one with world.input.simulate.resize(width, height)');
  }
}

// The last finger id each world's taps used: far above a real pointer's.
final _fingers = Expando<int>('tap fingers');

/// [expectFrame] on a [HeadlessView].
extension HeadlessViewTesting on HeadlessView {
  /// Draws the world as it is now and checks the picture against the PNG
  /// at [path], as [expectImage] does: `view.expectFrame('test/frames/start.png')`.
  void expectFrame(String path, {bool? bless, String failures = 'build/fynkel_failures'}) =>
      expectImage(path, read(frame()).rgba, width: width, height: height, bless: bless, failures: failures);
}

/// Checks [rgba] (RGBA8, rows top to bottom, [width] × [height]) against
/// the PNG at [path], a picture saved before. Two pictures are the same
/// when no channel differs by more than 2/255 (4/255 near black, where a
/// GPU's dither moves it), but for at most 1 pixel in 1000: what passes
/// from one Mac or GPU to another.
///
/// When they differ, it saves the picture and one of the difference
/// (brighter is further) in [failures] as `<name>.actual.png` and
/// `<name>.diff.png`, and throws an [ImageDiffers] saying how much.
///
/// [bless] (or `FYNKEL_BLESS=1` in the environment, as in
/// `FYNKEL_BLESS=1 flutter test`) saves [rgba] at [path] instead: to make
/// the picture the first time, or when a change to it is meant. Look at
/// it before keeping it.
void expectImage(
  String path,
  Uint8List rgba, {
  required int width,
  required int height,
  bool? bless,
  String failures = 'build/fynkel_failures',
}) {
  if (rgba.length != width * height * 4) {
    throw ArgumentError.value(rgba.length, 'rgba', 'is not $width × $height RGBA8 (${width * height * 4} bytes)');
  }
  final file = File(path);
  if (bless ?? Platform.environment['FYNKEL_BLESS'] == '1') {
    file.parent.createSync(recursive: true);
    file.writeAsBytesSync(encodePng(width, height, rgba, compress: true));
    return;
  }
  if (!file.existsSync()) {
    throw ImageDiffers('there is no picture at $path yet: run the test with FYNKEL_BLESS=1 to save this one');
  }
  final saved = decodePng(file.readAsBytesSync());
  if ((saved.width, saved.height) != (width, height)) {
    throw ImageDiffers('$path is ${saved.width} × ${saved.height}, this picture $width × $height');
  }
  final expected = saved.rgba;
  var off = 0, worst = 0;
  final diff = Uint8List(rgba.length);
  for (var i = 0; i < rgba.length; i += 4) {
    var d = 0, over = false;
    for (var c = 0; c < 4; c++) {
      final e = (rgba[i + c] - expected[i + c]).abs();
      if (e > d) d = e;
      if (e > (rgba[i + c] <= 48 && expected[i + c] <= 48 ? 4 : 2)) over = true;
    }
    if (d > worst) worst = d;
    if (over) off++;
    diff
      ..[i] = (d * 16).clamp(0, 255)
      ..[i + 3] = 255;
  }
  if (off * 1000 <= width * height) return;
  final name = file.uri.pathSegments.last.replaceFirst(RegExp(r'\.png$'), '');
  final out = Directory(failures)..createSync(recursive: true);
  File('${out.path}/$name.actual.png').writeAsBytesSync(encodePng(width, height, rgba, compress: true));
  File('${out.path}/$name.diff.png').writeAsBytesSync(encodePng(width, height, diff, compress: true));
  throw ImageDiffers(
    '$path: $off of ${width * height} pixels differ by more than 2/255 (4 near black), the worst by $worst; '
    'this picture and the difference are in ${out.path}',
  );
}

/// What [expectImage] throws: the picture isn't the one saved.
class ImageDiffers implements Exception {
  /// What differs, as [message].
  ImageDiffers(this.message);

  /// How much differs, and where the picture and the difference were saved.
  final String message;

  @override
  String toString() => 'ImageDiffers: $message';
}
