// Writes the package's assets — the gem material, the gem kind and the
// pickup prefab — from lib/src/pickup_spec.dart (`package:fynkel/authoring.dart`,
// ADR-0026 §2): `dart run tool/assets.dart` from the package, then
// `fynkel codegen --project .` for a new document's `Assets` constant.
// ProjectWriter writes into assets/ (fynkel.package.json) with ids of
// the package's own.

import 'dart:io';

import 'package:fynkel/authoring.dart';
import 'package:fynkel/gameplay.dart';
import 'package:pickups/generated/behaviours.g.dart';
import 'package:pickups/pickups.dart';

void main(List<String> args) {
  for (final d in writeAssets(args.isEmpty ? Directory.current.path : args.first)) {
    stdout.writeln('${d.changed ? 'wrote' : 'same '} assets/${d.path}');
  }
}

/// Writes every asset into the package at [packageDir]; what was written.
List<WrittenDocument> writeAssets(String packageDir) {
  final writer = ProjectWriter(packageDir, registerBehaviours: registerBehaviours);
  final gem = PickupKind();
  final material = writer.writeMaterial(
    'materials/gem',
    const PbrMaterial(
      baseColor: Vec4(0.3, 0.9, 0.8, 1),
      metallic: 0.1,
      roughness: 0.2,
      emissive: Vec3(0.3, 0.9, 0.8),
      emissiveStrength: 0.6,
    ),
  );
  final kind = writer.writeData('data/gem', gem, name: 'Gem');
  final prefab = writer.writePrefab('prefabs/pickup', pickup(gem, material.ref));
  return [material, kind, prefab];
}
