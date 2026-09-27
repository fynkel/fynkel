// The acceptance test of ADR-0026: the whole game, headless, both ways it
// can be made — built live from the specs (`BuildMode.live`) and loaded
// from the scene documents tool/scenes.dart writes from the same specs
// (`BuildMode.scenes`) — driven by one input route that collects every
// coin. The route is built here in Dart, key by key; nothing the game
// runs on is JSON someone wrote. `flutter test` runs it; the world reads
// the cooked cache, so the test cooks first (`cookProject`).

import 'dart:io';

import 'package:code_only/editor/editor.dart' show PlayTools;
import 'package:code_only/game/coin_kind.dart';
import 'package:code_only/game/collectible.dart';
import 'package:code_only/game/game.dart';
import 'package:code_only/game/game_state.dart';
import 'package:code_only/game/gate.dart';
import 'package:code_only/game/level.dart';
import 'package:code_only/game/player.dart';
import 'package:code_only/game/switch.dart';
import 'package:code_only/generated/assets.g.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:pickups/generated/assets.g.dart' as pickups;
import 'package:pickups/pickups.dart' show Pickup;

import '../tool/scenes.dart';

const dt = 1 / 60;

/// Keyboard input, frame by frame.
class Route {
  final frames = <RecordedFrame>[];

  int _frames(double seconds) => (seconds / dt).round();

  /// [key] held for [seconds].
  void hold(String key, double seconds) {
    final n = _frames(seconds);
    for (var i = 0; i < n; i++) {
      frames.add(RecordedFrame(dt, [if (i == 0) InputEvent.down(InputDevice.keyboard, key)]));
    }
    frames.add(RecordedFrame(dt, [InputEvent.up(InputDevice.keyboard, key)]));
  }

  /// [key] pressed for one frame.
  void tap(String key) => hold(key, dt);

  void wait(double seconds) {
    for (var i = 0; i < _frames(seconds); i++) {
      frames.add(const RecordedFrame(dt));
    }
  }

  InputRecording get recording => InputRecording(frames);
}

/// Every coin, the switch, the gate, the cell behind it: forward is −Z
/// (the camera's yaw stays 0), the player walks at 4 m/s.
InputRecording collectAll() => (Route()
      ..hold('W', 2.25) // coins 1 and 2, to z −9
      ..hold('D', 1) // coin 3 at x 4
      ..hold('A', 2.5) // back to the switch at x −6
      ..wait(0.3)
      ..tap('E') // the gate rises for 1.2 s
      ..wait(1.5)
      ..hold('A', 2.6) // through the gate: coins 4 and 5
      ..wait(0.5))
    .recording;

void main() {
  final projectDir = Directory.current.path;
  late NativeLibrary lib;
  setUpAll(() {
    cookProject(projectDir);
    lib = NativeLibrary.open();
  });
  tearDownAll(() => lib.dispose());

  test('the documents in assets/ are what tool/scenes.dart writes from the specs', () {
    final written = writeScenes(projectDir);
    expect(
      written.map((d) => d.path),
      containsAll(['scenes/menu.scene.json', 'scenes/level.scene.json', 'prefabs/coin.prefab.json', 'data/big_coin.data.json']),
    );
    expect(
      [for (final d in written) if (d.changed) d.path],
      isEmpty,
      reason: 'a spec changed: run `dart run tool/scenes.dart`, `fynkel cook` and commit the documents',
    );
  });

  Game load(BuildMode mode) {
    final game = Game.load(projectDir, mode: mode, library: lib);
    addTearDown(game.dispose);
    game.world.onLoadDiagnostics = (d) => fail('load diagnostics: $d');
    return game;
  }

  /// Plays until the level is up.
  Game playing(BuildMode mode) {
    final game = load(mode)..play();
    game.frame(dt);
    game.frame(dt);
    expect(game.state.screen.value, Screen.playing);
    return game;
  }

  for (final mode in BuildMode.values) {
    group('${mode.name}:', () {
      test('the Menu first; Play puts the level up', () {
        final game = load(mode);
        game.frame(dt);
        expect(game.state.screen.value, Screen.menu);
        expect(game.report.sceneId, isNull, reason: 'the project names no startup scene');
        expect(game.world.find('Big coin')!.get<Rotator>()!.speed, closeTo(0.8, 1e-6));
        expect(game.world.find('Player'), isNull);
        // The game uses no animation: the project turns the module off
        // and strips it (ADR-0029); physics and audio run.
        expect(game.report.modules, const Modules(animation: false));
        expect(game.world.modules.has(Module.animation), isFalse);
        expect(game.world.modules.has(Module.physics), isTrue);

        game.play();
        game.frame(dt);
        game.frame(dt);
        expect(game.state.screen.value, Screen.playing);
        expect(game.world.find('Big coin'), isNull, reason: 'the Menu is gone');
        expect(game.world.findObjectsOfType<Collectible>(), hasLength(5));
        expect(game.state.total.value, 5);
        expect(game.world.findAllWithTag(pickupTag), hasLength(5));
        final player = game.world.findWithTag(playerTag)!;
        expect(player.behaviour<Player>(), isNotNull);
        expect(player.layer, 'player');
        expect(game.world.findObjectOfType<Switch>()!.target, 'Gate');
        expect(game.world.find('Gate')!.behaviour<Gate>(), isNotNull);
        expect(game.world.find('Sweeper')!.get<Oscillate>()!.amplitude, 1.5);
        expect(game.world.find('Coin 3')!.localTransform.position, coinSpots[2]);

        // The kinds of coin (ADR-0027): four plain coins share one kind, the
        // last is the big one — its colour and size taken at start.
        final coins = [for (var i = 1; i <= 5; i++) game.world.find('Coin $i')!.behaviour<Collectible>()!];
        final (plain, big) = (coins.first.kind!, coins.last.kind!);
        for (final c in coins.take(4)) {
          expect(c.kind, same(plain));
        }
        expect((plain.value, big.value, big.size, big.pitch), (1, 5, 1.6, 0.7));
        expect(game.world.find('Coin 5')!.localTransform.scale.x, closeTo(1.6, 1e-6));
        Vec4? colour(String coin) {
          final disc = game.world.find(coin)!.children.single.get<MeshRenderer>()!.material;
          return game.world.runtimeMaterial(disc)?.baseColor;
        }

        expect(colour('Coin 5'), Vec4(big.color.x, big.color.y, big.color.z, 1));
        expect(colour('Coin 1'), Vec4(plain.color.x, plain.color.y, plain.color.z, 1));
        // Loaded from the scenes, the coins hold the written data assets;
        // built live, the same values made in code, with no asset.
        final written = game.world.data<CoinKind>(Assets.data.bigCoin);
        expect(written.value, 5);
        if (mode == BuildMode.scenes) {
          expect(big, same(written), reason: 'one shared instance per asset');
          expect(plain.asset, Assets.data.coin);
        } else {
          expect(big.asset, isNull);
        }

        // The gem: the pickups package's prefab (ADR-0029), its Pickup
        // holding the package's gem kind, coloured at start.
        final gem = game.world.find('Gem')!;
        expect(gem.localTransform.position, gemSpot);
        final pickup = gem.behaviour<Pickup>()!;
        expect(pickup.kind!.asset, pickups.Assets.data.gem);
        expect(pickup.kind!.points, 10);
        final gemColour = game.world.runtimeMaterial(gem.children.single.get<MeshRenderer>()!.material)?.baseColor;
        expect(gemColour, Vec4(pickup.kind!.color.x, pickup.kind!.color.y, pickup.kind!.color.z, 1));
      });

      test('the route collects every coin through the gate, then the Results', () {
        final game = playing(mode);
        final gate = game.world.find('Gate')!.behaviour<Gate>()!;
        var hinted = false;
        game.state.hint.addListener(() => hinted |= game.state.hint.value.isNotEmpty);

        collectAll().replay(game.world);
        expect(game.state.collected.value, 5);
        expect(game.state.score.value, 19, reason: 'four coins, a big one worth five and the gem worth ten');
        expect(game.world.find('Gem'), isNull, reason: 'the gem was taken on the way');
        expect(game.state.won.value, isTrue);
        expect(game.state.deaths.value, 0);
        expect(hinted, isTrue, reason: 'the switch prompted');
        expect(gate.opened, isTrue);
        expect(game.world.findObjectsOfType<Collectible>(), isEmpty);
        expect(game.world.findWithTag(playerTag)!.localTransform.position.x, lessThan(-15));

        for (var i = 0; i < 120 && game.state.screen.value != Screen.results; i++) {
          game.frame(dt);
        }
        expect(game.state.screen.value, Screen.results);
        final run = game.state.lastRun.value!;
        expect((run.collected, run.total, run.score, run.deaths), (5, 5, 19, 0));
        expect(run.won, isTrue);
        expect(game.world.find('Jingle')!.get<AudioSource>()!.playing, isTrue);

        game.toMenu();
        game.frame(dt);
        game.frame(dt);
        expect(game.state.screen.value, Screen.menu);
      });

      test("the editor's Level Tools during Play: every coin collected, the player sent back", () {
        final game = playing(mode);
        expect(PlayTools.collectAll(game.world), 5);
        expect(PlayTools.collectAll(game.world), 0, reason: 'each coin once');
        expect((game.state.collected.value, game.state.score.value, game.state.won.value), (5, 9, true));
        game.frame(dt);
        expect(game.world.findObjectsOfType<Collectible>(), isEmpty);

        final player = game.world.findWithTag(playerTag)!;
        player.transform.position = const Vec3(6, 0, 1);
        game.frame(dt);
        PlayTools.respawn(game.world);
        game.frame(dt);
        expect(game.state.deaths.value, 1);
        expect(player.localTransform.position.x, closeTo(0, 1e-3));
      });

      test('the sweeper kills: back to the start', () {
        final game = playing(mode);
        final player = game.world.findWithTag(playerTag)!;
        player.transform.position = const Vec3(6, 0, 1);
        game.frame(dt);
        (Route()..hold('W', 1.5)).recording.replay(game.world);
        expect(game.state.deaths.value, 1);
        expect(player.localTransform.position.x, closeTo(0, 1e-3), reason: 'respawned at the origin, walking on from there');
      });

      test('the restart action starts the level over', () {
        final game = playing(mode);
        (Route()..hold('W', 1)).recording.replay(game.world);
        expect(game.state.collected.value, 1);
        (Route()
              ..tap('R')
              ..wait(0.1))
            .recording
            .replay(game.world);
        expect(game.state.collected.value, 0);
        expect(game.state.score.value, 0);
        expect(game.state.total.value, 5);
        expect(game.world.findObjectsOfType<Collectible>(), hasLength(5));
      });
    });
  }

  test('both ways make the same level', () {
    List<String> shape(BuildMode mode) {
      final game = playing(mode);
      final out = <String>[];
      void walk(GameObject o, String at) {
        final path = '$at/${o.name}';
        final types = [for (final c in o.components) c.runtimeType.toString()]..sort();
        out.add('$path $types ${[for (final b in o.behaviours) b.runtimeType]} ${o.localTransform.position} ${o.layer} ${o.tags}');
        for (final c in o.children) {
          walk(c, path);
        }
      }

      for (final name in ['Sun', 'Environment', 'Camera', 'Player', 'Floor', 'Gate', 'Switch', 'Sweeper', 'Coin 1', 'Coin 5']) {
        walk(game.world.find(name)!, '');
      }
      return out;
    }

    expect(shape(BuildMode.scenes), shape(BuildMode.live));
  });
}
