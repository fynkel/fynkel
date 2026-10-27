// The feature lab: a list of scenes, one for each feature, and a page for
// each with its controls, readout and log over the game.

import 'package:flutter/material.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'lab.dart' as lab;
import 'palette.dart' as palette;
import 'scenes.dart';

void main() => runApp(const LabApp());

/// `--dart-define=LAB_SCENE=Minimap` opens the first scene whose title
/// starts with it, for measuring on a device without tapping through.
const _opened = String.fromEnvironment('LAB_SCENE');

/// `--dart-define=LAB_FULL=true` opens it with the panel hidden, the view
/// filling the screen.
const _full = bool.fromEnvironment('LAB_FULL');

class LabApp extends StatelessWidget {
  const LabApp({super.key});

  @override
  Widget build(BuildContext context) => ValueListenableBuilder(
    valueListenable: palette.themeMode,
    builder: (context, mode, _) => MaterialApp(
      title: 'Fynkel lab',
      debugShowCheckedModeBanner: false,
      theme: palette.labTheme(Brightness.light),
      darkTheme: palette.labTheme(Brightness.dark),
      themeMode: mode,
      home: _opened.isEmpty
          ? const SceneList()
          : ScenePage(scenes.firstWhere((make) => make().title.startsWith(_opened))),
    ),
  );
}

/// Every scene, by step.
class SceneList extends StatelessWidget {
  const SceneList({super.key});

  @override
  Widget build(BuildContext context) {
    // Made to read their names; never added, so they hold nothing.
    final about = [for (final make in scenes) make()];
    return Scaffold(
      appBar: AppBar(title: const Text('Fynkel lab')),
      body: ListView(
        children: [
          for (final (i, scene) in about.indexed)
            ListTile(
              title: Text(scene.title),
              subtitle: Text('${scene.step} · ${scene.about}', maxLines: 2, overflow: TextOverflow.ellipsis),
              trailing: const Icon(Icons.chevron_right),
              onTap: () => Navigator.of(context).push(MaterialPageRoute<void>(builder: (_) => ScenePage(scenes[i]))),
            ),
        ],
      ),
    );
  }
}

/// One scene: the game, and beside it what to do with it and what
/// happened. F5 (in a debug build) starts the scene over, its panel too.
class ScenePage extends StatefulWidget {
  const ScenePage(this.make, {super.key});
  final lab.LabScene Function() make;

  @override
  State<ScenePage> createState() => _ScenePageState();
}

class _ScenePageState extends State<ScenePage> {
  var _open = !_full;

  @override
  Widget build(BuildContext context) => FynkelGame(make: widget.make, builder: _page);

  Widget _page(BuildContext context, lab.LabScene scene) {
    final wide = MediaQuery.sizeOf(context).width >= 700;
    final panel = _Panel(scene);
    return Scaffold(
      appBar: AppBar(
        title: Text('${scene.title} · ${scene.step}'),
        actions: [
          IconButton(
            tooltip: _open ? 'Hide the panel' : 'Show the panel',
            icon: Icon(_open ? Icons.close_fullscreen : Icons.tune),
            onPressed: () => setState(() => _open = !_open),
          ),
        ],
      ),
      body: wide
          ? Row(
              children: [
                Expanded(child: _game(context, scene)),
                if (_open) SizedBox(width: 380, child: panel),
              ],
            )
          : Column(
              children: [
                Expanded(child: _game(context, scene)),
                if (_open) SizedBox(height: MediaQuery.sizeOf(context).height * 0.42, child: panel),
              ],
            ),
    );
  }
}

/// The scene's view, with its own widgets over it if it has any.
Widget _game(BuildContext context, lab.LabScene scene) {
  final view = scene.view(context);
  final over = scene.overlay(context);
  return over == null ? view : Stack(fit: StackFit.expand, children: [view, over]);
}

class _Panel extends StatelessWidget {
  const _Panel(this.scene);
  final lab.LabScene scene;

  @override
  Widget build(BuildContext context) {
    final text = Theme.of(context).textTheme;
    // A Material, not a ColoredBox: the switches paint on it.
    return Material(
      color: const Color(0xFF16191F),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Expanded(
            flex: 3,
            child: ListView(
              padding: const EdgeInsets.all(12),
              children: [
                // Folded on a phone, so the controls fit.
                ExpansionTile(
                  title: Text('About this scene', style: text.labelLarge),
                  initiallyExpanded: MediaQuery.sizeOf(context).width >= 700,
                  dense: true,
                  tilePadding: EdgeInsets.zero,
                  childrenPadding: const EdgeInsets.only(bottom: 8),
                  expandedAlignment: Alignment.topLeft,
                  children: [Text(scene.about, style: text.bodySmall)],
                ),
                // Unity's Profiler, for the Dart side of the frame.
                ExpansionTile(
                  title: Text('The frame, part by part (world.profile)', style: text.labelLarge),
                  dense: true,
                  tilePadding: EdgeInsets.zero,
                  expandedAlignment: Alignment.topLeft,
                  children: [
                    ValueListenableBuilder(
                      valueListenable: scene.profile,
                      builder: (context, profile, _) => Text(
                        profile.isEmpty ? 'after the first second' : profile,
                        style: text.bodySmall?.copyWith(fontFamily: 'Menlo', color: Colors.lightBlue[100]),
                      ),
                    ),
                  ],
                ),
                // The controls show the game's state, which changes each
                // frame: rebuild with the readout, and after each control,
                // as the readout may not change.
                ValueListenableBuilder(
                  valueListenable: scene.readout,
                  builder: (context, readout, _) => StatefulBuilder(
                    builder: (context, setState) => Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        Wrap(
                          spacing: 6,
                          runSpacing: 6,
                          children: [
                            for (final c in scene.controls)
                              if (c is lab.Button)
                                OutlinedButton(onPressed: () => setState(c.onPressed), child: Text(c.label)),
                          ],
                        ),
                        for (final c in scene.controls)
                          switch (c) {
                            lab.Toggle() => SwitchListTile(
                              dense: true,
                              contentPadding: EdgeInsets.zero,
                              title: Text(c.label),
                              value: c.value(),
                              onChanged: (v) => setState(() => c.onChanged(v)),
                            ),
                            lab.Slider() => Row(
                              children: [
                                SizedBox(width: 90, child: Text(c.label, style: text.bodySmall)),
                                Expanded(
                                  child: Slider(
                                    value: c.value().clamp(c.min, c.max),
                                    min: c.min,
                                    max: c.max,
                                    onChanged: (v) => setState(() => c.onChanged(v)),
                                  ),
                                ),
                                SizedBox(width: 40, child: Text(c.value().toStringAsFixed(2), style: text.bodySmall)),
                              ],
                            ),
                            lab.Button() => const SizedBox.shrink(),
                          },
                        if (readout.isNotEmpty) ...[
                          const SizedBox(height: 8),
                          Text(
                            readout,
                            style: text.bodySmall?.copyWith(fontFamily: 'Menlo', color: Colors.amber[200]),
                          ),
                        ],
                      ],
                    ),
                  ),
                ),
              ],
            ),
          ),
          const Divider(height: 1),
          Expanded(flex: 2, child: _LogView(scene.log)),
        ],
      ),
    );
  }
}

/// The scene's log, newest at the bottom, following it.
class _LogView extends StatelessWidget {
  const _LogView(this.log);
  final lab.Log log;

  @override
  Widget build(BuildContext context) => ValueListenableBuilder(
    valueListenable: log,
    builder: (context, lines, _) => ListView.builder(
      reverse: true,
      padding: const EdgeInsets.all(8),
      itemCount: lines.length,
      itemBuilder: (context, i) {
        final line = lines[lines.length - 1 - i];
        return Text(
          line.text,
          style: TextStyle(
            fontFamily: 'Menlo',
            fontSize: 11,
            color: line.error ? const Color(0xFFFF7070) : const Color(0xFFB8C0CC),
          ),
        );
      },
    ),
  );
}
