part of 'world.dart';

/// What's behind everything in a world ([World.sky]), and the light it
/// gives. A [GradientSky] or an [ImageSky] lights the scene: a shiny
/// surface mirrors it and a rough one takes its colour, the way it's lit
/// from above and below. A [ColorSky] adds its colour to the ambient
/// light.
sealed class Sky {
  const Sky._(this.lighting);

  /// One colour, seen where nothing is drawn and lighting as ambient
  /// light does.
  factory Sky.color(Color color, {double lighting}) = ColorSky;

  /// [zenith] straight up, [horizon] level, [ground] (the horizon's
  /// unless given) straight down, shown as picked: the theme's colours
  /// come out as they are.
  factory Sky.gradient({required Color zenith, required Color horizon, Color? ground, double lighting}) = GradientSky;

  /// An equirectangular [SkyImage]: its middle ahead (−Z), straight up its
  /// top row. Its light times [intensity], tone mapped as the scene is,
  /// turned [rotation] radians to the right.
  factory Sky.image(SkyImage image, {double intensity, double rotation, double lighting}) = ImageSky;

  /// The sky a new [World] has: a clear day's, deep blue overhead, pale at
  /// the horizon and a soft grey below it, lighting what the sun misses as
  /// a new scene's sky does in Unity. `world.sky = null` takes it away, for
  /// the camera's [Camera.clearColor].
  static final Sky daylight = Sky.gradient(
    zenith: const Color.rgb(0.25, 0.49, 0.78),
    horizon: const Color.rgb(0.83, 0.89, 0.94),
    ground: const Color.rgb(0.45, 0.45, 0.45),
  );

  /// What the light the sky gives is multiplied by: 0 for none, 1 unless
  /// given. What's seen of it is unchanged. 0 or more; NaN throws a
  /// [RangeError].
  final double lighting;

  static double _atLeast0(double value, String name) {
    if (!(value >= 0 && value.isFinite)) throw RangeError.value(value, name, 'must be 0 or more');
    return value;
  }

  void _write(FySky s);
}

/// A sky of one colour: [Sky.color].
final class ColorSky extends Sky {
  /// See [Sky.color].
  ColorSky(this.color, {double lighting = 1}) : super._(Sky._atLeast0(lighting, 'lighting'));

  /// Seen behind everything, and added to the ambient light.
  final Color color;

  @override
  void _write(FySky s) {
    s.kind = FY_SKY_COLOR;
    s.horizon.write(color);
  }

  @override
  String toString() => 'Sky.color($color)';
}

/// A sky from [zenith] to [horizon] to [ground]: [Sky.gradient].
final class GradientSky extends Sky {
  /// See [Sky.gradient].
  GradientSky({required this.zenith, required this.horizon, Color? ground, double lighting = 1})
    : ground = ground ?? horizon,
      super._(Sky._atLeast0(lighting, 'lighting'));

  /// Straight up.
  final Color zenith;

  /// Level: what [Fog] without a colour of its own fades into.
  final Color horizon;

  /// Straight down.
  final Color ground;

  @override
  void _write(FySky s) {
    s.kind = FY_SKY_GRADIENT;
    s.zenith.write(zenith);
    s.horizon.write(horizon);
    s.ground.write(ground);
  }

  @override
  String toString() => 'Sky.gradient(zenith: $zenith, horizon: $horizon, ground: $ground)';
}

/// A sky from an `.hdr` image: [Sky.image].
final class ImageSky extends Sky {
  /// See [Sky.image]. A disposed [image] throws a [StateError].
  ImageSky(this.image, {double intensity = 1, this.rotation = 0, double lighting = 1})
    : intensity = Sky._atLeast0(intensity, 'intensity'),
      super._(Sky._atLeast0(lighting, 'lighting')) {
    image._live;
    if (!rotation.isFinite) throw RangeError.value(rotation, 'rotation', 'must be finite');
  }

  /// The `.hdr` shown.
  final SkyImage image;

  /// What its light is multiplied by: 1 unless given, 0 or more.
  final double intensity;

  /// How far it's turned about the vertical, in radians: positive turns
  /// it to the right.
  final double rotation;

  @override
  void _write(FySky s) {
    s.kind = FY_SKY_IMAGE;
    s.image = image._live;
    s.intensity = intensity;
    s.rotation = rotation.inDegrees;
  }

  @override
  String toString() => 'Sky.image($image, intensity: $intensity, rotation: $rotation)';
}

/// An `.hdr` image for an [ImageSky]: light, as bright as the sun can be,
/// around the whole sphere. Load one with [SkyImage.load], or as an asset
/// (`world.assets.skyImage('sky.hdr')`). A world showing it keeps its own
/// share, so [dispose] doesn't take it away. Freed once nothing reaches it.
final class SkyImage implements Finalizable {
  SkyImage._(this._pointer, this.width, this.height) {
    _finalizer.attach(this, _pointer.cast(), detach: this);
  }

  static final _finalizer = NativeFinalizer(
    Native.addressOf<NativeFunction<Void Function(Pointer<FySkyImage>)>>(fy_sky_image_free).cast(),
  );

  /// Decodes a Radiance `.hdr` (equirectangular, twice as wide as high) on
  /// a helper isolate, so the UI thread never waits for it. Throws a
  /// [FormatException] saying what's wrong for anything else, a broken
  /// file, or one over 8192 pixels on a side. [bytes] is copied.
  static Future<SkyImage> load(Uint8List bytes) async {
    ensureEngine();
    // As Texture2D.load: native memory, and only its address to the helper.
    final copy = malloc<Uint8>(bytes.isEmpty ? 1 : bytes.length);
    copy.asTypedList(bytes.length).setAll(0, bytes);
    try {
      final (address, width, height) = await inHelper(_decodeAt(copy.address, bytes.length));
      return SkyImage._(Pointer<FySkyImage>.fromAddress(address), width, height);
    } finally {
      malloc.free(copy);
    }
  }

  static (int, int, int) Function() _decodeAt(int address, int length) => () {
    ensureEngine();
    return using((arena) {
      final out = arena<Pointer<FySkyImage>>();
      final (width, height) = (arena<Uint32>(), arena<Uint32>());
      checkFile(fy_sky_image_decode(Pointer<Uint8>.fromAddress(address), length, out));
      check(fy_sky_image_size(out.value, width, height));
      // Not attached to a finalizer here: it belongs to the caller's isolate.
      return (out.value.address, width.value, height.value);
    });
  };

  Pointer<FySkyImage> _pointer;

  /// In pixels.
  final int width, height;

  Pointer<FySkyImage> get _live {
    if (_pointer == nullptr) throw StateError('$this was disposed');
    return _pointer;
  }

  /// Whether [dispose] was called; a disposed image can't make a sky (a
  /// StateError).
  bool get isDisposed => _pointer == nullptr;

  /// Frees the image now. Worlds showing it keep their share. Calling it
  /// again does nothing.
  void dispose() {
    if (_pointer == nullptr) return;
    _finalizer.detach(this);
    fy_sky_image_free(_pointer);
    _pointer = nullptr;
  }

  @override
  String toString() => 'SkyImage($width × $height)';
}

/// Fog ([World.fog]): what's further than [start] metres from the camera
/// fades into [color], all of it by [end]. Thinner higher up when [height]
/// is given: halving every [height] metres above y = 0, for mist in a
/// valley.
final class Fog {
  /// Without a [color], the fog is the sky's horizon, so far things melt
  /// into it; with no sky, the camera's clear colour.
  const Fog({this.color, required this.start, required this.end, this.height});

  /// What it fades into: the sky's horizon when null.
  final Color? color;

  /// Metres from the camera where it begins, 0 or more.
  final double start;

  /// Metres from the camera where it hides everything, beyond [start].
  final double end;

  /// Metres above y = 0 over which it halves: as thick everywhere when
  /// null.
  final double? height;

  @override
  bool operator ==(Object other) =>
      other is Fog && other.color == color && other.start == start && other.end == end && other.height == height;

  @override
  int get hashCode => Object.hash(color, start, end, height);

  @override
  String toString() =>
      'Fog(${color == null ? '' : 'color: $color, '}start: $start, end: $end'
      '${height == null ? '' : ', height: $height'})';
}
