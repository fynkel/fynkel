import 'package:flutter/foundation.dart';
import 'package:fynkel/fynkel.dart' show Signal;

/// A Flutter [Listenable] (a `ValueNotifier` the HUD also reads, a
/// `ChangeNotifier` of settings) as a Fynkel [Signal], so game code
/// listens to it with `on`, which ends by itself when the object is
/// destroyed: `on(settings.changed, (_) => applySettings())`.
extension ListenableSignal on Listenable {
  /// A signal that emits each time this notifies. The same one each time
  /// it's asked for; it listens to this for as long as this lives.
  Signal<void> get changed => _signals[this] ??= _bridge(this);
}

final _signals = Expando<Signal<void>>('ListenableSignal');

Signal<void> _bridge(Listenable listenable) {
  final signal = Signal<void>();
  listenable.addListener(() => signal.emit(null));
  return signal;
}
