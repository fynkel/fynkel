import 'package:flutter/widgets.dart';
import 'package:fynkel/fynkel.dart' show ObjectError;

/// What a debug build's [FynkelView] shows over the game when its code
/// throws: the latest error, the object it came from, and the line in the
/// game's code, so a failed game is never just a black screen. A tap
/// hides it until the next error.
class ErrorPanel extends StatelessWidget {
  const ErrorPanel({super.key, required this.errors, required this.onDismiss});

  /// The errors since the panel was last dismissed, the latest last.
  final List<ObjectError> errors;

  final VoidCallback onDismiss;

  static const _style = TextStyle(fontSize: 13, height: 1.35, color: Color(0xFFFFFFFF));

  @override
  Widget build(BuildContext context) {
    final latest = errors.last;
    final where = gameFrame(latest.stackTrace);
    final more = errors.length - 1;
    return Directionality(
      textDirection: TextDirection.ltr,
      child: SafeArea(
        child: Align(
          alignment: Alignment.topCenter,
          child: GestureDetector(
            onTap: onDismiss,
            child: Container(
              margin: const EdgeInsets.all(12),
              padding: const EdgeInsets.all(12),
              constraints: const BoxConstraints(maxWidth: 900),
              decoration: BoxDecoration(color: const Color(0xE0B00020), borderRadius: BorderRadius.circular(8)),
              // A small view scrolls it.
              child: SingleChildScrollView(
                child: DefaultTextStyle(
                  style: _style,
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        '${latest.object ?? 'The world'} threw:',
                        style: _style.copyWith(fontWeight: FontWeight.bold),
                      ),
                      const SizedBox(height: 4),
                      Text('${latest.error}', maxLines: 8, overflow: TextOverflow.ellipsis),
                      if (where != null) ...[const SizedBox(height: 4), Text('at $where')],
                      const SizedBox(height: 8),
                      Text(
                        '${more > 0 ? '$more more before it. ' : ''}The whole trace is in the console. Tap to hide.',
                        style: _style.copyWith(fontSize: 11, color: const Color(0xCCFFFFFF)),
                      ),
                    ],
                  ),
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// The first place in [stack] that's the game's own code, not Dart's,
/// Flutter's or Fynkel's: `package:my_game/main.dart:19:26`.
@visibleForTesting
String? gameFrame(StackTrace stack) {
  for (final match in RegExp(r'\((package:[^)]+|file:[^)]+)\)').allMatches('$stack')) {
    final place = match.group(1)!;
    if (place.startsWith('package:fynkel/') ||
        place.startsWith('package:fynkel_flutter/') ||
        place.startsWith('package:flutter/')) {
      continue;
    }
    return place;
  }
  return null;
}
