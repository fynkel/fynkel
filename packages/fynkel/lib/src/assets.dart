part of 'world.dart';

/// Where a world's assets come from: the bytes of the file called [name],
/// such as `character.glb` or `textures/grass.png`. package:fynkel_flutter
/// reads the app's bundle; a game can read files or the network instead.
typedef AssetSource = Future<Uint8List> Function(String name);

/// Models and textures by name, from the world's [AssetSource], loaded
/// once and shared: ten characters asking for `character.glb` load it
/// once, and two asking in the same frame wait for the same load.
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
  Future<GameObject> model(String name) async {
    final asset = _hold(name, _Kind.model);
    final model = await asset.ready as Model;
    _stillUsable(name);
    asset.users++;
    final copy = model.instantiate();
    copy._described!.asset = asset;
    return copy;
  }

  /// The texture in [name] (a `.png` or `.jpg`), shared by everything
  /// that asks for it. Give it to a material the object [GameObject.own]s,
  /// so the material goes first and the texture can be freed.
  Future<Texture2D> texture(String name) async {
    final asset = _hold(name, _Kind.texture);
    final texture = await asset.ready as Texture2D;
    _stillUsable(name);
    return texture;
  }

  /// Loads [names] without adding anything, so what asks for them later
  /// has them at once: models (`.glb`) and textures (`.png`, `.jpg`,
  /// `.jpeg`), told apart by their extension. They're held as anything
  /// else asked for here is.
  Future<void> preload(Iterable<String> names) async {
    final assets = [for (final name in names) _hold(name, _Kind.of(name))];
    await Future.wait([for (final asset in assets) asset.ready]);
    _stillUsable(assets.map((a) => a.name).join(', '));
  }

  /// Takes a use of [name] for [_user] (or the world), starting its load
  /// if no one has it.
  _Asset _hold(String name, _Kind kind) {
    _stillUsable(name);
    final world = _world;
    final source = world._source;
    if (source == null) {
      throw StateError('this World has no asset source to load $name from: make it with World(source: ...)');
    }
    var asset = world._assets[name];
    if (asset != null && asset.kind != kind) {
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

enum _Kind {
  model,
  texture;

  static _Kind of(String name) {
    final lower = name.toLowerCase();
    if (lower.endsWith('.glb')) return model;
    if (lower.endsWith('.png') || lower.endsWith('.jpg') || lower.endsWith('.jpeg')) return texture;
    throw ArgumentError.value(name, 'name', 'is neither a model (.glb) nor a texture (.png, .jpg)');
  }
}

/// One loaded (or loading) asset, and how many uses of it are held.
final class _Asset {
  _Asset(this.world, this.name, this.kind);

  final World world;
  final String name;
  final _Kind kind;
  late final Future<Object> ready;

  // The model or texture, once loaded and while it's held.
  Object? value;
  int users = 0;

  void start(AssetSource source) {
    ready = () async {
      try {
        final bytes = await source(name);
        final Object loaded = switch (kind) {
          _Kind.model => await Model.load(bytes),
          _Kind.texture => await Texture2D.load(bytes),
        };
        if (users == 0 || world.isDisposed) {
          // Everyone who wanted it went while it loaded.
          world._free([loaded], null);
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
    if (value != null) _free([value], user);
  }
}
