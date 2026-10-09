/// Measures a Fynkel app the way the stage reports do. In the app: the
/// game run as the tool asks ([MeasuredApp], from the game's
/// `lib/measured.dart`), with the stats line ([Measurement], [StatsLine]),
/// the memory check's marks ([Measurement.mark]), the baseline
/// ([BaselineSpinner]), the on-screen clock ([OnScreenClock]) and the
/// bench ([bench]). Outside it: bin/measure.dart, run from the app's
/// directory, which builds and starts the app's profile build and reads
/// all of that.
library;

export 'src/bench.dart' show bench, castTimes;
export 'src/footprint.dart' show footprint, physicalFootprint;
export 'src/measured_app.dart' show MeasuredApp;
export 'src/measurement.dart';
export 'src/on_screen.dart' show OnScreenClock;
