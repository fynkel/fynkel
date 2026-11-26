// Footage for the promo film: a world drawn offscreen, frame by frame at
// 60 fps of game time, piped into ffmpeg as an .mp4. Only with
// `FYNKEL_FOOTAGE=<dir>`; without it the footage tests are skipped.

import 'dart:io';
import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

final footage = Platform.environment['FYNKEL_FOOTAGE'];
final skipFootage = footage == null ? 'set FYNKEL_FOOTAGE=<dir> to record the promo footage' : null;

/// Records [seconds] of [world] as `<footage>/<name>.mp4`. Each frame,
/// [before] runs before the update (input, the game's moves) and [after]
/// after it, before the frame is drawn (a camera of the film's own).
/// Both get the clip's time in seconds.
Future<void> record(
  World world,
  String name,
  double seconds, {
  int width = 1920,
  int height = 1080,
  void Function(double t)? before,
  void Function(double t)? after,
}) async {
  final view = HeadlessView(world, width: width, height: height);
  final ffmpeg = await Process.start('ffmpeg', [
    '-y', '-loglevel', 'error', //
    '-f', 'rawvideo', '-pix_fmt', 'rgba', '-s', '${width}x$height', '-r', '60', '-i', '-',
    '-c:v', 'libx264', '-preset', 'slow', '-crf', '12', '-pix_fmt', 'yuv420p',
    '$footage/$name.mp4',
  ]);
  ffmpeg.stderr.listen(stderr.add);
  final frames = (seconds * 60).round();
  for (var i = 0; i < frames; i++) {
    final t = i / 60;
    before?.call(t);
    world.update(1 / 60);
    after?.call(t);
    ffmpeg.stdin.add(view.read(view.frame()).rgba);
    if (i % 20 == 0) await ffmpeg.stdin.flush();
  }
  await ffmpeg.stdin.close();
  final code = await ffmpeg.exitCode;
  view.dispose();
  if (code != 0) throw StateError('ffmpeg failed for $name: $code');
  stdout.writeln('recorded $name.mp4 ($frames frames)');
}

double _ease(double v) => v < 0.5 ? 4 * v * v * v : 1 - math.pow(-2 * v + 2, 3) / 2;

/// A camera move: [camera] circles [target] from [eye], turning [degrees]
/// around it over [seconds], moving in by [push] of the distance (0.2 is
/// a fifth closer) and up by [rise] metres, eased in and out.
void Function(double t) orbit(
  GameObject camera,
  Vec3 eye,
  Vec3 target,
  double seconds, {
  double degrees = 30,
  double push = 0,
  double rise = 0,
  double startDegrees = 0,
}) {
  return (t) {
    final k = _ease((t / seconds).clamp(0.0, 1.0));
    final a = (startDegrees + degrees * (k - 0.5)) * math.pi / 180;
    final d = eye - target;
    final flat = Vec3(d.x * math.cos(a) - d.z * math.sin(a), 0, d.x * math.sin(a) + d.z * math.cos(a));
    final along = Vec3(flat.x, d.y + rise * k, flat.z) * (1 - push * k);
    final p = target + along;
    camera.position = p;
    camera.rotation = Quat.lookRotation(target - p);
  };
}

/// A camera that eases toward where it's asked to be, as an operator's would.
class Operator {
  Operator(this.camera);
  final GameObject camera;
  Vec3? _at, _look;

  void frame(Vec3 at, Vec3 look, {double ease = 0.06}) {
    _at = _at == null ? at : _at! + (at - _at!) * ease;
    _look = _look == null ? look : _look! + (look - _look!) * ease;
    camera.position = _at!;
    camera.rotation = Quat.lookRotation(_look! - _at!);
  }
}

/// 0 before [a], 1 after [b], smooth between.
double smooth(double a, double b, double t) {
  final v = ((t - a) / (b - a)).clamp(0.0, 1.0);
  return v * v * (3 - 2 * v);
}

Vec3 lerp(Vec3 a, Vec3 b, double k) => a + (b - a) * k;
