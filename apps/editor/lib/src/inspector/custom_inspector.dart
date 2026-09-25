// A custom Inspector (ADR-0028 §3, Unity's `CustomEditor`): a package's
// builder in place of a script card's fields or a data asset page's, over
// the same [FieldTarget] the default Inspector writes through — so a write
// is prechecked, one undo step, a prefab override where it should be, and
// the running game's during Play. A throwing builder falls back to the
// default fields with a line saying why (§6), and a Console error.
import 'package:flutter/material.dart';
import 'package:fynkel/authoring.dart' show ProjectWriter;
import 'package:fynkel_editor_api/fynkel_editor_api.dart';

import '../extensions/editor_extensions.dart';
import '../theme/tokens.dart';
import 'field_editor.dart';
import 'field_spec.dart';

/// [inspector]'s widget for the type [spec] on [target], or the default
/// fields ([defaults]) with the error when it throws.
Widget buildCustomInspector(
  BuildContext context, {
  required EditorExtensionHost host,
  required ExtensionInspector inspector,
  required FieldTarget target,
  required ComponentSpec spec,
  required EditorContext editor,
  required Widget Function(FieldSpec spec) field,
  required Widget Function(Set<String> except) defaults,
}) {
  final ctx = FieldTargetInspector(target: target, spec: spec, editor: editor, fieldOf: field, defaults: defaults);
  try {
    return KeyedSubtree(key: ValueKey('inspector.custom.${spec.type}'), child: inspector.build(context, ctx));
  } catch (e, stack) {
    // After the frame: the log notifies, and this is a build.
    WidgetsBinding.instance.addPostFrameCallback((_) => host.inspectorFailed(inspector, e, stack));
    final t = context.tokens;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.only(bottom: 6),
          child: Text(
            "${inspector.package}'s Inspector failed, so this is the default one: $e",
            key: const Key('inspector.custom.error'),
            style: Theme.of(context).textTheme.bodySmall?.copyWith(color: t.error),
          ),
        ),
        defaults(const {}),
      ],
    );
  }
}

/// An [InspectorContext] over a [FieldTarget]: the default Inspector's
/// reads, writes and field editors.
final class FieldTargetInspector implements InspectorContext {
  FieldTargetInspector({
    required this.target,
    required this.spec,
    required this.editor,
    required this.fieldOf,
    required this.defaults,
  });

  final FieldTarget target;
  final ComponentSpec spec;

  /// A field's standard editor, and the default fields but some.
  final Widget Function(FieldSpec spec) fieldOf;
  final Widget Function(Set<String> except) defaults;

  @override
  final EditorContext editor;

  @override
  String get type => spec.type;

  @override
  int get targets => target.instances.length;

  @override
  bool get canEdit => target.canEdit;

  @override
  List<String> get fields => [for (final f in spec.fields) f.name];

  FieldSpec _spec(String field) =>
      spec.field(field) ?? (throw ArgumentError.value(field, 'field', '${spec.type} has no such field'));

  @override
  Object? value(String field) => target.shown(field);

  @override
  bool isMixed(String field) => target.isMixed(field);

  @override
  bool set(String field, Object? value) {
    final f = _spec(field);
    if (!target.canEdit || f.readOnly) return false;
    return target.set(field, ProjectWriter.jsonOf(value), spec: f);
  }

  @override
  Widget field(String field) => fieldOf(_spec(field));

  @override
  Widget defaultInspector({Iterable<String> except = const []}) => defaults(except.toSet());
}
