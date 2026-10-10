import 'dart:io';
import 'dart:typed_data';

import 'package:arena/arena.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';

const width = 320.0, height = 200.0;

void main() {
  late World world;
  late ArenaGame game;
  setUp(() {
    world = World();
    world.input.resize(width, height);
    game = world.add(ArenaGame(seed: 1));
  });
  tearDown(() => world.dispose());

  Player player() => game.round.player;

  /// [seconds] of frames at 60 a second.
  void run(double seconds) {
    for (var i = 0; i < (seconds * 60).round(); i++) {
      world.update(1 / 60);
    }
  }

  /// Wave 1's enemies, all in and grown.
  List<Enemy> firstWave() {
    run(1.5 + 3 * 0.5 + 0.5);
    return world.all<Enemy>().toList();
  }

  /// Puts [enemy] [distance] metres in front of the player, still.
  void ahead(Enemy enemy, double distance) {
    for (final other in world.all<Enemy>()) {
      other.position = Vec3(-8, 0.5, 8);
    }
    enemy.position = player().position + player().facing * distance;
  }

  test('a round starts with the player in the middle; wave 1 brings three enemies, one by one', () {
    run(0.1);
    expect(player().position, Vec3(0, 0.5, 0));
    expect(game.wave.value, 1);
    expect(world.all<Enemy>(), isEmpty, reason: 'a pause first');
    run(1.5);
    expect(world.all<Enemy>(), hasLength(1));
    run(1);
    expect(world.all<Enemy>(), hasLength(3));
    final shot = Platform.environment['ARENA_SHOT'];
    if (shot != null) {
      run(1);
      final view = HeadlessView(world, width: width.toInt(), height: height.toInt());
      addTearDown(view.dispose);
      File(shot).writeAsBytesSync(_png(view.read(view.frame()).rgba, width.toInt(), height.toInt()));
    }
  });

  test('W, A, S, D move the player, inside the walls; it faces the way it goes', () {
    run(0.1);
    world.input.keyDown(KeyCode.d);
    run(0.5);
    expect(player().position.x, closeTo(Player.speed * 0.5, 0.1));
    expect(player().facing, Vec3.right);
    run(5);
    expect(player().position.x, size - 0.5);
    world.input
      ..keyUp(KeyCode.d)
      ..keyDown(KeyCode.w);
    run(0.2);
    expect(player().position.z, lessThan(0), reason: 'up is away from the camera, -Z');
  });

  test('a finger dragging alone is a stick', () {
    run(0.1);
    world.input.fingerDown(1, 100, 100);
    world.input.fingerMove(1, 100, 160);
    run(0.5);
    expect(player().position.z, closeTo(Player.speed * 0.5, 0.2), reason: 'down the view is +Z');
    expect(player().position.x, closeTo(0, 1e-9));
  });

  test('space dashes ahead; a dash through an enemy kills it, scores, and leaves sparks', () {
    final enemy = firstWave().first;
    ahead(enemy, 3);
    world.input.keyDown(KeyCode.space);
    run(0.1);
    world.input.keyUp(KeyCode.space);
    expect(player().dashing, isTrue);
    run(0.2);
    expect(player().dashing, isFalse);
    expect(player().position.z, closeTo(-5, 1e-6));
    expect(enemy.health.isDead, isTrue);
    expect(game.score.value, 1);
    expect(world.all<Sparks>(), hasLength(1));
    run(0.6);
    expect(enemy.isDestroyed, isTrue);
    expect(world.all<Sparks>(), isEmpty);
  });

  test('a swipe dashes the way it went', () {
    run(0.1);
    world.input.fingerDown(1, 100, 100);
    for (var i = 1; i <= 6; i++) {
      world.input.fingerMove(1, 100 + 20.0 * i, 100);
      run(1 / 60);
    }
    world.input.fingerUp(1, 220, 100);
    run(0.4);
    expect(player().position.x, greaterThan(4.9), reason: 'a dash is 5 metres; the drag moved it a little too');
  });

  test('an enemy that reaches the player hurts it, once a second; the HUD and the camera hear', () {
    final enemy = firstWave().first;
    ahead(enemy, 0.5);
    run(1 / 60);
    expect(player().health.left, Player.hearts - 1);
    expect(game.health.value, Player.hearts - 1);
    expect(world.cameras.isShaking, isTrue);
    run(0.5);
    expect(player().health.left, Player.hearts - 1, reason: 'guarded for a second after a hit');
    run(0.6);
    expect(player().health.left, Player.hearts - 2);
  });

  test('pause stops the game\'s time: enemies stand still, and a dash waits', () {
    final enemy = firstWave().first;
    world.input.keyDown(KeyCode.escape);
    run(1 / 60);
    world.input.keyUp(KeyCode.escape);
    final at = enemy.position;
    expect(game.paused.value, isTrue);
    expect(world.time.scale, 0);
    run(1);
    expect(enemy.position, at);
    player().dash(Vec3.right);
    expect(player().dashing, isFalse);
    game.togglePause();
    expect(world.time.scale, 1);
    run(0.1);
    expect(enemy.position, isNot(at));
  });

  test('a tap dashes towards what it hits, through the sparks: they are on the effects layer', () {
    run(0.1);
    final target = Vec3(4, 0, 0);
    final at = world.camera!.worldToScreen(target, width: width, height: height)!;
    // Sparks right where the tap goes, between the camera and the floor.
    final sparks = game.round.add(Sparks(Vec3(3, 1, 0)));
    run(1 / 60);
    world.input
      ..fingerDown(9, at.x, at.y)
      ..fingerUp(9, at.x, at.y);
    run(1 / 60);
    expect(player().dashing, isTrue);
    expect(player().facing.x, closeTo(1, 0.05));
    expect(sparks.layer, effects);
  });

  test('losing every heart ends the round; again starts wave 1 with full hearts', () async {
    final enemy = firstWave().first;
    ahead(enemy, 0.5);
    run(Player.hearts + 0.5);
    expect(player().health.isDead, isTrue);
    expect(game.over.value, isTrue);
    final score = game.score.value, old = game.round;
    expect(score, 0);
    await game.restart();
    expect(old.isDestroyed, isTrue);
    expect([game.over.value, game.health.value, game.score.value], [false, Player.hearts, 0]);
    run(0.1);
    expect(world.all<Enemy>(), isEmpty);
    expect(game.wave.value, 1);
  });
}

/// An uncompressed PNG of [rgba], for a look at the arena (ARENA_SHOT).
Uint8List _png(Uint8List rgba, int width, int height) {
  final raw = BytesBuilder();
  for (var y = 0; y < height; y++) {
    raw.addByte(0);
    raw.add(rgba.sublist(y * width * 4, (y + 1) * width * 4));
  }
  Uint8List chunk(String type, List<int> data) {
    final body = [...type.codeUnits, ...data];
    final b = BytesBuilder()
      ..add(_u32(data.length))
      ..add(body)
      ..add(_u32(_crc(body)));
    return b.toBytes();
  }

  final header = [..._u32(width), ..._u32(height), 8, 6, 0, 0, 0];
  return (BytesBuilder()
        ..add([137, 80, 78, 71, 13, 10, 26, 10])
        ..add(chunk('IHDR', header))
        ..add(chunk('IDAT', ZLibCodec().encode(raw.toBytes())))
        ..add(chunk('IEND', [])))
      .toBytes();
}

List<int> _u32(int v) => [v >> 24 & 255, v >> 16 & 255, v >> 8 & 255, v & 255];

int _crc(List<int> bytes) {
  var c = 0xFFFFFFFF;
  for (final b in bytes) {
    c ^= b;
    for (var k = 0; k < 8; k++) {
      c = c & 1 != 0 ? 0xEDB88320 ^ (c >> 1) : c >> 1;
    }
  }
  return c ^ 0xFFFFFFFF;
}
