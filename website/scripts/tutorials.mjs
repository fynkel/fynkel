import { mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '..', '..');

// The tutorials, in website/tutorials/: each `<!-- include: path -->`
// becomes that file of the repository, as a code block titled with its
// path, so a tutorial shows the example's code as it is (CI tests it).
const tutorials = join(here, '..', 'tutorials');
const tutorialsOut = join(here, '..', 'src', 'content', 'docs', 'tutorials');
rmSync(tutorialsOut, { recursive: true, force: true });
mkdirSync(tutorialsOut, { recursive: true });
for (const name of readdirSync(tutorials).filter((f) => f.endsWith('.md'))) {
  const source = readFileSync(join(tutorials, name), 'utf8');
  const expanded = source.replace(/^<!-- include: (\S+) -->$/gm, (_, path) => {
    const code = readFileSync(join(root, path), 'utf8').trimEnd();
    const lang = path.endsWith('.dart') ? 'dart' : path.endsWith('.yaml') ? 'yaml' : '';
    const file = path.replace(/^examples\/[^/]+\//, '');
    return '```' + lang + ` title="${file}"\n` + code + '\n```';
  });
  writeFileSync(join(tutorialsOut, name), expanded);
}
console.log(`tutorials: in ${tutorialsOut}`);

// Every name the engine gives a shader: its own file, as it's shipped.
const names = readFileSync(join(root, 'packages/fynkel/wgsl/fynkel.wgsl'), 'utf8');
const reference = join(here, '..', 'src', 'content', 'docs', 'reference');
mkdirSync(reference, { recursive: true });
writeFileSync(
  join(reference, 'shader-names.md'),
  `---
title: Every shader name
description: The functions and fields the engine gives a shader.
editUrl: false
---

What a shader can use, as the engine declares it. [A first
shader](/shaders/first/) shows how.

\`\`\`wgsl
${names.trimEnd()}
\`\`\`
`,
);
