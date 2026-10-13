/// A list filled and emptied every frame that keeps its storage when
/// emptied. A Dart list gives its storage back on `clear()` (and on
/// `length = 0`), so it's grown again, allocating, the next frame.
///
/// For a package's own per-frame lists, as the core's moved log is one.
final class FrameList<T extends Object> {
  var _items = List<T?>.filled(8, null);
  var _length = 0;

  /// How many there are.
  int get length => _length;

  /// Whether there are none.
  bool get isEmpty => _length == 0;

  /// Whether there is at least one.
  bool get isNotEmpty => _length != 0;

  /// The one at [index], 0 to one less than [length].
  T operator [](int index) {
    RangeError.checkValidIndex(index, this, 'index', _length);
    return _items[index]!;
  }

  /// Puts [item] at the end.
  void add(T item) {
    if (_length == _items.length) {
      _items = List<T?>.filled(_length * 2, null)..setRange(0, _length, _items);
    }
    _items[_length++] = item;
  }

  /// Takes away those [test] is true of, keeping the others in order.
  void removeWhere(bool Function(T item) test) {
    var kept = 0;
    for (var i = 0; i < _length; i++) {
      final item = _items[i]!;
      if (!test(item)) _items[kept++] = item;
    }
    _items.fillRange(kept, _length, null);
    _length = kept;
  }

  /// Empties it, keeping the storage; lets go of what it held.
  void clear() {
    _items.fillRange(0, _length, null);
    _length = 0;
  }
}
