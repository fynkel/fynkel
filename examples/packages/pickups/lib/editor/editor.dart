// The package's editor part (ADR-0028): what a game depending on
// pickups gets in its editor. Only files under lib/editor/ may import
// package:fynkel_editor_api (FYN-P0015), so a game never ships it.

import 'dart:math' show pow;

import 'package:flutter/material.dart' hide Transform;
import 'package:flutter/widgets.dart' as widgets show Transform;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_editor_api/fynkel_editor_api.dart';

import '../generated/assets.g.dart';
import '../src/pickup_kind.dart';

void registerEditor(EditorExtensions editor) {
  // The package's prefab, one undo step, as the Hierarchy's Create would
  // add it: under the selected object, or at the top.
  editor.menuItem(
    'Tools/Pickups/Add Pickup',
    id: 'pickups.addPickup',
    description: 'Adds a pickup (the pickups package\'s prefab) under the selected object.',
    enabled: (context) => context.scene != null && !context.scene!.isPrefab,
    run: (context) {
      final result = context.edit(
        'Add Pickup',
        (edit) => edit.add(
          ObjectSpec.prefab(
            Assets.prefabs.pickup,
            name: 'Pickup',
            transform: const Transform(position: Vec3(0, 0.5, 0)),
          ),
          parent: context.selection.firstOrNull,
        ),
      );
      if (result.ok) context.select(result.added);
    },
  );

  // Unity's CustomEditor: a pickup kind's page shows the gem in its
  // colour with what it gives, above the default fields.
  editor.inspector<PickupKind>(
    (context, inspector) => Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        GemPreview(color: inspector.value('color'), points: inspector.value('points')),
        inspector.defaultInspector(),
      ],
    ),
  );
}

/// A pickup kind as it looks: a diamond of its colour (the document's
/// linear RGB, shown in sRGB) and what it gives.
class GemPreview extends StatelessWidget {
  const GemPreview({super.key, required this.color, required this.points});

  /// As the document holds them: `[r, g, b]`, an integer.
  final Object? color;
  final Object? points;

  static double _srgb(num linear) {
    final c = linear.clamp(0, 1).toDouble();
    return c <= 0.0031308 ? c * 12.92 : 1.055 * pow(c, 1 / 2.4) - 0.055;
  }

  Color get swatch {
    final c = color;
    final rgb = c is List && c.length >= 3 ? [for (final x in c.take(3)) x is num ? x : 0] : const [1, 1, 1];
    return Color.from(alpha: 1, red: _srgb(rgb[0]), green: _srgb(rgb[1]), blue: _srgb(rgb[2]));
  }

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      height: 72,
      child: Row(
        children: [
          SizedBox(
            width: 72,
            child: Center(
              child: widgets.Transform.rotate(
                angle: 0.785398, // a diamond
                child: Container(
                  key: const Key('gemPreview.gem'),
                  width: 30,
                  height: 30,
                  decoration: BoxDecoration(
                    color: swatch,
                    border: Border.all(color: Colors.black26),
                  ),
                ),
              ),
            ),
          ),
          const SizedBox(width: 12),
          Expanded(child: Text('Gives ${points ?? 10} points', style: Theme.of(context).textTheme.titleSmall)),
        ],
      ),
    );
  }
}
