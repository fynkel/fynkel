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

/// Which way a [TextRenderer] faces.
enum Facing {
  /// As its object turns: its front along the object's +Z, read from
  /// there; from behind, nothing shows.
  none(FY_FACING_NONE),

  /// Square to the camera, its top the camera's up: a hit number.
  camera(FY_FACING_CAMERA),

  /// Turned round up to face the camera, standing upright: a name over a
  /// head.
  upright(FY_FACING_UPRIGHT),

  /// Flat, facing up, its top away along -Z: words painted on the ground.
  up(FY_FACING_UP);

  const Facing(this._native);

  final int _native;
}

/// Flat text in the scene, as TextMeshPro's: a sign, a name over a head, a
/// number popping from a hit. Each letter is drawn from its [font]'s atlas
/// of distances, so it stays crisp close up and still reads a few pixels
/// high, an [outline] round it if asked. Its first line sits on its
/// object's X axis, [align]ed across its origin, [size] metres to the em.
///
/// Changing [text] lays it out again in the engine, making nothing once
/// its letters were seen: 200 numbers changed every frame are fine.
/// Unlit by default, shown as its [color] says in the dark too; hidden
/// behind what's solid unless it's [onTop].
///
/// Each setter sends the change at once, and throws an [ArgumentError] for
/// one that can't be, keeping the old value.
final class TextRenderer extends Component {
  /// White, centred, unlit, turned as its object is.
  TextRenderer(
    this._text, {
    required this._font,
    this._size = 1,
    this._color = Color.white,
    this._outline = 0,
    this._outlineColor = Color.black,
    this._softness = 0,
    this._shadowColor = const Color.rgb(0, 0, 0, 0),
    Vec2? shadowOffset,
    this._shadowSoftness = 0,
    this._align = TextAlignment.center,
    this._lineSpacing = 1,
    this._facing = Facing.none,
    this._onTop = false,
    this._lit = false,
  }) : _shadowOffset = shadowOffset ?? Vec2(0.05, -0.05),
       super._() {
    if (_refused() case final error?) throw error;
  }

  String _text;
  Font _font;
  double _size, _outline, _softness, _shadowSoftness, _lineSpacing;
  Color _color, _outlineColor, _shadowColor;
  Vec2 _shadowOffset;
  TextAlignment _align;
  Facing _facing;
  bool _onTop, _lit;

  /// What it shows; `\n` starts a line. The same again does nothing.
  String get text => _text;
  set text(String value) {
    _checkChangeable();
    if (value == _text) return;
    _text = value;
    final object = _live;
    if (object == null || !_enabledFlag) return;
    final length = scratch.utf8Quiet(value);
    check(fy_text_renderer_set_text(object._engine('its text can\'t be changed'), object.id, scratch.bytes(), length));
  }

  /// What its letters are drawn from.
  Font get font => _font;
  set font(Font value) => _set(() => _font, (v) => _font = v, value);

  /// Its em, in metres: a capital is about 0.7 of it.
  double get size => _size;
  set size(double value) => _set(() => _size, (v) => _size = v, value);

  /// Its letters' colour.
  Color get color => _color;
  set color(Color value) => _set(() => _color, (v) => _color = v, value);

  /// How wide the outline round each letter is: 0 none, 1 as far as the
  /// atlas reaches, about a ninth of [size].
  double get outline => _outline;
  set outline(double value) => _set(() => _outline, (v) => _outline = v, value);

  /// The outline's colour.
  Color get outlineColor => _outlineColor;
  set outlineColor(Color value) => _set(() => _outlineColor, (v) => _outlineColor = v, value);

  /// How soft its edges are: 0 crisp, 1 blurred: a glow, with an outline.
  double get softness => _softness;
  set softness(double value) => _set(() => _softness, (v) => _softness = v, value);

  /// A shadow under the letters (TextMeshPro's underlay), drawn with them:
  /// none while its alpha is 0, as by default.
  Color get shadowColor => _shadowColor;
  set shadowColor(Color value) => _set(() => _shadowColor, (v) => _shadowColor = v, value);

  /// Where the shadow is from the letters, in ems: (0.05, -0.05), a little
  /// right and down, by default.
  Vec2 get shadowOffset => _shadowOffset;
  set shadowOffset(Vec2 value) => _set(() => _shadowOffset, (v) => _shadowOffset = v, value);

  /// How soft the shadow's edges are, 0 to 1, as [softness].
  double get shadowSoftness => _shadowSoftness;
  set shadowSoftness(double value) => _set(() => _shadowSoftness, (v) => _shadowSoftness = v, value);

  /// Where each line goes across its object's origin.
  TextAlignment get align => _align;
  set align(TextAlignment value) => _set(() => _align, (v) => _align = v, value);

  /// Lines are this many times the font's own line height apart.
  double get lineSpacing => _lineSpacing;
  set lineSpacing(double value) => _set(() => _lineSpacing, (v) => _lineSpacing = v, value);

  /// Which way it faces: see [Facing].
  Facing get facing => _facing;
  set facing(Facing value) => _set(() => _facing, (v) => _facing = v, value);

  /// Drawn over everything, walls and all.
  bool get onTop => _onTop;
  set onTop(bool value) => _set(() => _onTop, (v) => _onTop = v, value);

  /// Lit as the scene is, else shown as [color] says.
  bool get lit => _lit;
  set lit(bool value) => _set(() => _lit, (v) => _lit = v, value);

  void _set<T>(T Function() read, void Function(T) write, T value) {
    _checkChangeable();
    final old = read();
    write(value);
    if (_refused() case final error?) {
      write(old);
      throw error;
    }
    if (!_enabledFlag) return;
    final object = _live;
    if (object == null) return;
    try {
      _put(object);
    } catch (_) {
      write(old);
      rethrow;
    }
  }

  @override
  void _put(GameObject object) {
    final w = object._engine('its TextRenderer can\'t be changed');
    final world = object.world;
    _textViews.settings
      ..font = world._fontId(_font)
      ..size = _size
      ..align = _align._native
      ..line_spacing = _lineSpacing
      ..outline = _outline
      ..softness = _softness
      ..shadow_softness = _shadowSoftness
      ..facing = _facing._native
      ..on_top = _onTop
      ..lit = _lit;
    _textViews.color.write(_color);
    _textViews.outlineColor.write(_outlineColor);
    _textViews.shadowColor.write(_shadowColor);
    _textViews.shadowOffset
      ..x = _shadowOffset.x
      ..y = _shadowOffset.y;
    final length = scratch.utf8Quiet(_text);
    check(fy_text_renderer_set(w, object.id, scratch.textSettings, scratch.bytes(), length));
  }

  @override
  void _takeOut(GameObject object) => check(fy_text_renderer_remove(object.world.pointer, object.id));

  ArgumentError? _refused() {
    if (!_size.isFinite || _size < 0) return ArgumentError.value(_size, 'size', 'must be 0 or more');
    if (!(_outline >= 0 && _outline <= 1)) return ArgumentError.value(_outline, 'outline', 'must be 0 to 1');
    if (!(_softness >= 0 && _softness <= 1)) return ArgumentError.value(_softness, 'softness', 'must be 0 to 1');
    if (!(_shadowSoftness >= 0 && _shadowSoftness <= 1)) {
      return ArgumentError.value(_shadowSoftness, 'shadowSoftness', 'must be 0 to 1');
    }
    if (!_shadowOffset.x.isFinite || !_shadowOffset.y.isFinite) {
      return ArgumentError.value(_shadowOffset, 'shadowOffset', 'must be finite');
    }
    if (!_lineSpacing.isFinite) return ArgumentError.value(_lineSpacing, 'lineSpacing', 'must be finite');
    return null;
  }

  @override
  String toString() => 'TextRenderer(${_text.length > 20 ? '${_text.substring(0, 20)}…' : _text})';
}

/// [scratch]'s text settings' views, made once: a struct's field read
/// makes a view each time.
final class _TextViews {
  _TextViews(this.settings)
    : color = settings.color,
      outlineColor = settings.outline_color,
      shadowColor = settings.shadow_color,
      shadowOffset = settings.shadow_offset;

  final FyTextSettings settings;
  final FyColor color, outlineColor, shadowColor;
  final FyVec2 shadowOffset;
}

final _textViews = _TextViews(scratch.textSettings.ref);

extension on World {
  /// [font]'s id in this world, kept here the first time a text uses it.
  int _fontId(Font font) {
    final known = _fontIds[font];
    if (known != null) return known;
    check(fy_font_add(pointer, font._bytes, font._length, scratch.id));
    return _fontIds[font] = scratch.id.value;
  }
}
