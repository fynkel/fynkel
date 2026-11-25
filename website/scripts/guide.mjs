// Splits docs/writing-a-game.md, the guide (checked by tools/check_guide.py),
// into one page per section under src/content/docs/guide/, so the site
// shows the guide as it is and never a copy that drifts. Run before every
// build and dev server (package.json's prebuild/predev).

import { mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '..', '..');
const repo = 'https://github.com/fynkel/fynkel/blob/main';
const out = join(here, '..', 'src', 'content', 'docs', 'guide');

const text = readFileSync(join(root, 'docs', 'writing-a-game.md'), 'utf8');
const lines = text.split('\n');

// The intro (before the first "## "), then a section for each "## N. Title".
const sections = [];
let current = { number: null, title: 'Writing a game', lines: [] };
let inCode = false;
for (const line of lines) {
  if (line.startsWith('```')) inCode = !inCode;
  const heading = !inCode && line.match(/^## (\d+[a-z]?)\. (.+)$/);
  if (heading) {
    sections.push(current);
    current = { number: heading[1], title: heading[2], lines: [] };
  } else if (!(current.number === null && line.startsWith('# '))) {
    current.lines.push(line);
  }
}
sections.push(current);

const slug = (s) =>
  s.number === null
    ? 'index'
    : `${s.number.padStart(s.number.match(/[a-z]$/) ? 3 : 2, '0')}-${s.title
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, '-')
        .replace(/^-|-$/g, '')}`;
const byNumber = new Map(sections.filter((s) => s.number).map((s) => [s.number, slug(s)]));

// "§12" and "§5b" become links to their pages; links to files beside the
// guide go to GitHub.
function rewrite(body) {
  let inFence = false;
  return body
    .split('\n')
    .map((line) => {
      if (line.trimStart().startsWith('```')) inFence = !inFence;
      if (inFence) return line;
      return line
        .replace(/\]\(((?!https?:|#)[^)]+\.md)\)/g, (_, file) => `](${repo}/docs/${file})`)
        .replace(/(?<![\[\w])§(\d+[a-z]?)(?![\w\]])/g, (whole, n) =>
          byNumber.has(n) ? `[§${n}](/guide/${byNumber.get(n)}/)` : whole,
        );
    })
    .join('\n');
}

rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
sections.forEach((s, i) => {
  const title = s.number === null ? s.title : `${s.number}. ${s.title}`;
  const frontmatter = [
    '---',
    `title: ${JSON.stringify(title)}`,
    `sidebar:`,
    `  order: ${i}`,
    s.number === null ? '  label: "Introduction"' : null,
    `editUrl: ${repo.replace('/blob/', '/edit/')}/docs/writing-a-game.md`,
    '---',
    '',
    '<!-- Made by website/scripts/guide.mjs from docs/writing-a-game.md: edit that. -->',
    '',
  ]
    .filter((l) => l !== null)
    .join('\n');
  writeFileSync(join(out, `${slug(s)}.md`), frontmatter + rewrite(s.lines.join('\n').trim()) + '\n');
});
console.log(`guide: ${sections.length} pages in ${out}`);

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
