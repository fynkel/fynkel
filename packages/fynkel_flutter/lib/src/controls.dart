import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:fynkel/fynkel.dart' show Input, KeyCode;

import 'input_feed.dart' show keyCodeOf;

/// The keys a player can change, by name, for a settings page (Unity's
/// rebinding): `Controls({'up': KeyCode.w, ..., 'dash': KeyCode.space})`.
/// The app makes it, the settings page changes it ([ControlsSettings]),
/// and the game binds it as it starts and again each time it changes:
///
/// ```dart
/// controls.bindTo(input);
/// on(controls.changed, (_) => controls.bindTo(input));
/// ```
///
/// [toJson] and [Controls.fromJson] keep the player's keys between runs,
/// in what the app saves its settings with.
class Controls extends ChangeNotifier {
  /// Controls with these names and first keys.
  Controls(Map<String, KeyCode> defaults) : defaults = Map.unmodifiable(defaults), _keys = Map.of(defaults);

  /// Controls with these names and first keys, and the keys [toJson] gave
  /// (`Controls.fromJson(saved, defaults: {...})`); [defaults]' alone when
  /// [json] is null, as before anything is saved. A name or key it doesn't
  /// know (an older version's) is left out, and the rest kept as they are.
  factory Controls.fromJson(Map<String, Object?>? json, {required Map<String, KeyCode> defaults}) {
    final controls = Controls(defaults);
    for (final MapEntry(key: name, value: keyName) in (json ?? const <String, Object?>{}).entries) {
      if (!controls._keys.containsKey(name) || keyName is! String) continue;
      if (KeyCode.values.asNameMap()[keyName] case final key?) controls._swap(name, key);
    }
    return controls;
  }

  /// The names and their first keys, in order: what [reset] goes back to.
  final Map<String, KeyCode> defaults;

  final Map<String, KeyCode> _keys;

  /// The key for `name`. Throws an [ArgumentError] for a name that isn't
  /// one of [defaults]'.
  KeyCode operator [](String name) => _keys[name] ?? (throw _unknown(name));

  ArgumentError _unknown(String name) => ArgumentError.value(name, 'name', 'is none of ${defaults.keys.join(', ')}');

  /// Gives [name] the [key]. What had that key takes [name]'s old one, so
  /// no two share a key.
  void set(String name, KeyCode key) {
    if (_swap(name, key)) notifyListeners();
  }

  bool _swap(String name, KeyCode key) {
    final old = _keys[name] ?? (throw _unknown(name));
    if (old == key) return false;
    for (final other in _keys.keys) {
      if (_keys[other] == key) _keys[other] = old;
    }
    _keys[name] = key;
    return true;
  }

  /// Back to [defaults].
  void reset() {
    _keys.addAll(defaults);
    notifyListeners();
  }

  /// Binds them in [input]: `up`, `down`, `left` and `right` are the
  /// `'move'` axis ([Input.axis2]), which the arrow keys move too; every
  /// other name is an action of that name ([Input.action]).
  void bindTo(Input input) {
    if (_keys.keys.any(_move.contains)) {
      input.bindAxis2(
        'move',
        up: [?_keys['up'], KeyCode.arrowUp],
        down: [?_keys['down'], KeyCode.arrowDown],
        left: [?_keys['left'], KeyCode.arrowLeft],
        right: [?_keys['right'], KeyCode.arrowRight],
      );
    }
    for (final MapEntry(key: name, value: key) in _keys.entries) {
      if (!_move.contains(name)) input.bindAction(name, [key]);
    }
  }

  static const _move = {'up', 'down', 'left', 'right'};

  /// The keys by name, as their [KeyCode] names: `{'dash': 'space'}`.
  Map<String, String> toJson() => {for (final MapEntry(:key, :value) in _keys.entries) key: value.name};
}

/// A settings page's list of [controls]: a heading with a Reset button
/// and a [note], then one row for each name in [labels], in their order,
/// whose button asks for a key ([askForKey]).
class ControlsSettings extends StatelessWidget {
  /// The list for [controls], with these [labels] by name.
  const ControlsSettings(this.controls, {super.key, required this.labels, this.title = 'Controls', this.note});

  /// What it changes.
  final Controls controls;

  /// The names shown, and what each is called on the page.
  final Map<String, String> labels;

  /// The heading.
  final String title;

  /// Said under the heading, as `'The arrow keys always move too.'`.
  final String? note;

  @override
  Widget build(BuildContext context) => Column(
    mainAxisSize: MainAxisSize.min,
    children: [
      ListTile(
        title: Text(title, style: Theme.of(context).textTheme.titleMedium),
        subtitle: note == null ? null : Text(note!),
        trailing: TextButton(onPressed: controls.reset, child: const Text('Reset')),
      ),
      ListenableBuilder(
        listenable: controls,
        builder: (context, _) => Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            for (final MapEntry(key: name, value: label) in labels.entries)
              ListTile(
                title: Text(label),
                trailing: OutlinedButton(
                  onPressed: () async {
                    final key = await askForKey(context, label);
                    if (key != null) controls.set(name, key);
                  },
                  child: Text(controls[name].label),
                ),
              ),
          ],
        ),
      ),
    ],
  );
}

/// Waits for the player to press a key for [what], and gives it; null for
/// Escape, or a key the game has none for.
Future<KeyCode?> askForKey(BuildContext context, String what) => showDialog<KeyCode>(
  context: context,
  builder: (context) => Focus(
    autofocus: true,
    onKeyEvent: (node, event) {
      if (event is KeyDownEvent) {
        Navigator.of(context).pop(event.logicalKey == LogicalKeyboardKey.escape ? null : keyCodeOf(event));
      }
      return KeyEventResult.handled;
    },
    child: AlertDialog(title: Text('Press a key for $what'), content: const Text('Escape keeps the one it has.')),
  ),
);
