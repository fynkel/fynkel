/// Sound for Fynkel, as Unity has it (docs/stage-11.md).
///
/// Load a sound with `assets.audio('coin.ogg')` (WAV, OGG Vorbis or MP3),
/// put an [AudioSource] on an object and [AudioSource.play] it, or play a
/// click that belongs to no object with `audio.playOneShot`. A source with
/// a `spatialBlend` of 1 is 3D, heard from where its object is by the
/// [AudioListener] (or the camera); `audio.playAt` plays one at a place. The world's
/// [Audio] holds the master volume, follows the game's pause and the
/// app's, and plays through the device once the world is on screen.
library;

export 'src/audio.dart'
    show
        AssetsAudio,
        Audio,
        AudioClip,
        AudioException,
        AudioListener,
        AudioRolloffMode,
        AudioSession,
        AudioSource,
        AudioStats,
        BehaviourAudio,
        GameObjectAudio,
        WorldAudio;
