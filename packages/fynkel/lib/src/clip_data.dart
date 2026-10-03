/// A clip made in code (ADR-0026 §7, ADR-0033 § 2): what `World.createClip`
/// plays.
library;

import 'dart:convert';
import 'dart:typed_data';

import '../math.dart';

/// How a channel is read between its keys.
enum ClipInterpolation {
  /// Straight lines (a spherical blend for a rotation).
  linear,

  /// Holds each key until the next.
  step,
}

/// One keyed property of one bound node under the animator.
class ClipChannel {
  const ClipChannel._(this.target, this.property, this.interpolation, this.times, this.values);

  /// The node's AnimationBinding key, unique under the entity playing the clip.
  final String target;

  /// 0 position, 1 rotation, 2 scale (the `ANIM` payload's numbers).
  final int property;
  final ClipInterpolation interpolation;
  final List<double> times;

  /// 3 floats per key (4 for a rotation: x, y, z, w).
  final List<double> values;
}

/// An animation clip made in code: channels that key a node's local
/// position, rotation or scale, and markers. Make one with [ClipBuilder]
/// and give it to `World.createClip` for an `AssetRef` that an
/// `Animator`, a one-shot (`animation.playOneShot`) or a blend space plays
/// like an imported clip.
class ClipData {
  ClipData._(this.length, this.channels, this.markers);

  /// Seconds.
  final double length;
  final List<ClipChannel> channels;

  /// `(time, name)` in time order: each raises `AnimationMarker` as the
  /// clip passes it.
  final List<(double, String)> markers;

  /// The `ANIM` chunk payload (`fynkel_world_set_clip`).
  Uint8List toPayload() {
    final out = BytesBuilder(copy: false);
    final b = ByteData(8);
    void u32(int v) {
      b.setUint32(0, v, Endian.little);
      out.add(b.buffer.asUint8List(0, 4).toList());
    }

    void f32(double v) {
      b.setFloat32(0, v, Endian.little);
      out.add(b.buffer.asUint8List(0, 4).toList());
    }

    void str(String s) {
      final bytes = utf8.encode(s);
      u32(bytes.length);
      out.add(bytes);
      out.add(List.filled((4 - bytes.length % 4) % 4, 0));
    }

    u32(0x32444e42); // BND2: stable animation bindings.
    u32(channels.length);
    f32(length);
    for (final c in channels) {
      str(c.target);
      str(''); // Runtime clips need no display-name retargeting metadata.
      u32(c.property);
      u32(c.interpolation == ClipInterpolation.step ? 0 : 1);
      u32(c.times.length);
      c.times.forEach(f32);
      c.values.forEach(f32);
    }
    if (markers.isNotEmpty) {
      u32(markers.length);
      for (final (t, name) in markers) {
        f32(t);
        str(name);
      }
    }
    return out.toBytes();
  }
}

/// Builds a [ClipData] (Unity's `AnimationClip.SetCurve`, by node and
/// property rather than by curve):
///
/// ```dart
/// final wave = world.createClip((ClipBuilder()
///       ..rotation('RightArm', {
///         0: Quat.identity,
///         0.4: Quat.euler(const Vec3(0, 0, 120)),
///         0.8: Quat.identity,
///       })
///       ..marker(0.4, 'wave'))
///     .build());
/// rig.animation.playOneShot(wave, slot: 'UpperBody');
/// ```
///
/// Keys are `{seconds: value}`; each node's AnimationBinding key is resolved under the
/// entity that plays the clip. The clip is as long as its last key unless
/// [length] says otherwise.
class ClipBuilder {
  ClipBuilder({this.length});

  /// Seconds; null: the last key's (or marker's) time.
  final double? length;

  final List<ClipChannel> _channels = [];
  final List<(double, String)> _markers = [];

  /// Keys [target]'s local position.
  void position(String target, Map<double, Vec3> keys, {ClipInterpolation interpolation = ClipInterpolation.linear}) =>
      _add(target, 0, interpolation, keys, (v) => [v.x, v.y, v.z]);

  /// Keys [target]'s local rotation.
  void rotation(String target, Map<double, Quat> keys, {ClipInterpolation interpolation = ClipInterpolation.linear}) =>
      _add(target, 1, interpolation, keys, (q) => [q.x, q.y, q.z, q.w]);

  /// Keys [target]'s local scale.
  void scale(String target, Map<double, Vec3> keys, {ClipInterpolation interpolation = ClipInterpolation.linear}) =>
      _add(target, 2, interpolation, keys, (v) => [v.x, v.y, v.z]);

  /// A named time: `AnimationMarker` fires as the clip passes it.
  void marker(double seconds, String name) {
    if (!seconds.isFinite || seconds < 0) {
      throw ArgumentError.value(seconds, 'seconds', 'must be finite and >= 0');
    }
    if (name.isEmpty) {
      throw ArgumentError.value(name, 'name', 'must not be empty');
    }
    _markers.add((seconds, name));
  }

  void _add<T>(
    String target,
    int property,
    ClipInterpolation interpolation,
    Map<double, T> keys,
    List<double> Function(T) floats,
  ) {
    if (target.isEmpty) {
      throw ArgumentError.value(target, 'target', "must specify an AnimationBinding key");
    }
    if (keys.isEmpty) {
      throw ArgumentError.value(keys, 'keys', 'needs at least one key');
    }
    final times = keys.keys.toList()..sort();
    final values = <double>[];
    for (final t in times) {
      if (!t.isFinite || t < 0) {
        throw ArgumentError.value(t, 'keys', 'key times must be finite and >= 0');
      }
      final v = floats(keys[t] as T);
      if (!v.every((x) => x.isFinite)) {
        throw ArgumentError.value(keys[t], 'keys', 'at $t: not finite');
      }
      values.addAll(v);
    }
    _channels.add(ClipChannel._(target, property, interpolation, List.unmodifiable(times), List.unmodifiable(values)));
  }

  /// The clip. Throws [ArgumentError] when [length] is shorter than a key
  /// or a marker, or nothing was keyed.
  ClipData build() {
    if (_channels.isEmpty) {
      throw StateError('ClipBuilder.build: key at least one property');
    }
    var last = 0.0;
    for (final c in _channels) {
      if (c.times.last > last) last = c.times.last;
    }
    for (final (t, _) in _markers) {
      if (t > last && length == null) last = t;
    }
    final len = length ?? last;
    if (!len.isFinite || len < last) {
      throw ArgumentError.value(length, 'length', 'shorter than the last key or marker ($last s)');
    }
    for (final (t, name) in _markers) {
      if (t > len) {
        throw ArgumentError.value(t, 'marker $name', 'past the clip\'s length ($len s)');
      }
    }
    final markers = [..._markers]..sort((a, b) => a.$1.compareTo(b.$1));
    return ClipData._(len, List.unmodifiable(_channels), List.unmodifiable(markers));
  }
}
