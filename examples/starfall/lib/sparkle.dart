import 'package:fynkel/fynkel.dart';

/// A puff of sparks where something was caught. It plays when the pool
/// hands it out, and goes back to the pool by itself when its last spark
/// is gone (`StopAction.disable`).
class Sparkle extends GameObject {
  Sparkle()
    : super(
        components: [
          ParticleSystem(
            looping: false,
            duration: 0.2,
            startSpeed: const Range(2, 5),
            startSize: const Range(0.1, 0.25),
            startLifetime: const Range(0.3, 0.6),
            gravityModifier: 0.5,
            simulationSpace: ParticleSpace.world,
            stopAction: StopAction.disable,
            emission: Emission(rateOverTime: 0, bursts: [Burst(count: const Range(25, 35))]),
            shape: const ParticleShape.sphere(radius: 0.2),
            colorOverLifetime: ColorGradient.fadeOut(),
            material: Material(color: Color.white, blending: Blending.additive),
          ),
        ],
      );

  /// Its sparks' colour, set before it plays.
  set color(Color value) => get<ParticleSystem>()!.startColor = value;
}
