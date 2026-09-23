// An editor opened from its built app while its Dart code is older than
// the files (ADR-0021 P3c, fast open) runs the old code for about two
// seconds, until `fynkel editor` has attached and hot restarted it. A hot
// restart starts the editor over, so whatever was done meanwhile — a
// project opened from the launcher, a selection, a dialog — would vanish.
// So the window is covered, and takes no clicks or keys, until then.
//
// `fynkel editor` says it with a file (`FYNKEL_EDITOR_UPDATING`): `stale`
// from launch, `restarting` once the restart is asked for, gone when it is
// over. Only an editor that starts while it says `stale` is the old code;
// the restarted one starts on `restarting` and shows nothing.

import 'dart:async';
import 'dart:io';

import 'package:flutter/widgets.dart';

class UpdatingCodeBarrier extends StatefulWidget {
  const UpdatingCodeBarrier({
    super.key,
    required this.path,
    required this.child,
    this.interval = const Duration(milliseconds: 100),
    this.timeout = const Duration(seconds: 30),
  });

  /// The file `fynkel editor` keeps; null when it did not start this editor.
  final String? path;
  final Widget child;
  final Duration interval;

  /// A `fynkel editor` that died leaves the file: the window uncovers anyway.
  final Duration timeout;

  @override
  State<UpdatingCodeBarrier> createState() => _UpdatingCodeBarrierState();
}

class _UpdatingCodeBarrierState extends State<UpdatingCodeBarrier> {
  bool _covered = false;
  Timer? _poll;
  var _waited = Duration.zero;

  String? _read() {
    final path = widget.path;
    if (path == null || path.isEmpty) return null;
    try {
      return File(path).readAsStringSync().trim();
    } on FileSystemException {
      return null;
    }
  }

  @override
  void initState() {
    super.initState();
    if (_read() != 'stale') return;
    _covered = true;
    _poll = Timer.periodic(widget.interval, (_) {
      _waited += widget.interval;
      if (_read() != null && _waited < widget.timeout) return;
      _poll?.cancel();
      setState(() => _covered = false);
    });
  }

  @override
  void dispose() {
    _poll?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Directionality(
      textDirection: TextDirection.ltr,
      child: Stack(
        fit: StackFit.expand,
        children: [
          ExcludeFocus(excluding: _covered, child: widget.child),
          if (_covered)
            const Positioned.fill(
              key: Key('updatingCode'),
              child: ColoredBox(
                color: Color(0xB3000000),
                child: Center(
                  child: DecoratedBox(
                    decoration: BoxDecoration(
                      color: Color(0xFF26272B),
                      borderRadius: BorderRadius.all(Radius.circular(8)),
                    ),
                    child: Padding(
                      padding: EdgeInsets.symmetric(horizontal: 20, vertical: 14),
                      child: Text(
                        'Loading code changes…',
                        style: TextStyle(color: Color(0xFFE6E6E6), fontSize: 13, decoration: TextDecoration.none),
                      ),
                    ),
                  ),
                ),
              ),
            ),
        ],
      ),
    );
  }
}
