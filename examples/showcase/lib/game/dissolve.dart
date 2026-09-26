import 'package:fynkel/fynkel.dart';

/// Burns its object away and back again: sets the `amount` of its
/// material — a material of `shaders/dissolve.shader` — every frame, as
/// Unity's `material.SetFloat` (ADR-0031). The value goes 0 → 1 over half
/// of [period], stays gone for [hold] seconds, then comes back.
///
/// `setMaterialValue` changes the material itself, so every renderer
/// using it dissolves together; the change lasts for this run only (the
/// material file keeps its authored `amount`, which is what
/// `fynkel render` draws).
@FynkelBehaviour(id: 'game.Dissolve')
class Dissolve extends Behaviour {
  @Expose(min: 0.5, step: 0.5, tooltip: 'Seconds for one burn away and back')
  double period = 6.0;

  @Expose(min: 0, step: 0.1, tooltip: 'Seconds to stay whole, and to stay gone')
  double hold = 1.0;

  /// Seconds since start (runtime state).
  double time = 0;

  /// The amount set last.
  double amount = 0;

  AssetRef? _material;

  @override
  void start() {
    _material = get<MeshRenderer>()?.material;
  }

  @override
  void update(UpdateContext context) {
    final material = _material;
    if (material == null) return;
    time += context.dt;
    amount = amountAt(time);
    world.setMaterialValue(material, 'amount', amount);
  }

  /// The amount at [t] seconds: whole for [hold], burning for
  /// `period / 2`, gone for [hold], coming back for `period / 2`.
  double amountAt(double t) {
    final half = period / 2;
    final cycle = 2 * (hold + half);
    final u = t % cycle;
    if (u < hold) return 0;
    if (u < hold + half) return (u - hold) / half;
    if (u < 2 * hold + half) return 1;
    return 1 - (u - 2 * hold - half) / half;
  }
}
