# ADR-0040: The cook runs in the background, with parallel imports

**Status:** Accepted 2026-09-29 (the user agreed, with both recommendations below; docs/foundation.md 5.3)  
**Date:** 2026-09-29  
**Related:** ADR-0021 (Play in the editor), ADR-0038 (LoadSceneAsync, the same start/poll/finish shape), [documents.md](../documents.md) § The cook, [editor.md](../editor.md)

## Context

The editor cooks the project in process, through the authoring FFI, on
the UI thread. It does so in four places:

1. the first time a Scene view opens a preview (`ensure_assets`);
2. every time an asset changes on disk (`_onProject` → `_recook`), for
   example when the user drops an FBX into the project;
3. ⌃⇧⌘R (Rebuild Cache);
4. Play (`_cookForPlay`, the unsaved documents cooked as the editor
   holds them).

The call is synchronous and the cook imports one source after another.
A big FBX takes seconds, and the editor freezes for all of that time:
no repaint, no cursor, no way to cancel.

Unity imports on the main thread too, but it shows a progress bar
("Importing Assets", with the file name and a count). The bar keeps
repainting, some steps can be cancelled, and its Parallel Import setting
imports textures and models in worker processes.

In our cook, the sources (models, images, sounds) depend on nothing, so
they can be imported at the same time. The documents (scenes, prefabs,
materials…) depend on the sources and on each other; they are fast and
keep their order.

## Decision

### 1. Parallel import (`fynkel_import`)

The sources are imported on worker threads (`std::thread::scope`; no new
crate). The number of workers is `CookOptions::jobs`. The default is the
machine's cores less one, at most 8, because each worker can hold a
large model in memory. `fynkel cook --jobs N` sets it, and `--jobs 1`
is today's cook.

The result is the same as a one-worker cook, byte for byte:

- each source's result goes into its own slot;
- the prefab library and the diagnostics are then built in the same
  order as today.

The documents cook after the sources, on the calling thread, in
dependency order, as today.

A worker that panics fails only its source (5.7's guard).

### 2. Progress and cancel (`fynkel_import`)

`CookOptions` gains a shared `CookProgress`, which can be read from any
thread. It holds:

- `total`: the number of assets;
- `done`: how many have finished;
- `current`: the path of the last asset started;
- `stop`: set it to cancel.

A cancelled cook stops at the next asset, and the running imports
finish first. It does not write the index, so the cache stays as the
last full cook left it. Objects already written stay: they are content
addressed, and the next cook reuses them. The report says `cancelled`.

### 3. A cook job (`fynkel_authoring`, `fynkel_native`)

`OpenProject::start_cook(live)` takes a snapshot on the calling thread:
the asset database, the schemas, the project settings and, for Play,
the unsaved texts. It then cooks on a thread, so the session stays
usable while it runs.

The authoring FFI gets four calls, in the same shape as ADR-0038:

- `cookStart {live}`: returns a ticket.
- `cookProgress`: returns `state` (running, done, failed or cancelled),
  `done`, `total` and `current`. It is cheap, so the editor polls it
  once per frame.
- `cookFinish`: applies the finished cook on the calling thread. It
  reopens the store, reloads every preview over it (what `recook` does
  now), and returns the same report as `recook`.
- `cookCancel`.

One cook runs at a time. A start while one runs does not start a second
cook. The editor waits for the running cook, then cooks once more if
something changed since (§4).

The synchronous `recook` stays, as start-then-wait. The Dart tests and
anything without a frame loop keep using it.

### 4. The editor

The editor does what Unity does. A cook that takes longer than 0.3 s
shows a small progress window over the editor with:

- the title, "Importing Assets";
- the current file;
- "3 of 12" and a bar;
- a Cancel button.

The editor keeps repainting, but it takes no edits until the cook ends.
The Scene view draws the old cache until the new one is applied.

- **An asset changed on disk:** the cook starts on its own. Changes that
  arrive while it runs make one more cook when it ends.
- **Play:** it waits for the cook. Cancel stops the cook, and Play does
  not start.
- **⌃⇧⌘R:** it waits for the cook too. A cancelled rebuild leaves the
  last cache in place and says so.
- **The first preview:** it opens empty, then draws when the cook is
  applied.

### 5. What does not change

- The cache format, the keys and the index.
- The CLI's output, except for the new `--jobs` flag.

## Consequences

- A big FBX no longer freezes the editor; the user sees what is being
  imported and can cancel.
- A project with many sources cooks faster, about as many times faster
  as there are workers, while the documents stay serial. A full cook of
  the reference game is to be measured before and after.
- Peak memory during a cook grows with the number of workers.
- The authoring session's cook moves to a thread. The snapshot must not
  be shared with the session, so nothing needs a lock beyond the
  existing `Assets` store.

## Options not taken

- **Keep editing during a cook** (no window, only a status-bar bar).
  The picture and the Inspector would show a cache that no longer
  matches the files. A rename or a delete made during the cook would
  race the snapshot. Unity blocks edits for the same reason.
- **Worker processes** (Unity's Parallel Import). Threads are enough,
  since each import is already guarded against panics (5.7). Processes
  would also protect against an abort or a stack overflow in a
  third-party reader. They could come later behind the same `jobs`
  setting.
- **Import a source only when something first uses it.** That would
  change what "cooked" means for the CLI and for builds.

## Questions for the user (answered)

1. **Block edits during a cook, like Unity (§4)?** Recommended: yes,
   with a progress window that repaints and has Cancel. **Answer:
   yes.**
2. **The default number of workers (§1):** cores − 1, at most 8.
   Recommended: yes. **Answer: yes.**

## Plan

1. `fynkel_import`: `CookProgress` (total/done/current/stop) and cancel.
   Test: a cancelled cook leaves the index as it was.
2. `fynkel_import`: parallel source import, `jobs`, `fynkel cook
   --jobs`. Tests: the cache and report are the same with 1 and 8
   workers; timing on the reference game.
3. `fynkel_authoring` + `fynkel_native`: the cook job (start, progress,
   finish, cancel); `recook` = start + wait. Test: the session answers
   other calls while a cook runs.
4. `packages/fynkel_authoring`: `CookJob` (`progress`, `current`,
   `Future<CookReport> done`, `cancel()`).
5. Editor: the progress window, auto-recook coalescing, Play and
   ⌃⇧⌘R waiting, the first preview. Tests: widget tests with a slow
   fake cook.
6. Docs: documents.md § The cook, editor.md, cli.md; foundation.md
   5.3.

## Progress

None yet.
