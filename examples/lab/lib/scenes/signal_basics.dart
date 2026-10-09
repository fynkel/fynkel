// Step 4: signals, the basics. A doorbell with one signal, and lamps that
// listen to it with `on`.

import 'package:fynkel/fynkel.dart';

import '../lab.dart';

class SignalBasicsScene extends LabScene {
  @override
  String get title => 'Signals: the basics';
  @override
  String get step => 'step 4';
  @override
  String get about =>
      'The blue doorbell has a signal, rung. Tap it (or "ring"): it emits rung, and every lamp that listens with '
      'on() lights up at once. Remove a lamp and ring again: it\'s gone, so it hears nothing. Add one: it hears '
      'from then on. The doorbell never knows who listens.';

  late final Doorbell _bell;
  final _lamps = <Lamp>[];
  var _made = 0;

  // Where the lamps stand, the first three in the middle.
  static const _spots = [-1.5, 0.0, 1.5, -3.0, 3.0];

  @override
  void build() {
    _bell = add(Doorbell());
    for (var i = 0; i < 3; i++) {
      _addLamp();
    }
  }

  void _addLamp() {
    if (_lamps.length == 5) return;
    _lamps.add(add(Lamp(_bell, number: ++_made, at: Vec3(_spots[_lamps.length], 0.4, 0.5))));
    log.add('lamp $_made added: it listens with on(doorbell.rung, ...)');
  }

  void _removeLamp() {
    if (_lamps.isEmpty) return;
    final lamp = _lamps.removeLast();
    log.add('lamp ${lamp.number} destroyed: its on() stops by itself');
    lamp.destroy();
  }

  @override
  void onUpdate() {
    super.onUpdate();
    readout.value = 'rung ${_bell.times} times   lamps listening: ${_lamps.length}';
  }

  @override
  List<Control> get controls => [
    Button('ring', _bell.ring),
    Button('remove a lamp', _removeLamp),
    Button('add a lamp', _addLamp),
    Button('clear log', log.clear),
  ];
}

/// Owns the signal, and rings it. It doesn't know who listens.
class Doorbell extends GameObject {
  Doorbell()
    : super(
        name: 'doorbell',
        transform: Transform(position: Vec3(0, 0.5, -2)),
      );

  /// The signal: it carries how many times the bell has rung.
  final rung = Signal<int>();
  var times = 0;

  @override
  void onLoad() {
    super.onLoad();
    addComponent(MeshRenderer(Mesh.cube(), own(Material(color: const Color(0xFF4CA6E0)))));
  }

  void ring() {
    times++;
    ancestor<LabScene>()?.log.add('doorbell: rung.emit($times)');
    rung.emit(times);
  }

  @override
  bool onClick(Finger finger) {
    ring();
    return true;
  }
}

/// Listens to the doorbell, and lights up when it rings.
class Lamp extends GameObject {
  Lamp(this.bell, {required this.number, required Vec3 at})
    : super(
        name: 'lamp $number',
        transform: Transform(position: at, scale: Vec3(0.6, 0.6, 0.6)),
      );

  final Doorbell bell;
  final int number;
  late final Material _glass;
  static const _off = Color(0xFF3A3F4A);
  static const _on = Color(0xFFFFD040);

  @override
  void onLoad() {
    super.onLoad();
    _glass = own(Material(color: _off));
    addComponent(MeshRenderer(Mesh.sphere(), _glass));

    // The one line that matters: when the bell rings, run this.
    on(bell.rung, (times) {
      ancestor<LabScene>()?.log.add('  lamp $number heard ring $times');
      _light();
    });
  }

  void _light() {
    start(() async {
      _glass
        ..color = _on
        ..emissive = _on;
      await wait(0.4);
      _glass
        ..color = _off
        ..emissive = Color.black;
    });
  }
}
