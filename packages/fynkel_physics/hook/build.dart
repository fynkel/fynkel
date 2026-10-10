// Builds the physics (crates/fynkel_physics_ffi) for the target and hands it
// to the Dart and Flutter tools as the code asset
// `package:fynkel_physics/fynkel_physics`, which the `@Native` bindings in
// lib/src/bindings.g.dart resolve through. Only games that depend on this
// package build and ship it.

import 'package:fynkel/hooks.dart';
import 'package:hooks/hooks.dart';

void main(List<String> args) async {
  await build(
    args,
    (input, output) => buildRustCrate(input, output, crate: 'fynkel_physics_ffi', asset: 'fynkel_physics'),
  );
}
