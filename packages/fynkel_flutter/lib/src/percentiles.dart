// Percentiles of a window of millisecond samples — shared by the game
// view's frame stats and the viewport's stats snapshot.

/// Percentiles of a sample window, in milliseconds.
class MsPercentiles {
  const MsPercentiles(this.p50, this.p95, this.p99, this.count);
  final double p50, p95, p99;
  final int count;

  static const empty = MsPercentiles(0, 0, 0, 0);

  factory MsPercentiles.of(List<double> samples) {
    if (samples.isEmpty) return empty;
    final s = List<double>.from(samples)..sort();
    double at(double q) => s[((s.length - 1) * q).round()];
    return MsPercentiles(at(.5), at(.95), at(.99), s.length);
  }

  String format([int digits = 2]) =>
      'p50=${p50.toStringAsFixed(digits)} p95=${p95.toStringAsFixed(digits)} p99=${p99.toStringAsFixed(digits)}';
}
