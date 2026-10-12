import 'dart:io';
import 'dart:typed_data';

import 'package:coins/coins.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';

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
    final frame = view.read(view.frame());
    final shot = Platform.environment['COINS_SHOT'];
    if (shot != null) File(shot).writeAsBytesSync(_png(frame.rgba, width, height));

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
    final at = ViewPoint(width / 2, height * 0.8, width: width.toDouble(), height: height.toDouble());
    world.input.simulate
      ..resize(width.toDouble(), height.toDouble())
      ..fingerDown(1, at.x, at.y)
      ..fingerUp(1, at.x, at.y);
    world.update(0);
    for (var i = 0; i < 120; i++) {
      world.update(1 / 60);
    }
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

/// [rgba] as a PNG, unfiltered.
Uint8List _png(Uint8List rgba, int width, int height) {
  final raw = BytesBuilder();
  for (var y = 0; y < height; y++) {
    raw.addByte(0);
    raw.add(Uint8List.sublistView(rgba, y * width * 4, (y + 1) * width * 4));
  }
  Uint8List chunk(String kind, List<int> data) {
    final body = [...kind.codeUnits, ...data];
    final b = ByteData(8 + data.length + 4);
    b.setUint32(0, data.length);
    final out = b.buffer.asUint8List()..setAll(4, body);
    b.setUint32(8 + data.length, _crc(body));
    return out;
  }

  final header = ByteData(13)
    ..setUint32(0, width)
    ..setUint32(4, height)
    ..setUint8(8, 8)
    ..setUint8(9, 6);
  return Uint8List.fromList([
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, //
    ...chunk('IHDR', header.buffer.asUint8List()),
    ...chunk('IDAT', zlib.encode(raw.takeBytes())),
    ...chunk('IEND', const []),
  ]);
}

int _crc(List<int> bytes) {
  var c = 0xFFFFFFFF;
  for (final byte in bytes) {
    c ^= byte;
    for (var k = 0; k < 8; k++) {
      c = c & 1 != 0 ? 0xEDB88320 ^ (c >> 1) : c >> 1;
    }
  }
  return c ^ 0xFFFFFFFF;
}
