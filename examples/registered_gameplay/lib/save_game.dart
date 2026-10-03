import 'dart:convert';

import 'package:fynkel/gameplay.dart';

import 'combat.dart';
import 'generated/behaviours.g.dart';

/// The sample's versioned gameplay save, not an engine execution snapshot.
/// Keys are game-owned persistent IDs, independent of names and EntityRef.
/// Every referenced object must be included. Disk storage belongs to the app.
String saveCombat(Map<String, GameObject> objects) {
  final ids = <GameObject, String>{};
  for (final entry in objects.entries) {
    if (entry.key.isEmpty ||
        !entry.value.isAlive ||
        ids.containsKey(entry.value)) {
      throw ArgumentError(
        'Save IDs must be nonempty and objects must be unique and alive.',
      );
    }
    ids[entry.value] = entry.key;
  }
  final rows = <Map<String, Object?>>[];
  for (final entry in objects.entries) {
    final health = entry.value.readData(gameHealthType);
    if (health == null || !health.hp.isFinite) {
      throw StateError('${entry.key} needs finite Health data.');
    }
    final target = health.target;
    if (target != null && !ids.containsKey(target)) {
      throw StateError('${entry.key} references an object outside this save.');
    }
    rows.add({
      'id': entry.key,
      'name': entry.value.name,
      'hp': health.hp,
      'invulnerable': health.invulnerable,
      'target': target == null ? null : ids[target],
    });
  }
  return jsonEncode({
    'format': 'fynkel.combat-save',
    'version': 1,
    'objects': rows,
  });
}

/// Restores new objects in one edit, resolving all references before commit.
/// The caller registers combat schemas first and owns replacement of a session.
/// Failure leaves the destination world unchanged. No live handles are loaded.
Map<String, GameObject> restoreCombat(World world, String source) {
  final document = jsonDecode(source);
  if (document is! Map<String, dynamic> ||
      document['format'] != 'fynkel.combat-save' ||
      document['version'] is! int ||
      document['version'] != 1 ||
      document.keys.any(
        (k) => !const {'format', 'version', 'objects'}.contains(k),
      ) ||
      document['objects'] is! List) {
    throw const FormatException(
      'Unsupported combat save; an explicit migration is required.',
    );
  }
  final rows = <String, Map<String, dynamic>>{};
  for (final raw in document['objects'] as List) {
    if (raw is! Map<String, dynamic> ||
        raw.length != 5 ||
        raw['id'] is! String ||
        (raw['id'] as String).isEmpty ||
        raw['name'] is! String ||
        raw['hp'] is! num ||
        !(raw['hp'] as num).isFinite ||
        raw['invulnerable'] is! bool ||
        !raw.containsKey('target') ||
        (raw['target'] != null && raw['target'] is! String)) {
      throw const FormatException('Invalid saved combat object.');
    }
    if (rows.containsKey(raw['id'])) {
      throw const FormatException('Duplicate saved object ID.');
    }
    rows[raw['id'] as String] = raw;
  }
  for (final row in rows.values) {
    if (row['target'] != null && !rows.containsKey(row['target'])) {
      throw const FormatException('Unresolved saved object reference.');
    }
  }
  return world.edit(() {
    final objects = {
      for (final row in rows.entries)
        row.key: world.spawn(name: row.value['name'] as String),
    };
    for (final row in rows.entries) {
      objects[row.key]!.writeData(
        gameHealthType,
        Health()
          ..hp = (row.value['hp'] as num).toDouble()
          ..invulnerable = row.value['invulnerable'] as bool
          ..target = objects[row.value['target']],
      );
    }
    return objects;
  });
}
