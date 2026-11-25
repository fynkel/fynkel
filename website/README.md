# fynkel.com

The website: [Starlight](https://starlight.astro.build), a static site.

```sh
npm ci
npm run api     # the API reference, from dartdoc, into public/api (not in git)
npm run dev     # at localhost:4321
npm run build   # into dist/
```

The guide's pages are made from `docs/writing-a-game.md` before each build
(`scripts/guide.mjs`): edit that file. `public/shots/` are drawn by the
examples.
