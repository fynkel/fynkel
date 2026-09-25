/// Pickups (ADR-0029's example package): a [Pickup] script, a
/// [PickupKind] data type, the [pickup] code prefab, and — in
/// `assets/` — the pickup prefab, its material and a gem kind, mounted in
/// a game as `Packages/pickups/`. `package:pickups/generated/assets.g.dart`
/// names them (import it with a prefix: the game has an `Assets` too).
library;

export 'src/pickup.dart';
export 'src/pickup_kind.dart';
export 'src/pickup_spec.dart';
