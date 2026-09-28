// Scenes, runtime prefabs and settings from Dart (M3-12, blueprint §19.2d
// item 12; docs/scenes.md): the gate — a headless world loads the
// fixture's Playground, the Player walks into a runtime-spawned coin
// (`fynkel.TriggerZone` → `ZoneEntered("collect")`), a behaviour asks
// the `SceneManager` for Results, and the user's `settings.json` is
// saved and read back with its rebinds laid over the project's actions.

import 'dart:io';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

const playground = 'c0000000-0000-4000-8000-000000000031';
const results = 'c0000000-0000-4000-8000-000000000033';
const coinPrefab = 'c0000000-0000-4000-8000-000000000042';

/// The player's rules: collect what it walks into, and once the coin is
/// collected, ask for the Results scene at the next frame.
class Collector extends Behaviour {
  final List<ZoneEvent> seen = [];
  int collected = 0;
  bool asked = false;

  @override
  void onZoneEnter(ZoneEvent event) {
    seen.add(event);
    if (event.event != 'collect') return;
    collected++;
    // The zone's entity is the coin's root: destroy the whole spawn.
    world.byEntity(event.zone)?.destroy();
    expect(Time.tick, greaterThan(0), reason: 'the world context is current');
  }

  @override
  void fixedUpdate(FixedUpdateContext context) {
    if (collected > 0 && !asked) {
      asked = true;
      world.scenes.transition(results);
      expect(world.scenes.state, SceneState.pending);
    }
  }
}

/// Keeps its object across loads and asks for the next level in the same
/// callback (ADR-0037): the mark must hold before the transition runs at
/// the start of the next frame, ahead of any tick.
class Keeper extends Behaviour {
  String? next;
  int destroyed = 0;

  @override
  void fixedUpdate(FixedUpdateContext context) {
    final to = next;
    if (to == null) return;
    next = null;
    world.dontDestroyOnLoad(this); // a component: its object
    world.scenes.transition(to);
  }

  @override
  void onDestroy() => destroyed++;
}

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late String cache;
  late NativeLibrary lib;

  setUpAll(() {
    cache = Directory.systemTemp.createTempSync('fynkel-scenes-cache-').path;
    cookProject(fixture, cacheDir: cache);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    Directory(cache).deleteSync(recursive: true);
    lib.dispose();
  });

  World fresh() {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    w.onLoadDiagnostics = (_) {};
    addTearDown(w.dispose);
    return w;
  }

  test('the gate: Playground, a collected coin, a transition to Results, settings saved and reloaded', () async {
    final w = fresh();
    final states = <SceneState>[];
    w.scenes.addListener(states.add);
    expect(w.scenes.state, SceneState.empty);

    final report = w.loadScene(playground, cacheDir: cache);
    expect(report.mode, 'replace');
    expect(report.scenes.map((s) => s.name), ['Playground']);
    expect(w.loadedScenes.single.entityCount, 5);
    expect(w.scenes.current, playground);
    expect(w.scenes.state, SceneState.ready);
    expect(w.scenes.isLoaded(playground), isTrue);
    expect(w.cacheDir, cache);
    final ready = w.drainEvents().whereType<SceneReady>().single;
    expect(ready.scene, playground, reason: 'SceneReady names its scene (M3-12)');

    // A coin from the cooked prefab, in the Player's path (W walks −z at
    // 4 m/s: the coin at z −2 — its 0.5 m sensor meets the 0.4 m capsule
    // after ~1.1 m — is reached at ~0.3 s, before the jump at 20 frames).
    final coin = w.instantiate(coinPrefab, transform: identityTransform.copyWith(position: const Vec3(0, 0, -2)));
    final zone = coin; // the coin's root holds the zone (ADR-0022)
    expect(zone.get<TriggerZone>(), const TriggerZone(event: 'collect', once: true));
    expect(zone.get<Collider>()!.isSensor, isTrue);
    expect(w.allEntities(), hasLength(6));

    final player = w.find('Player')!;
    final playerEntity = player.entity;
    final zoneEntity = zone.entity;
    final collector = Collector();
    player.addBehaviour(collector);
    final hub = <ZoneEntered>[];
    w.zones.on('collect', hub.add);
    final exits = <ZoneExited>[];
    w.zones.onExit('collect', exits.add);
    final carried = <EngineEvent>[];

    final recording = loadInputRecording('$fixture/test/platform.input.json');
    var framesToResults = -1;
    var frame = 0;
    recording.replay(
      w,
      fixedUpdate: (_) {
        frame++;
        if (framesToResults < 0 && w.scenes.current == results) framesToResults = frame;
      },
      update: () {
        carried.addAll(w.drainEvents());
      },
    );

    // The zone fired once, to the behaviour on the Player, to the hub,
    // with the zone's name; `once` means no exit even though the coin
    // was destroyed right away.
    expect(collector.seen, hasLength(1));
    final e = collector.seen.single;
    expect(e, isA<ZoneEntered>());
    expect(e.event, 'collect');
    expect(e.other, playerEntity);
    expect(e.zone, zoneEntity);
    expect(hub.single, same(e));
    expect(exits, isEmpty);
    expect(collector.collected, 1);
    expect(coin.isAlive, isFalse, reason: 'destroyed by the callback');

    // The transition ran at the start of the frame after the request:
    // Playground went (its 5 entities), Results is in, the Player's
    // handle is dead and its behaviour destroyed.
    expect(framesToResults, inInclusiveRange(10, 30), reason: 'collected at ~0.3 s, the load one frame later');
    expect(w.scenes.current, results);
    expect(w.scenes.state, SceneState.ready);
    expect(w.scenes.lastReport!.sceneName, 'Results');
    expect(w.loadedScenes.map((s) => s.name), ['Results']);
    expect(states, [SceneState.ready, SceneState.pending, SceneState.ready]);
    expect(player.isAlive, isFalse);
    expect(collector.isDestroyed, isTrue);
    expect(w.behaviours, isEmpty);
    expect(w.find('Player'), isNull);
    expect(w.find('Trophy'), isNotNull);
    expect(w.allEntities(), hasLength(3));
    expect(
      carried.whereType<SceneUnloaded>(),
      isEmpty,
      reason: 'the frame drained the transition\'s events into the next fixedUpdate (pull-only, ADR-0008)',
    );

    // Settings: the defaults on a first open (no file), a change is on
    // disk at once and its rebinds sit over the project's actions; a
    // fresh store reads the same document back.
    final dir = Directory.systemTemp.createTempSync('fynkel-settings-');
    addTearDown(() => dir.deleteSync(recursive: true));
    final path = '${dir.path}/settings.json';
    final store = SettingsStore.open(path, world: w);
    w.useSettings(store);
    expect(store.current, SettingsDocument.defaults);
    expect(File(path).existsSync(), isFalse);
    expect(w.context.settings, same(store));
    final changed = <SettingsDocument>[];
    store.addListener(changed.add);
    store.edit((s) => s.copyWith(musicVolume: 0.5, muted: true, quality: 'high').rebind('jump', ['keyboard:J']));
    expect(changed, hasLength(1));
    expect(File(path).existsSync(), isTrue);
    expect(File(path).readAsStringSync(), contains('"music_volume": 0.5'));
    expect(File('$path.tmp').existsSync(), isFalse);

    final again = SettingsStore.open(path);
    expect(again.current, store.current);
    expect(again.current.rebinds, {
      'jump': ['keyboard:J'],
    });
    expect(again.current.quality, 'high');
    expect(again.lastError, isNull);

    // The world's actions: `jump` is the user's, the rest the project's,
    // and the rebinds survive the next load.
    final bound = w.loadScene(playground, mode: LoadMode.additive);
    final jump = bound.inputActions.singleWhere((a) => a.name == 'jump');
    expect(jump.rebound, isTrue);
    expect(jump.bindings, ['keyboard:J']);
    expect(bound.inputActions.singleWhere((a) => a.name == 'move').rebound, isFalse);
    // Back to the project's bindings.
    store.edit((s) => s.rebind('jump', const []));
    expect(store.current.rebinds, isEmpty);
    expect(w.bindActions(const {}).inputActions.singleWhere((a) => a.name == 'jump').bindings, hasLength(3));
  });

  test('additive loads stack, unload takes one scene and leaves runtime spawns, errors are reported', () {
    final w = fresh();
    w.loadScene(playground, cacheDir: cache);
    final r = w.loadScene(results, mode: LoadMode.additive);
    expect(r.mode, 'additive');
    expect(w.loadedScenes.map((s) => s.name), ['Playground', 'Results']);
    expect(w.allEntities(), hasLength(8));
    expect(w.scenes.current, playground, reason: 'an additive load does not change the current scene');
    expect(
      () => w.loadScene(results, mode: LoadMode.additive),
      throwsA(isA<FynkelLoadError>().having((e) => e.report.errors.map((d) => d.code), 'codes', contains('FYN-R0011'))),
    );
    expect(w.allEntities(), hasLength(8), reason: 'a refused load leaves the world alone');

    final crate = w.instantiate('c0000000-0000-4000-8000-000000000040');
    w.frame(1 / 60);
    expect(w.allEntities(), hasLength(9));

    final gone = w.unloadScene(playground);
    expect(gone.sceneName, 'Playground');
    expect(gone.entityCount, 5);
    expect(gone.scenes.map((s) => s.name), ['Results']);
    expect(w.loadedScenes.map((s) => s.name), ['Results']);
    expect(w.scenes.current, isNull);
    final events = w.drainEvents();
    expect(events.whereType<EntityDestroyed>().length, 5);
    expect(events.whereType<SceneUnloaded>().single.entities, 5);
    expect(w.allEntities(), hasLength(4), reason: 'Results and the crate stay');
    expect(crate.isAlive, isTrue);
    expect(w.find('Player'), isNull);
    expect(
      () => w.unloadScene(playground),
      throwsA(
        isA<FynkelUnloadError>().having((e) => e.report.errors.map((d) => d.code), 'codes', contains('FYN-R0010')),
      ),
    );
    expect(() => w.unloadScene('nope'), throwsA(isA<FynkelBridgeError>()));

    // Replace clears the crate too; the events say what went, then what
    // came: DESTROYED ×4 (Results' 3, the crate),
    // SCENE_UNLOADED (Results, 3), SCENE_READY (Playground).
    w.loadScene(playground);
    expect(w.allEntities(), hasLength(5));
    expect(crate.isAlive, isTrue, reason: 'until the next drain');
    final replaced = w.drainEvents();
    expect(crate.isAlive, isFalse);
    expect(replaced.whereType<EntityDestroyed>().length, 4);
    final unloaded = replaced.whereType<SceneUnloaded>().single;
    expect(unloaded.scene, results);
    expect(unloaded.entities, 3);
    final ready = replaced.whereType<SceneReady>().single;
    expect(ready.scene, playground);
    expect(replaced.indexOf(unloaded), lessThan(replaced.indexOf(ready)));
    expect(replaced.last, same(ready));
    expect(w.loadedScenes.map((s) => s.name), ['Playground']);
    expect(w.scenes.current, playground);

    // The manager's transition: a failed one is observable and does not
    // touch the world; cancel drops a pending one.
    final states = <SceneState>[];
    w.scenes.addListener(states.add);
    final future = w.scenes.transition('c0000000-0000-4000-8000-0000000000ff');
    expect(w.scenes.pending, 'c0000000-0000-4000-8000-0000000000ff');
    w.frame(1 / 60);
    expect(w.scenes.state, SceneState.failed);
    expect(w.scenes.error!.report.errors.map((d) => d.code), contains('FYN-R0003'));
    expect(future, throwsA(isA<FynkelLoadError>()));
    expect(w.allEntities(), hasLength(5));
    expect(w.scenes.pending, isNull);
    final cancelled = w.scenes.transition(results);
    w.scenes.cancel();
    expect(cancelled, throwsA(isA<StateError>()));
    expect(w.scenes.state, SceneState.ready);
    expect(states, [SceneState.pending, SceneState.failed, SceneState.pending, SceneState.ready]);
    w.frame(1 / 60);
    expect(w.scenes.current, playground, reason: 'nothing ran');

    // No cache remembered: a fresh world says so.
    final bare = fresh();
    expect(() => bare.loadScene(playground), throwsStateError);
    expect(() => bare.unloadScene(playground), throwsA(isA<FynkelUnloadError>()));
    expect(() => bare.bindActions(const {}), throwsA(isA<FynkelLoadError>()));
  });

  test('dontDestroyOnLoad keeps roots with their children and scripts across a replace load (ADR-0037)', () {
    const crateId = 'c0000000-0000-4000-8000-000000000040';
    final w = fresh();
    w.loadScene(playground, cacheDir: cache);
    final music = w.spawn(name: 'Music');
    w.dontDestroyOnLoad(music); // pending: marked when the tick makes it
    final track = w.spawn(name: 'Track', parent: music);
    final keeper = music.addBehaviour(Keeper());
    final crate = w.instantiate(crateId);
    final inner = w.instantiate(crateId, parent: crate);
    w.dontDestroyOnLoad(inner); // a child: nothing kept, a warning
    w.frame(1 / 60);
    final player = w.find('Player')!;
    w.dontDestroyOnLoad(player); // a scene's root
    expect(w.persistentObjects, [music, player]);

    keeper.next = results;
    w.frame(1 / 60); // marks again and asks for Results
    w.frame(1 / 60); // Results replaces Playground
    expect(w.scenes.current, results);
    expect(w.scenes.lastReport!.persistent, 2);
    expect(music.isAlive && track.isAlive && player.isAlive, isTrue);
    expect(track.parent, same(music));
    expect(crate.isAlive || inner.isAlive, isFalse, reason: 'not kept: gone with the rest');
    expect(keeper.destroyed, 0);
    expect(music.behaviour<Keeper>(), same(keeper));
    expect(w.find('Music'), same(music));
    expect(w.loadedScenes.map((s) => s.name), ['Results'], reason: 'the kept objects are in no loaded scene');
    expect(w.persistentObjects, [music, player]);

    // Destroyed, it is no longer kept; the next load takes nothing more.
    player.destroy();
    w.frame(1 / 60);
    expect(w.persistentObjects, [music]);
    w.loadScene(playground);
    w.frame(1 / 60);
    expect(music.isAlive, isTrue);
    expect(w.findAll('Player'), hasLength(1), reason: "Playground's own, new");
  });

  test('loadAsync loads in the background, activates at a frame start in request order (ADR-0038)', () async {
    final w = fresh();
    w.loadScene(playground, cacheDir: cache);
    final states = <SceneState>[];
    w.scenes.addListener(states.add);

    /// Frames until [load] has done its background work.
    void untilReady(SceneLoad load) {
      final started = DateTime.now();
      while (!load.isReady && !load.isDone) {
        w.frame(1 / 60);
        expect(DateTime.now().difference(started).inSeconds, lessThan(30), reason: 'the load never finished');
        sleep(const Duration(milliseconds: 2));
      }
    }

    // Additive, held: ready, but it waits until the game lets it go.
    final held = w.scenes.loadAsync(results, mode: LoadMode.additive)..allowActivation = false;
    expect((held.sceneId, held.mode, held.isDone), (results, LoadMode.additive, false));
    expect(w.scenes.state, SceneState.loading);
    untilReady(held);
    expect(held.progress, 1.0, reason: 'progress reaches 1 when the loading is done');
    w.frame(1 / 60);
    w.frame(1 / 60);
    expect(held.isDone, isFalse);
    expect(w.loadedScenes.map((s) => s.name), ['Playground'], reason: 'the old level plays on');
    expect(w.scenes.state, SceneState.loading);

    // A held load holds only itself: a later one activates past it.
    final unknown = w.scenes.loadAsync('c0000000-0000-4000-8000-0000000000ff', mode: LoadMode.additive);
    untilReady(unknown);
    w.frame(1 / 60);
    expect(unknown.isDone, isTrue);
    await expectLater(unknown.done, throwsA(isA<FynkelLoadError>()));
    expect(w.scenes.error!.report.diagnostics.first.code, 'FYN-R0003');
    expect(w.scenes.state, SceneState.loading, reason: 'the held load is still unfinished');

    held.allowActivation = true;
    w.frame(1 / 60);
    expect(held.isDone, isTrue);
    final report = await held.done;
    expect(report.mode, 'additive');
    expect(w.loadedScenes.map((s) => s.name), ['Playground', 'Results']);
    expect(w.scenes.state, SceneState.ready);
    expect(w.find('Trophy'), isNotNull);

    // Cancelled: never activates, its future says so.
    final dropped = w.scenes.loadAsync(playground, mode: LoadMode.additive);
    dropped.cancel();
    expect(dropped.isDone, isTrue);
    await expectLater(dropped.done, throwsStateError);
    w.frame(1 / 60);
    expect(w.loadedScenes, hasLength(2));

    // A replace load: the world is taken down at activation, not before.
    final next = w.scenes.loadAsync(playground);
    untilReady(next);
    expect(w.loadedScenes, hasLength(2));
    w.frame(1 / 60);
    expect((await next.done).sceneName, 'Playground');
    expect(w.loadedScenes.map((s) => s.name), ['Playground']);
    expect(w.scenes.current, playground);
    expect(w.find('Player'), isNotNull);
    expect(states.first, SceneState.loading);
    expect(states.last, SceneState.ready);
  });

  test('the settings document reads leniently, validates, and a broken file falls back to the defaults', () {
    final doc = SettingsDocument.fromJson({'fynkel': 'settings', 'formatVersion': 1, 'sfx_volume': 0.25});
    expect(doc.sfxVolume, 0.25);
    expect(doc.masterVolume, 1);
    expect(doc.rebinds, isEmpty);
    expect(doc.record, const Settings(masterVolume: 1, musicVolume: 1, sfxVolume: 0.25, muted: false, quality: ''));
    expect(SettingsDocument.fromJson(doc.toJson()), doc);
    expect(doc.toJson().keys, [
      'fynkel',
      'formatVersion',
      'master_volume',
      'music_volume',
      'sfx_volume',
      'muted',
      'quality',
      'input',
    ]);
    expect(() => SettingsDocument.fromJson({'fynkel': 'scene', 'formatVersion': 1}), throwsFormatException);
    expect(() => SettingsDocument.fromJson({'fynkel': 'settings', 'formatVersion': 2}), throwsFormatException);
    expect(
      () => SettingsDocument.fromJson({'fynkel': 'settings', 'formatVersion': 1, 'muted': 'yes'}),
      throwsFormatException,
    );
    expect(
      () => SettingsDocument.fromJson({
        'fynkel': 'settings',
        'formatVersion': 1,
        'input': {
          'rebinds': {'jump': 'keyboard:J'},
        },
      }),
      throwsFormatException,
    );
    expect(doc.copyWith(masterVolume: 2).validate(), ['master_volume must be within 0..1, got 2.0']);
    expect(doc.rebind('jump', ['keyboard:J!']).validate(), [
      'input.rebinds.jump: "keyboard:J!" is not <device>:<control>',
    ]);
    expect(doc.rebind('jump', ['keyboard:J', 'gamepad:south']).validate(), isEmpty);

    final dir = Directory.systemTemp.createTempSync('fynkel-settings-');
    addTearDown(() => dir.deleteSync(recursive: true));
    final path = '${dir.path}/nested/settings.json';
    final memory = SettingsStore();
    expect(memory.isPersistent, isFalse);
    expect(() => memory.update(doc.copyWith(musicVolume: -1)), throwsArgumentError);
    memory.update(doc);
    expect(memory.current, doc);
    expect(memory.save(), isTrue);

    File(path).createSync(recursive: true);
    File(path).writeAsStringSync('{"fynkel":"settings","formatVersion":1,"master_volume":7}');
    final errors = <String>[];
    final broken = SettingsStore.open(path, onError: errors.add);
    expect(broken.current, SettingsDocument.defaults);
    expect(errors.single, contains('master_volume'));
    expect(broken.lastError, errors.single);
    broken.update(doc);
    expect(broken.lastError, isNull);
    expect(SettingsStore.open(path).current, doc);
    File(path).writeAsStringSync('not json');
    expect(broken.load(), isFalse);
    expect(broken.current, doc, reason: 'unchanged by a failed load');

    expect(SettingsStore.defaultPath('My Game', environment: {'HOME': '/Users/me'}), endsWith('My Game/settings.json'));
  });
}
