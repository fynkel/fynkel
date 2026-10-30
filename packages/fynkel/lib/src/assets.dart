part of 'world.dart';

/// Where a world's assets come from: the bytes of the file called [name],
/// such as `character.glb` or `textures/grass.png`. package:fynkel_flutter
/// reads the app's bundle; a game can read files or the network instead.
typedef AssetSource = Future<Uint8List> Function(String name);

/// Models, textures and the kinds of asset packages add ([AssetKind]),
/// by name, from the world's [AssetSource], loaded once and shared: ten
/// characters asking for `character.glb` load it once, and two asking in
/// the same frame wait for the same load.
///
/// Each object's [GameObject.assets] holds what it asks for until the
/// object is destroyed; each copy of a model holds it too, until it's
/// destroyed. When the last of them goes, the asset is freed completely
/// (a model's materials, meshes and textures, then the model), so the
/// memory comes back. [World.assets] holds what it loads for as long as
/// the world lives.
final class Assets {
  Assets._(this._world, this._user);

  final World _world;

  // The object holding what's asked for, or null for the world.
  final GameObject? _user;

  /// A new copy of the model in [name] (a `.glb`), to add: a GameObject
  /// tree sharing the model's meshes, materials and textures with every
  /// other copy ([Model.instantiate]). Find its parts by name once it's
  /// added.
  Future<GameObject> instantiate(String name) async {
    final asset = _hold(name, _model);
    final model = await asset.ready as Model;
    _stillUsable(name);
    asset.users++;
    final copy = model.instantiate();
    copy._described!.asset = asset;
    return copy;
  }

  /// The texture in [name] (a `.png` or `.jpg`), shared by everything
  /// that asks for it. Freed when the last object holding it is
  /// destroyed, or, if a material still uses it then, with the material.
  Future<Texture2D> texture(String name) async {
    final asset = _hold(name, _texture);
    final texture = await asset.ready as Texture2D;
    _stillUsable(name);
    return texture;
  }

  /// The sky image in [name] (a Radiance `.hdr`), for [Sky.image], shared
  /// by everything that asks for it. A world showing it keeps its own
  /// share, so the sky stays when the last object holding it goes.
  Future<SkyImage> skyImage(String name) async {
    final asset = _hold(name, _skyImage);
    final image = await asset.ready as SkyImage;
    _stillUsable(name);
    return image;
  }

  /// The shader in [name] (a `.wgsl`), compiled, shared by everything
  /// that asks for it. A shader that doesn't compile throws a
  /// [ShaderError]. [World.reloadShaders] reads it again.
  Future<Shader> shader(String name) async {
    final asset = _hold(name, _shader);
    final shader = await asset.ready as Shader;
    _stillUsable(name);
    return shader;
  }

  /// The font in [name] (a `.ttf` or `.otf`), for [Mesh.text], shared by
  /// everything that asks for it. A file that isn't a font throws a
  /// [FormatException].
  Future<Font> font(String name) async {
    final asset = _hold(name, _font);
    final font = await asset.ready as Font;
    _stillUsable(name);
    return font;
  }

  /// The asset in [name] as [kind] loads it: a package's kind of asset,
  /// such as a sound, held and shared as a texture is, and freed with
  /// [AssetKind.free]. A package gives it a getter of its own
  /// (`assets.sound('hit.wav')`).
  Future<T> load<T extends Object>(String name, AssetKind<T> kind) async {
    final asset = _hold(name, kind);
    final value = await asset.ready as T;
    _stillUsable(name);
    return value;
  }

  /// Loads [names] without adding anything, so what asks for them later
  /// has them at once: models (`.glb`), textures (`.png`, `.jpg`,
  /// `.jpeg`), sky images (`.hdr`), shaders (`.wgsl`) and the kinds packages added ([WorldSystems.addAssetKind]),
  /// told apart by their extension. They're held as anything else asked
  /// for here is.
  Future<void> preload(Iterable<String> names) async {
    final assets = [for (final name in names) _hold(name, _kindOf(name))];
    await Future.wait([for (final asset in assets) asset.ready]);
    _stillUsable(assets.map((a) => a.name).join(', '));
  }

  AssetKind<Object> _kindOf(String name) {
    final lower = name.toLowerCase();
    final kinds = <AssetKind<Object>>[_model, _texture, _skyImage, _shader, _font, ..._world._assetKinds];
    for (final kind in kinds) {
      for (final extension in kind.extensions) {
        if (lower.endsWith(extension.toLowerCase())) return kind;
      }
    }
    final known = [for (final kind in kinds) '${kind.name} (${kind.extensions.join(', ')})'].join(', ');
    throw ArgumentError.value(name, 'name', 'is none of the kinds this world loads: $known');
  }

  /// Takes a use of [name] for [_user] (or the world), starting its load
  /// if no one has it.
  _Asset _hold(String name, AssetKind<Object> kind) {
    _stillUsable(name);
    final world = _world;
    final source = world._source;
    if (source == null) {
      throw StateError('this World has no asset source to load $name from: make it with World(source: ...)');
    }
    var asset = world._assets[name];
    if (asset != null && asset.kind.runtimeType != kind.runtimeType) {
      throw ArgumentError.value(name, 'name', 'is loaded as a ${asset.kind.name}, not as a ${kind.name}');
    }
    asset ??= world._assets[name] = _Asset(world, name, kind)..start(source);
    asset.users++;
    if (_user case final user?) (user._uses ??= []).add(asset);
    return asset;
  }

  void _stillUsable(String what) {
    if (_user case final user? when user._destroyed) {
      throw StateError('$user was destroyed, so it can\'t load $what');
    }
    _world.pointer;
  }

  @override
  String toString() => 'Assets(${_user ?? 'World'})';
}

/// A kind of asset [Assets] loads, shares and frees, as it does models
/// and textures: a package's, such as sounds. Load one with
/// [Assets.load]; [WorldSystems.addAssetKind] lets [Assets.preload] load
/// it by its extension too. One instance per kind is the rule (a `const`,
/// or a `static final`): a name is loaded as one kind only.
abstract class AssetKind<T extends Object> {
  /// A kind; `const`, so one instance can be shared.
  const AssetKind();

  /// What the kind is called in errors: `sound`.
  String get name;

  /// The ends of the file names it loads, with the dot: `['.wav', '.ogg']`.
  List<String> get extensions;

  /// Makes the asset from [bytes], the file [name]'s, for [world].
  Future<T> load(World world, String name, Uint8List bytes);

  /// Frees [value] when the last object holding it is destroyed, or when
  /// [world] is disposed (before its systems are). What it throws is
  /// reported to [World.errors].
  void free(World world, T value) {}

  /// Whether a hot reload reads this kind's files again ([reload]): false
  /// unless a kind says so.
  bool get reloads => false;

  /// Puts [bytes], the file [name]'s as it is now, into [value], which was
  /// loaded from it ([World.reloadAssets], on a hot reload): a sound whose
  /// file was changed plays the new sound. Only for a kind that [reloads].
  /// What it throws is reported to [World.errors], and [value] stays as it
  /// was.
  Future<void> reload(World world, String name, T value, Uint8List bytes) async {}

  void _free(World world, Object value) => free(world, value as T);

  Future<void> _reload(World world, String name, Object value, Uint8List bytes) =>
      reload(world, name, value as T, bytes);
}

/// Models and textures, which the world frees itself ([_Resources._free]).
sealed class _BuiltInKind<T extends Object> extends AssetKind<T> {
  const _BuiltInKind();
}

final class _ModelKind extends _BuiltInKind<Model> {
  const _ModelKind();

  @override
  String get name => 'model';

  @override
  List<String> get extensions => const ['.glb'];

  @override
  Future<Model> load(World world, String name, Uint8List bytes) => Model._load(bytes, name);
}

final class _TextureKind extends _BuiltInKind<Texture2D> {
  const _TextureKind();

  @override
  String get name => 'texture';

  @override
  List<String> get extensions => const ['.png', '.jpg', '.jpeg'];

  @override
  Future<Texture2D> load(World world, String name, Uint8List bytes) => Texture2D.load(bytes);
}

final class _ShaderKind extends _BuiltInKind<Shader> {
  const _ShaderKind();

  @override
  String get name => 'shader';

  @override
  List<String> get extensions => const ['.wgsl'];

  @override
  Future<Shader> load(World world, String name, Uint8List bytes) async => Shader(utf8.decode(bytes), name: name);
}

/// Sky images: not one of a world's resources, so freed as a package's
/// kind is.
final class _SkyImageKind extends AssetKind<SkyImage> {
  const _SkyImageKind();

  @override
  String get name => 'sky image';

  @override
  List<String> get extensions => const ['.hdr'];

  @override
  Future<SkyImage> load(World world, String name, Uint8List bytes) => SkyImage.load(bytes);

  @override
  void free(World world, SkyImage value) => value.dispose();
}

const _model = _ModelKind();
const _texture = _TextureKind();
const _skyImage = _SkyImageKind();
const _shader = _ShaderKind();

/// One loaded (or loading) asset, and how many uses of it are held.
final class _Asset {
  _Asset(this.world, this.name, this.kind);

  final World world;
  final String name;
  final AssetKind<Object> kind;
  late final Future<Object> ready;

  // The model or texture, once loaded and while it's held.
  Object? value;
  int users = 0;

  void start(AssetSource source) {
    ready = () async {
      try {
        final bytes = await source(name);
        final loaded = await kind.load(world, name, bytes);
        if (users == 0 || world.isDisposed) {
          // Everyone who wanted it went while it loaded.
          world._freeAsset(kind, loaded, null);
        } else {
          value = loaded;
        }
        return loaded;
      } catch (_) {
        // Not kept, so asking again tries again.
        if (identical(world._assets[name], this)) world._assets.remove(name);
        rethrow;
      }
    }();
  }

  @override
  String toString() => "asset '$name'";
}

extension on World {
  /// Gives back one use of [asset], for [user], and frees it when that
  /// was the last.
  void _release(_Asset asset, GameObject? user) {
    if (--asset.users > 0) return;
    if (identical(_assets[asset.name], asset)) _assets.remove(asset.name);
    final value = asset.value;
    asset.value = null;
    if (value != null) _freeAsset(asset.kind, value, user);
  }

  /// Frees [value], a [kind] of asset [user] held (or the world).
  void _freeAsset(AssetKind<Object> kind, Object value, GameObject? user) {
    if (kind is _BuiltInKind) return _free([value], user);
    try {
      kind._free(this, value);
    } catch (error, stack) {
      _report(user, error, stack);
    }
  }
}
