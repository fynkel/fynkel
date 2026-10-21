part of 'audio.dart';

/// A sound, decoded (Unity's `AudioClip`): from a WAV, OGG Vorbis or MP3
/// file with `assets.audio('coin.ogg')`, or made in code with
/// [AudioClip.fromSamples]. Play it with an [AudioSource] or
/// [Audio.playOneShot]. Short sounds are decoded whole as they load; a
/// minute of stereo takes about 21 MB ([bytes]).
final class AudioClip implements Finalizable {
  AudioClip._(this._clip, this.name) {
    _finalizer.attach(this, _clip.cast(), detach: this);
    final info = calloc<FaClipInfo>();
    try {
      _check(fa_clip_info(_clip, info));
      final i = info.ref;
      rate = i.rate;
      channels = i.channels;
      frames = i.frames;
      length = i.seconds;
      bytes = i.bytes;
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

  /// Decodes a WAV, OGG Vorbis or MP3 file from its [bytes], off the
  /// game's thread. [name] (the file's) helps find the format and goes in
  /// the error. Throws a [FormatException] for bytes that aren't one of
  /// those, or have more than two channels.
  static Future<AudioClip> decode(Uint8List bytes, {String name = ''}) async {
    _library;
    final copy = malloc<Uint8>(bytes.isEmpty ? 1 : bytes.length);
    copy.asTypedList(bytes.length).setAll(0, bytes);
    try {
      final (address, error) = await Isolate.run(() => _decodeAt(copy.address, bytes.length, name));
      if (error != null) throw FormatException(error);
      return AudioClip._(Pointer.fromAddress(address), name);
    } finally {
      malloc.free(copy);
    }
  }

  /// A sound made in code: [samples] from -1 to 1, interleaved in
  /// [channels] (1 or 2), at [rate] frames a second. Throws an
  /// [ArgumentError] for samples that aren't whole frames, or aren't
  /// numbers.
  factory AudioClip.fromSamples(Float32List samples, {int rate = 48000, int channels = 1, String name = ''}) {
    _library;
    final copy = malloc<Float>(samples.isEmpty ? 1 : samples.length);
    final out = calloc<Pointer<FaClip>>();
    try {
      copy.asTypedList(samples.length).setAll(0, samples);
      _check(fa_clip_new(rate, channels, copy, samples.length, out));
      return AudioClip._(out.value, name);
    } finally {
      malloc.free(copy);
      calloc.free(out);
    }
  }

  Pointer<FaClip> _clip;

  /// The file it came from, or what it was named.
  final String name;

  /// Its rate in frames a second (Unity's `frequency`), and its channels,
  /// 1 or 2.
  late final int rate, channels;

  /// How many frames it has (Unity's `samples`).
  late final int frames;

  /// How long it plays, in seconds, at pitch 1.
  late final double length;

  /// The memory its samples take, in bytes.
  late final int bytes;

  /// Whether it was freed: by `assets`, when the last object holding it
  /// went, or by [dispose]. A freed clip can't be played; sounds playing it
  /// play on.
  bool get isDisposed => _clip == nullptr;

  Pointer<FaClip> get _native {
    if (isDisposed) throw StateError('the clip $name was freed');
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
      'AudioClip(${name.isEmpty ? '' : '$name, '}${length.toStringAsFixed(2)} s, $rate Hz × $channels)';
}

/// In a helper isolate: decodes and returns the clip's address, or the
/// error.
(int, String?) _decodeAt(int address, int length, String name) {
  final bytes = Pointer<Uint8>.fromAddress(address);
  final utf8Name = name.toNativeUtf8(allocator: malloc);
  final out = calloc<Pointer<FaClip>>();
  try {
    final status = fa_clip_decode(bytes, length, utf8Name.cast(), utf8Name.length, out);
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
}

/// Sounds from the game's assets.
extension AssetsAudio on Assets {
  /// The sound in [name] (a `.wav`, `.ogg` or `.mp3`), decoded, shared by
  /// everything that asks for it, and freed when the last object that
  /// asked is destroyed. A file that isn't sound throws a
  /// [FormatException].
  Future<AudioClip> audio(String name) => load(name, AudioClip.kind);
}
