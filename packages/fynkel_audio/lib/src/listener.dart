part of 'audio.dart';

/// The ears of the world's 3D sound, as Unity's `AudioListener`: put it on
/// the object that hears (the player, or a camera that isn't the world's).
/// Without one, the world's camera ([World.camera]) hears; without that,
/// whoever hears stands at the origin looking along -Z. With more than one
/// enabled, the last enabled hears.
///
/// ```dart
/// player.addComponent(AudioListener());
/// ```
final class AudioListener extends Behaviour {
  Audio? _audio;

  @override
  void onEnable() {
    super.onEnable();
    final audio = gameObject.audio;
    _audio = audio;
    audio._listeners.add(this);
  }

  @override
  void onDisable() {
    super.onDisable();
    final audio = _audio;
    if (audio != null && !audio._disposed) audio._listeners.remove(this);
    _audio = null;
  }

  @override
  String toString() => 'AudioListener';
}
