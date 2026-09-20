// Observable gameplay models for the HUD (M3-13, blueprint §10, ADR-0008,
// docs/hud.md): the engine snapshot never feeds widgets — a `Behaviour`
// writes what the screen shows into a `GameModel`'s fields from `update`,
// and the widgets bound to those fields rebuild, nothing else does.
//
// `package:fynkel` imports nothing from Flutter, so the field type is its
// own `ModelValue<T>` with the `ValueNotifier` shape (`value`, listeners);
// `package:fynkel_flutter` adapts it (`ModelBuilder`, `.listenable`).

/// An observable field of a [GameModel]: assigning [value] notifies its
/// listeners when the value changed (`==`), synchronously, on the caller's
/// stack — a bound widget marks itself dirty and rebuilds once in the
/// frame's build phase however many writes the tick made.
class ModelValue<T> {
  ModelValue(this._value);

  T _value;
  final List<void Function()> _listeners = [];
  bool _disposed = false;

  T get value => _value;
  set value(T v) {
    if (_disposed || v == _value) return;
    _value = v;
    notify();
  }

  /// Sets and notifies even when `==` says nothing changed (a mutable
  /// value edited in place).
  void force(T v) {
    if (_disposed) return;
    _value = v;
    notify();
  }

  /// Runs the listeners (after an in-place edit of a mutable [value]).
  void notify() {
    if (_listeners.isEmpty) return;
    for (final l in _listeners.toList(growable: false)) {
      l();
    }
  }

  bool get hasListeners => _listeners.isNotEmpty;
  void addListener(void Function() listener) => _listeners.add(listener);
  void removeListener(void Function() listener) => _listeners.remove(listener);

  /// Drops every listener; later writes are ignored.
  void dispose() {
    _disposed = true;
    _listeners.clear();
  }

  @override
  String toString() => 'ModelValue($_value)';
}

/// What the screen shows of the game, as observable fields.
///
/// Subclass it with one [ModelValue] per fact the HUD needs (`score`,
/// `health`, `screen`), register the fields with [field] so [dispose]
/// and [notifyAll] reach them, and write them from a `Behaviour`'s
/// `update` (or from `SceneManager` listeners, settings, anything on the
/// main isolate). The pattern for menu / HUD / pause / settings / results
/// screens is in `docs/hud.md` and `docs/design/dart-sdk.md` §21.5.
///
/// ```dart
/// class HudModel extends GameModel {
///   late final score = field(ModelValue(0));
///   late final coins = field(ModelValue(0));
/// }
/// ```
///
/// A `late final` field registers on its first access (a field nobody
/// touched has no listeners, so [notifyAll] and [dispose] lose nothing by
/// not knowing it; one first touched after [dispose] is disposed at once).
abstract class GameModel {
  final List<ModelValue<Object?>> _fields = [];
  bool _disposed = false;

  /// Registers a field with the model (returns it, for `late final`).
  F field<F extends ModelValue<Object?>>(F value) {
    if (_disposed) value.dispose();
    _fields.add(value);
    return value;
  }

  /// The registered fields.
  List<ModelValue<Object?>> get fields => List.unmodifiable(_fields);

  /// Notifies every field's listeners (after a bulk edit).
  void notifyAll() {
    for (final f in _fields) {
      f.notify();
    }
  }

  /// Disposes every registered field.
  void dispose() {
    _disposed = true;
    for (final f in _fields) {
      f.dispose();
    }
  }
}
