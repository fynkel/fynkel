// An editor started on a project (`fynkel editor <dir>`, a handover, a
// restart) shows this until the project is open, as Unity shows a loading
// window and never the Hub. The launcher comes only if the open fails.

import 'package:flutter/material.dart';
import 'package:path/path.dart' as p;

import '../prefs/preferences.dart';

class OpeningScreen extends StatelessWidget {
  const OpeningScreen({super.key, required this.dir, required this.prefs});

  final String dir;
  final Preferences prefs;

  String get name {
    final recent = prefs.recents.where((r) => p.equals(r.dir, dir)).firstOrNull;
    return recent == null || recent.name.isEmpty ? p.basename(dir) : recent.name;
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Scaffold(
      key: const Key('opening'),
      body: Center(
        child: SizedBox(
          width: 320,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text('Opening $name…', style: theme.textTheme.titleMedium),
              const SizedBox(height: 4),
              Text(dir, style: theme.textTheme.bodySmall, overflow: TextOverflow.ellipsis),
              const SizedBox(height: 12),
              const LinearProgressIndicator(),
            ],
          ),
        ),
      ),
    );
  }
}
