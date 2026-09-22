// Notices: the one line the editor says something in. What a Material app
// would pop up as a snackbar — "Reveal in Finder failed", "Reimport refused",
// "Could not open …" — sits in the status bar instead (the launcher's own
// strip before a project is open), appears and goes without animation, and
// stays until it is dismissed or replaced: an error that slides away after a
// few seconds is an error nobody read.
import 'package:flutter/material.dart';

import '../theme/icons.dart';
import '../theme/tokens.dart';

enum NoticeSeverity { error, warning, info }

final class Notice {
  const Notice(this.message, this.severity);

  final String message;
  final NoticeSeverity severity;
}

/// The current notice, if any. One at a time: a new one replaces the old.
final class Notices extends ChangeNotifier {
  Notice? _current;

  Notice? get current => _current;

  void show(String message, {NoticeSeverity severity = NoticeSeverity.error}) {
    _current = Notice(message, severity);
    notifyListeners();
  }

  void dismiss() {
    if (_current == null) return;
    _current = null;
    notifyListeners();
  }

  /// The notices of the editor window around [context], or null outside one
  /// (a panel pumped on its own in a test).
  static Notices? maybeOf(BuildContext context) => context.dependOnInheritedWidgetOfExactType<NoticesScope>()?.notifier;
}

class NoticesScope extends InheritedNotifier<Notices> {
  const NoticesScope({super.key, required Notices notices, required super.child}) : super(notifier: notices);
}

/// The notice as one status-bar-height line: severity icon, the message
/// (ellipsised, the whole of it in the tooltip) and a dismiss button.
/// Shows nothing when there is no notice.
class NoticeLine extends StatelessWidget {
  const NoticeLine({super.key, required this.notices});

  final Notices notices;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: notices,
      builder: (context, _) {
        final n = notices.current;
        if (n == null) return const SizedBox.shrink();
        final t = context.tokens;
        final (icon, color) = switch (n.severity) {
          NoticeSeverity.error => (FIcons.errorOutline, t.error),
          NoticeSeverity.warning => (FIcons.warningAmber, t.warning),
          NoticeSeverity.info => (FIcons.infoOutline, t.info),
        };
        return Row(
          key: const Key('notice'),
          children: [
            Icon(icon, size: 13, color: color),
            const SizedBox(width: 6),
            Expanded(
              child: Tooltip(
                message: n.message,
                child: Text(
                  n.message.replaceAll('\n', ' '),
                  style: Theme.of(context).textTheme.bodySmall?.copyWith(color: t.text),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
            ),
            SizedBox.square(
              dimension: t.row,
              child: IconButton(
                key: const Key('notice.dismiss'),
                tooltip: 'Dismiss',
                padding: EdgeInsets.zero,
                iconSize: 12,
                onPressed: notices.dismiss,
                icon: const Icon(FIcons.close),
              ),
            ),
          ],
        );
      },
    );
  }
}

/// A status strip holding only the notice: the launcher's, where there is no
/// status bar yet. Nothing when there is no notice.
class NoticeStrip extends StatelessWidget {
  const NoticeStrip({super.key, required this.notices});

  final Notices notices;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: notices,
      builder: (context, _) {
        if (notices.current == null) return const SizedBox.shrink();
        final t = context.tokens;
        return Container(
          height: t.row + 2,
          padding: EdgeInsets.only(left: t.pad, right: 2),
          decoration: BoxDecoration(
            color: t.header,
            border: Border(top: BorderSide(color: t.line)),
          ),
          child: NoticeLine(notices: notices),
        );
      },
    );
  }
}
