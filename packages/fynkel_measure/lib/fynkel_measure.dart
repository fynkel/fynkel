/// Measures a Fynkel app the way the stage reports do. In the app: the
/// stats line ([Measurement], [StatsLine]), the memory check's marks
/// ([Measurement.mark]) and the baseline ([BaselineSpinner]). Outside it:
/// bin/measure.dart, run from the app's directory, which starts the app's
/// profile build and reads all of that.
library;

export 'src/footprint.dart' show footprint, physicalFootprint;
export 'src/measurement.dart';
