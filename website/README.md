# fynkel.com

The website: [Starlight](https://starlight.astro.build), a static site.

```sh
npm ci
npm run api     # the API reference, from dartdoc, into public/api (not in git)
npm run dev     # at localhost:4321
npm run build   # into dist/
```

The documentation is `src/content/docs/`, a page a file, in the order of
`astro.config.mjs`'s sidebar. Its Dart and WGSL compile and run in CI
(`tools/check_guide.py`, and `fynkel_render`'s reference test). The
tutorials are made from `tutorials/` and the examples' own code before
each build (`scripts/tutorials.mjs`). The pictures in `public/shots/` are
drawn from the lab's scenes by `examples/lab/test/shots_test.dart`.
