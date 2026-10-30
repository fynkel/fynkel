part of 'world.dart';

/// Where each line of [Mesh.text] goes across the origin, as Unity's
/// `TextMesh.alignment`.
enum TextAlignment {
  /// Lines start at the origin.
  left(FY_TEXT_LEFT),

  /// Lines are centred on it.
  center(FY_TEXT_CENTER),

  /// Lines end at it.
  right(FY_TEXT_RIGHT);

  const TextAlignment(this._native);

  final int _native;
}

/// A TrueType (`.ttf`) or OpenType (`.otf`) font, for [Mesh.text]: from a
/// file's bytes, or as an asset (`world.assets.font('fonts/title.ttf')`).
/// Freed once nothing reaches it.
final class Font implements Finalizable {
  /// The font in [bytes] (the first, for a collection). Throws a
  /// [FormatException] saying why for bytes that aren't a font the engine
  /// reads. [bytes] is copied.
  Font(Uint8List bytes) : _length = bytes.length, _bytes = malloc<Uint8>(bytes.isEmpty ? 1 : bytes.length) {
    _bytes.asTypedList(_length).setAll(0, bytes);
    _free.attach(this, _bytes.cast(), detach: this, externalSize: _length);
    ensureEngine();
    checkFile(fy_font_check(_bytes, _length));
  }

  static final _free = NativeFinalizer(malloc.nativeFree);

  // The file, in native memory: read again for each call, as the engine
  // keeps nothing of it.
  final Pointer<Uint8> _bytes;
  final int _length;

  /// How wide the widest line of [text] is (x) and how tall its lines are
  /// together (y), in metres, as [Mesh.text] lays it out at [size] and
  /// [lineSpacing]: for a board to fit behind a sign.
  Vec2 measure(String text, {double size = 1, double lineSpacing = 1}) {
    return using((arena) {
      final utf8Text = utf8.encode(text);
      final out = arena<FyVec2>();
      check(fy_text_measure(_bytes, _length, _utf8In(arena, utf8Text), utf8Text.length, size, lineSpacing, out));
      return Vec2(out.ref.x, out.ref.y);
    });
  }

  /// [text] as UTF-8 in [arena]'s memory.
  static Pointer<Uint8> _utf8In(Arena arena, Uint8List text) {
    final p = arena<Uint8>(text.isEmpty ? 1 : text.length);
    p.asTypedList(text.length).setAll(0, text);
    return p;
  }
}

final class _FontKind extends AssetKind<Font> {
  const _FontKind();

  @override
  String get name => 'font';

  @override
  List<String> get extensions => const ['.ttf', '.otf', '.ttc'];

  @override
  Future<Font> load(World world, String name, Uint8List bytes) async => Font(bytes);
}

const _font = _FontKind();
