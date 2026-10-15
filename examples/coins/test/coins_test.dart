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
}
