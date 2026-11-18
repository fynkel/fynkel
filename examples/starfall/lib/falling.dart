import 'package:fynkel/fynkel.dart';

/// A star to catch, or a rock to keep away from.
enum Kind { star, rock }

/// One falling thing. The game hands them out from a pool and gives them
/// back when they're caught or fall past the basket, so a long game makes
/// no new objects.
class Falling extends GameObject {
  Falling() : super(components: [MeshRenderer(Mesh.sphere(), _starMaterial)]);

  static final _starMaterial = Material(color: const Color(0xFFFFD45A), emissive: const Color(0xFFFFB020));
  static final _rockMaterial = Material(color: const Color(0xFF6B5E57), roughness: 0.9);

  late Kind kind;

  /// Metres a second, downwards.
  var speed = 0.0;

  late final _renderer = get<MeshRenderer>()!;

  /// Sets it up for another fall: called by the pool's `get` before it's shown.
  void launch(Kind kind, Vec3 from, double speed) {
    this.kind = kind;
    this.speed = speed;
    position = from;
    _renderer.material = kind == Kind.star ? _starMaterial : _rockMaterial;
    localScale = kind == Kind.star ? Vec3.one * 0.6 : Vec3.one * 0.8;
  }

  @override
  void onUpdate() {
    super.onUpdate();
    position = position + Vec3(0, -speed * time.delta, 0);
    rotate(Vec3.up, 2 * time.delta);
  }
}
