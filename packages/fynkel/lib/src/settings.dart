// User settings (M3-12, docs/scenes.md § Settings): the `settings.json`
// document — audio bus gains, the quality profile, input rebinds — read
// once at startup from the platform's application-support directory and
// written on every change, reached through `GameContext.settings`.

import 'dart:convert';
import 'dart:io';

import '../generated/schema.g.dart' as gen;
import 'world.dart';

/// The `settings.json` document (`schemas/settings.schema.json`): the
/// `fynkel.Settings` record's fields plus `input.rebinds`. Immutable;
/// [copyWith] makes the next one.
class SettingsDocument {
  const SettingsDocument({
    this.masterVolume = 1,
    this.musicVolume = 1,
    this.sfxVolume = 1,
    this.muted = false,
    this.quality = '',
    this.rebinds = const {},
  });

  /// Every field at its default: full volume, not muted, the project's
  /// default quality profile, no rebinds.
  static const SettingsDocument defaults = SettingsDocument();

  static const String kind = 'settings';
  static const int formatVersion = 1;

  /// Reads a document; a field it lacks keeps its default (an older
  /// file), a wrong kind, format version or field shape is a
  /// [FormatException].
  factory SettingsDocument.fromJson(Map<String, Object?> j) {
    if (j['fynkel'] != kind) {
      throw FormatException('not a settings document: "fynkel" is ${j['fynkel']}, expected "$kind"');
    }
    final version = j['formatVersion'];
    if (version != formatVersion) {
      throw FormatException('settings formatVersion $version is not $formatVersion');
    }
    double gain(String key) {
      final v = j[key];
      if (v == null) return 1;
      if (v is! num) throw FormatException('$key must be a number, got $v');
      return v.toDouble();
    }

    final muted = j['muted'];
    if (muted != null && muted is! bool) throw FormatException('muted must be a boolean, got $muted');
    final quality = j['quality'];
    if (quality != null && quality is! String) throw FormatException('quality must be a string, got $quality');
    final rebinds = <String, List<String>>{};
    final input = j['input'];
    if (input != null) {
      if (input is! Map) throw const FormatException('input must be an object');
      final r = input['rebinds'];
      if (r != null) {
        if (r is! Map) throw const FormatException('input.rebinds must be an object');
        for (final e in r.entries) {
          final v = e.value;
          if (v is! List || v.any((b) => b is! String)) {
            throw FormatException('input.rebinds.${e.key} must be an array of strings');
          }
          rebinds[e.key as String] = List.unmodifiable(v.cast<String>());
        }
      }
    }
    return SettingsDocument(
      masterVolume: gain('master_volume'),
      musicVolume: gain('music_volume'),
      sfxVolume: gain('sfx_volume'),
      muted: muted as bool? ?? false,
      quality: quality as String? ?? '',
      rebinds: Map.unmodifiable(rebinds),
    );
  }

  /// Gain of the `master` bus, `0..1`.
  final double masterVolume;

  /// Gain of the `music` bus, `0..1`.
  final double musicVolume;

  /// Gain of the `sfx` bus, `0..1`.
  final double sfxVolume;

  /// Every bus silent; the volumes are kept.
  final bool muted;

  /// A quality profile name from the project file; empty means the
  /// project's default profile.
  final String quality;

  /// The user's bindings by action: an action here replaces the
  /// project's bindings for this user, an action absent keeps them.
  final Map<String, List<String>> rebinds;

  /// The `fynkel.Settings` wire record of these values.
  gen.Settings get record => gen.Settings(
        masterVolume: masterVolume,
        musicVolume: musicVolume,
        sfxVolume: sfxVolume,
        muted: muted,
        quality: quality,
      );

  SettingsDocument copyWith({
    double? masterVolume,
    double? musicVolume,
    double? sfxVolume,
    bool? muted,
    String? quality,
    Map<String, List<String>>? rebinds,
  }) =>
      SettingsDocument(
        masterVolume: masterVolume ?? this.masterVolume,
        musicVolume: musicVolume ?? this.musicVolume,
        sfxVolume: sfxVolume ?? this.sfxVolume,
        muted: muted ?? this.muted,
        quality: quality ?? this.quality,
        rebinds: rebinds ?? this.rebinds,
      );

  /// This document with [action] bound to [bindings] for this user
  /// (empty: the project's bindings again).
  SettingsDocument rebind(String action, List<String> bindings) {
    final next = Map<String, List<String>>.from(rebinds);
    if (bindings.isEmpty) {
      next.remove(action);
    } else {
      next[action] = List.unmodifiable(bindings);
    }
    return copyWith(rebinds: Map.unmodifiable(next));
  }

  static final RegExp _binding = RegExp(r'^(keyboard|mouse|gamepad|touch):[A-Za-z0-9_./+-]{1,64}(\*[0-9]*\.?[0-9]+([eE][-+]?[0-9]+)?)?$');

  /// What is wrong with these values (empty when nothing): a gain out
  /// of `0..1`, a binding that does not fit the `<device>:<control>`
  /// grammar. The runtime's own checks (`fynkel validate` on the same
  /// document, the world's binding errors) are the reference.
  List<String> validate() {
    final out = <String>[];
    void gain(String name, double v) {
      if (v.isNaN || v < 0 || v > 1) out.add('$name must be within 0..1, got $v');
    }

    gain('master_volume', masterVolume);
    gain('music_volume', musicVolume);
    gain('sfx_volume', sfxVolume);
    for (final e in rebinds.entries) {
      if (e.key.isEmpty) out.add('input.rebinds has an empty action name');
      for (final b in e.value) {
        if (!_binding.hasMatch(b)) out.add('input.rebinds.${e.key}: "$b" is not <device>:<control>');
      }
    }
    return out;
  }

  /// The canonical document: every field, `snake_case` keys, the
  /// rebinds sorted by action.
  Map<String, Object?> toJson() {
    final keys = rebinds.keys.toList()..sort();
    return {
      'fynkel': kind,
      'formatVersion': formatVersion,
      'master_volume': masterVolume,
      'music_volume': musicVolume,
      'sfx_volume': sfxVolume,
      'muted': muted,
      'quality': quality,
      'input': {
        'rebinds': {for (final k in keys) k: rebinds[k]},
      },
    };
  }

  String toText() => '${const JsonEncoder.withIndent('  ').convert(toJson())}\n';

  @override
  bool operator ==(Object other) =>
      other is SettingsDocument &&
      other.masterVolume == masterVolume &&
      other.musicVolume == musicVolume &&
      other.sfxVolume == sfxVolume &&
      other.muted == muted &&
      other.quality == quality &&
      _sameRebinds(other.rebinds, rebinds);

  static bool _sameRebinds(Map<String, List<String>> a, Map<String, List<String>> b) {
    if (a.length != b.length) return false;
    for (final e in a.entries) {
      final o = b[e.key];
      if (o == null || o.length != e.value.length) return false;
      for (var i = 0; i < o.length; i++) {
        if (o[i] != e.value[i]) return false;
      }
    }
    return true;
  }

  @override
  int get hashCode => Object.hash(masterVolume, musicVolume, sfxVolume, muted, quality, rebinds.length);

  @override
  String toString() => 'SettingsDocument(${jsonEncode(toJson())})';
}

/// The user's settings as a `GameContext` service: read at startup,
/// [update]d on change, applied to the world ([SettingsStore]).
abstract interface class SettingsService {
  /// The settings in force.
  SettingsDocument get current;

  /// Replaces the settings: persisted (when the service persists) and
  /// applied before this returns; listeners run after.
  void update(SettingsDocument next);

  /// Runs [listener] after every [update].
  void addListener(void Function(SettingsDocument settings) listener);
  void removeListener(void Function(SettingsDocument settings) listener);
}

/// The settings store (M3-12): a file at [path] (or memory only, for
/// tests and worlds without a user), read once with [SettingsStore.open]
/// — a missing file is the defaults, an unreadable or invalid one is
/// reported through [onError] and the defaults are used — and written
/// on every [update] through a temporary file (a crash keeps the old
/// one). Attached to a [World] ([attach], or `World.useSettings`) it
/// lays the rebinds over the project's actions on attach and after every
/// update; the audio gains and the quality profile are values a host
/// reads from [current] (there is no audio engine yet; a Flutter host
/// picks the render quality profile).
class SettingsStore implements SettingsService {
  SettingsStore({this.path, SettingsDocument initial = SettingsDocument.defaults, World? world})
      : _current = initial {
    _world = world;
  }

  /// Reads [path] (a missing file is [SettingsDocument.defaults]); an
  /// unreadable or invalid file goes to [onError] (default: prints) and
  /// the defaults are used — the next [update] overwrites the file.
  factory SettingsStore.open(String path, {World? world, void Function(String message)? onError}) {
    final store = SettingsStore(path: path, world: world);
    if (onError != null) store.onError = onError;
    store.load();
    return store;
  }

  /// `<application support>/<appName>/settings.json`: on macOS
  /// `~/Library/Application Support/<appName>`, on Linux
  /// `$XDG_CONFIG_HOME/<appName>` (default `~/.config/<appName>`), on
  /// Windows `%APPDATA%\<appName>`; elsewhere (iOS, Android — where the
  /// Flutter host knows the sandbox) the current directory. A host that
  /// knows better passes its own [path].
  static String defaultPath(String appName, {Map<String, String>? environment}) {
    final env = environment ?? Platform.environment;
    final home = env['HOME'] ?? env['USERPROFILE'] ?? '.';
    final String base;
    if (Platform.isMacOS) {
      base = '$home/Library/Application Support';
    } else if (Platform.isWindows) {
      base = env['APPDATA'] ?? '$home\\AppData\\Roaming';
    } else if (Platform.isLinux) {
      base = env['XDG_CONFIG_HOME'] ?? '$home/.config';
    } else {
      base = Directory.current.path;
    }
    final sep = Platform.isWindows ? '\\' : '/';
    return '$base$sep$appName${sep}settings.json';
  }

  /// The file, or null for a memory-only store.
  final String? path;
  SettingsDocument _current;
  World? _world;
  final List<void Function(SettingsDocument)> _listeners = [];

  /// Where a load or save problem goes (default: `print`).
  void Function(String message) onError = print;

  /// The last load or save problem, null when the last one succeeded.
  String? lastError;

  bool get isPersistent => path != null;

  @override
  SettingsDocument get current => _current;

  /// The world the rebinds apply to (null: none yet).
  World? get world => _world;

  /// Applies the bus gains and the mute switch (M3-11) and the rebinds
  /// to [world] now and after every [update].
  void attach(World world) {
    _world = world;
    apply();
  }

  /// Re-reads [path] into [current] and applies it; false when the file
  /// could not be read or is invalid (then [current] is unchanged, the
  /// defaults on a first open, and [lastError] says why).
  bool load() {
    final p = path;
    if (p == null) return true;
    final file = File(p);
    String text;
    try {
      if (!file.existsSync()) {
        lastError = null;
        apply();
        return true;
      }
      text = file.readAsStringSync();
    } on IOException catch (e) {
      return _fail('cannot read $p: $e');
    }
    try {
      final json = jsonDecode(text);
      if (json is! Map) throw const FormatException('the document is not an object');
      final doc = SettingsDocument.fromJson(json.cast<String, Object?>());
      final problems = doc.validate();
      if (problems.isNotEmpty) throw FormatException(problems.join('; '));
      _current = doc;
      lastError = null;
      apply();
      return true;
    } on FormatException catch (e) {
      return _fail('$p: ${e.message}');
    }
  }

  /// Writes [current] to [path] (nothing for a memory store); false and
  /// [lastError] when it could not be written.
  bool save() {
    final p = path;
    if (p == null) return true;
    try {
      final file = File(p);
      file.parent.createSync(recursive: true);
      final tmp = File('$p.tmp');
      tmp.writeAsStringSync(_current.toText(), flush: true);
      tmp.renameSync(p);
      lastError = null;
      return true;
    } on IOException catch (e) {
      return _fail('cannot write $p: $e');
    }
  }

  bool _fail(String message) {
    lastError = message;
    onError(message);
    return false;
  }

  /// Lays [current]'s rebinds over the attached world's actions (nothing
  /// without a world or before its project is loaded).
  void apply() {
    final w = _world;
    if (w == null) return;
    w.audio.applySettings(_current);
    if (w.cacheDir == null) return;
    w.bindActions(_current.rebinds);
  }

  @override
  void update(SettingsDocument next) {
    final problems = next.validate();
    if (problems.isNotEmpty) throw ArgumentError.value(next, 'next', problems.join('; '));
    _current = next;
    save();
    apply();
    for (final l in _listeners.toList(growable: false)) {
      l(next);
    }
  }

  /// [update] with [current] changed by [change].
  void edit(SettingsDocument Function(SettingsDocument current) change) => update(change(_current));

  @override
  void addListener(void Function(SettingsDocument settings) listener) => _listeners.add(listener);

  @override
  void removeListener(void Function(SettingsDocument settings) listener) => _listeners.remove(listener);
}
