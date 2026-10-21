/// Sound for Fynkel, as Unity has it (docs/stage-11.md).
///
/// Load a sound with `assets.audio('coin.ogg')` (WAV, OGG Vorbis or MP3),
/// put an [AudioSource] on an object and [AudioSource.play] it, or play a
/// click that belongs to no object with `audio.playOneShot`. The world's
/// [Audio] holds the master volume, follows the game's pause and the
/// app's, and plays through the device once the world is on screen.
library;

export 'src/audio.dart'
    show
        AssetsAudio,
        Audio,
        AudioClip,
        AudioException,
        AudioSession,
        AudioSource,
        AudioStats,
        BehaviourAudio,
        GameObjectAudio,
        WorldAudio;
