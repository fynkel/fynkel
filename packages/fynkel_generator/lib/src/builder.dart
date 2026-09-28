/// The `build_runner` builder: resolves every library under `lib/`, reads
/// the `@FynkelBehaviour` classes and their `@Expose` fields into
/// [BehaviourClass]es and writes the two generated files.
library;

import 'dart:async';

import 'package:analyzer/dart/ast/ast.dart';
import 'package:analyzer/dart/element/element.dart';
import 'package:analyzer/dart/element/nullability_suffix.dart';
import 'package:analyzer/dart/element/type.dart';
import 'package:build/build.dart';
import 'package:glob/glob.dart';
import 'package:source_gen/source_gen.dart';

import 'emit.dart';
import 'model.dart';

const String dartOutput = 'lib/generated/behaviours.g.dart';
const String jsonOutput = 'lib/generated/behaviours.g.json';

const _behaviourAnnotation = TypeChecker.typeNamedLiterally('FynkelBehaviour', inPackage: 'fynkel');
const _exposeAnnotation = TypeChecker.typeNamedLiterally('Expose', inPackage: 'fynkel');
const _gizmoAnnotation = TypeChecker.typeNamedLiterally('Gizmo', inPackage: 'fynkel');
const _behaviourBase = TypeChecker.typeNamedLiterally('Behaviour', inPackage: 'fynkel');
const _dataAnnotation = TypeChecker.typeNamedLiterally('FynkelDataAsset', inPackage: 'fynkel');
const _dataBase = TypeChecker.typeNamedLiterally('DataAsset', inPackage: 'fynkel');
const _recordAnnotation = TypeChecker.typeNamedLiterally('FynkelRecord', inPackage: 'fynkel');

/// Aggregates the package's `@FynkelBehaviour` classes into
/// `lib/generated/behaviours.g.dart` and `.g.json`. Runs once per package
/// (input `$lib$`); generated files themselves are never inputs.
class BehaviourBuilder implements Builder {
  BehaviourBuilder(this.options);

  final BuilderOptions options;

  @override
  Map<String, List<String>> get buildExtensions => const {
        r'$lib$': ['generated/behaviours.g.dart', 'generated/behaviours.g.json'],
      };

  @override
  Future<void> build(BuildStep buildStep) async {
    final package = buildStep.inputId.package;
    final classes = await collect(buildStep);
    final dart = AssetId(package, dartOutput);
    final json = AssetId(package, jsonOutput);
    await buildStep.writeAsString(dart, emitDart(package, classes));
    await buildStep.writeAsString(json, emitJson(package, classes));
  }

  /// Every behaviour and data class in the package, sorted by id (one id
  /// space for both); throws [BehaviourGenerationError] on the first
  /// problem.
  static Future<List<BehaviourClass>> collect(BuildStep buildStep) async {
    final resolver = buildStep.resolver;
    final out = <BehaviourClass>[];
    final byId = <String, BehaviourClass>{};
    final inputs = await buildStep.findAssets(Glob('lib/**.dart')).toList();
    inputs.sort((a, b) => a.path.compareTo(b.path));
    for (final id in inputs) {
      if (id.path.startsWith('lib/generated/') || id.path.endsWith('.g.dart')) continue;
      if (!await resolver.isLibrary(id)) continue;
      final library = await resolver.libraryFor(id);
      final reader = LibraryReader(library);
      for (final (annotated, data) in [
        for (final a in reader.annotatedWith(_behaviourAnnotation)) (a, false),
        for (final a in reader.annotatedWith(_dataAnnotation)) (a, true),
      ]) {
        final c = await _readClass(resolver, library, annotated, data: data);
        final previous = byId[c.id];
        if (previous != null) {
          throw BehaviourGenerationError(
            '${data ? 'data type' : 'behaviour'} id "${c.id}" is also used by ${previous.className} in ${previous.libraryUri}',
            library: c.libraryUri,
            member: c.className,
          );
        }
        byId[c.id] = c;
        out.add(c);
      }
    }
    out.sort((a, b) => a.id.compareTo(b.id));
    return out;
  }
}

Future<BehaviourClass> _readClass(
  Resolver resolver,
  LibraryElement library,
  AnnotatedElement annotated, {
  required bool data,
}) async {
  final element = annotated.element;
  final libraryUri = library.uri.toString();
  final name = element.name ?? '<unnamed>';
  final what = data ? '@FynkelDataAsset' : '@FynkelBehaviour';
  final base = data ? _dataBase : _behaviourBase;
  if (element is! ClassElement) {
    throw BehaviourGenerationError('$what applies to classes only', library: libraryUri, member: name);
  }
  if (!base.isAssignableFrom(element)) {
    throw BehaviourGenerationError(
      '$what class must extend ${data ? 'DataAsset' : 'Behaviour'}',
      library: libraryUri,
      member: name,
    );
  }
  if (element.isAbstract) {
    throw BehaviourGenerationError('$what class must be concrete', library: libraryUri, member: name);
  }
  if (element.isPrivate) {
    throw BehaviourGenerationError(
      '$what class must be public (the generated file constructs it)',
      library: libraryUri,
      member: name,
    );
  }
  final ctor = element.unnamedConstructor;
  if (ctor == null || ctor.formalParameters.any((p) => p.isRequired)) {
    throw BehaviourGenerationError(
      '$what class needs an unnamed constructor without required parameters',
      library: libraryUri,
      member: name,
    );
  }
  final id = annotated.annotation.peek('id')?.stringValue;
  if (id == null || !RegExp(r'^[A-Za-z][A-Za-z0-9]*(\.[A-Za-z][A-Za-z0-9]*)+$').hasMatch(id)) {
    throw BehaviourGenerationError(
      'id must be a namespaced type id such as "${data ? 'game.Weapon' : 'game.Spinner'}" (got ${id == null ? 'none' : '"$id"'})',
      library: libraryUri,
      member: name,
    );
  }
  if (id.startsWith('fynkel.')) {
    throw BehaviourGenerationError('the "fynkel." namespace is reserved for the engine', library: libraryUri, member: name);
  }
  final version = annotated.annotation.peek('version')?.intValue ?? 1;
  if (version < 1) {
    throw BehaviourGenerationError('version must be >= 1', library: libraryUri, member: name);
  }

  // Own fields first, then inherited ones up to (not including) the base.
  final fields = <ExposedField>[];
  final seen = <String>{};
  for (InterfaceElement? cls = element; cls != null && !base.isExactly(cls); cls = cls.supertype?.element) {
    for (final f in cls.fields) {
      final annotation = _exposeAnnotation.firstAnnotationOf(f);
      if (annotation == null) {
        if (_gizmoAnnotation.hasAnnotationOf(f)) {
          throw BehaviourGenerationError('@Gizmo needs @Expose on the same field', library: libraryUri, member: '$name.${f.name}');
        }
        continue;
      }
      final fieldName = f.name ?? '';
      if (!seen.add(fieldName)) continue;
      final field = await _readField(resolver, f, ConstantReader(annotation), libraryUri, '$name.$fieldName', const {});
      if (data && field.kind.isReference) {
        throw BehaviourGenerationError(
          '@Expose ${f.type.getDisplayString()} on a data asset: an asset cannot point into a scene '
          '(GameObject and script fields belong on a behaviour)',
          library: libraryUri,
          member: '$name.$fieldName',
        );
      }
      if (data && field.gizmo != null) {
        throw BehaviourGenerationError('@Gizmo on a data asset: it has no place in the scene',
            library: libraryUri, member: '$name.$fieldName');
      }
      fields.add(field);
    }
  }
  fields.sort((a, b) => a.name.compareTo(b.name));
  for (final f in fields) {
    final gizmo = f.gizmo;
    if (gizmo == null) continue;
    final problem = gizmoProblem(gizmo, f, fields);
    if (problem != null) throw BehaviourGenerationError(problem, library: libraryUri, member: '$name.${f.name}');
  }
  _checkShowIf(fields, libraryUri, name);
  final description = annotated.annotation.peek('description')?.stringValue;
  return BehaviourClass(
    id: id,
    version: version,
    className: name,
    libraryUri: libraryUri,
    fields: fields,
    doc: description ?? _doc(element.documentationComment),
    icon: annotated.annotation.peek('icon')?.stringValue,
    category: data ? null : annotated.annotation.peek('category')?.stringValue,
    isData: data,
    menu: data ? annotated.annotation.peek('menu')?.stringValue : null,
  );
}

Future<ExposedField> _readField(
  Resolver resolver,
  FieldElement f,
  ConstantReader expose,
  String libraryUri,
  String member,
  Set<String> records,
) async {
  final fieldName = f.name ?? '';
  if (f.isStatic) {
    throw BehaviourGenerationError('@Expose field must be an instance field', library: libraryUri, member: member);
  }
  if (f.isPrivate) {
    throw BehaviourGenerationError('@Expose field must be public', library: libraryUri, member: member);
  }
  if (f.isFinal || f.isConst || f.setter == null) {
    throw BehaviourGenerationError(
      '@Expose field must be assignable (not final or const)',
      library: libraryUri,
      member: member,
    );
  }
  final type = f.type;
  final kind = _kindOf(type);
  if (kind == null) {
    throw BehaviourGenerationError(
      '@Expose field type ${type.getDisplayString()} is not supported; use ${FieldKind.values.map((k) => k.dartType).join(', ')} (optionally nullable)',
      library: libraryUri,
      member: member,
    );
  }
  final nullable = type.nullabilitySuffix == NullabilitySuffix.question;
  // A list's item: what it holds decides the rest (an enum's values).
  final itemType = kind == FieldKind.list ? (type as InterfaceType).typeArguments.single : null;
  final item = itemType == null ? null : _kindOf(itemType);
  final described = itemType ?? type;
  final describedKind = item ?? kind;
  // An enum's values, a referenced script's class and type id.
  String? typeLibrary, typeName, behaviourId;
  var enumValues = const <String>[];
  if (describedKind == FieldKind.enumeration || describedKind == FieldKind.behaviour) {
    final element = (described as InterfaceType).element;
    typeLibrary = element.library.uri.toString();
    typeName = element.name;
    if (element.isPrivate) {
      throw BehaviourGenerationError(
        '@Expose field type $typeName must be public (the generated file names it)',
        library: libraryUri,
        member: member,
      );
    }
    if (element is EnumElement) enumValues = [for (final c in element.constants) c.name ?? ''];
  }
  if ((kind.isReference || kind == FieldKind.data) && !nullable) {
    throw BehaviourGenerationError(
      '@Expose ${type.getDisplayString()} field must be nullable (${type.getDisplayString()}?): '
      'a reference is None until one is picked, and again when its object is deleted',
      library: libraryUri,
      member: member,
    );
  }
  String? dataId;
  if (describedKind == FieldKind.data) {
    final element = (described as InterfaceType).element;
    final annotation = _dataAnnotation.firstAnnotationOfExact(element);
    dataId = annotation == null ? null : ConstantReader(annotation).peek('id')?.stringValue;
    typeLibrary = element.library.uri.toString();
    typeName = element.name;
    if (dataId == null) {
      throw BehaviourGenerationError(
        '@Expose ${type.getDisplayString()} field: $typeName is not a @FynkelDataAsset class',
        library: libraryUri,
        member: member,
      );
    }
    if (element.isPrivate) {
      throw BehaviourGenerationError(
        '@Expose field type $typeName must be public (the generated file names it)',
        library: libraryUri,
        member: member,
      );
    }
  }
  RecordClass? record;
  if (describedKind == FieldKind.record) {
    if (kind == FieldKind.record && nullable) {
      throw BehaviourGenerationError(
        '@Expose ${type.getDisplayString()} field must not be nullable: a record is a value, '
        'initialize it (`${(described as InterfaceType).element.name} ${f.name} = ${described.element.name}();`)',
        library: libraryUri,
        member: member,
      );
    }
    record = await _readRecord(resolver, (described as InterfaceType).element as ClassElement, member, records);
    typeLibrary = record.libraryUri;
    typeName = record.className;
  }
  if (kind == FieldKind.behaviour) {
    final element = (type as InterfaceType).element;
    final annotation = _behaviourAnnotation.firstAnnotationOfExact(element);
    behaviourId = annotation == null ? null : ConstantReader(annotation).peek('id')?.stringValue;
    if (behaviourId == null) {
      throw BehaviourGenerationError(
        '@Expose ${type.getDisplayString()} field: $typeName is not a @FynkelBehaviour class '
        '(a script field names one; use GameObject? for any object)',
        library: libraryUri,
        member: member,
      );
    }
  }
  String? componentClass, componentName;
  if (kind == FieldKind.component) {
    // `NativeComponent<RigidBody>`: the record class, and its schema name
    // from its `static const schemaName`.
    final arg = (type as InterfaceType).typeArguments.single;
    final element = arg is InterfaceType ? arg.element : null;
    componentClass = element?.name;
    final lib = element?.library.uri;
    final generated = lib != null && lib.isScheme('package') && lib.pathSegments.first == 'fynkel';
    componentName =
        element is ClassElement ? element.getField('schemaName')?.computeConstantValue()?.toStringValue() : null;
    if (!generated || componentName == null) {
      throw BehaviourGenerationError(
        '@Expose ${type.getDisplayString()} field: NativeComponent holds a built-in component '
        '(RigidBody, PointLight, …)',
        library: libraryUri,
        member: member,
      );
    }
    if (componentName == 'fynkel.Transform' || componentName == 'fynkel.Name') {
      throw BehaviourGenerationError(
        '@Expose ${type.getDisplayString()} field: $componentName is not a component field; '
        'use a GameObject? field and its ${componentName == 'fynkel.Name' ? 'name' : 'transform'}',
        library: libraryUri,
        member: member,
      );
    }
  }
  Object? defaultValue;
  var hasDefault = false;
  final node = await resolver.astNodeFor(f.firstFragment);
  // The initializer, else the unnamed constructor's `this.<field> = …`
  // default (`Weapon({this.damage = 1})`, ADR-0027 §2).
  var init = node is VariableDeclaration ? node.initializer : null;
  if (init == null) {
    final owner = f.enclosingElement;
    final ctor = owner is ClassElement ? owner.unnamedConstructor : null;
    for (final p in ctor?.formalParameters ?? const <FormalParameterElement>[]) {
      if (p is! FieldFormalParameterElement || p.name != fieldName) continue;
      final param = await resolver.astNodeFor(p.firstFragment);
      if (param is FormalParameter) init = param.defaultClause?.value;
    }
  }
  if (init != null) {
    final v = _literal(init, kind, item: item, record: typeName);
    if (v != null) {
      defaultValue = v.value;
      hasDefault = true;
    }
  }
  if (hasDefault && defaultValue == null && !nullable) hasDefault = false;
  final hintKind = expose.peek('kind')?.stringValue;
  if (hintKind != null) {
    final problem = exposeKindProblem(hintKind, describedKind);
    if (problem != null) throw BehaviourGenerationError('@Expose: $problem', library: libraryUri, member: member);
  }
  final order = expose.peek('order');
  final gizmoAnnotation = _gizmoAnnotation.firstAnnotationOf(f);
  GizmoSpec? gizmo;
  if (gizmoAnnotation != null) {
    final g = ConstantReader(gizmoAnnotation);
    String option(String name, List<String> values, String fallback) {
      final v = g.peek(name);
      final index = v == null || v.isNull ? null : v.objectValue.getField('index')?.toIntValue();
      return index == null ? fallback : values[index];
    }

    final color = g.peek('color');
    gizmo = GizmoSpec(
      shape: g.read('shape').stringValue,
      color: color == null || color.isNull ? null : color.intValue,
      when: option('when', const ['selected', 'always'], 'selected'),
      space: option('space', const ['local', 'world'], 'local'),
      angle: g.peek('angle')?.stringValue,
    );
  }
  return ExposedField(
    name: fieldName,
    kind: kind,
    nullable: nullable,
    defaultValue: defaultValue,
    hasDefault: hasDefault,
    label: expose.peek('label')?.stringValue,
    tooltip: expose.peek('tooltip')?.stringValue,
    min: _num(expose.peek('min')),
    max: _num(expose.peek('max')),
    step: _num(expose.peek('step')),
    group: expose.peek('group')?.stringValue,
    order: order == null || order.isNull ? null : order.intValue,
    header: expose.peek('header')?.stringValue,
    unit: expose.peek('unit')?.stringValue,
    displayKind: hintKind,
    readOnly: expose.peek('readOnly')?.boolValue ?? false,
    hidden: expose.peek('hidden')?.boolValue ?? false,
    showIf: expose.peek('showIf')?.stringValue,
    drawer: expose.peek('drawer')?.stringValue,
    gizmo: gizmo,
    doc: _doc(f.documentationComment),
    typeLibrary: typeLibrary,
    typeName: typeName,
    enumValues: enumValues,
    behaviourId: behaviourId,
    componentClass: componentClass,
    componentName: componentName,
    item: item,
    dataId: dataId,
    record: record,
  );
}

/// A `@FynkelRecord` class a field holds, its fields read as a type's
/// are; [records] the record classes already being read (a record cannot
/// hold itself, even through a list: its schema would never end).
Future<RecordClass> _readRecord(Resolver resolver, ClassElement element, String member, Set<String> records) async {
  final libraryUri = element.library.uri.toString();
  final name = element.name ?? '<unnamed>';
  final key = '$libraryUri#$name';
  if (records.contains(key)) {
    throw BehaviourGenerationError('record $name holds itself: a record is a value, it cannot contain one of its own',
        library: libraryUri, member: member);
  }
  if (element.isAbstract || element.isPrivate) {
    throw BehaviourGenerationError('@FynkelRecord class $name must be public and concrete (the generated file constructs it)',
        library: libraryUri, member: name);
  }
  final ctor = element.unnamedConstructor;
  if (ctor == null || ctor.formalParameters.any((p) => p.isRequired)) {
    throw BehaviourGenerationError(
      '@FynkelRecord class needs an unnamed constructor without required parameters',
      library: libraryUri,
      member: name,
    );
  }
  final inner = {...records, key};
  final fields = <ExposedField>[];
  final seen = <String>{};
  for (InterfaceElement? cls = element; cls != null && !cls.library.isDartCore; cls = cls.supertype?.element) {
    for (final f in cls.fields) {
      final annotation = _exposeAnnotation.firstAnnotationOf(f);
      final fieldName = f.name ?? '';
      if (annotation == null) {
        if (_gizmoAnnotation.hasAnnotationOf(f)) {
          throw BehaviourGenerationError('@Gizmo needs @Expose on the same field', library: libraryUri, member: '$name.$fieldName');
        }
        continue;
      }
      if (!seen.add(fieldName)) continue;
      final field = await _readField(resolver, f, ConstantReader(annotation), libraryUri, '$name.$fieldName', inner);
      if (field.kind.isReference || field.dataId != null) {
        throw BehaviourGenerationError(
          '@Expose ${f.type.getDisplayString()} on a record: a record holds values only '
          '(GameObject, script and data-asset fields belong on the behaviour or data asset)',
          library: libraryUri,
          member: '$name.$fieldName',
        );
      }
      if (field.gizmo != null) {
        throw BehaviourGenerationError('@Gizmo on a record field: a gizmo draws a field of the script itself',
            library: libraryUri, member: '$name.$fieldName');
      }
      fields.add(field);
    }
  }
  fields.sort((a, b) => a.name.compareTo(b.name));
  _checkShowIf(fields, libraryUri, name);
  return RecordClass(className: name, libraryUri: libraryUri, fields: fields, doc: _doc(element.documentationComment));
}

/// Every `showIf` names a sibling `bool` field; every `drawer` is one
/// word.
void _checkShowIf(List<ExposedField> fields, String libraryUri, String name) {
  for (final f in fields) {
    final drawer = f.drawer;
    if (drawer != null && !RegExp(r'^[A-Za-z_][A-Za-z0-9_.]*$').hasMatch(drawer)) {
      throw BehaviourGenerationError(
        "drawer '$drawer' must be one word (letters, digits, '_' and '.')",
        library: libraryUri,
        member: '$name.${f.name}',
      );
    }
    final showIf = f.showIf;
    if (showIf == null) continue;
    final target = showIf.startsWith('!') ? showIf.substring(1) : showIf;
    ExposedField? sibling;
    for (final o in fields) {
      if (o.name == target) sibling = o;
    }
    if (sibling == null || sibling.kind != FieldKind.boolean || sibling.name == f.name) {
      throw BehaviourGenerationError(
        "showIf '$showIf' must name another @Expose bool field of the class",
        library: libraryUri,
        member: '$name.${f.name}',
      );
    }
  }
}

num? _num(ConstantReader? r) {
  if (r == null || r.isNull) return null;
  if (r.isInt) return r.intValue;
  if (r.isDouble) return r.doubleValue;
  return null;
}

FieldKind? _kindOf(DartType type) {
  if (type is! InterfaceType) return null;
  final element = type.element;
  final name = element.name;
  if (name == null) return null;
  if (element is EnumElement) return FieldKind.enumeration;
  if (element is ClassElement && _behaviourBase.isAssignableFrom(element) && !_behaviourBase.isExactly(element)) {
    return FieldKind.behaviour;
  }
  if (element is ClassElement && _dataBase.isAssignableFrom(element) && !_dataBase.isExactly(element)) {
    return FieldKind.data;
  }
  if (element is ClassElement && _recordAnnotation.hasAnnotationOfExact(element)) return FieldKind.record;
  final lib = element.library.uri;
  if (lib.isScheme('dart') && lib.path == 'core' && name != 'List') {
    return switch (name) {
      'double' => FieldKind.number,
      'int' => FieldKind.integer,
      'bool' => FieldKind.boolean,
      'String' => FieldKind.string,
      _ => null,
    };
  }
  if (lib.isScheme('dart') && lib.path == 'core' && name == 'List') {
    final item = type.typeArguments.single;
    if (item.nullabilitySuffix != NullabilitySuffix.none) return null;
    final kind = _kindOf(item);
    if (kind == FieldKind.vec3) return FieldKind.vec3List;
    return FieldKind.listItems.contains(kind) ? FieldKind.list : null;
  }
  if (lib.isScheme('package') && lib.pathSegments.first == 'fynkel') {
    return switch (name) {
      'Vec3' => FieldKind.vec3,
      'Quat' => FieldKind.quat,
      'AssetRef' => FieldKind.assetRef,
      'GameObject' => FieldKind.gameObject,
      'NativeComponent' => FieldKind.component,
      _ => null,
    };
  }
  return null;
}

/// A literal the generator can read into JSON; `null` when the initializer
/// is anything else (the class default still applies at runtime, the
/// metadata just has no `default`).
class _Literal {
  const _Literal(this.value);
  final Object? value;
}

_Literal? _literal(Expression e, FieldKind kind, {FieldKind? item, String? record}) {
  if (e is NullLiteral) return const _Literal(null);
  if (e is ParenthesizedExpression) return _literal(e.expression, kind, item: item, record: record);
  switch (kind) {
    case FieldKind.number:
      final n = _number(e);
      return n == null ? null : _Literal(n.toDouble());
    case FieldKind.integer:
      final n = _number(e);
      return n is int ? _Literal(n) : null;
    case FieldKind.boolean:
      return e is BooleanLiteral ? _Literal(e.value) : null;
    case FieldKind.string:
      return e is SimpleStringLiteral ? _Literal(e.value) : null;
    case FieldKind.vec3:
      return _vector(e, 'Vec3', 3, const {
        'zero': [0.0, 0.0, 0.0],
        'one': [1.0, 1.0, 1.0],
        'up': [0.0, 1.0, 0.0],
        'right': [1.0, 0.0, 0.0],
        'forward': [0.0, 0.0, -1.0],
      });
    case FieldKind.quat:
      return _vector(e, 'Quat', 4, const {
        'identity': [0.0, 0.0, 0.0, 1.0],
      });
    case FieldKind.assetRef:
      return _assetRef(e);
    case FieldKind.enumeration:
      // `Mode.fast` (a prefixed `lib.Mode.fast` too).
      return switch (e) {
        PrefixedIdentifier(:final identifier) => _Literal(identifier.name),
        PropertyAccess(:final propertyName) => _Literal(propertyName.name),
        _ => null,
      };
    case FieldKind.gameObject:
    case FieldKind.behaviour:
    case FieldKind.component:
    case FieldKind.data:
      return null;
    case FieldKind.record:
      // `Wave()` (`const`/`new` too): the class's own defaults, `{}`.
      final (type, arguments) = switch (e) {
        InstanceCreationExpression(:final constructorName, :final argumentList) when constructorName.name == null =>
          (constructorName.type.name.lexeme, argumentList),
        MethodInvocation(target: null, :final methodName, :final argumentList) => (methodName.name, argumentList),
        _ => (null, null),
      };
      return type == record && arguments!.arguments.isEmpty ? const _Literal(<String, Object?>{}) : null;
    case FieldKind.list:
      if (e is! ListLiteral || item == null) return null;
      final items = <Object?>[];
      for (final element in e.elements) {
        if (element is! Expression) return null;
        final v = _literal(element, item, record: record);
        if (v == null || v.value == null) return null;
        items.add(v.value);
      }
      return _Literal(items);
    case FieldKind.vec3List:
      if (e is! ListLiteral) return null;
      final points = <Object?>[];
      for (final element in e.elements) {
        if (element is! Expression) return null;
        final p = _literal(element, FieldKind.vec3);
        if (p == null || p.value == null) return null;
        points.add(p.value);
      }
      return _Literal(points);
  }
}

num? _number(Expression e) {
  if (e is IntegerLiteral) return e.value;
  if (e is DoubleLiteral) return e.value;
  if (e is PrefixExpression && e.operator.lexeme == '-') {
    final n = _number(e.operand);
    return n == null ? null : -n;
  }
  return null;
}

/// `Vec3(x, y, z)`, `Vec3.all(v)`, `Vec3.up`, … (and the `Quat` forms).
_Literal? _vector(Expression e, String type, int arity, Map<String, List<double>> constants) {
  if (e is PrefixedIdentifier && e.prefix.name == type) {
    final c = constants[e.identifier.name];
    return c == null ? null : _Literal(c);
  }
  // Unresolved, `Vec3(1, 2, 3)` without `const`/`new` parses as a call.
  String? ctor;
  ArgumentList? arguments;
  if (e is InstanceCreationExpression && e.constructorName.type.name.lexeme == type) {
    ctor = e.constructorName.name?.name;
    arguments = e.argumentList;
  } else if (e is MethodInvocation && e.target == null && e.methodName.name == type) {
    arguments = e.argumentList;
  } else if (e is MethodInvocation && e.target is SimpleIdentifier && (e.target as SimpleIdentifier).name == type) {
    ctor = e.methodName.name;
    arguments = e.argumentList;
  }
  if (arguments != null) {
    final args = <num>[];
    for (final a in arguments.arguments) {
      if (a is! Expression) return null;
      final n = _number(a);
      if (n == null) return null;
      args.add(n);
    }
    if (ctor == null && args.length == arity) return _Literal([for (final a in args) a.toDouble()]);
    if (ctor == 'all' && args.length == 1 && type == 'Vec3') {
      return _Literal(List<double>.filled(arity, args[0].toDouble()));
    }
  }
  return null;
}

/// `AssetRef('uuid')`, `AssetRef('uuid', sub: 'mesh/0')`, `AssetRef.none`.
_Literal? _assetRef(Expression e) {
  if (e is PrefixedIdentifier && e.prefix.name == 'AssetRef' && e.identifier.name == 'none') {
    return const _Literal({'asset': '00000000-0000-0000-0000-000000000000'});
  }
  if (e is InstanceCreationExpression &&
      e.constructorName.type.name.lexeme == 'AssetRef' &&
      e.constructorName.name == null) {
    String? asset;
    String? sub;
    for (final a in e.argumentList.arguments) {
      if (a is NamedArgument) {
        if (a.name.lexeme != 'sub') return null;
        final v = a.argumentExpression;
        if (v is NullLiteral) continue;
        if (v is! SimpleStringLiteral) return null;
        sub = v.value;
      } else if (a is SimpleStringLiteral && asset == null) {
        asset = a.value;
      } else {
        return null;
      }
    }
    if (asset == null) return null;
    return _Literal({'asset': asset, 'sub': ?sub});
  }
  return null;
}

/// The first paragraph of a `///` comment, joined with spaces.
String? _doc(String? comment) {
  if (comment == null) return null;
  final lines = <String>[];
  for (final raw in comment.split('\n')) {
    var line = raw.trim();
    if (line.startsWith('///')) line = line.substring(3).trim();
    if (line.isEmpty) {
      if (lines.isNotEmpty) break;
      continue;
    }
    lines.add(line);
  }
  return lines.isEmpty ? null : lines.join(' ');
}
