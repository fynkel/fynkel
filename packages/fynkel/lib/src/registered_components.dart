part of 'world.dart';

/// Field kinds supported by runtime-defined ECS data. These are data values,
/// never behaviours, coroutine state or service objects.
enum ComponentFieldKind { number, integer, boolean, text, object, asset }

class ComponentField {
  const ComponentField(this.name, this.kind, this.defaultValue);
  final String name;
  final ComponentFieldKind kind;
  final Object? defaultValue;
}

/// Stable schema and a pure typed codec. Generated declarations use this same
/// contract; records are value copies, not a second mutable component store.
class ComponentType<T> {
  ComponentType({
    required this.id,
    this.version = 1,
    required List<ComponentField> fields,
    required List<Object?> Function(T) encode,
    required this.decode,
    // Keep the public named argument while exposing a covariant method, not a function-valued getter.
    // ignore: prefer_initializing_formals
  }) : _encode = encode,
       fields = List.unmodifiable(fields);
  final String id;
  final int version;
  final List<ComponentField> fields;
  final List<Object?> Function(T) _encode;
  List<Object?> encode(T value) => _encode(value);
  final T Function(List<Object?>) decode;
  Type get valueType => T;
  bool sameSchema(ComponentType<dynamic> other) =>
      id == other.id &&
      version == other.version &&
      valueType == other.valueType &&
      fields.length == other.fields.length &&
      List.generate(
        fields.length,
        (i) =>
            fields[i].name == other.fields[i].name &&
            fields[i].kind == other.fields[i].kind &&
            fields[i].defaultValue == other.fields[i].defaultValue,
      ).every((v) => v);
}

extension RegisteredComponents on World {
  Iterable<ComponentType<dynamic>> get registeredComponentTypes =>
      List.unmodifiable(_componentTypes.values);
  void registerComponent<T>(ComponentType<T> type) {
    _rejectDuringEdit('component registration');
    if (isDisposed) throw StateError('world is disposed');
    final previous = _componentTypes[type.id];
    if (previous != null && !previous.sameSchema(type)) {
      throw StateError('component ${type.id} already has a Dart codec');
    }
    if (type.version <= 0 || type.version > 0xffffffff) {
      throw ArgumentError('invalid component version');
    }
    final defaults = type.fields.map((f) => f.defaultValue).toList();
    final roundTrip = type.encode(type.decode(List.unmodifiable(defaults)));
    if (roundTrip.length != defaults.length ||
        List.generate(
          defaults.length,
          (i) => roundTrip[i] == defaults[i],
        ).contains(false)) {
      throw ArgumentError('component defaults do not round-trip');
    }
    _native.registerComponent(type.id, type.version, [
      for (final f in type.fields)
        NativeRegisteredField(
          f.name,
          NativeRegisteredValue(f.kind.index, f.defaultValue),
        ),
    ]);
    _componentTypes[type.id] = type;
  }

  void _requireComponentType<T>(ComponentType<T> type) {
    if (!(_componentTypes[type.id]?.sameSchema(type) ?? false)) {
      throw StateError('unregistered component codec ${type.id}');
    }
  }

  /// An owned, read-only population snapshot. Records do not write themselves
  /// back; owners retain ordinary GameObject liveness rules after this call.
  /// Within an edit this performs one native population read.
  List<({GameObject owner, T value})> queryComponents<T>(
    ComponentType<T> type,
  ) => _withGameplay(() {
    _requireComponentType(type);
    final edit = _gameplay!;
    final batch = edit.native.registeredQuery(type.id);
    return List.unmodifiable([
      for (final row in batch.rows)
        (
          owner: edit.object(row.$1, validatedPopulation: true),
          value: _decodeComponent(
            type,
            row.$2,
            edit,
            validatedPopulation: true,
          ),
        ),
    ]);
  });

  /// One batched read and one batched write, inside the shared transaction.
  /// Membership is frozen. Mutating registered data or entity membership from
  /// inside [update] expires the batch and rejects the whole edit.
  void updateComponents<T>(
    ComponentType<T> type,
    T Function(GameObject owner, T value) update,
  ) {
    _withGameplay(() {
      _requireComponentType(type);
      final edit = _gameplay!;
      final batch = edit.native.registeredQuery(type.id);
      final output = <List<NativeRegisteredValue>>[];
      for (final row in batch.rows) {
        output.add(
          _encodeComponent(
            type,
            update(
              edit.object(row.$1, validatedPopulation: true),
              _decodeComponent(type, row.$2, edit, validatedPopulation: true),
            ),
            edit,
          ),
        );
      }
      edit.native.registeredApply(batch, output);
    });
  }

  T _decodeComponent<T>(
    ComponentType<T> type,
    List<NativeRegisteredValue> fields,
    _GameplayAdapter edit, {
    bool validatedPopulation = false,
  }) => type.decode(
    List.unmodifiable([
      for (final f in fields)
        f.kind == ComponentFieldKind.object.index && f.value != null
            ? edit.object(
                f.value as NativeGameplayRef,
                validatedPopulation: validatedPopulation,
              )
            : f.value,
    ]),
  );

  List<NativeRegisteredValue> _encodeComponent<T>(
    ComponentType<T> type,
    T value,
    _GameplayAdapter edit,
  ) {
    final fields = type.encode(value);
    if (fields.length != type.fields.length) {
      throw ArgumentError('component field count mismatch');
    }
    return [
      for (var i = 0; i < fields.length; i++)
        NativeRegisteredValue(
          type.fields[i].kind.index,
          type.fields[i].kind == ComponentFieldKind.object && fields[i] != null
              ? edit.ref(
                  edit._specObjects[(fields[i] as GameObject)._spec] ??
                      fields[i] as GameObject,
                )
              : fields[i],
        ),
    ];
  }
}

extension GameObjectData on GameObject {
  T? readData<T>(ComponentType<T> type) => world._withGameplay(() {
    world._requireComponentType(type);
    final edit = world._gameplay!;
    final value = edit.native.registeredGet(edit.ref(this), type.id);
    return value == null ? null : world._decodeComponent(type, value, edit);
  });
  void writeData<T>(ComponentType<T> type, T value) => world._withGameplay(() {
    world._requireComponentType(type);
    final edit = world._gameplay!;
    edit.native.registeredSet(
      edit.ref(this),
      type.id,
      world._encodeComponent(type, value, edit),
    );
  });
  void addData<T>(ComponentType<T> type) => writeData(
    type,
    type.decode(List.unmodifiable(type.fields.map((f) => f.defaultValue))),
  );
  void removeData<T>(ComponentType<T> type) => world._withGameplay(() {
    world._requireComponentType(type);
    final edit = world._gameplay!;
    edit.native.registeredRemove(edit.ref(this), type.id);
  });
}

/// A plain class annotated with this and @Expose fields becomes registered ECS
/// data through codegen. It need not extend Behaviour or contain native code.
class FynkelComponent {
  const FynkelComponent({
    required this.id,
    this.version = 1,
    this.description,
    this.icon,
    this.category,
  });
  final String id;
  final int version;
  final String? description, icon, category;
}

/// A typed component value in an ObjectSpec, usable by both World.build and
/// the authored scene/prefab writer.
class ComponentData<T> {
  const ComponentData(this.type, this.value);
  final ComponentType<T> type;
  final T value;
  void apply(GameObject owner) => owner.writeData(type, value);
  Map<String, Object?> toJson() {
    final values = type.encode(value);
    if (values.length != type.fields.length) {
      throw ArgumentError('component field count mismatch');
    }
    return {
      for (var i = 0; i < values.length; i++)
        type.fields[i].name: switch (values[i]) {
          GameObject go => referenceJson(go),
          AssetRef asset => {
            'asset': asset.asset,
            if (asset.sub != null) 'sub': asset.sub,
          },
          final double number when !number.isFinite => throw ArgumentError(
            'component number must be finite',
          ),
          final value => value,
        },
    };
  }
}
