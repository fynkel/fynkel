// The project's optional engine modules (ADR-0029 Decision 3): physics,
// audio and animation, each on unless `fynkel.project.json` turns it off
// (`modules`). The contract stays whole — every component, command and
// record is still there — but a module that is off does not run: its
// components do nothing, the engine rejects its commands (`module_off`,
// FYN-R0014) and the SDK's calls into it throw a [StateError] naming it.
// Ask [World.modules] first when a script must work either way.

/// One optional engine module.
enum Module {
  physics('physics'),
  audio('audio'),
  animation('animation');

  const Module(this.key);

  /// The key in `fynkel.project.json`'s `modules`.
  final String key;
}

/// Which optional modules the loaded project runs ([World.modules]).
class Modules {
  const Modules({this.physics = true, this.audio = true, this.animation = true});

  /// A load report's `modules`; every module on for a report without it.
  factory Modules.fromJson(Map<String, Object?> j) => Modules(
        physics: j['physics'] != false,
        audio: j['audio'] != false,
        animation: j['animation'] != false,
      );

  /// Every module on: the default, and what a world runs before a load.
  static const all = Modules();

  final bool physics;
  final bool audio;
  final bool animation;

  /// The module runs.
  bool has(Module module) => switch (module) {
        Module.physics => physics,
        Module.audio => audio,
        Module.animation => animation,
      };

  /// Throws a [StateError] naming [module] and [what] needed it when the
  /// project turns it off.
  void require(Module module, String what) {
    if (has(module)) return;
    throw StateError('$what needs the ${module.key} module, which this project turns off '
        '(modules.${module.key} in fynkel.project.json; check world.modules.has(Module.${module.name}) first)');
  }

  @override
  bool operator ==(Object other) =>
      other is Modules && other.physics == physics && other.audio == audio && other.animation == animation;

  @override
  int get hashCode => Object.hash(physics, audio, animation);

  @override
  String toString() => 'Modules(physics: $physics, audio: $audio, animation: $animation)';
}
