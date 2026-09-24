// A Physics Material's fields on its Inspector page (ADR-0025), edited in
// place as in Unity's: Friction, Bounciness and how two touching colliders
// combine each. Every change is one undo step, saved at once
// ([AssetEdits]); Play reads the cooked values, and a recook updates the
// colliders naming the material.
import 'package:flutter/material.dart';

import '../theme/tokens.dart';
import 'asset_edits.dart';
import 'field_widgets.dart';
import 'inspector_chrome.dart';

/// The combine rules, in Unity's priority (the higher one of the two
/// colliders' wins).
const _combines = {'average': 'Average', 'minimum': 'Minimum', 'multiply': 'Multiply', 'maximum': 'Maximum'};

class PhysicsMaterialInspector extends StatefulWidget {
  const PhysicsMaterialInspector({super.key, required this.edits, required this.id});

  final AssetEdits edits;
  final String id;

  @override
  State<PhysicsMaterialInspector> createState() => _PhysicsMaterialInspectorState();
}

class _PhysicsMaterialInspectorState extends State<PhysicsMaterialInspector> {
  AssetEdits get edits => widget.edits;
  String get id => widget.id;

  @override
  void initState() {
    super.initState();
    edits.addListener(_changed);
    edits.shown = id;
  }

  @override
  void didUpdateWidget(covariant PhysicsMaterialInspector old) {
    super.didUpdateWidget(old);
    if (old.edits != edits) {
      old.edits.removeListener(_changed);
      if (old.edits.shown == old.id) old.edits.shown = null;
      edits.addListener(_changed);
    }
    edits.shown = id;
  }

  @override
  void dispose() {
    edits.removeListener(_changed);
    if (edits.shown == id) edits.shown = null;
    super.dispose();
  }

  void _changed() {
    if (mounted) setState(() {});
  }

  void _set(String field, Object? value) => edits.set(id, {field: value});

  @override
  Widget build(BuildContext context) {
    final f = edits.fields(id);
    if (f == null) {
      return InspectorSection(
        title: 'Physics Material',
        children: [InspectorValueRow('Error', edits.lastError ?? 'The physics material could not be opened.')],
      );
    }
    final enabled = edits.canEdit;
    num number(String k, num fallback) => f[k] is num ? f[k] as num : fallback;

    Widget combine(String field, String label, String tooltip) => KeyedSubtree(
      key: Key('physicsMaterial.$field'),
      child: FieldRow(
        label: label,
        tooltip: tooltip,
        child: EnumField(
          value: f[field] is String ? f[field] as String : 'average',
          values: _combines.keys.toList(),
          display: (v) => _combines[v] ?? v,
          enabled: enabled,
          onCommit: (v) => _set(field, v),
        ),
      ),
    );

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        InspectorSection(
          title: 'Physics Material',
          children: [
            KeyedSubtree(
              key: const Key('physicsMaterial.friction'),
              child: NumberField(
                label: 'Friction',
                value: number('friction', 0.6),
                min: 0,
                max: null,
                step: 0.05,
                slider: false,
                enabled: enabled,
                onCommit: (v) => _set('friction', v),
              ),
            ),
            KeyedSubtree(
              key: const Key('physicsMaterial.bounciness'),
              child: NumberField(
                label: 'Bounciness',
                value: number('bounciness', 0),
                min: 0,
                max: 1,
                step: 0.01,
                slider: true,
                enabled: enabled,
                onCommit: (v) => _set('bounciness', v),
              ),
            ),
            combine('friction_combine', 'Friction Combine', 'How the two colliders\' friction is combined.'),
            combine('bounce_combine', 'Bounce Combine', 'How the two colliders\' bounciness is combined.'),
          ],
        ),
        if (edits.lastError != null)
          Padding(
            padding: EdgeInsets.fromLTRB(context.tokens.pad + 2, 8, context.tokens.pad + 2, 0),
            child: Text(
              edits.lastError!,
              key: const Key('physicsMaterial.error'),
              style: Theme.of(context).textTheme.bodySmall?.copyWith(color: context.tokens.error),
            ),
          ),
      ],
    );
  }
}
