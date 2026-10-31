part of 'audio.dart';

/// A sound (Unity's `AudioClip`): from a WAV, OGG Vorbis or MP3 file with
/// `assets.audio('coin.ogg')`, or made in code with
/// [AudioClip.fromSamples]. Play it with an [AudioSource], [Audio.music] or
/// [Audio.playOneShot].
///
/// Short sounds are decoded whole as they load: a minute of stereo would
/// take about 21 MB ([bytes]). One longer than 10 seconds (music, say) is
/// [streamed] instead: kept as its file, and decoded as it plays, a second
/// ahead, off the game's thread.
///
/// On a hot reload, a clip from `assets` whose file changed takes the new
/// sound, from the next time it's played.
final class AudioClip implements Finalizable {
  AudioClip._(this._clip, this.name, this._streaming) {
    _finalizer.attach(this, _clip.cast(), detach: this);
    _readInfo();
  }

  void _readInfo() {
    final info = calloc<FaClipInfo>();
    try {
      _check(fa_clip_info(_clip, info));
      final i = info.ref;
      _rate = i.rate;
      _channels = i.channels;
      _frames = i.frames;
      _length = i.seconds;
      _bytes = i.bytes;
      _streamed = i.streamed == 1;
    } finally {
      calloc.free(info);
    }
  }

  static final _finalizer = NativeFinalizer(
    Native.addressOf<NativeFunction<Void Function(Pointer<FaClip>)>>(fa_clip_free).cast(),
  );

  /// The kind `assets.load` loads clips as: `.wav`, `.ogg` and `.mp3`
  /// files. `assets.audio(name)` is the short way.
  static const AssetKind<AudioClip> kind = _ClipKind();

  /// Loads a WAV, OGG Vorbis or MP3 file from its [bytes], off the game's
  /// thread: decoded whole, or, with [stream] (or, unless it's false, when
  /// it's longer than 10 seconds), kept to be decoded as it plays. [name]
  /// (the file's) helps find the format and goes in the error. Throws a
  /// [FormatException] for bytes that aren't one of those, or have more
  /// than two channels.
  static Future<AudioClip> decode(Uint8List bytes, {String name = '', bool? stream}) async {
    _library;
    final streaming = switch (stream) {
      null => FaStreaming.FA_STREAMING_AUTO,
      true => FaStreaming.FA_STREAMING_ALWAYS,
      false => FaStreaming.FA_STREAMING_NEVER,
    };
    final clip = AudioClip._(await _load(bytes, name, streaming), name, streaming);
    assert(() {
      clip._hash = _hashOf(bytes);
      return true;
    }());
    return clip;
  }

  /// The clip [bytes] makes, decoded on a helper isolate.
  static Future<Pointer<FaClip>> _load(Uint8List bytes, String name, int streaming) async {
    final copy = malloc<Uint8>(bytes.isEmpty ? 1 : bytes.length);
    copy.asTypedList(bytes.length).setAll(0, bytes);
    try {
      final (address, error) = await Isolate.run(() => _decodeAt(copy.address, bytes.length, name, streaming));
      if (error != null) throw FormatException(error);
      return Pointer.fromAddress(address);
    } finally {
      malloc.free(copy);
    }
  }

  /// A sound made in code: [samples] from -1 to 1, interleaved in
  /// [channels] (1 or 2), at [rate] frames a second. Throws an
  /// [ArgumentError] for a rate that isn't above 0, or samples that aren't
  /// whole frames, or aren't numbers; one past 8 times full scale is taken
  /// as that.
  factory AudioClip.fromSamples(Float32List samples, {int rate = 48000, int channels = 1, String name = ''}) {
    if (rate <= 0) throw ArgumentError.value(rate, 'rate', 'is not above 0 (frames a second, such as 48000)');
    if (channels != 1 && channels != 2) throw ArgumentError.value(channels, 'channels', 'is not 1 or 2');
    _library;
    final copy = malloc<Float>(samples.isEmpty ? 1 : samples.length);
    final out = calloc<Pointer<FaClip>>();
    try {
      copy.asTypedList(samples.length).setAll(0, samples);
      _check(fa_clip_new(rate, channels, copy, samples.length, out));
      return AudioClip._(out.value, name, FaStreaming.FA_STREAMING_NEVER);
    } finally {
      malloc.free(copy);
      calloc.free(out);
    }
  }

  Pointer<FaClip> _clip;
  final int _streaming;

  /// In debug builds, a hash of the file it was loaded from, so a hot
  /// reload decodes only those that changed.
  int? _hash;

  /// The file it came from, or what it was named.
  final String name;

  /// Its rate in frames a second (Unity's `frequency`).
  int get rate => _rate;
  late int _rate;

  /// Its channels, 1 or 2.
  int get channels => _channels;
  late int _channels;

  /// How many frames it has (Unity's `samples`).
  int get frames => _frames;
  late int _frames;

  /// How long it plays, in seconds, at pitch 1.
  double get length => _length;
  late double _length;

  /// The memory it takes, in bytes: its samples, or, [streamed], its file.
  int get bytes => _bytes;
  late int _bytes;

  /// Whether it's decoded as it plays (Unity's *Streaming* load type),
  /// rather than whole as it loaded: a clip longer than 10 seconds, unless
  /// it was loaded with `stream: false`.
  bool get streamed => _streamed;
  late bool _streamed;

  /// Takes the sound in [bytes] in place of its own, from the next time
  /// it's played (sounds playing it now play on): a hot reload's.
  Future<void> _reload(Uint8List bytes) async {
    final hash = _hashOf(bytes);
    if (hash == _hash) return;
    final loaded = await _load(bytes, name, _streaming);
    if (isDisposed) {
      fa_clip_free(loaded);
      return;
    }
    _finalizer.detach(this);
    fa_clip_free(_clip);
    _clip = loaded;
    _finalizer.attach(this, _clip.cast(), detach: this);
    _hash = hash;
    _readInfo();
  }

  /// Whether it was freed: by `assets`, when the last object holding it
  /// went, or by [dispose]. A freed clip can't be played; sounds playing it
  /// play on.
  bool get isDisposed => _clip == nullptr;

  Pointer<FaClip> get _native {
    if (isDisposed) {
      throw StateError(
        '$this was freed (by dispose(), or by assets when the last object that loaded it was destroyed): '
        'load it again, or load it from an object that stays',
      );
    }
    return _clip;
  }

  /// Frees it now rather than when nothing reaches it. Sounds playing it
  /// play to their end. Not for a clip from `assets`, which frees its own.
  void dispose() {
    if (isDisposed) return;
    _finalizer.detach(this);
    fa_clip_free(_clip);
    _clip = nullptr;
  }

  @override
  String toString() =>
      'AudioClip(${name.isEmpty ? '' : '$name, '}${length.toStringAsFixed(2)} s, $rate Hz × $channels'
      '${streamed ? ', streamed' : ''})';
}

/// FNV-1a over [bytes], to tell a changed file from the same one.
int _hashOf(Uint8List bytes) {
  var hash = 0xcbf29ce484222325;
  for (final byte in bytes) {
    hash = (hash ^ byte) * 0x100000001b3;
  }
  return hash ^ bytes.length;
}

/// In a helper isolate: decodes and returns the clip's address, or the
/// error.
(int, String?) _decodeAt(int address, int length, String name, int streaming) {
  final bytes = Pointer<Uint8>.fromAddress(address);
  final utf8Name = name.toNativeUtf8(allocator: malloc);
  final out = calloc<Pointer<FaClip>>();
  try {
    final status = fa_clip_decode(bytes, length, utf8Name.cast(), utf8Name.length, streaming, out);
    if (status == FaStatus.FA_STATUS_CANT_DECODE) return (0, _lastError());
    _check(status);
    return (out.value.address, null);
  } finally {
    malloc.free(utf8Name);
    calloc.free(out);
  }
}

final class _ClipKind extends AssetKind<AudioClip> {
  const _ClipKind();

  @override
  String get name => 'sound';

  @override
  List<String> get extensions => const ['.wav', '.ogg', '.mp3'];

  @override
  Future<AudioClip> load(World world, String name, Uint8List bytes) => AudioClip.decode(bytes, name: name);

  @override
  void free(World world, AudioClip value) => value.dispose();

  @override
  bool get reloads => true;

  @override
  Future<void> reload(World world, String name, AudioClip value, Uint8List bytes) => value._reload(bytes);
}

/// Sounds from the game's assets.
extension AssetsAudio on Assets {
  /// The sound in [name] (a `.wav`, `.ogg` or `.mp3`), shared by
  /// everything that asks for it, and freed when the last object that
  /// asked is destroyed: decoded whole, or [AudioClip.streamed] when it's
  /// longer than 10 seconds. A file that isn't sound throws a
  /// [FormatException].
  Future<AudioClip> audio(String name) => load(name, AudioClip.kind);
}
