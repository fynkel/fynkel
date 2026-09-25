// Project › Modules… (ADR-0029 §3; Unity's Package Manager built-in
// modules): physics, audio and animation switched on or off for the game,
// plus whether a runtime built from source leaves the ones that are off
// out (`strip`). Applied as one asset operation (`setModules`, undone with
// the Project browser's ⌥⌘Z). A module turned off keeps its components in
// the scenes and prefabs; validate reports them (FYN-P0018) and Add
// Component stops offering them.
import 'package:flutter/material.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';

import '../theme/tokens.dart';
import 'editor_project.dart';

Future<void> showModulesDialog(BuildContext context, EditorProject project) {
  return showDialog<void>(
    animationStyle: AnimationStyle.noAnimation,
    context: context,
    builder: (_) => ModulesDialog(project: project),
  );
}

class ModulesDialog extends StatefulWidget {
  const ModulesDialog({super.key, required this.project});

  final EditorProject project;

  @override
  State<ModulesDialog> createState() => _ModulesDialogState();
}

class _ModulesDialogState extends State<ModulesDialog> {
  late final ProjectModules _old = widget.project.modules();
  late bool _physics = _old.physics;
  late bool _audio = _old.audio;
  late bool _animation = _old.animation;
  late bool _strip = _old.strip;
  String? _refusal;

  ProjectModules get _draft => ProjectModules(physics: _physics, audio: _audio, animation: _animation, strip: _strip);

  static const _about = {
    'physics': 'Rigid bodies, colliders, the character controller, raycasts and contacts.',
    'audio': 'Audio sources and the listener, world.audio and the buses.',
    'animation': 'Animators and skinned meshes: clips, blends and root motion.',
  };

  void _apply() {
    try {
      widget.project.setModules(
        physics: _physics == _old.physics ? null : _physics,
        audio: _audio == _old.audio ? null : _audio,
        animation: _animation == _old.animation ? null : _animation,
        strip: _strip == _old.strip ? null : _strip,
      );
      Navigator.of(context).pop();
    } on AuthoringException catch (e) {
      setState(() => _refusal = e.diagnostics.isEmpty ? '$e' : e.diagnostics.first.message);
    }
  }

  Widget _module(String key, String title, bool value, ValueChanged<bool> set) {
    final theme = Theme.of(context);
    final t = context.tokens;
    return CheckboxListTile(
      key: Key('modules.$key'),
      dense: true,
      contentPadding: EdgeInsets.zero,
      controlAffinity: ListTileControlAffinity.leading,
      value: value,
      onChanged: (v) => setState(() {
        set(v ?? true);
        _refusal = null;
      }),
      title: Text(title),
      subtitle: Text(
        '${_about[key]} ${ProjectModules.components[key]!.map((c) => c.substring('fynkel.'.length)).join(', ')}.',
        style: theme.textTheme.bodySmall?.copyWith(color: t.textSecondary),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final t = context.tokens;
    final draft = _draft;
    final turnedOff = [
      for (final m in ['physics', 'audio', 'animation'])
        if (_old.has(m) && !draft.has(m)) m,
    ];
    return AlertDialog(
      title: const Text('Modules'),
      content: SizedBox(
        width: 520,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(
                'The engine modules this game uses. A module that is off does not run: its components do '
                'nothing and scripts calling it get an error naming it.',
                style: theme.textTheme.bodySmall?.copyWith(color: t.textSecondary),
              ),
              const SizedBox(height: 8),
              _module('physics', 'Physics', _physics, (v) => _physics = v),
              _module('audio', 'Audio', _audio, (v) => _audio = v),
              _module('animation', 'Animation', _animation, (v) => _animation = v),
              const Divider(height: 17),
              CheckboxListTile(
                key: const Key('modules.strip'),
                dense: true,
                contentPadding: EdgeInsets.zero,
                controlAffinity: ListTileControlAffinity.leading,
                value: _strip,
                onChanged: (v) => setState(() {
                  _strip = v ?? false;
                  _refusal = null;
                }),
                title: const Text('Leave modules that are off out of the build'),
                subtitle: Text(
                  'Only for a runtime built from source; the prebuilt runtime keeps every module.',
                  style: theme.textTheme.bodySmall?.copyWith(color: t.textSecondary),
                ),
              ),
              if (turnedOff.isNotEmpty)
                Padding(
                  padding: const EdgeInsets.only(top: 6),
                  child: Text(
                    'The ${turnedOff.join(' and ')} components stay in your scenes and prefabs; Validate reports '
                    'each one until you remove it or turn the module back on.',
                    key: const Key('modules.warning'),
                    style: theme.textTheme.bodySmall?.copyWith(color: t.warning),
                  ),
                ),
              if (_refusal case final note?)
                Padding(
                  padding: const EdgeInsets.only(top: 6),
                  child: Text(
                    note,
                    key: const Key('modules.error'),
                    style: theme.textTheme.bodySmall?.copyWith(color: t.error),
                  ),
                ),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.of(context).pop(), child: const Text('Cancel')),
        FilledButton(
          key: const Key('modules.apply'),
          onPressed: draft != _old ? _apply : null,
          child: const Text('Apply'),
        ),
      ],
    );
  }
}
