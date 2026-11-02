part of 'world.dart';

/// A material's own look, written in WGSL (docs/writing-a-game.md, "14. Shaders"): a
/// `.wgsl` file with a `surface` function, lit by the engine as the
/// built-in material is, or an `unlit` one, which gives the colour as it
/// is. Load it with `assets.shader('lava.wgsl')` and give it to a
/// [Material]:
///
/// ```dart
/// final lava = Material(shader: await assets.shader('lava.wgsl'), values: {'hot': Color(0xFFFF6A10)});
/// ```
///
/// What it declares a material sets by name ([Material.set]): its
/// `struct Values` and its textures (`var picture: color_texture;`). Its
/// `struct PerObject`, up to four numbers, each object sets for itself
/// ([MeshRenderer.set]), as a hit flash. A `vertex` function moves its
/// mesh's vertices, as grass in the wind ([movesVertices]); the material
/// then says how far with [Material.reach]. A shader can be used in any
/// number of worlds; like a [Mesh], a world lets it go once nothing reaches
/// it, and [dispose] frees it sooner.
final class Shader implements Finalizable {
  Shader._(this._pointer, this.name, this._layout) {
    _finalizer.attach(this, _pointer.cast(), detach: this);
  }

  /// Compiles [source], WGSL. [name] is its file's, for messages. Throws a
  /// [ShaderError] saying where in [source] it's wrong when it doesn't
  /// compile.
  factory Shader(String source, {String name = 'shader.wgsl'}) {
    final (pointer, layout) = _compile(source, name);
    return Shader._(pointer, name, layout);
  }

  static final _finalizer = NativeFinalizer(
    Native.addressOf<NativeFunction<Void Function(Pointer<FyShader>)>>(fy_shader_free).cast(),
  );

  static (Pointer<FyShader>, _ShaderLayout) _compile(String source, String name) {
    ensureEngine();
    return using((arena) {
      final n = name.toNativeUtf8(allocator: arena);
      final s = source.toNativeUtf8(allocator: arena);
      final status = fy_shader_new(n.cast(), n.length, s.cast(), s.length, scratch.shader);
      if (status == FyStatus.FY_STATUS_INVALID_VALUE) {
        throw ShaderError._parse(fy_last_error().cast<Utf8>().toDartString());
      }
      check(status);
      final pointer = scratch.shader.value;
      check(fy_shader_describe(pointer, nullptr, 0, scratch.count));
      final length = scratch.count.value;
      final text = arena<Uint8>(length == 0 ? 1 : length);
      check(fy_shader_describe(pointer, text, length, scratch.count));
      return (pointer, _ShaderLayout.parse(utf8.decode(text.asTypedList(length))));
    });
  }

  /// Its file's name, for messages.
  final String name;

  Pointer<FyShader> _pointer;
  _ShaderLayout _layout;

  final _release = _Release(_Resource.shader);

  /// The screen effects using it, for [reload] to lay their values out
  /// again.
  final _effects = <WeakReference<ScreenEffect>>[];

  Pointer<FyShader> get _live {
    if (_pointer == nullptr) throw StateError('$this was disposed');
    return _pointer;
  }

  /// Whether it has a `surface`, which the engine lights; false for an
  /// `unlit` one.
  bool get isLit => _layout.lit;

  /// The names of its `Values`, which [Material.set] sets, in order.
  List<String> get values => [for (final v in _layout.values) v.name];

  /// The names of its textures, which [Material.set] (or an effect's
  /// [ScreenEffect.set]) sets too, in order.
  List<String> get textures => [for (final t in _layout.textures) t.name];

  /// The names of its `PerObject` numbers, which [MeshRenderer.set] sets,
  /// in order.
  List<String> get perObject => List.unmodifiable(_layout.own);

  /// Whether it has a `vertex`, which moves its mesh's vertices: grass in
  /// the wind, waves. Its shadow moves with it. Give its materials a
  /// [Material.reach].
  bool get movesVertices => _layout.moves;

  /// Whether it reads what's behind its surface (`scene_depth`,
  /// `scene_color`): water with a foaming shore, glass that bends what's
  /// behind it. Its materials are drawn see-through, over the solid
  /// scene, whatever their [Material.blending]. A frame with one in view
  /// keeps the scene's depth and colour; other frames pay nothing. An
  /// effect's: whether it reads the solid scene's depth (`scene_depth`,
  /// `scene_position`), which a frame it runs in keeps.
  bool get readsBehind => _layout.behind;

  /// Whether it has a `light`, which lights it in place of the engine,
  /// one sun or lamp at a time: banded toon light, light that wraps round.
  /// Shadows, fog and the sky's light stay the engine's.
  bool get hasOwnLighting => _layout.lights;

  /// Whether it has an `effect`, which runs over a camera's whole picture
  /// ([ScreenEffect], [Camera.effects]) rather than drawing a material.
  bool get isEffect => _layout.effect;

  /// Whether [dispose] was called; a disposed shader can't be used in a
  /// material (a StateError).
  bool get isDisposed => _pointer == nullptr;

  /// Puts [source] in place of its code, as when its file changes: every
  /// material using it, in every world, draws with the new code from the
  /// next frame, its values set again by name; one the new code doesn't
  /// have, or has as another kind (a number now a texture), is left out
  /// until it's set again. Throws a [ShaderError] and keeps the code it
  /// had when [source] doesn't compile.
  void reload(String source) {
    final old = _live;
    final (pointer, layout) = _compile(source, name);
    _finalizer.detach(this);
    _pointer = pointer;
    _layout = layout;
    _finalizer.attach(this, pointer.cast(), detach: this);
    try {
      for (final world in _release.worlds) {
        world._reloadShader(this);
      }
      _effects.removeWhere((e) => e.target == null);
      for (final effect in _effects) {
        effect.target?._changed();
      }
    } finally {
      fy_shader_free(old);
    }
  }

  /// Frees the shader: every world that has it lets it go. Throws a
  /// [StateError] while a material in a world uses it. Calling it again
  /// does nothing.
  void dispose() {
    if (_pointer == nullptr) return;
    for (final world in _release.worlds) {
      world._releaseShader(this);
    }
    _release.forget();
    _finalizer.detach(this);
    fy_shader_free(_pointer);
    _pointer = nullptr;
  }

  @override
  String toString() => "Shader('$name')";
}

/// A shader run over a camera's whole picture once it's drawn: an old TV's
/// lines, a palette, a flash when hit. Its shader has an `effect`
/// function ([Shader.isEffect]), which gives each pixel's colour from the
/// picture's (`picture(in.uv)`); its `struct Values` it sets by name, as a
/// material's:
///
/// ```dart
/// camera.effects = [ScreenEffect(await assets.shader('old_tv.wgsl'), values: {'lines': 240.0})];
/// ```
///
/// A camera's effects run in order, each over what the one before made.
/// One effect can be given to any number of cameras; [set] changes it for
/// all of them.
final class ScreenEffect {
  /// [shader]'s effect, with [values] set as [set] sets them. Throws an
  /// [ArgumentError] for a shader without an `effect`, and a [StateError]
  /// for a disposed one.
  ScreenEffect(this.shader, {Map<String, Object> values = const {}}) {
    if (shader.isDisposed) throw StateError('$shader was disposed');
    if (!shader.isEffect) {
      throw ArgumentError(
        'shader is $shader, which has no effect function, so is no screen effect: give it to a Material',
      );
    }
    values.forEach(_setValue);
    _bytes = shader._layout.pack(_values, shader);
    _addWeak(shader._effects, this);
  }

  /// Its shader.
  final Shader shader;

  final _values = <String, Object>{};
  Uint8List? _bytes;

  /// The cameras it's in, to send it to again when it changes.
  final _cameras = <WeakReference<Camera>>[];

  /// Sets its shader's value or texture [name], as [Material.set] does: a
  /// number, a [Vec2], a [Vec3], a [Color], a list of numbers, a list of
  /// those for an array, or a [Texture2D] (a colour grade's table, a
  /// noise). Every camera with it
  /// shows the change from the next frame. Throws an [ArgumentError] for a
  /// name the shader doesn't have or a value that isn't its kind.
  void set(String name, Object value) {
    _setValue(name, value);
    _changed();
  }

  /// What [set] last gave [name], or null.
  Object? value(String name) => _values[name];

  void _setValue(String name, Object value) {
    final layout = shader._layout;
    if (value is Texture2D) {
      if (layout.texture(name) < 0) {
        throw ArgumentError.value(name, 'name', 'is no texture of $shader, which has: ${layout.names}');
      }
      _values[name] = Material._usable(value)!;
      return;
    }
    final v = layout.value(name);
    if (v == null) throw ArgumentError.value(name, 'name', 'is no value of $shader, which has: ${layout.names}');
    v.parseAll(value, shader);
    _values[name] = value;
  }

  /// The texture its shader's texture [index] is given, or null: also
  /// when what it has by that name is a value, from before a reload.
  Texture2D? _texture(int index) {
    final textures = shader._layout.textures;
    if (index >= textures.length) return null;
    return switch (_values[textures[index].name]) {
      final Texture2D texture => texture,
      _ => null,
    };
  }

  /// Lays its values out for its shader as it is now and sends them to
  /// the cameras it's in.
  void _changed() {
    _bytes = shader._layout.pack(_values, shader);
    _cameras.removeWhere((c) => c.target == null);
    for (final camera in _cameras) {
      camera.target?._writeEffects();
    }
  }

  @override
  String toString() => 'ScreenEffect(${shader.name})';
}

/// A shader that doesn't compile: where in its file, and what's wrong.
final class ShaderError implements Exception {
  ShaderError._(this.file, this.line, this.column, this.message);

  factory ShaderError._parse(String text) {
    final at = RegExp(r'^(.*?):(\d+):(\d+): (.*)$', dotAll: true).firstMatch(text);
    if (at != null) {
      return ShaderError._(at[1]!, int.parse(at[2]!), int.parse(at[3]!), at[4]!);
    }
    final file = RegExp(r'^(.*?): (.*)$', dotAll: true).firstMatch(text);
    if (file != null) return ShaderError._(file[1]!, null, null, file[2]!);
    return ShaderError._('', null, null, text);
  }

  /// The shader's file name.
  final String file;

  /// The line and column in it, from 1, or null when the mistake isn't on
  /// one of its lines (a name the engine has too, most often).
  final int? line, column;

  /// What's wrong.
  final String message;

  @override
  String toString() => switch ((line, column)) {
    (final l?, final c?) => '$file:$l:$c: $message',
    _ => '$file: $message',
  };
}

/// What a shader takes, as `fy_shader_describe` tells it.
final class _ShaderLayout {
  _ShaderLayout(
    this.lit,
    this.size,
    this.values,
    this.textures,
    this.own, {
    this.moves = false,
    this.behind = false,
    this.lights = false,
    this.effect = false,
  });

  factory _ShaderLayout.parse(String text) {
    var lit = true;
    var size = 0;
    final values = <_ShaderValue>[];
    final textures = <({String name, bool srgb})>[];
    final own = <String>[];
    var moves = false;
    var behind = false;
    var lights = false;
    var effect = false;
    for (final line in const LineSplitter().convert(text)) {
      final words = line.split(' ');
      switch (words) {
        case ['lit']:
          lit = true;
        case ['unlit']:
          lit = false;
        case ['values', final bytes]:
          size = int.parse(bytes);
        case ['value', final name, final kind, final parts, final offset]:
          values.add(_ShaderValue(name, kind, int.parse(parts), int.parse(offset)));
        case ['value', final name, final kind, final parts, final offset, final count, final stride]:
          values.add(
            _ShaderValue(
              name,
              kind,
              int.parse(parts),
              int.parse(offset),
              count: int.parse(count),
              stride: int.parse(stride),
            ),
          );
        case ['texture', final name, final space]:
          textures.add((name: name, srgb: space == 'srgb'));
        case ['own', final name]:
          own.add(name);
        case ['moves']:
          moves = true;
        case ['behind']:
          behind = true;
        case ['lights']:
          lights = true;
        case ['effect']:
          effect = true;
      }
    }
    return _ShaderLayout(
      lit,
      size,
      values,
      textures,
      own,
      moves: moves,
      behind: behind,
      lights: lights,
      effect: effect,
    );
  }

  final bool lit;

  /// How many bytes its `Values` take.
  final int size;
  final List<_ShaderValue> values;
  final List<({String name, bool srgb})> textures;
  final List<String> own;
  final bool moves;
  final bool behind;
  final bool lights;
  final bool effect;

  _ShaderValue? value(String name) {
    for (final v in values) {
      if (v.name == name) return v;
    }
    return null;
  }

  int texture(String name) => textures.indexWhere((t) => t.name == name);

  /// [given] laid out as its `Values`, or null when it has none. What it
  /// doesn't have, or has as another kind, is left out.
  Uint8List? pack(Map<String, Object> given, Shader shader) {
    if (size == 0) return null;
    final bytes = Uint8List(size);
    final data = ByteData.sublistView(bytes);
    for (final MapEntry(key: name, :value) in given.entries) {
      final v = this.value(name);
      if (v == null || value is Texture2D) continue;
      final List<List<num>> elements;
      try {
        elements = v.parseAll(value, shader);
      } on ArgumentError {
        continue;
      }
      for (final (e, parts) in elements.indexed) {
        for (final (i, n) in parts.indexed) {
          final at = v.offset + e * v.stride + i * 4;
          switch (v.kind) {
            case 'f32':
              data.setFloat32(at, n.toDouble(), Endian.little);
            case 'i32':
              data.setInt32(at, n.toInt(), Endian.little);
            default:
              data.setUint32(at, n.toInt(), Endian.little);
          }
        }
      }
    }
    return bytes;
  }

  /// Every name a material sets, for messages.
  String get names {
    final all = [for (final v in values) v.name, for (final t in textures) t.name];
    return all.isEmpty ? 'none' : all.join(', ');
  }
}

/// One of a shader's `Values`: `f32`, `i32` or `u32`, of [parts] (1 for a
/// number, 2 to 4 for a vector), at [offset] bytes; or an array of [count]
/// of them, [stride] bytes apart.
final class _ShaderValue {
  _ShaderValue(this.name, this.kind, this.parts, this.offset, {this.count = 1, int? stride})
    : stride = stride ?? parts * 4;

  final String name;
  final String kind;
  final int parts;
  final int offset;
  final int count;
  final int stride;

  bool get isArray => count > 1;

  String get _type {
    final one = parts == 1 ? kind : 'vec$parts<$kind>';
    return isArray ? 'array<$one, $count>' : one;
  }

  /// [value] as each of its elements' parts: one for a value, up to
  /// [count] for an array (a list of them; those it leaves out are 0).
  List<List<num>> parseAll(Object value, Shader shader) {
    if (!isArray) return [parse(value, shader)];
    if (value is! List || value.length > count) {
      throw ArgumentError(
        "$name is $value; that's no value for $shader's $name, which is $_type: a list of up to $count",
      );
    }
    return [for (final element in value) parse(element as Object, shader)];
  }

  /// [value] as its parts, or what's wrong with it: one element's, for an
  /// array.
  List<num> parse(Object value, Shader shader) {
    List<num> wrong() => _wrong(value, shader);
    final List<num> got = switch (value) {
      num n => [n],
      Vec2 v => [v.x, v.y],
      Vec3 v => [v.x, v.y, v.z],
      // A colour is picked in sRGB; the shader works in linear light, as
      // the engine's own materials do.
      Color c => [_toLinear(c.r), _toLinear(c.g), _toLinear(c.b), c.a],
      List<num> l => l,
      _ => wrong(),
    };
    // A colour for a vec3 drops its alpha.
    final parts_ = value is Color && parts == 3 ? got.sublist(0, 3) : got;
    if (parts_.length != parts) return wrong();
    for (final n in parts_) {
      if (kind == 'f32' && !n.isFinite) throw ArgumentError('$name is $value; it must be finite');
      if (kind != 'f32' && n is! int) return wrong();
      if (kind == 'u32' && n < 0) throw ArgumentError('$name is $value; it must be 0 or more');
    }
    return parts_;
  }

  /// Lays [value], one [parse] takes, out at [offset] in [data] without
  /// making anything (stage 14's DF7a: a tween sets one every frame), and
  /// checked as [parse] checks it before writing. False for an array,
  /// which [parseAll] lays out.
  bool write(ByteData data, Object value, Shader shader) {
    if (isArray) return false;
    num x = 0, y = 0, z = 0, w = 0;
    final int given;
    switch (value) {
      case num n:
        (x, given) = (n, 1);
      case Vec2 v:
        (x, y, given) = (v.x, v.y, 2);
      case Vec3 v:
        (x, y, z, given) = (v.x, v.y, v.z, 3);
      case Color c:
        // A colour for a vec3 drops its alpha.
        (x, y, z, w) = (_toLinear(c.r), _toLinear(c.g), _toLinear(c.b), c.a);
        given = parts == 3 ? 3 : 4;
      case List<num> l when l.length == parts:
        x = l[0];
        if (parts > 1) y = l[1];
        if (parts > 2) z = l[2];
        if (parts > 3) w = l[3];
        given = parts;
      default:
        _wrong(value, shader);
    }
    if (given != parts) _wrong(value, shader);
    _check(x, value, shader);
    if (parts > 1) _check(y, value, shader);
    if (parts > 2) _check(z, value, shader);
    if (parts > 3) _check(w, value, shader);
    _put(data, offset, x);
    if (parts > 1) _put(data, offset + 4, y);
    if (parts > 2) _put(data, offset + 8, z);
    if (parts > 3) _put(data, offset + 12, w);
    return true;
  }

  Never _wrong(Object value, Shader shader) =>
      throw ArgumentError("$name is $value; that's no value for $shader's $name, which is $_type");

  void _check(num n, Object value, Shader shader) {
    if (kind == 'f32' && !n.isFinite) throw ArgumentError('$name is $value; it must be finite');
    if (kind != 'f32' && n is! int) _wrong(value, shader);
    if (kind == 'u32' && n < 0) throw ArgumentError('$name is $value; it must be 0 or more');
  }

  void _put(ByteData data, int at, num n) {
    switch (kind) {
      case 'f32':
        data.setFloat32(at, n.toDouble(), Endian.little);
      case 'i32':
        data.setInt32(at, n.toInt(), Endian.little);
      default:
        data.setUint32(at, n.toInt(), Endian.little);
    }
  }

  static double _toLinear(double s) => s <= 0.04045 ? s / 12.92 : math.pow((s + 0.055) / 1.055, 2.4).toDouble();
}

/// Adds a weak reference to [target] to [list], first letting go of those
/// whose targets are gone once it has 16, 32, 64… (stage 14's DF13): an
/// effect made for each hit, or a camera for each shot, doesn't grow the
/// list for good.
void _addWeak<T extends Object>(List<WeakReference<T>> list, T target) {
  final n = list.length;
  if (n >= 16 && n & (n - 1) == 0) list.removeWhere((w) => w.target == null);
  list.add(WeakReference(target));
}
