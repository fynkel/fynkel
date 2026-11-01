part of 'world.dart';

/// How far in from each edge a [SpriteRenderer]'s corners and edges are,
/// in its texture's pixels, for 9-slice: left, top, right and bottom, as
/// Flutter's `EdgeInsets.fromLTRB`.
final class Insets {
  /// [left], [top], [right] and [bottom], in pixels.
  const Insets(this.left, this.top, this.right, this.bottom);

  /// The same on every side.
  const Insets.all(double value) : left = value, top = value, right = value, bottom = value;

  /// [horizontal] at the left and right, [vertical] at the top and bottom.
  const Insets.symmetric({double horizontal = 0, double vertical = 0})
    : left = horizontal,
      top = vertical,
      right = horizontal,
      bottom = vertical;

  /// Not sliced: the texture stretched across the whole sprite.
  static const zero = Insets.all(0);

  /// In pixels, from each edge.
  final double left, top, right, bottom;

  bool get _isFinite => left.isFinite && top.isFinite && right.isFinite && bottom.isFinite;

  @override
  bool operator ==(Object other) =>
      other is Insets && other.left == left && other.top == top && other.right == right && other.bottom == bottom;

  @override
  int get hashCode => Object.hash(left, top, right, bottom);

  @override
  String toString() => 'Insets($left, $top, $right, $bottom)';
}

/// A picture in the scene, as Unity's Sprite Renderer: [texture] on a quad
/// [size] metres, times [color], from its [pivot], drawn by its
/// [sortingLayer] and [order] before by its distance, higher over lower. A
/// marker on the ground, a leaf, a cloud, a speech bubble ([slice]), or a
/// 2D game's every picture.
///
/// See-through by the texture's alpha, or cut out below [alphaCutoff];
/// unlit by default, shown as its texture and colour say. Sprites of one
/// texture, sorting layer and order are drawn together, so a thousand of
/// them cost one draw.
///
/// Each setter sends the change at once, and throws an [ArgumentError] for
/// one that can't be, keeping the old value. Its texture can't be
/// disposed while it shows it.
final class SpriteRenderer extends Component {
  /// White, its pivot its middle, unlit, turned as its object is, its size
  /// its texture's pixels at [pixelsPerMetre] (100, as Unity's) unless
  /// given.
  SpriteRenderer(
    this._texture, {
    this._size,
    this._color = Color.white,
    this._flipX = false,
    this._flipY = false,
    Vec2? pivot,
    this._slice = Insets.zero,
    this._pixelsPerMetre = 100,
    this._alphaCutoff = 0,
    this._facing = Facing.none,
    this._sortingLayer = 0,
    this._order = 0,
    this._lit = false,
  }) : _pivot = pivot ?? Vec2(0.5, 0.5),
       super._() {
    if (_refused() case final error?) throw error;
  }

  Texture2D _texture;
  Vec2? _size;
  Color _color;
  bool _flipX, _flipY, _lit;
  Vec2 _pivot;
  Insets _slice;
  double _pixelsPerMetre, _alphaCutoff;
  Facing _facing;
  int _sortingLayer, _order;

  /// What it shows.
  Texture2D get texture => _texture;
  set texture(Texture2D value) {
    _checkChangeable();
    final old = _texture;
    _texture = value;
    if (_write() case final failed?) {
      _texture = old;
      failed.raise();
    }
  }

  /// Its width and height, in metres: its texture's pixels at
  /// [pixelsPerMetre] until it's set.
  Vec2 get size => _size ?? Vec2(_texture.width / _pixelsPerMetre, _texture.height / _pixelsPerMetre);
  set size(Vec2 value) {
    _checkChangeable();
    final old = _size;
    _size = value;
    if (_write() case final failed?) {
      _size = old;
      failed.raise();
    }
  }

  /// Multiplied into its texture's colours: its alpha fades it.
  Color get color => _color;
  set color(Color value) {
    _checkChangeable();
    final old = _color;
    _color = value;
    if (_write() case final failed?) {
      _color = old;
      failed.raise();
    }
  }

  /// Mirrored across, about its pivot: a character turned to walk left.
  bool get flipX => _flipX;
  set flipX(bool value) {
    _checkChangeable();
    final old = _flipX;
    _flipX = value;
    if (_write() case final failed?) {
      _flipX = old;
      failed.raise();
    }
  }

  /// Mirrored up and down, about its pivot.
  bool get flipY => _flipY;
  set flipY(bool value) {
    _checkChangeable();
    final old = _flipY;
    _flipY = value;
    if (_write() case final failed?) {
      _flipY = old;
      failed.raise();
    }
  }

  /// The point of it its object's origin is, 0 to 1 across and up from its
  /// bottom left: (0.5, 0.5) its middle, (0.5, 0) its feet.
  Vec2 get pivot => _pivot;
  set pivot(Vec2 value) {
    _checkChangeable();
    final old = _pivot;
    _pivot = value;
    if (_write() case final failed?) {
      _pivot = old;
      failed.raise();
    }
  }

  /// 9-slice: how far in from each edge its corners are, in its texture's
  /// pixels. Its corners keep their size, [pixelsPerMetre] to the metre,
  /// however big it's made, and its edges and middle stretch: a speech
  /// bubble, a panel, a health bar. [Insets.zero] for none.
  Insets get slice => _slice;
  set slice(Insets value) {
    _checkChangeable();
    final old = _slice;
    _slice = value;
    if (_write() case final failed?) {
      _slice = old;
      failed.raise();
    }
  }

  /// How many of its texture's pixels make a metre: its size until one's
  /// given, and its slice's corners.
  double get pixelsPerMetre => _pixelsPerMetre;
  set pixelsPerMetre(double value) {
    _checkChangeable();
    final old = _pixelsPerMetre;
    _pixelsPerMetre = value;
    if (_write() case final failed?) {
      _pixelsPerMetre = old;
      failed.raise();
    }
  }

  /// 0, see-through by its texture's alpha; above 0 (to 1), cut out where
  /// the alpha is below it, for leaves and fences.
  double get alphaCutoff => _alphaCutoff;
  set alphaCutoff(double value) {
    _checkChangeable();
    final old = _alphaCutoff;
    _alphaCutoff = value;
    if (_write() case final failed?) {
      _alphaCutoff = old;
      failed.raise();
    }
  }

  /// Which way it faces: see [Facing].
  Facing get facing => _facing;
  set facing(Facing value) {
    _checkChangeable();
    final old = _facing;
    _facing = value;
    if (_write() case final failed?) {
      _facing = old;
      failed.raise();
    }
  }

  /// Drawn by this first, higher over lower, then by [order], then by
  /// distance.
  int get sortingLayer => _sortingLayer;
  set sortingLayer(int value) {
    _checkChangeable();
    final old = _sortingLayer;
    _sortingLayer = value;
    if (_write() case final failed?) {
      _sortingLayer = old;
      failed.raise();
    }
  }

  /// Its order in its sorting layer: higher over lower.
  int get order => _order;
  set order(int value) {
    _checkChangeable();
    final old = _order;
    _order = value;
    if (_write() case final failed?) {
      _order = old;
      failed.raise();
    }
  }

  /// Lit as the scene is, else shown as its texture and [color] say.
  bool get lit => _lit;
  set lit(bool value) {
    _checkChangeable();
    final old = _lit;
    _lit = value;
    if (_write() case final failed?) {
      _lit = old;
      failed.raise();
    }
  }

  /// Checks its settings and sends them, when it's on and live: the
  /// failure, for the setter to put the old value back and raise. Written
  /// out in each setter, not through a closure, so a setter called every
  /// frame makes nothing.
  _Failure? _write() {
    if (_refused() case final error?) return _Failure(error, StackTrace.current);
    if (!_enabledFlag) return null;
    final object = _live;
    if (object == null) return null;
    try {
      _put(object);
      return null;
    } catch (error, stack) {
      return _Failure(error, stack);
    }
  }

  @override
  void _put(GameObject object) {
    final w = object._engine('its SpriteRenderer can\'t be changed');
    final world = object.world;
    _spriteViews.settings
      ..texture = world._textureId(_texture)
      ..flip_x = _flipX
      ..flip_y = _flipY
      ..pixels_per_metre = _pixelsPerMetre
      ..alpha_cutoff = _alphaCutoff
      ..facing = _facing._native
      ..sorting_layer = _sortingLayer
      ..order = _order
      ..lit = _lit;
    // Written out, not through a getter: a double returned from a call the
    // JIT doesn't inline is boxed.
    final size = _size;
    if (size != null) {
      _spriteViews.size
        ..x = size.x
        ..y = size.y;
    } else {
      _spriteViews.size
        ..x = _texture.width / _pixelsPerMetre
        ..y = _texture.height / _pixelsPerMetre;
    }
    _spriteViews.pivot
      ..x = _pivot.x
      ..y = _pivot.y;
    _spriteViews.color.write(_color);
    // The engine's slice is left, bottom, right, top.
    _spriteViews.slice
      ..[0] = _slice.left
      ..[1] = _slice.bottom
      ..[2] = _slice.right
      ..[3] = _slice.top;
    check(fy_sprite_renderer_set(w, object.id, scratch.spriteSettings));
  }

  @override
  void _takeOut(GameObject object) => check(fy_sprite_renderer_remove(object.world.pointer, object.id));

  ArgumentError? _refused() {
    if (_texture.isDisposed) return ArgumentError.value(_texture, 'texture', 'was disposed');
    if (!(_pixelsPerMetre.isFinite && _pixelsPerMetre > 0)) {
      return ArgumentError.value(_pixelsPerMetre, 'pixelsPerMetre', 'must be more than 0');
    }
    // Its texture's size at a pixels per metre above 0 is always fine.
    final size = _size;
    if (size != null && !(size.x.isFinite && size.y.isFinite && size.x >= 0 && size.y >= 0)) {
      return ArgumentError.value(size, 'size', 'must be 0 or more');
    }
    if (!(_pivot.x.isFinite && _pivot.y.isFinite)) return ArgumentError.value(_pivot, 'pivot', 'must be finite');
    if (!_slice._isFinite || _slice.left < 0 || _slice.top < 0 || _slice.right < 0 || _slice.bottom < 0) {
      return ArgumentError.value(_slice, 'slice', 'must be pixels of 0 or more');
    }
    if (!(_alphaCutoff >= 0 && _alphaCutoff <= 1)) {
      return ArgumentError.value(_alphaCutoff, 'alphaCutoff', 'must be 0 to 1');
    }
    return null;
  }

  @override
  String toString() => 'SpriteRenderer($_texture, size: $size)';
}

/// [scratch]'s sprite settings' views, made once: a struct's field read
/// makes a view each time.
final class _SpriteViews {
  _SpriteViews(this.settings)
    : size = settings.size,
      pivot = settings.pivot,
      color = settings.color,
      slice = settings.slice;

  final FySpriteSettings settings;
  final FyVec2 size, pivot;
  final FyColor color;
  final Array<Float> slice;
}

final _spriteViews = _SpriteViews(scratch.spriteSettings.ref);
