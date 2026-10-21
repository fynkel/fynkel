import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'game.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await SystemChrome.setPreferredOrientations([DeviceOrientation.portraitUp]);
  final prefs = await SharedPreferences.getInstance();
  runApp(
    FynkelApp(
      () => PocketGame(
        level: (prefs.getInt('level') ?? 1).clamp(1, 1000000),
        onCompleted: (level) {
          unawaited(prefs.setInt('level', level));
        },
      ),
      title: 'Pocket Hole',
      theme: ThemeData(
        useMaterial3: true,
        colorScheme: ColorScheme.fromSeed(seedColor: const Color(0xFF7054CD)),
        fontFamily: 'Arial',
      ),
      overlay: (context, game) => PocketHud(game: game, prefs: prefs),
    ),
  );
}

class PocketHud extends StatefulWidget {
  const PocketHud({super.key, required this.game, required this.prefs});
  final PocketGame game;
  final SharedPreferences prefs;
  @override
  State<PocketHud> createState() => _PocketHudState();
}

class _PocketHudState extends State<PocketHud> with WidgetsBindingObserver {
  bool haptics = true;
  PocketGame get game => widget.game;
  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    haptics = widget.prefs.getBool('haptics') ?? true;
    game.revision.addListener(changed);
  }

  void changed() {
    if (haptics && game.state == PlayState.cleared) HapticFeedback.mediumImpact();
    if (mounted) setState(() {});
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state != AppLifecycleState.resumed) game.pause();
  }

  @override
  void dispose() {
    game.revision.removeListener(changed);
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  Widget button(String text, VoidCallback action) => FilledButton(
    onPressed: action,
    style: FilledButton.styleFrom(
      backgroundColor: const Color(0xFF6B4BC3),
      foregroundColor: Colors.white,
      minimumSize: const Size(230, 56),
    ),
    child: Text(text, style: const TextStyle(fontSize: 17, fontWeight: FontWeight.w700)),
  );
  @override
  Widget build(BuildContext context) {
    final playing = game.state == PlayState.playing;
    return LayoutBuilder(
      builder: (context, constraints) => Stack(
        fit: StackFit.expand,
        children: [
          GestureDetector(
            behavior: HitTestBehavior.opaque,
            dragStartBehavior: DragStartBehavior.down,
            onPanUpdate: (d) {
              final unit = 16.6 / constraints.maxHeight;
              game.move(d.delta.dx * unit, d.delta.dy * unit * 1.28);
            },
            child: const SizedBox.expand(),
          ),
          SafeArea(
            child: Padding(
              padding: const EdgeInsets.fromLTRB(26, 18, 26, 18),
              child: Column(
                children: [
                  Row(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            const Text(
                              'POCKET HOLE',
                              style: TextStyle(
                                color: Color(0xFF443266),
                                fontSize: 15,
                                fontWeight: FontWeight.w900,
                                letterSpacing: 2.5,
                              ),
                            ),
                            const SizedBox(height: 5),
                            Text(
                              'Level ${game.level.toString().padLeft(2, '0')}',
                              style: const TextStyle(
                                color: Color(0xFF302344),
                                fontSize: 32,
                                fontWeight: FontWeight.w800,
                              ),
                            ),
                          ],
                        ),
                      ),
                      if (playing)
                        IconButton.filledTonal(
                          tooltip: 'Pause',
                          onPressed: game.pause,
                          icon: const Icon(Icons.pause_rounded),
                        ),
                    ],
                  ),
                  const SizedBox(height: 16),
                  ClipRRect(
                    borderRadius: BorderRadius.circular(10),
                    child: LinearProgressIndicator(
                      value: game.progress,
                      minHeight: 7,
                      backgroundColor: const Color(0xFFD5CDEB),
                      color: const Color(0xFF7956D4),
                    ),
                  ),
                  const SizedBox(height: 8),
                  Row(
                    mainAxisAlignment: MainAxisAlignment.spaceBetween,
                    children: [
                      const Text(
                        'CLEAR THE BOARD',
                        style: TextStyle(
                          fontSize: 10,
                          letterSpacing: 1.8,
                          color: Color(0xFF716386),
                          fontWeight: FontWeight.bold,
                        ),
                      ),
                      Text(
                        '${game.collected} / ${game.total}',
                        style: const TextStyle(color: Color(0xFF54416F), fontWeight: FontWeight.bold),
                      ),
                    ],
                  ),
                  const Spacer(),
                  if (playing)
                    const Text(
                      'Drag anywhere to move the hole',
                      style: TextStyle(color: Color(0xFF655477), fontSize: 14, fontWeight: FontWeight.w600),
                    ),
                ],
              ),
            ),
          ),
          if (!playing)
            Positioned.fill(
              child: ColoredBox(
                color: const Color(0xB3ECE8FA),
                child: SafeArea(
                  child: Center(
                    child: Padding(
                      padding: const EdgeInsets.all(30),
                      child: Column(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          Icon(
                            game.state == PlayState.cleared ? Icons.auto_awesome_rounded : Icons.trip_origin,
                            size: 64,
                            color: const Color(0xFF7152C5),
                          ),
                          const SizedBox(height: 22),
                          Text(
                            switch (game.state) {
                              PlayState.title => 'Small hole.\nBig appetite.',
                              PlayState.paused => 'Take a breath.',
                              _ => 'Beautifully\ncleared.',
                            },
                            textAlign: TextAlign.center,
                            style: const TextStyle(
                              fontSize: 43,
                              height: 1.05,
                              letterSpacing: -1.8,
                              fontWeight: FontWeight.w900,
                              color: Color(0xFF382551),
                            ),
                          ),
                          const SizedBox(height: 18),
                          Text(
                            switch (game.state) {
                              PlayState.title => 'Sweep up every color. Make room for calm.',
                              PlayState.paused => 'Your board is right where you left it.',
                              _ => '${game.total} blocks • ${game.elapsed.toStringAsFixed(1)} seconds',
                            },
                            textAlign: TextAlign.center,
                            style: const TextStyle(fontSize: 15, color: Color(0xFF77658C)),
                          ),
                          const SizedBox(height: 32),
                          button(
                            switch (game.state) {
                              PlayState.title => 'Let’s play',
                              PlayState.paused => 'Keep playing',
                              _ => 'Next level →',
                            },
                            () {
                              switch (game.state) {
                                case PlayState.title:
                                  game.start();
                                case PlayState.paused:
                                  game.resume();
                                case PlayState.cleared:
                                  game.next();
                                case PlayState.playing:
                                  break;
                              }
                            },
                          ),
                          if (game.state != PlayState.title)
                            TextButton(onPressed: game.start, child: const Text('Restart level')),
                          const SizedBox(height: 16),
                          TextButton.icon(
                            onPressed: () {
                              setState(() => haptics = !haptics);
                              unawaited(widget.prefs.setBool('haptics', haptics));
                            },
                            icon: Icon(haptics ? Icons.vibration : Icons.phone_android),
                            label: Text('Haptics ${haptics ? 'on' : 'off'}'),
                          ),
                        ],
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
