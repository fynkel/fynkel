// An extension's window in its panel (ADR-0028 step 5): the package's
// `build`, run again whenever what its context reads changes — the
// document in front and its selection, the project, the Project panel —
// or the extensions register again (a hot reload). A throw is shown in the
// window's frame and logged once; a window the last reload dropped says
// so and keeps its place.
import 'package:flutter/material.dart';

import '../theme/tokens.dart';
import 'editor_extensions.dart';

class ExtensionWindowView extends StatefulWidget {
  const ExtensionWindowView({super.key, required this.host, required this.id});

  final EditorExtensionHost host;

  /// The window's id as registered (`game.levelTools`).
  final String id;

  @override
  State<ExtensionWindowView> createState() => _ExtensionWindowViewState();
}

class _ExtensionWindowViewState extends State<ExtensionWindowView> {
  List<Listenable> _watched = const [];

  @override
  void initState() {
    super.initState();
    _watch();
  }

  @override
  void didUpdateWidget(ExtensionWindowView old) {
    super.didUpdateWidget(old);
    _watch();
  }

  @override
  void dispose() {
    for (final l in _watched) {
      l.removeListener(_changed);
    }
    super.dispose();
  }

  /// Listens to what the context reads now (the document in front changes
  /// with the tabs, so the set is taken again on every change).
  void _watch() {
    final next = <Listenable>[widget.host, ...?widget.host.access?.watch().nonNulls];
    for (final l in _watched) {
      if (!next.contains(l)) l.removeListener(_changed);
    }
    for (final l in next) {
      if (!_watched.contains(l)) l.addListener(_changed);
    }
    _watched = next;
  }

  void _changed() {
    if (!mounted) return;
    _watch();
    setState(() {});
  }

  @override
  Widget build(BuildContext context) {
    final w = widget.host.window(widget.id);
    if (w == null) {
      return _Message(
        key: const Key('extension.window.missing'),
        text:
            'No extension registers the window "${widget.id}" now: its editor part removed it or failed '
            '(the Console says why). Close this tab, or save lib/editor/ again.',
      );
    }
    Widget child;
    try {
      child = w.build(context, widget.host.contextFor(w.package));
    } catch (e, stack) {
      // After the frame: the log notifies, and this is a build.
      WidgetsBinding.instance.addPostFrameCallback((_) => widget.host.windowFailed(w, e, stack));
      child = _Message(key: const Key('extension.window.error'), text: '${w.title} failed: $e', error: true);
    }
    return Material(type: MaterialType.transparency, child: child);
  }
}

class _Message extends StatelessWidget {
  const _Message({super.key, required this.text, this.error = false});

  final String text;
  final bool error;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    return Center(
      child: Padding(
        padding: EdgeInsets.all(t.pad * 2),
        child: Text(
          text,
          textAlign: TextAlign.center,
          style: Theme.of(context).textTheme.bodySmall?.copyWith(color: error ? t.error : t.textSecondary),
        ),
      ),
    );
  }
}
