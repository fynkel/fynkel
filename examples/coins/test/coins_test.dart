import 'dart:io';

import 'package:coins/coins.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';

const width = 320, height = 200;

void main() {
  late World world;
  late CoinsGame game;
  setUp(() {
    world = World(source: (asset) async => File('assets/$asset').readAsBytesSync());
    game = world.add(CoinsGame());
  });
  tearDown(() => world.dispose());

  /// Puts the player on each coin in turn, as walking there would. One
  /// may be picked up with another, close by.
  void pickUp(Iterable<Coin> coins) {
    for (final coin in coins.where((coin) => !coin.isDestroyed)) {
      final at = coin.transform.position;
      game.player.transform = Transform(position: Vec3(at.x, 0, at.z));
      // A fixed step, for the coin's trigger to meet the player, and the
      // frame after, which hears of it.
      world.update(1 / 50);
      world.update(1 / 50);
    }
  }

  /// The model the player carries.
  String carried() => world.all<Prop>().singleWhere((prop) => prop.isPartOf(game.player)).asset;

  /// What the engine holds for drawing: [ViewStats.textureBytes] and
  /// [ViewStats.meshBytes] after a frame.
  (int, int) held(HeadlessView view) {
    view.read(view.frame());
    return (view.stats.textureBytes, view.stats.meshBytes);
  }

  test('the meadow loads with eight coins; a coin the player reaches is picked up and scored', () async {
    await game.loaded;
    world.update(0);
    expect(world.first<Level>()!.props, contains('tree_oak'));
    expect(world.all<Coin>(), hasLength(8));
    expect(carried(), 'flower_yellowA');

    final view = HeadlessView(world, width: width, height: height);
    addTearDown(view.dispose);
    final shot = Platform.environment['COINS_SHOT'];
    if (shot != null) File(shot).writeAsBytesSync(view.png());

    final coin = world.all<Coin>().first;
    pickUp([coin]);
    expect(coin.isDestroyed, isTrue);
    expect(world.all<Coin>(), hasLength(7));
    expect(game.player.score.value, 1);
  });

  test('the last coin goes to the next level; the player, its score and the camera stay', () async {
    await game.loaded;
    world.update(0);
    final player = game.player, camera = world.first<FollowCamera>();
    final meadow = world.first<Level>()!;
    pickUp(world.all<Coin>());
    final dusk = world.all<Level>().last;
    expect(meadow.isDestroyed, isFalse, reason: 'on screen until the next level is');
    await dusk.loaded;
    expect(meadow.isDestroyed, isTrue);
    world.update(0);

    expect(world.first<Level>()!.props, contains('rock_largeA'));
    expect(world.all<Coin>(), hasLength(8));
    expect(world.first<Player>(), same(player));
    expect(world.first<FollowCamera>(), same(camera));
    expect(player.score.value, 8);
    expect(carried(), 'mushroom_red');
    expect(player.transform.position, Vec3.zero, reason: 'each level starts in the middle');

    pickUp(world.all<Coin>());
    await world.all<Level>().last.loaded;
    world.update(0);
    expect(world.all<Level>().single.props, contains('tree_oak'), reason: 'and then the meadow again');
    expect(player.score.value, 16);
  });

  test('changing the level back and forth frees what the last one loaded', () async {
    await game.loaded;
    final view = HeadlessView(world, width: width, height: height);
    addTearDown(view.dispose);
    final meadow = held(view);
    await game.nextLevel();
    final dusk = held(view);
    expect(dusk, isNot(meadow));
    for (var i = 0; i < 3; i++) {
      await game.nextLevel();
      expect(held(view), meadow);
      await game.nextLevel();
      expect(held(view), dusk);
    }
  });

  test('a tap on the level walks the player there', () async {
    await game.loaded;
    world.update(0);
    // Below the middle of the view: the ground in front of the player.
    world.input.simulate.resize(width.toDouble(), height.toDouble());
    world.tap(width / 2, height * 0.8);
    world.update(0);
    world.run(2);
    final position = game.player.transform.position;
    expect(position.z, greaterThan(1), reason: 'towards the camera');
    expect(position.x.abs(), lessThan(0.5));
  });

  test('the player sprints far, walks near, stands still, and bends to a coin while its legs walk', () async {
    await game.loaded;
    world.update(0);
    final player = game.player, moves = player.moves;
    world.run(0.5);
    expect(moves.weight('idle'), closeTo(1, 1e-4), reason: 'standing at the start');
    expect(moves.layer(1).current, 'holding-right', reason: 'the arm holds the flower');

    // Far away: up to a sprint.
    player.walkTo(Vec3(0, 0, -12));
    world.run(1.5);
    expect(player.speed, closeTo(Player.sprint, 0.1));
    expect(moves.weight('sprint'), closeTo(1, 0.05));
    // Near: down to a walk, then standing.
    while ((player.transform.position - Vec3(0, 0, -12)).length > 1.5) {
      world.update(1 / 60);
    }
    expect(player.speed, closeTo(Player.walk, 0.3));
    expect(moves.weight('walk'), greaterThan(0.7), reason: 'walking the last metres');
    world.run(3);
    expect(player.speed, 0);
    expect(moves.weight('idle'), closeTo(1, 1e-4));

    // A coin: the bend, from the torso up, while it walks on.
    player.walkTo(Vec3(0, 0, 0));
    world.run(0.5);
    final leg = player.find('leg-left')!;
    final coin = world.all<Coin>().first;
    final at = coin.transform.position;
    player.transform = Transform(position: Vec3(at.x, 0, at.z) + Vec3(0, 0, 4), rotation: player.transform.rotation);
    world.update(1 / 50);
    final legFrom = leg.localRotation;
    player.transform = Transform(position: Vec3(at.x, 0, at.z), rotation: player.transform.rotation);
    world.update(1 / 50);
    world.update(1 / 50);
    expect(coin.isDestroyed, isTrue);
    world.run(0.1);
    expect(moves.layer(2).current, 'pick-up');
    expect(leg.localRotation, isNot(legFrom), reason: 'the legs walk on under the bend');
    // Stopped, so it walks into no other coin.
    player.walkTo(player.transform.position);
    world.run(0.6);
    expect(moves.layer(2).current, isNull, reason: 'the bend faded out when it ended');
  });

  test('coins spin and bob; the swarm moves its motes', () async {
    await game.loaded;
    world.update(0);
    final coin = world.all<Coin>().first, swarm = world.first<Swarm>()!;
    final mote = swarm.children.first;
    final (coinAt, moteAt) = (coin.transform, mote.transform.position);
    world.update(0.1);
    expect(coin.transform.rotation, isNot(coinAt.rotation));
    expect(coin.transform.position.y, isNot(coinAt.position.y));
    expect(mote.transform.position, isNot(moteAt));
    expect(swarm.children, hasLength(300));
  });

  /// Every renderer in [o] and under it.
  List<MeshRenderer> renderers(GameObject o) => [...o.getAll<MeshRenderer>(), ...o.children.expand(renderers)];

  /// With `COINS_SHOTS=<dir>`, a picture of the level as it is now, in
  /// [name].png there, to look at.
  void shot(String name) {
    final dir = Platform.environment['COINS_SHOTS'];
    if (dir == null) return;
    final view = HeadlessView(world, width: 960, height: 600);
    addTearDown(view.dispose);
    File('$dir/$name.png').writeAsBytesSync(view.png());
  }

  test('the coins are holograms: unlit, added to what is behind them', () async {
    await game.loaded;
    world.update(0);
    for (final coin in world.all<Coin>()) {
      final material = coin.get<MeshRenderer>()!.material!;
      expect(material.shader, same(game.hologram));
      expect(material.shader!.isLit, isFalse);
      expect(material.blending, Blending.additive);
    }
    world.run(0.5);
    shot('holograms');
  });

  test('the toon setting draws the whole game in bands, outlined, and back; a new level comes in the style', () async {
    await game.loaded;
    world.update(0);
    world.run(0.5);
    // What the style changes: the player, the props and the ground.
    List<MeshRenderer> styled() => [
      ...renderers(game.player),
      ...renderers(world.first<Level>()!)
          .where((r) => r.gameObject is! Coin && !r.gameObject.isPartOf(world.first<Swarm>()!)),
    ];
    final plain = {for (final r in styled()) r: r.material};
    expect(plain.length, greaterThan(10));
    expect(plain.values.every((m) => m?.shader == null), isTrue);

    game.toon.value = true;
    for (final r in styled()) {
      expect(r.material!.shader?.hasOwnLighting, isTrue, reason: '${r.gameObject.name} in bands');
      expect(r.material!.outline, isNotNull);
      expect(r.material!.color, plain[r]!.color, reason: 'its own colours');
    }
    expect(
      world.all<Coin>().first.get<MeshRenderer>()!.material!.shader,
      same(game.hologram),
      reason: 'still holograms',
    );
    expect(
      world.first<Swarm>()!.children.first.get<MeshRenderer>()!.material!.shader,
      isNull,
      reason: 'the motes glow',
    );
    final shared = {for (final r in styled()) r.material};
    expect(shared.length, lessThan(plain.length), reason: 'one toon copy for each material, shared');
    shot('toon');

    // The next level, and the flower the player carries there, load in it.
    pickUp(world.all<Coin>());
    await world.all<Level>().last.loaded;
    world.run(0.5);
    expect(styled().every((r) => r.material!.shader?.hasOwnLighting ?? false), isTrue);
    shot('toon dusk');

    game.toon.value = false;
    expect(styled().every((r) => r.material!.shader == null), isTrue, reason: 'back as it was');
  });
}
