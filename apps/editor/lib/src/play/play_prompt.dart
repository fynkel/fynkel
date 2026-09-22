// Play's save prompt: the game process reads files from disk, so anything
// unsaved is not in the run. The prompt says which documents those are and
// offers the three honest answers — save them and play, play the last saved
// bytes anyway, or go back.
import 'package:flutter/material.dart';

import '../documents/open_document.dart';

enum PlaySaveChoice { save, playAnyway, cancel }

Future<PlaySaveChoice> showPlaySavePrompt(BuildContext context, List<OpenDocument> dirty) async {
  final r = await showDialog<PlaySaveChoice>(
    context: context,
    builder: (context) => AlertDialog(
      key: const Key('dialog.playSave'),
      title: Text('Save ${dirty.length} document${dirty.length == 1 ? '' : 's'} before playing?'),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const Text('The game runs the files on disk — unsaved changes are not in the run.'),
          const SizedBox(height: 8),
          for (final tab in dirty.take(8))
            Text(
              '• ${tab.title} — ${tab.document.history} transaction${tab.document.history == 1 ? '' : 's'}',
              style: Theme.of(context).textTheme.bodySmall,
            ),
          if (dirty.length > 8)
            Text('• and ${dirty.length - 8} more', style: Theme.of(context).textTheme.bodySmall),
        ],
      ),
      actions: [
        TextButton(
          key: const Key('playSave.cancel'),
          onPressed: () => Navigator.pop(context, PlaySaveChoice.cancel),
          child: const Text('Cancel'),
        ),
        TextButton(
          key: const Key('playSave.playAnyway'),
          onPressed: () => Navigator.pop(context, PlaySaveChoice.playAnyway),
          child: const Text('Play Without Saving'),
        ),
        FilledButton(
          key: const Key('playSave.save'),
          onPressed: () => Navigator.pop(context, PlaySaveChoice.save),
          child: const Text('Save and Play'),
        ),
      ],
    ),
  );
  return r ?? PlaySaveChoice.cancel;
}
