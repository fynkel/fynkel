// Typed builders for the seventeen authoring commands (docs/documents.md
// § Authoring commands, blueprint §11.1). Each returns the JSON object the
// ABI's `apply` reads, so a panel never spells a key by hand.

/// An asset reference as documents write it: `{ asset, sub? }`.
Map<String, Object?> assetRef(String asset, {String? sub}) => {'asset': asset, 'sub': ?sub};

/// A component to create: `{ id?, type, version?, data? }`.
Map<String, Object?> componentSpec(String type, {String? id, int? version, Map<String, Object?>? data}) => {
      'id': ?id,
      'type': type,
      'version': ?version,
      'data': ?data,
    };

/// The command JSON objects. Ids are UUID strings; `index` is a position in
/// the parent's children (append when absent).
final class Command {
  Command._();

  static Map<String, Object?> createObject({
    required String name,
    String? id,
    String? parent,
    int? index,
    List<Map<String, Object?>> components = const [],
  }) =>
      {
        'command': 'createObject',
        'id': ?id,
        'name': name,
        'parent': ?parent,
        'index': ?index,
        'components': components,
      };

  static Map<String, Object?> deleteObject(String id) => {'command': 'deleteObject', 'id': id};

  static Map<String, Object?> renameObject(String id, String name) =>
      {'command': 'renameObject', 'id': id, 'name': name};

  /// `parent` null moves the object to the root.
  static Map<String, Object?> reparentObject(String id, {String? parent, int? index}) =>
      {'command': 'reparentObject', 'id': id, 'parent': parent, 'index': ?index};

  /// Deep-copies the subtree under [id] with fresh ids (the copy's own id
  /// may be chosen with [newId]); one undo step.
  static Map<String, Object?> duplicateObject(String id, {String? newId, String? name, int? index}) => {
        'command': 'duplicateObject',
        'id': id,
        'newId': ?newId,
        'name': ?name,
        'index': ?index,
      };

  static Map<String, Object?> addComponent(String entity, Map<String, Object?> component) =>
      {'command': 'addComponent', 'entity': entity, 'component': component};

  static Map<String, Object?> removeComponent(String entity, String component) =>
      {'command': 'removeComponent', 'entity': entity, 'component': component};

  static Map<String, Object?> setField(String entity, String component, String field, Object? value) =>
      {'command': 'setField', 'entity': entity, 'component': component, 'field': field, 'value': value};

  static Map<String, Object?> unsetField(String entity, String component, String field) =>
      {'command': 'unsetField', 'entity': entity, 'component': component, 'field': field};

  static Map<String, Object?> assignAsset(String entity, String component, String field, Map<String, Object?> asset) =>
      {'command': 'assignAsset', 'entity': entity, 'component': component, 'field': field, 'asset': asset};

  /// Instances [prefab] (a prefab document or an imported `#prefab`
  /// hierarchy, see [assetRef]) as a new entity.
  static Map<String, Object?> instantiatePrefab({
    required Map<String, Object?> prefab,
    String? id,
    String? name,
    String? parent,
    int? index,
    String? instance,
    List<Map<String, Object?>> components = const [],
  }) =>
      {
        'command': 'instantiatePrefab',
        'id': ?id,
        'name': ?name,
        'parent': ?parent,
        'index': ?index,
        'prefab': prefab,
        'instance': ?instance,
        'components': components,
      };

  /// Adds any override op (the prefab model's `{ op, entity, ... }` object)
  /// on the instance rooted at [entity], replacing one with the same
  /// target — `setField`/`unsetField` by field (a whole key of the
  /// component's `data`, never a pointer into it), `addComponent`/
  /// `removeComponent` by component, `addObject`/`removeObject` by object.
  static Map<String, Object?> setPrefabOverrideOp(String entity, Map<String, Object?> op) =>
      {'command': 'setPrefabOverride', 'entity': entity, 'override': op};

  /// Adds a `setField` override on the instance rooted at [entity]: the
  /// prefab's [prefabEntity]/[component]/[field] take [value] here.
  static Map<String, Object?> setPrefabOverride(
    String entity, {
    required String prefabEntity,
    required String component,
    required String field,
    required Object? value,
  }) =>
      {
        'command': 'setPrefabOverride',
        'entity': entity,
        'override': {'op': 'setField', 'entity': prefabEntity, 'component': component, 'field': field, 'value': value},
      };

  /// Removes the override at [index] of the instance rooted at [entity], or
  /// every override when [index] is null.
  static Map<String, Object?> revertPrefabOverride(String entity, {int? index}) =>
      {'command': 'revertPrefabOverride', 'entity': entity, 'index': ?index};

  /// Replaces the instance rooted at [entity] by its expansion, in place:
  /// the anchor becomes a plain entity whose trailing children are the
  /// instance roots, the derived entities follow it in the document under
  /// the ids the runtime already used (M4-07). One undo step.
  static Map<String, Object?> unpackPrefab(String entity) => {'command': 'unpackPrefab', 'entity': entity};

  /// Sets a top-level or nested field of the document itself (a material's
  /// `/base_color`, a scene's `/name`) by JSON pointer.
  static Map<String, Object?> setDocumentField(String pointer, Object? value) =>
      {'command': 'setDocumentField', 'pointer': pointer, 'value': value};

  static Map<String, Object?> unsetDocumentField(String pointer) =>
      {'command': 'unsetDocumentField', 'pointer': pointer};

  /// Writes a new material document at [path] (relative to the project's
  /// assets directory); refuses to overwrite. Runs after every other
  /// command of the transaction.
  static Map<String, Object?> createMaterial({required String name, required String path, String? id, String? shader}) => {
        'command': 'createMaterial',
        'id': ?id,
        'name': name,
        'shader': ?shader,
        'path': path,
      };

  /// Saves the document canonically, in place or to [path]; runs last.
  static Map<String, Object?> saveDocument({String? path}) =>
      {'command': 'saveDocument', 'path': ?path};

  /// Every name `apply` accepts, in the documented order.
  static const names = [
    'createObject',
    'deleteObject',
    'renameObject',
    'reparentObject',
    'duplicateObject',
    'addComponent',
    'removeComponent',
    'setField',
    'unsetField',
    'assignAsset',
    'instantiatePrefab',
    'setPrefabOverride',
    'revertPrefabOverride',
    'unpackPrefab',
    'setDocumentField',
    'unsetDocumentField',
    'createMaterial',
    'saveDocument',
  ];
}
