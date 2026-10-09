// The feature lab: a list of scenes, one for each feature, and a page for
// each with its controls, readout and log over the game.

import 'package:flutter/material.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'lab.dart' as lab;
import 'scenes.dart';

void main() => runApp(const LabApp());

class LabApp extends StatelessWidget {
  const LabApp({super.key});

  @override
  Widget build(BuildContext context) => MaterialApp(
    title: 'Fynkel lab',
    debugShowCheckedModeBanner: false,
    theme: ThemeData(brightness: Brightness.dark, colorSchemeSeed: const Color(0xFF4CA6E0), useMaterial3: true),
    home: const SceneList(),
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
              onTap: () => Navigator.of(context).push(MaterialPageRoute<void>(builder: (_) => ScenePage(scenes[i]()))),
            ),
        ],
      ),
    );
  }
}

/// One scene: the game, and over it what to do with it and what happened.
class ScenePage extends StatefulWidget {
  const ScenePage(this.scene, {super.key});
  final lab.LabScene scene;

  @override
  State<ScenePage> createState() => _ScenePageState();
}

class _ScenePageState extends State<ScenePage> {
  var _open = true;

  lab.LabScene get scene => widget.scene;

  @override
  Widget build(BuildContext context) {
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
                Expanded(child: FynkelView(root: scene)),
                if (_open) SizedBox(width: 380, child: panel),
              ],
            )
          : Column(
              children: [
                Expanded(child: FynkelView(root: scene)),
                if (_open) SizedBox(height: MediaQuery.sizeOf(context).height * 0.42, child: panel),
              ],
            ),
    );
  }
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
                Text(scene.about, style: text.bodySmall),
                const SizedBox(height: 8),
                // The controls show the game's state, which changes each
                // frame: rebuild with the readout.
                ValueListenableBuilder(
                  valueListenable: scene.readout,
                  builder: (context, readout, _) => Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      Wrap(
                        spacing: 6,
                        runSpacing: 6,
                        children: [
                          for (final c in scene.controls)
                            if (c is lab.Button) OutlinedButton(onPressed: c.onPressed, child: Text(c.label)),
                        ],
                      ),
                      for (final c in scene.controls)
                        switch (c) {
                          lab.Toggle() => SwitchListTile(
                            dense: true,
                            contentPadding: EdgeInsets.zero,
                            title: Text(c.label),
                            value: c.value(),
                            onChanged: c.onChanged,
                          ),
                          lab.Slider() => Row(
                            children: [
                              SizedBox(width: 90, child: Text(c.label, style: text.bodySmall)),
                              Expanded(
                                child: Slider(
                                  value: c.value().clamp(c.min, c.max),
                                  min: c.min,
                                  max: c.max,
                                  onChanged: c.onChanged,
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
