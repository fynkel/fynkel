// The game's particles: dust at the dog's and the sheep's feet as they
// run, embers off a wolf as it burns away, and a puff where a sheep comes
// home. Their textures are from Kenney's Particle Pack (assets/CREDITS.md).
// They're on the effects layer, so the minimap leaves them out.

import 'package:fynkel/fynkel.dart';

import 'game.dart' show effects;

/// The particles' materials, made once for the whole game.
class ParticleLooks {
  ParticleLooks._(this.dust, this.embers, this.puff, this.sparkle);

  /// Loads the textures and makes the materials.
  static Future<ParticleLooks> load(Assets assets) async {
    final (smoke, round, dot, star) = await (
      assets.texture('particles/smoke_04.png'),
      assets.texture('particles/smoke_07.png'),
      assets.texture('particles/circle_05.png'),
      assets.texture('particles/star_06.png'),
    ).wait;
    return ParticleLooks._(
      Material(name: 'dust', baseColorTexture: smoke, blending: Blending.transparent, roughness: 1),
      Material(name: 'embers', baseColorTexture: dot, blending: Blending.additive),
      Material(name: 'puff', baseColorTexture: round, blending: Blending.transparent, roughness: 1),
      Material(name: 'sparkle', baseColorTexture: star, blending: Blending.additive),
    );
  }

  /// Lit by the sun (or the moon) as the field is: dust and the puff.
  final Material dust, puff;

  /// Glowing: the embers and the puff's sparkles.
  final Material embers, sparkle;
}

/// Dust kicked up at an animal's feet, [lift] below its middle: a puff
/// every [every] metres while it goes faster than [from] metres a second,
/// none at a walk. [pace] tells it how fast the animal goes.
class Dust extends GameObject {
  Dust(ParticleLooks looks, {required double lift, required double from, double every = 0.2, double size = 0.45})
    : this._(
        ParticleSystem(
          maxParticles: 80,
          startLifetime: const Range(0.7, 1.1),
          startSpeed: const Range(0.1, 0.4),
          startSize: Range(size * 0.8, size * 1.2),
          startRotation: const Range(0, 6.28),
          startColor: const ColorRange(Color(0xFFB8A27E), Color(0xFFD8C8A4)),
          gravityModifier: -0.03,
          simulationSpace: ParticleSpace.world,
          emission: _none,
          shape: const ParticleShape.hemisphere(radius: 0.15),
          // Thrown back a little, from the way it runs.
          inheritVelocity: const InheritVelocity(curve: ParticleCurve(-0.15)),
          sizeOverLifetime: (t) => 1 + 1.2 * t,
          colorOverLifetime: ColorGradient.fadeInOut(Color.white),
          lit: true,
          softness: 0.3,
          material: looks.dust,
        ),
        lift,
        from,
        Emission(rateOverTime: 0, rateOverDistance: 1 / every),
      );

  Dust._(this.system, double lift, this._from, this._kick)
    : super(
        name: 'dust',
        layer: effects,
        transform: Transform(position: Vec3(0, 0.05 - lift, 0)),
        components: [system],
      );

  final ParticleSystem system;
  final double _from;
  final Emission _kick;
  bool _kicking = false;

  static const _none = Emission(rateOverTime: 0);

  /// How fast the animal goes now, in metres a second. The emission is
  /// sent to the engine only when it starts or stops kicking up dust.
  void pace(double speed) {
    final kicking = speed > _from;
    if (kicking == _kicking) return;
    _kicking = kicking;
    system.emission = kicking ? _kick : _none;
  }
}

/// Embers off [wolf] for [seconds], as it burns away: they rise from all
/// over it, glowing, and fade. They follow it while it's there; once
/// they've all gone out, they're gone too.
class Embers extends GameObject {
  Embers(this.wolf, ParticleLooks looks, {required double seconds})
    : super(
        name: 'embers',
        layer: effects,
        transform: Transform(position: wolf.position, rotation: wolf.rotation),
        components: [
          ParticleSystem(
            duration: seconds,
            looping: false,
            stopAction: StopAction.destroy,
            maxParticles: 220,
            startLifetime: const Range(0.5, 1),
            startSpeed: const Range(0.2, 0.6),
            startSize: const Range(0.14, 0.24),
            startColor: const ColorRange(Color(0xFFFFE070), Color(0xFFFF7020)),
            gravityModifier: -0.15,
            simulationSpace: ParticleSpace.world,
            emission: const Emission(rateOverTime: 160),
            shape: const ParticleShape.box(size: Vec3(0.55, 0.55, 1.2)),
            velocityOverLifetime: const ParticleVector(y: ParticleCurve.between(0.6, 1.2)),
            sizeOverLifetime: (t) => 1 - 0.6 * t,
            colorOverLifetime: ColorGradient.fadeOut(),
            material: looks.embers,
          ),
        ],
      );

  final GameObject wolf;

  @override
  void onUpdate() {
    super.onUpdate();
    if (wolf.isDestroyed) return;
    position = wolf.position;
    rotation = wolf.rotation;
  }
}

/// A puff of white with a few sparkles in it, at [at]: where a sheep
/// comes home. Gone once it has faded.
class Puff extends GameObject {
  Puff(Vec3 at, ParticleLooks looks)
    : super(
        name: 'puff',
        layer: effects,
        transform: Transform(position: at),
        components: [
          ParticleSystem(
            duration: 0.2,
            looping: false,
            stopAction: StopAction.destroy,
            maxParticles: 20,
            startLifetime: const Range(0.7, 1.1),
            startSpeed: const Range(1.5, 2.5),
            startSize: const Range(0.55, 0.85),
            startRotation: const Range(0, 6.28),
            simulationSpace: ParticleSpace.world,
            emission: Emission(rateOverTime: 0, bursts: [Burst(count: const Range(14))]),
            shape: const ParticleShape.sphere(radius: 0.3),
            limitVelocity: const LimitVelocity(limit: ParticleCurve(100), drag: ParticleCurve(3)),
            sizeOverLifetime: (t) => 1 + t,
            colorOverLifetime: ColorGradient.fadeOut(),
            lit: true,
            material: looks.puff,
          ),
        ],
        children: [
          GameObject(
            name: 'sparkles',
            layer: effects,
            components: [
              ParticleSystem(
                duration: 0.2,
                looping: false,
                maxParticles: 10,
                startLifetime: const Range(0.5, 0.8),
                startSpeed: const Range(1.5, 2.5),
                startSize: const Range(0.3, 0.45),
                startColor: const Color(0xFFFFE080),
                gravityModifier: 0.4,
                simulationSpace: ParticleSpace.world,
                emission: Emission(rateOverTime: 0, bursts: [Burst(count: const Range(8))]),
                shape: const ParticleShape.hemisphere(radius: 0.3),
                sizeOverLifetime: (t) => 1 - t,
                material: looks.sparkle,
              ),
            ],
          ),
        ],
      );
}
