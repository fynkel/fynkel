// The seam between the Inspector's 3D previews and the native preview
// stages (authoring ABI v4, ADR-0030 §3): the session's `PreviewStage`
// behind an interface, so widget tests can stand a fake in.
import 'package:fynkel_authoring/fynkel_authoring.dart';

/// One open preview stage: an asset alone in a small world the viewport
/// draws.
abstract class StageBackend {
  /// The native `FynkelWorld*`; 0 once closed.
  int get worldAddress;
  bool get isClosed;

  /// After the last call that loaded or reported it.
  StageState get state;

  /// A material's stage on another shape (a built-in's name, or
  /// `{asset, sub}` for a mesh). Throws `AuthoringException` when the
  /// mesh does not load; the stage keeps its shape then.
  StageState setShape(Object shape);

  /// Turns the stage's sun like an orbit (radians).
  void turnLight(double yaw, double pitch);

  /// The counts again, after the store changed under it (a recook).
  StageState refresh();

  /// A material document's fields drawn by every world of the session.
  void setMaterial(String material, Map<String, Object?> data);

  /// A sound stage's waveform, playback and playhead.
  SoundPeaks peaks(int columns);
  SoundPlayhead play({double from = 0, bool? loop, bool device = true});
  SoundPlayhead stop();
  SoundPlayhead setLoop(bool loop);
  SoundPlayhead playhead();

  void close();
}

/// [StageBackend] over the session.
final class SessionStage implements StageBackend {
  SessionStage(this.stage);

  final PreviewStage stage;

  @override
  int get worldAddress => stage.worldAddress;
  @override
  bool get isClosed => stage.isClosed;
  @override
  StageState get state => stage.state;
  @override
  StageState setShape(Object shape) => stage.setShape(shape);
  @override
  void turnLight(double yaw, double pitch) => stage.turnLight(yaw, pitch);
  @override
  StageState refresh() => stage.refresh();
  @override
  void setMaterial(String material, Map<String, Object?> data) => stage.setMaterial(material, data);
  @override
  SoundPeaks peaks(int columns) => stage.peaks(columns);
  @override
  SoundPlayhead play({double from = 0, bool? loop, bool device = true}) =>
      stage.play(from: from, loop: loop, device: device);
  @override
  SoundPlayhead stop() => stage.stop();
  @override
  SoundPlayhead setLoop(bool loop) => stage.setLoop(loop);
  @override
  SoundPlayhead playhead() => stage.playhead();
  @override
  void close() => stage.close();
}
