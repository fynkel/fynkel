// HUD model binding (M3-13, blueprint §10, ADR-0008): widgets read a
// `GameModel`'s `ModelValue` fields and rebuild when a field changes —
// never per frame, never from the engine snapshot. `ModelBinding` hands a
// model down the tree, `ModelBuilder` rebuilds one subtree from one field,
// and `.listenable` adapts a field to Flutter's `ValueListenable` for
// `ValueListenableBuilder`, `AnimatedBuilder` and friends.

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:fynkel/fynkel.dart';

/// Makes a [GameModel] available to the widgets below it:
/// `ModelBinding.of<HudModel>(context)`.
///
/// The binding rebuilds its dependents only when the model instance is
/// replaced; field changes reach widgets through [ModelBuilder] or
/// [ModelValueListenable.listenable], so a HUD updates one text at a time.
class ModelBinding<M extends GameModel> extends InheritedWidget {
  const ModelBinding({super.key, required this.model, required super.child});

  final M model;

  static M of<M extends GameModel>(BuildContext context) {
    final m = maybeOf<M>(context);
    assert(m != null, 'No ModelBinding<$M> above this widget');
    return m!;
  }

  static M? maybeOf<M extends GameModel>(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<ModelBinding<M>>()?.model;

  @override
  bool updateShouldNotify(ModelBinding<M> old) => !identical(old.model, model);
}

/// Rebuilds [builder] whenever [value] changes (`ModelValue` notifies on
/// `!=` only). [child] is passed through untouched for the part of the
/// subtree that does not depend on the value.
class ModelBuilder<T> extends StatefulWidget {
  const ModelBuilder({super.key, required this.value, required this.builder, this.child});

  final ModelValue<T> value;
  final Widget Function(BuildContext context, T value, Widget? child) builder;
  final Widget? child;

  @override
  State<ModelBuilder<T>> createState() => _ModelBuilderState<T>();
}

class _ModelBuilderState<T> extends State<ModelBuilder<T>> {
  late T _value;

  @override
  void initState() {
    super.initState();
    _value = widget.value.value;
    widget.value.addListener(_changed);
  }

  @override
  void didUpdateWidget(ModelBuilder<T> old) {
    super.didUpdateWidget(old);
    if (!identical(old.value, widget.value)) {
      old.value.removeListener(_changed);
      _value = widget.value.value;
      widget.value.addListener(_changed);
    }
  }

  @override
  void dispose() {
    widget.value.removeListener(_changed);
    super.dispose();
  }

  void _changed() {
    if (!mounted) return;
    setState(() => _value = widget.value.value);
  }

  @override
  Widget build(BuildContext context) => widget.builder(context, _value, widget.child);
}

/// A [ModelValue] as a Flutter [ValueListenable].
extension ModelValueListenable<T> on ModelValue<T> {
  /// The same field seen through Flutter's interface (listeners added here
  /// are the field's own; `ValueListenableBuilder(valueListenable:
  /// model.score.listenable, …)`).
  ValueListenable<T> get listenable => _ModelValueListenable<T>(this);
}

class _ModelValueListenable<T> implements ValueListenable<T> {
  const _ModelValueListenable(this._field);

  final ModelValue<T> _field;

  @override
  T get value => _field.value;

  @override
  void addListener(VoidCallback listener) => _field.addListener(listener);

  @override
  void removeListener(VoidCallback listener) => _field.removeListener(listener);

  @override
  bool operator ==(Object other) => other is _ModelValueListenable<T> && identical(other._field, _field);

  @override
  int get hashCode => _field.hashCode;
}
