// Input recordings (`*.input.json`, docs/input.md): a host frame's dt and
// raw control events per frame. A game records what its host captured
// (`InputRecorder`), a test replays it into a world frame by frame
// (`InputRecording.replay`) — the same document `fynkel test --headless
// --input` replays in the Rust host.

import 'dart:convert';

import 'input.dart';
import 'world.dart';

/// One recorded host frame.
class RecordedFrame {
  const RecordedFrame(this.dt, [this.events = const []]);

  /// Seconds handed to `beginFrame`.
  final double dt;
  final List<InputEvent> events;

  Map<String, Object?> toJson() => {
        'dt': dt,
        if (events.isNotEmpty) 'events': [for (final e in events) e.toJson()],
      };

  factory RecordedFrame.fromJson(Map<String, Object?> json) {
    final dt = json['dt'];
    if (dt is! num || !dt.isFinite || dt < 0) throw FormatException('a frame needs a finite, non-negative dt', json);
    return RecordedFrame(
      dt.toDouble(),
      [for (final e in (json['events'] as List?) ?? const []) InputEvent.fromJson((e as Map).cast<String, Object?>())],
    );
  }
}

/// A recorded input stream: `{"fynkel": "input-recording", "formatVersion":
/// 1, "frames": [{dt, events: [{device, control, phase, x, y}]}]}`.
class InputRecording {
  const InputRecording(this.frames);

  static const String format = 'input-recording';
  static const int formatVersion = 1;

  final List<RecordedFrame> frames;

  int get eventCount => frames.fold(0, (n, f) => n + f.events.length);

  /// Throws [FormatException] for a document that is not a recording.
  factory InputRecording.fromJson(Map<String, Object?> json) {
    if (json['fynkel'] != format) throw FormatException('`fynkel` is `${json['fynkel']}`, expected `$format`', json);
    if (json['formatVersion'] != formatVersion) {
      throw FormatException('formatVersion ${json['formatVersion']} is not $formatVersion', json);
    }
    final frames = json['frames'];
    if (frames is! List) throw FormatException('`frames` must be an array', json);
    return InputRecording([for (final f in frames) RecordedFrame.fromJson((f as Map).cast<String, Object?>())]);
  }

  factory InputRecording.parse(String text) => InputRecording.fromJson(jsonDecode(text) as Map<String, Object?>);

  Map<String, Object?> toJson() => {
        'fynkel': format,
        'formatVersion': formatVersion,
        'frames': [for (final f in frames) f.toJson()],
      };

  String encode() => const JsonEncoder.withIndent('  ').convert(toJson());

  /// Runs every recorded frame through [World.frame] (its dt, its events)
  /// and returns the plans; [fixedUpdate]/[update] as there. The world's
  /// project must be loaded first — its `inputActions` are the bindings.
  List<FramePlan> replay(World world, {void Function(int tick)? fixedUpdate, void Function()? update}) => [
        for (final f in frames) world.frame(f.dt, input: f.events, fixedUpdate: fixedUpdate, update: update),
      ];
}

/// Accumulates the frames a host captures into an [InputRecording]
/// (`FynkelGameView.recorder`, or by hand around `World.frame`).
class InputRecorder {
  final List<RecordedFrame> _frames = [];

  int get length => _frames.length;

  /// Records one host frame. [events] is copied.
  void frame(double dt, List<InputEvent> events) => _frames.add(RecordedFrame(dt, List.of(events)));

  void clear() => _frames.clear();

  InputRecording get recording => InputRecording(List.unmodifiable(_frames));
}
