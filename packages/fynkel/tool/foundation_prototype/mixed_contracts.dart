import 'dart:convert';
import 'dart:math' as math;

import 'mixed_world.dart';
import 'slice.dart';

void check(bool value, String message) {
  if (!value) throw StateError(message);
}

void rejects(void Function() run, String message) {
  try {
    run();
  } catch (_) {
    return;
  }
  throw StateError(message);
}

Map<String, Object?> trs({
  List<double> position = const [0, 0, 0],
  List<double> rotation = const [0, 0, 0, 1],
  List<double> scale = const [1, 1, 1],
}) => {'position': position, 'rotation': rotation, 'scale': scale};

Map<String, Object?> contracts(bool adapter) {
  final world = MixedWorld(adapter: adapter);
  try {
    late MixedRef root, child, leaf, player, door;
    world.transaction(
      writes: mixedFields,
      run: (access) {
        root = access.spawn('root');
        child = access.spawn('child', parent: root);
        player = access.spawn('player');
        door = access.spawn('door');
        leaf = access.spawn('leaf', parent: child);
        access.object(root).localTransform = trs(scale: [2, 1, 1]);
        access.object(child).localTransform = trs(
          rotation: [0, 0, math.sin(math.pi / 8), math.cos(math.pi / 8)],
        );
        access.object(leaf).localTransform = trs(position: [1, 0, 0]);
        access.object(player).health = const Health(poisonPerSecond: 2);
        access.object(leaf).health = const Health(hp: 20);
        access.object(door).door = const DoorState();
        check(
          identical(access.find('leaf'), leaf),
          'Pending spawn/find disagree',
        );
        access.object(root).name = 'renamed';
        check(
          access.find('root') == null &&
              identical(access.find('renamed'), root),
          'Rename/find disagree',
        );
        final matrix = access.object(leaf).transform['worldMatrix'] as List;
        check(
          ((matrix[13] as num) - math.sqrt(.5)).abs() < .00001,
          'Staged ancestor matrix composition wrong',
        );
      },
    );
    // Poison system and local behaviour commit sequentially, mixing built-in/custom edits.
    world.transaction(
      writes: ['Health', 'Transform'],
      run: (access) {
        access.forEach('Health', (object) {
          final health = object.health!;
          object.health = Health(
            hp: health.hp - health.poisonPerSecond / 60,
            poisonPerSecond: health.poisonPerSecond,
          );
        });
        access.object(player).localTransform = trs(position: [3, 0, 0]);
      },
    );
    world.transaction(
      writes: ['DoorState', 'Transform'],
      run: (access) {
        access.object(door).door = DoorState(
          open: access.object(player).health!.hp < 100,
        );
        access.object(door).localTransform = trs(position: [0, 1, 0]);
        check(access.object(door).door!.open, 'Behaviour missed system commit');
      },
    );
    rejects(
      () => world.transaction(
        writes: mixedFields,
        run: (access) {
          access.object(root).name = 'must-rollback';
          access.object(player).health = const Health(hp: 1);
          access.object(player).localTransform = trs(position: [99, 0, 0]);
          access.reparent(root, leaf);
        },
      ),
      'Cycle accepted',
    );
    world.transaction(
      run: (access) {
        check(access.find('must-rollback') == null, 'Name partially committed');
        check(
          access.object(player).health!.hp > 99,
          'Health partially committed',
        );
        check(
          ((access.object(player).transform['worldMatrix'] as List)[12]
                  as num) ==
              3,
          'Transform partially committed',
        );
      },
    );
    // Even a caught validation rejection marks the whole callback failed.
    rejects(
      () => world.transaction(
        writes: ['Health', 'Name'],
        run: (access) {
          access.object(root).name = 'invalid-inspector';
          try {
            access.object(player).editHealth({'unknown': 7});
          } catch (_) {}
        },
      ),
      'Caught rejection did not fail transaction',
    );
    world.transaction(
      writes: ['Activity', 'Hierarchy'],
      run: (access) {
        access.object(root).activeSelf = false;
        check(
          access.object(leaf).activity['self'] == true &&
              access.object(leaf).activity['hierarchy'] == false,
          'Descendant activity stale',
        );
        access.reparent(leaf, player);
        check(
          access.object(leaf).activity['hierarchy'] == true,
          'Reparent activity stale',
        );
      },
    );
    final published = world.publish();
    final rows = published['rows'] as List;
    final render = published['render'] as List;
    for (final row in rows.cast<Map<String, Object?>>()) {
      if ((row['activity'] as Map)['hierarchy'] != true) continue;
      final rendered = render.cast<Map<String, Object?>>().singleWhere(
        (entry) => jsonEncode(entry['ref']) == jsonEncode(row['ref']),
      );
      final matrix = (row['transform'] as Map)['worldMatrix'] as List;
      final drawn = rendered['worldMatrix'] as List;
      check(
        List.generate(
          16,
          (i) => ((matrix[i] as num) - (drawn[i] as num)).abs(),
        ).every((v) => v < .00001),
        'Gameplay/render extraction disagree',
      );
    }
    late MixedRef born;
    world.transaction(
      writes: mixedFields,
      run: (access) {
        var visits = 0;
        access.forEach('Health', (object) {
          visits++;
          born = access.spawn('new-projectile');
          access.object(born).health = const Health(hp: 5);
          access.destroy(
            object.ref,
          ); // Player's subtree includes leaf; remaining cursor skips it.
        });
        check(
          visits == 1,
          'Query cursor visited destroyed subtree or newly spawned entity',
        );
        var next = 0;
        access.forEach('Health', (_) => next++);
        check(
          next == 1 && access.find('player') == null,
          'Fresh query/find disagree with structural edits',
        );
      },
    );
    rejects(
      () => world.transaction(run: (access) => access.object(player).health),
      'Destroyed reference accepted',
    );
    late MixedRef aborted;
    rejects(
      () => world.transaction(
        writes: mixedFields,
        run: (access) {
          aborted = access.spawn('aborted');
          access.object(born).health = const Health(hp: -1);
        },
      ),
      'Invalid custom edit accepted',
    );
    rejects(
      () => world.transaction(run: (access) => access.object(aborted)),
      'Aborted temporary reference accepted',
    );
    final other = MixedWorld();
    try {
      rejects(
        () => other.transaction(
          writes: ['Name'],
          run: (access) => access.object(root).name = 'foreign',
        ),
        'Foreign reference accepted',
      );
    } finally {
      other.dispose();
    }
    late MixedObject expired;
    world.transaction(run: (access) => expired = access.object(born));
    rejects(() => expired.health, 'Expired object accessor accepted');
    rejects(
      () => world.transaction(
        writes: ['Name'],
        run: (access) => access.object(born).health = const Health(),
      ),
      'Undeclared write accepted',
    );
    return {
      'strategy': adapter ? 'preflight-adapter' : 'shared-transaction-view',
      'mixedAtomicRollback': true,
      'spawnRenameFind': true,
      'stagedWorldMatrices': true,
      'descendantActivityAndReparent': true,
      'structuralIteration': true,
      'inspectorParity': true,
      'sequentialSystemBehaviourVisibility': true,
      'referenceAndScopeRejection': true,
      'nativeStepAndRenderExtractParity': true,
      'ffiCalls': world.calls,
      'requestBytes': world.requestBytes,
      'responseBytes': world.responseBytes,
    };
  } finally {
    world.dispose();
  }
}

void main() => print(
  const JsonEncoder.withIndent('  ').convert({
    'mixedContracts': [contracts(false), contracts(true)],
  }),
);
