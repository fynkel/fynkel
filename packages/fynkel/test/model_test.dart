// Observable gameplay models (M3-13, blueprint §10, ADR-0008): a
// `GameModel`'s `ModelValue` fields notify on change only, so a HUD bound
// to them rebuilds when the game changed what it shows and never per frame.

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

class HudModel extends GameModel {
  late final score = field(ModelValue(0));
  late final title = field(ModelValue('ready'));
  late final tags = field(ModelValue(<String>[]));
}

class LateModel extends GameModel {
  late final hp = field(ModelValue(0));
}

void main() {
  test('a field notifies when its value changes, not when it is rewritten', () {
    final m = HudModel();
    var runs = 0;
    m.score.addListener(() => runs++);
    expect(m.score.hasListeners, isTrue);
    m.score.value = 0;
    expect(runs, 0, reason: 'same value');
    m.score.value = 5;
    expect(runs, 1);
    expect(m.score.value, 5);
    m.score.value = 5;
    expect(runs, 1);
    // A mutable value edited in place: force / notify.
    m.tags.addListener(() => runs++);
    m.tags.value.add('hot');
    m.tags.value = m.tags.value;
    expect(runs, 1, reason: 'identical list, nothing seen');
    m.tags.notify();
    expect(runs, 2);
    m.tags.force(['cold']);
    expect(runs, 3);
    expect(m.tags.value, ['cold']);
  });

  test('listeners can remove themselves while notifying', () {
    final v = ModelValue(1);
    late void Function() once;
    var seen = 0;
    once = () {
      seen++;
      v.removeListener(once);
    };
    v.addListener(once);
    v.addListener(() => seen += 10);
    v.value = 2;
    v.value = 3;
    expect(seen, 21);
  });

  test('the model reaches every registered field', () {
    final m = HudModel();
    expect(m.fields, isEmpty, reason: 'late fields register on first access');
    expect(m.fields, isEmpty);
    m.score;
    expect(m.fields, [m.score]);
    m.title;
    m.tags;
    expect(m.fields, [m.score, m.title, m.tags]);
    var runs = 0;
    for (final f in m.fields) {
      f.addListener(() => runs++);
    }
    m.notifyAll();
    expect(runs, 3);
    m.dispose();
    expect(m.score.hasListeners, isFalse);
    m.score.value = 9;
    expect(m.score.value, 0, reason: 'a disposed field ignores writes');
    expect(runs, 3);
    // A field first touched after dispose is dead too.
    final late = LateModel()..dispose();
    late.hp.value = 3;
    expect(late.hp.value, 0);
    expect(late.hp.hasListeners, isFalse);
  });
}
