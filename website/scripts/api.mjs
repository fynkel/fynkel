// The API reference: dartdoc for each public package, into public/api/<package>/
// (not kept in git; made again by `npm run api`). It needs Flutter and
// dartdoc 9.0.8 or later (`dart pub global activate dartdoc`): Flutter's own
// `dart doc` stops on a doc-comment bug fixed there.
import { execFileSync } from 'node:child_process';
import { rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '..', '..');
const flutter = execFileSync('which', ['flutter'], { encoding: 'utf8' }).trim();
// Flutter's dart, not another one found first on the PATH.
const dart = join(dirname(execFileSync('realpath', [flutter], { encoding: 'utf8' }).trim()), 'dart');

for (const name of ['fynkel', 'fynkel_flutter', 'fynkel_physics', 'fynkel_audio']) {
  const out = join(here, '..', 'public', 'api', name);
  rmSync(out, { recursive: true, force: true });
  execFileSync(dart, ['pub', 'global', 'run', 'dartdoc', '--output', out], {
    cwd: join(root, 'packages', name),
    stdio: ['ignore', 'ignore', 'inherit'],
  });
  console.log(`api: ${name} in ${out}`);
}
