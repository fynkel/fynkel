part of 'world.dart';

/// A material's own look, written in WGSL (docs/guide.md, "Shaders"): a
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
/// ([MeshRenderer.set]), as a hit flash. A shader can be used in any
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

  Pointer<FyShader> get _live {
    if (_pointer == nullptr) throw StateError('$this was disposed');
    return _pointer;
  }

  /// Whether it has a `surface`, which the engine lights; false for an
  /// `unlit` one.
  bool get isLit => _layout.lit;

  /// The names of its `Values`, which [Material.set] sets, in order.
  List<String> get values => [for (final v in _layout.values) v.name];

  /// The names of its textures, which [Material.set] sets too, in order.
  List<String> get textures => [for (final t in _layout.textures) t.name];

  /// The names of its `PerObject` numbers, which [MeshRenderer.set] sets,
  /// in order.
  List<String> get perObject => List.unmodifiable(_layout.own);

  /// Whether [dispose] was called; a disposed shader can't be used in a
  /// material (a StateError).
  bool get isDisposed => _pointer == nullptr;

  /// Puts [source] in place of its code, as when its file changes: every
  /// material using it, in every world, draws with the new code from the
  /// next frame, its values set again by name. Throws a [ShaderError] and
  /// keeps the code it had when [source] doesn't compile.
  void reload(String source) {
    final old = _live;
    final (pointer, layout) = _compile(source, name);
    _finalizer.detach(this);
    _pointer = pointer;
    _layout = layout;
    _finalizer.attach(this, pointer.cast(), detach: this);
    for (final world in _release.worlds) {
      world._reloadShader(this);
    }
    fy_shader_free(old);
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
  _ShaderLayout(this.lit, this.size, this.values, this.textures, this.own);

  factory _ShaderLayout.parse(String text) {
    var lit = true;
    var size = 0;
    final values = <_ShaderValue>[];
    final textures = <({String name, bool srgb})>[];
    final own = <String>[];
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
        case ['texture', final name, final space]:
          textures.add((name: name, srgb: space == 'srgb'));
        case ['own', final name]:
          own.add(name);
      }
    }
    return _ShaderLayout(lit, size, values, textures, own);
  }

  final bool lit;

  /// How many bytes its `Values` take.
  final int size;
  final List<_ShaderValue> values;
  final List<({String name, bool srgb})> textures;
  final List<String> own;

  _ShaderValue? value(String name) {
    for (final v in values) {
      if (v.name == name) return v;
    }
    return null;
  }

  int texture(String name) => textures.indexWhere((t) => t.name == name);

  /// Every name a material sets, for messages.
  String get names {
    final all = [for (final v in values) v.name, for (final t in textures) t.name];
    return all.isEmpty ? 'none' : all.join(', ');
  }
}

/// One of a shader's `Values`: `f32`, `i32` or `u32`, of [parts] (1 for a
/// number, 2 to 4 for a vector), at [offset] bytes.
final class _ShaderValue {
  _ShaderValue(this.name, this.kind, this.parts, this.offset);

  final String name;
  final String kind;
  final int parts;
  final int offset;

  /// [value] as its parts, or what's wrong with it.
  List<num> parse(Object value, Shader shader) {
    List<num> wrong() => throw ArgumentError.value(
      value,
      name,
      "is no value for $shader's $name, which is ${parts == 1 ? kind : 'vec$parts<$kind>'}",
    );
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
      if (kind == 'f32' && !n.isFinite) throw ArgumentError.value(value, name, 'must be finite');
      if (kind != 'f32' && n is! int) return wrong();
      if (kind == 'u32' && n < 0) throw ArgumentError.value(value, name, 'must be 0 or more');
    }
    return parts_;
  }

  static double _toLinear(double s) => s <= 0.04045 ? s / 12.92 : math.pow((s + 0.055) / 1.055, 2.4).toDouble();
}
