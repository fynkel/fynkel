// Builds the engine (crates/fynkel_ffi) for the target and hands it to the
// Dart and Flutter tools as the code asset `package:fynkel/fynkel`, which the
// `@Native` bindings in lib/src/bindings.g.dart resolve through. How, and
// why always in release: lib/hooks.dart.

import 'package:fynkel/hooks.dart';
import 'package:hooks/hooks.dart';

void main(List<String> args) async {
  await build(args, (input, output) => buildRustCrate(input, output, crate: 'fynkel_ffi', asset: 'fynkel'));
}
