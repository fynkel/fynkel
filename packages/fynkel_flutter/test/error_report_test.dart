// A game's uncaught errors reach the editor channel as `error` log records
// with their stack, while an editor listens; the previous handlers still run.
import 'dart:convert';
import 'dart:ui';

import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/gameplay.dart';
import 'package:fynkel_flutter/src/error_report.dart';

void main() {
  test('Flutter and platform errors are sent while the channel is on, and passed on', () {
    TestWidgetsFlutterBinding.ensureInitialized();
    final lines = <String>[];
    final previous = FlutterError.onError;
    final dispatcher = PlatformDispatcher.instance;
    final previousPlatform = dispatcher.onError;
    final seen = <String>[];
    FlutterError.onError = (d) => seen.add('flutter ${d.exceptionAsString()}');
    dispatcher.onError = (e, s) {
      seen.add('platform $e');
      return true;
    };
    addTearDown(() {
      FlutterError.onError = previous;
      dispatcher.onError = previousPlatform;
      EditorChannel.debugUse();
    });

    ChannelErrorReporting.install();
    ChannelErrorReporting.install();
    EditorChannel.debugUse(enabled: true, sink: lines.add);
    FlutterError.onError!(FlutterErrorDetails(exception: StateError('no player'), stack: StackTrace.current));
    expect(dispatcher.onError!(ArgumentError('bad'), StackTrace.current), isTrue);

    final records = [for (final l in lines) jsonDecode(l) as Map<String, Object?>];
    expect(records.map((r) => (r['fynkel'], r['source'], r['severity'])), everyElement(('log', 'exception', 'error')));
    expect(records.map((r) => r['message']), ['Bad state: no player', 'Invalid argument(s): bad']);
    expect(records.first['stack'], contains('error_report_test.dart'));
    expect(seen, ['flutter Bad state: no player', 'platform Invalid argument(s): bad']);

    // No editor listening: nothing is sent, the handlers still run.
    lines.clear();
    EditorChannel.debugUse(enabled: false, sink: lines.add);
    FlutterError.onError!(FlutterErrorDetails(exception: StateError('quiet')));
    expect(lines, isEmpty);
    expect(seen.last, 'flutter Bad state: quiet');
  });
}
