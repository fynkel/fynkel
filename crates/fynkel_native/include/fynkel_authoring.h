/*
 * Fynkel authoring ABI — version 3 (M4-01, blueprint §19.2e item 1,
 * ADR-0016; version 2 added fynkel_authoring_operate and the assetDetail,
 * assetHistory and operations queries for item 5, the Project browser;
 * version 3 added the edit-preview world — fynkel_authoring_preview_open /
 * _close / _world / _call and the "previews" reports — for item 6, the
 * Scene view).
 *
 * The editor's entry points over the authoring session
 * (crates/fynkel_authoring): one open project — its file, lock, schemas,
 * asset database, behaviour metadata — and any number of open scene,
 * prefab or material documents, each an authoring Editor behind a small
 * integer handle. Every mutation is one atomic transaction with byte-exact
 * undo; every read is JSON.
 *
 * Exported by the same library as fynkel.h (libfynkel_native /
 * libfynkel_platform_apple) ONLY when it was built with the cargo feature
 * `authoring`. A game's runtime is built without it and has none of these
 * symbols (§1.1 rule 6); package:fynkel's build hook adds the feature when
 * the app's pubspec lists it under hooks.user_defines.fynkel.features.
 *
 * Conventions (the same as fynkel.h)
 * - Text in is UTF-8 with an explicit length, never NUL-terminated.
 * - Every call that does something writes ONE JSON result, read with
 *   fynkel_authoring_result(): `{ "ok": true, ... }` on success,
 *   `{ "ok": false, "diagnostics": [ ... ] }` when refused (the ADR-0014
 *   record: code, severity, message, document?, pointer?, id?, line?,
 *   column?, suggestion?, related?). The view is valid until the next
 *   call on the same session, or until the session is destroyed.
 * - The status says which of the two it is without parsing:
 *     FYNKEL_STATUS_OK                 done, payload in the result
 *     FYNKEL_STATUS_COMMANDS_REJECTED  an edit was refused by the editor's
 *                                      rules (FYN-E####: stale baseRevision,
 *                                      invalid command, validation error,
 *                                      read-only project, nothing to undo)
 *     FYNKEL_STATUS_LOAD_FAILED        a project or document could not be
 *                                      opened, read or written (FYN-D####,
 *                                      FYN-P####, FYN-T0006)
 *     FYNKEL_STATUS_INVALID_ARGUMENT   NULL, non-UTF-8 or malformed input,
 *                                      no project open, unknown handle or
 *                                      query (FYN-T0001..T0004)
 *     FYNKEL_STATUS_PANIC              a Rust panic was caught (FYN-T0005);
 *                                      the session is poisoned and answers
 *                                      FYNKEL_STATUS_POISONED from then on —
 *                                      destroy it and open a new one
 * - Every function is safe to call with NULL (an error status, or an empty
 *   view). Rust panics never cross this boundary (ADR-0014).
 * - Not thread-safe: one session is driven from one thread at a time.
 * - The session never runs game code (ADR-0016/0018). Its edit-preview
 *   worlds (version 3) are fynkel_core worlds in edit mode — no systems,
 *   nothing ticks — that read the project's cooked cache (.fynkel/cache,
 *   written by the session itself with `fynkel cook` in process) for
 *   meshes, materials and skins, and that the viewport ABI draws like any
 *   world.
 */
#ifndef FYNKEL_AUTHORING_H
#define FYNKEL_AUTHORING_H

#include <stdint.h>
#include "fynkel.h"

#ifdef __cplusplus
extern "C" {
#endif

#define FYNKEL_AUTHORING_ABI_VERSION 3u

typedef struct FynkelAuthoringSession FynkelAuthoringSession;
/* The edit-preview world's type is fynkel.h's FynkelWorld. */

/* FYNKEL_AUTHORING_ABI_VERSION of the loaded library. */
uint32_t fynkel_authoring_abi_version(void);

/* A session with no project. NULL only if creation panicked. */
FynkelAuthoringSession* fynkel_authoring_session_create(void);
/* Closes the project and every document (unsaved edits are lost — ask
 * first) and frees the session. NULL is ignored. */
void fynkel_authoring_session_destroy(FynkelAuthoringSession* session);

/* Opens the project at `dir` (absolute, or relative to the process's
 * working directory), closing any open project and its documents first.
 * Reads fynkel.project.json, its diagnostics, the embedded schemas plus
 * the project's behaviour schemas, fynkel.lock.json (checked against this
 * build: a lock written by a NEWER Fynkel opens the project READ-ONLY —
 * every edit is then FYN-E0007 — an older one is a warning), the asset
 * database (a full scan) with every asset reference indexed, and
 * lib/generated/behaviours.g.json when present (its absence is a
 * FYN-T0006 warning saying to run `fynkel codegen`).
 * Result: { "ok", "project": { name, dir, assetsDir, readOnly: string |
 * null, hasMetadata, assetCount, referenceCount, componentTypes[],
 * behaviourTypes[], diagnostics[], errors, ... } }.
 * FYNKEL_STATUS_LOAD_FAILED with the diagnostics (FYN-P####, FYN-T0006)
 * when there is no project there or it cannot be read. */
FynkelStatus fynkel_authoring_open_project(FynkelAuthoringSession* session,
                                           const uint8_t* dir_utf8,
                                           uint32_t dir_len);
/* Closes the project and every open document; their handles become
 * FYN-T0002. Result: { "ok": true }. */
FynkelStatus fynkel_authoring_close_project(FynkelAuthoringSession* session);

/* Opens a scene, prefab or material document by path (absolute, or
 * relative to the project directory) or by asset id (36-char UUID text)
 * and writes its handle to `out_handle` (may be NULL). Idempotent: a
 * document already open returns its handle. Handles are never reused
 * within a session. Result: { "ok", "handle", "state" } where state is
 * { handle, path, relativePath, kind, id, name, formatVersion, revision,
 * dirty, canUndo, canRedo, history, readOnly }.
 * FYNKEL_STATUS_LOAD_FAILED: no such file or id (FYN-T0006), not a
 * document (FYN-D####), a kind the editor does not edit, a newer or an
 * outdated document (FYN-E0007). */
FynkelStatus fynkel_authoring_open_document(FynkelAuthoringSession* session,
                                            const uint8_t* path_or_id_utf8,
                                            uint32_t len,
                                            uint32_t* out_handle);
/* Drops the document and its history (unsaved edits are lost — ask
 * first) — and its edit-preview world, when one is open. Result:
 * { "ok": true, "handle", "previewClosed" }. */
FynkelStatus fynkel_authoring_close_document(FynkelAuthoringSession* session,
                                             uint32_t handle);
/* Re-reads the document from disk: the history is dropped, the revision
 * is the file's, edits are lost. The editor's "revert to saved" and the
 * silent reload after an external change with no local edits. Result:
 * { "ok", "state" }. */
FynkelStatus fynkel_authoring_reload_document(FynkelAuthoringSession* session,
                                              uint32_t handle);

/* Applies ONE transaction: `json_utf8` is the patch `fynkel scene apply`
 * reads — a JSON array of commands, or { "transactionId"?,
 * "baseRevision"?, "commands": [ ... ] }, each command
 * { "command": "<name>", ... } (docs/documents.md § Authoring commands;
 * the names are the `commands` query). The editor's rules apply
 * unchanged: a stale baseRevision is FYN-E0001 and nothing is touched; a
 * failing command or a validation error the transaction introduces rolls
 * the document back to the byte; saveDocument/createMaterial run last.
 * Result: the Applied record { "ok", "transactionId", "baseRevision",
 * "revision", "mutations": [{ commandId, command, transactionId,
 * baseRevision, affectedIds[], ok, diagnostics[], undo[] }],
 * "diagnostics": [...], "state": {...} }. FYNKEL_STATUS_COMMANDS_REJECTED
 * when "ok" is false (the record still says why, per command);
 * FYNKEL_STATUS_INVALID_ARGUMENT when the text is not JSON or the handle
 * is unknown (FYN-E0004 for a malformed request is COMMANDS_REJECTED). */
FynkelStatus fynkel_authoring_apply(FynkelAuthoringSession* session,
                                    uint32_t handle,
                                    const uint8_t* json_utf8,
                                    uint32_t json_len);
/* Replays the inverse (undo) or forward (redo) edits of the last
 * committed transaction, byte-exact. Result: { "ok", "transactionId",
 * "baseRevision", "revision", "commands": [names], "affectedIds": [...],
 * "state" }. FYNKEL_STATUS_COMMANDS_REJECTED with FYN-E0009 when there is
 * nothing to undo or redo, FYN-E0007 when the project is read-only. */
FynkelStatus fynkel_authoring_undo(FynkelAuthoringSession* session, uint32_t handle);
FynkelStatus fynkel_authoring_redo(FynkelAuthoringSession* session, uint32_t handle);
/* Writes the document canonically to `path_utf8` (absolute, or relative
 * to the process's working directory) or, with NULL and 0, where it was
 * read from; then rescans that path so the asset database sees the new
 * name or location. The document's path becomes the written one. Result:
 * { "ok", "path", "state" }. FYNKEL_STATUS_LOAD_FAILED (FYN-E0011) when
 * the file cannot be written. */
FynkelStatus fynkel_authoring_save(FynkelAuthoringSession* session,
                                   uint32_t handle,
                                   const uint8_t* path_utf8,
                                   uint32_t path_len);

/* Every call above that can change a document or what a scene's prefabs
 * expand to — apply, undo, redo, reload_document, save, rescan and operate —
 * brings every open edit-preview world up to date before it returns (the
 * incremental sync rule: the diff of the world against the document, a
 * fresh load when the diff does not settle) and adds "previews": [{
 * handle, spawned, destroyed, updated, reordered, steps, reloaded,
 * diagnostics[], describeDiagnostics? }] to its result when at least one
 * preview is open. */

/* Rescans the project's asset root. `paths_json_utf8` is a JSON array of
 * paths (absolute, or relative to the project directory) — what a file
 * watcher reports: only those files are re-read (a source or its sidecar
 * means the pair, a folder everything under it, a path that is gone drops
 * what it held) and only the documents at them re-indexed, about a
 * millisecond in a project of thousands of assets. NULL with 0 (or `[]`)
 * re-reads everything (tens of milliseconds). Result:
 * { "ok", "assetCount", "scanned": [...], "diagnostics": [of what was read] }. */
FynkelStatus fynkel_authoring_rescan(FynkelAuthoringSession* session,
                                     const uint8_t* paths_json_utf8,
                                     uint32_t len);

/* A read-only query by name; `args_json_utf8` is a JSON object or NULL
 * with 0. Result: { "ok", "kind", "result": <below> }.
 *   project                              the open project (as open_project)
 *   documents                            every open document's state
 *   document        handle, root?=true   state + "root": the whole document as JSON
 *   inspect         handle               the typed view `fynkel scene|prefab inspect --json`
 *                                        prints (a material: shader, alphaMode,
 *                                        alphaCutoff, doubleSided, fields) + "diagnostics"
 *   validate        handle               { ok, diagnostics } of the document now,
 *                                        asset references checked
 *   validateProject                      the `fynkel validate --json` report
 *   assets          kind?, document?     the asset entries (kind "document" | "source";
 *                                        document a kind name such as "scene")
 *   asset           asset, sub?          one resolved entry (FYN-A0004 if unknown)
 *   dependents      asset                every reference to the asset:
 *                                        [{ document, path, pointer, asset }]
 *   schemas                              { components: [{ type, id, authored, wireSize,
 *                                        allowed, schema }], behaviours: [{ type,
 *                                        version, dart, hasSchema, allowed, schema }] }
 *   schema          type                 one component or behaviour schema (with its
 *                                        x-fynkel display hints)
 *   assetDetail     asset                the entry + "file": { size, modified },
 *                                        "dependents": [as dependents], and for a
 *                                        source "sidecar": { path, importer,
 *                                        importerVersion, settings, subAssets }, for a
 *                                        material "material": { shader, alphaMode,
 *                                        alphaCutoff, doubleSided, fields }, for a
 *                                        scene or prefab "entities": <count>, for a
 *                                        rig or blend "fields"
 *   assetHistory                         the asset operations' undo/redo state:
 *                                        { canUndo, canRedo, undoLabel, redoLabel,
 *                                        undo: [labels], redo: [labels] }
 *   metadata                             lib/generated/behaviours.g.json, or null
 *   commands                             the command names apply accepts
 *   operations                           the operation names operate accepts
 * FYNKEL_STATUS_INVALID_ARGUMENT for an unknown kind (FYN-T0004, the
 * suggestion lists them), a missing or malformed argument (FYN-T0003), no
 * project (FYN-T0001) or an unknown handle (FYN-T0002). */
FynkelStatus fynkel_authoring_query(FynkelAuthoringSession* session,
                                    const uint8_t* kind_utf8,
                                    uint32_t kind_len,
                                    const uint8_t* args_json_utf8,
                                    uint32_t args_len);

/* An asset operation by name — the session's own mutations of the asset
 * tree, next to a document's transactions (blueprint §19.2e item 5). Every
 * one re-reads only the paths it touched; the ones that change the tree go
 * on the session's asset history, an undo/redo stack of its own (a
 * document's undo never moves a file). Paths are relative to the asset
 * root (or absolute inside it). `args_json_utf8` is a JSON object.
 *   newDocument        kind, path, name?, shader?   a scene, prefab, material or volume
 *                                                  as `Document::new` (kind's suffix
 *                                                  added when missing; name defaults to the
 *                                                  file's stem; a material's shader
 *                                                  validated against the project's)
 *   renameAsset        asset, path                 moves the file — and a source's
 *                                                  sidecar — so the id survives; `path`
 *                                                  ending in `/` or naming a folder keeps
 *                                                  the name; the kind (suffix, extension)
 *                                                  is kept; an open document follows
 *   duplicateAsset     asset, path?                a copy beside it (`coin 1`, …) under
 *                                                  a fresh id — a source with a sidecar
 *                                                  of its own and fresh sub-asset ids
 *   installPrimitives  —                           the built-in shapes (ADR-0023): the
 *                                                  existing asset of the fixed id, or
 *                                                  `Fynkel/Primitives.glb` written
 *   importFiles        paths, folder?              files from outside copied into folder
 *                                                  (a taken name counts on); a source's
 *                                                  own sidecar kept when its id is free;
 *                                                  one step undoes them all
 *   deleteAsset        asset, force?               refused with every referencing field
 *                                                  in `related` while a document names
 *                                                  the asset (or a sub-asset) unless
 *                                                  force; refused while open; file and
 *                                                  sidecar move to .fynkel/trash/<id>/
 *   setImportSettings  asset, settings             replaces the sidecar's settings object
 *   reimport           asset                       runs the importer with the sidecar's
 *                                                  settings; updates subAssets when they
 *                                                  changed; { ok: false, diagnostics }
 *                                                  when the importer refused; not undoable
 *   setLayersAndTags   layers, tags, layersFrom?,  replaces the project's lists (ADR-0020);
 *                      tagsFrom?                   `*From[i]` = the old index entry i comes
 *                                                  from or null (absent: by index); a
 *                                                  reorder or removal rewrites every scene
 *                                                  and prefab (indices, bits, masks,
 *                                                  overrides); refused while such a document
 *                                                  has unsaved edits; `reload` lists the
 *                                                  open documents the client re-reads
 *   registerBehaviour  id, description?            after `fynkel codegen` wrote a new
 *                                                  behaviour's metadata and schema: adds
 *                                                  it to the project's component registry
 *                                                  (when missing; undoable) and re-reads
 *                                                  the metadata and schemas; answers
 *                                                  { id, registered, inMetadata }
 *   reloadBehaviours                               after `fynkel codegen` regenerated the
 *                                                  metadata (a script saved): re-reads the
 *                                                  metadata and schemas; writes nothing;
 *                                                  answers { types }
 *   undoAsset / redoAsset                         the asset history
 * Result: { "ok", "op", "label", "entry", "assetCount", "canUndo", "canRedo",
 * "undoLabel", "redoLabel", ...per operation }. FYNKEL_STATUS_INVALID_ARGUMENT
 * with FYN-T0003 for a refused operation (nothing was touched) or FYN-T0004
 * for an unknown kind; FYNKEL_STATUS_LOAD_FAILED with FYN-A0004 for an
 * unknown asset or FYN-T0006 for an I/O failure; FYNKEL_STATUS_COMMANDS_REJECTED
 * with FYN-E0009 when there is nothing to undo or redo or FYN-E0007 when the
 * project is read-only. A reimport the importer refused answers
 * COMMANDS_REJECTED with { ok: false, diagnostics } and leaves the sidecar
 * alone. */
FynkelStatus fynkel_authoring_operate(FynkelAuthoringSession* session,
                                      const uint8_t* kind_utf8,
                                      uint32_t kind_len,
                                      const uint8_t* args_json_utf8,
                                      uint32_t args_len);

/* ---- The edit-preview world (version 3; blueprint §19.2e item 6) ------
 *
 * Opens the edit-preview world of the scene or prefab document at `handle`:
 * a FynkelWorld in edit mode (no systems — nothing moves unless a command
 * moved it) loaded fresh from the live document (its prefabs expanded, the
 * ones open in other tabs from their live documents), drawing from the
 * project's cooked cache. The first preview of a session cooks the project
 * in process (`fynkel cook`; the report is in "cook") and opens the cache;
 * later previews share the store. Idempotent: an open preview is left as it
 * is ("opened": false, no "cook"). Result: { "ok", "handle", "opened",
 * "cook"?: <the fynkel cook --json report>, "stats": { entities, live,
 * meshes, lights, hidden, tick, loads, syncs }, "diagnostics": [of the
 * description and the load] }. FYNKEL_STATUS_INVALID_ARGUMENT with
 * FYN-T0003 for a material (nothing to preview), FYN-T0002 for an unknown
 * handle, FYN-T0001 without a project; FYNKEL_STATUS_LOAD_FAILED when the
 * cache cannot be opened at all (a cook with failed assets still opens —
 * the preview draws the others and the report says which failed). */
FynkelStatus fynkel_authoring_preview_open(FynkelAuthoringSession* session,
                                           uint32_t handle);
/* Drops the edit-preview world of `handle`; the document stays open. The
 * world pointer must not be rendered from afterwards. Result:
 * { "ok", "handle", "closed" }. */
FynkelStatus fynkel_authoring_preview_close(FynkelAuthoringSession* session,
                                            uint32_t handle);
/* The edit-preview world of `handle` for the viewport ABI
 * (fynkel_apple_viewport_render draws it and adopts its asset store like
 * any world's) and for fynkel.h's reads (fynkel_world_stats,
 * fynkel_world_query). NULL when no preview is open for the handle, or the
 * session is NULL or poisoned. Valid until fynkel_authoring_preview_close,
 * the document's close, the project's close or the session's destruction
 * — and stable across recook. Never write to it through
 * fynkel_world_submit / fynkel_world_step / fynkel_world_load_*: the
 * preview owns its commands and its steps, and the next resync would undo
 * or fight the change. */
const FynkelWorld* fynkel_authoring_preview_world(const FynkelAuthoringSession* session,
                                                  uint32_t handle);
/* A call on the edit-preview world of `handle`, by kind with JSON
 * arguments (an object, or NULL with 0). Vectors are JSON arrays of
 * numbers; a matrix is 16 numbers column-major; ids are UUID strings; a
 * `view` is { world: [16], aspect, projection?: "perspective" |
 * "orthographic", fovY? (radians), orthoHeight?, near?, far?,
 * exposureMode?: "auto" | "manual", exposure? (EV100) } — the camera
 * fields default to the engine's Camera defaults. Every result carries
 * "ok" and "handle". The world holds the document *expanded*: a prefab
 * instance's inner entities have derived ids the document does not
 * list, so `pick`, `pickRect`, `hidden` and `lines.outline` speak in
 * document entities — a hit inside an instance answers as the entity
 * that carries the instance (its anchor, once), hiding or outlining an
 * anchor hides or outlines its whole instance. `worldMatrix`,
 * `setTransforms` and `dump` take and give the ids as they are.
 *   camera          world, projection?, fovY?, orthoHeight?, near?, far?,
 *                   exposureMode?, exposure?
 *                                       the editor camera the picture is drawn through,
 *                                       in the scene camera's place (never a game camera)
 *   hidden          ids                 exactly these entities and their descendants left
 *                                       out of the picture and of picking (the editor's
 *                                       hidden flag; [] shows all) → "hidden": count
 *   pick            view + ndc [2], or ray { origin [3], dir [3] }
 *                                       every entity the ray hits, nearest first: a
 *                                       visible mesh through its bounding sphere then
 *                                       its triangles, anything else through a 0.25 m
 *                                       sphere at its origin → "ray", "hits": [{ id,
 *                                       distance, point [3], mesh }]
 *   pickRect        view, min [2], max [2] (NDC)
 *                                       the entities whose projected bounds overlap the
 *                                       rectangle (the marquee) → "ids"
 *   lines           outline?: { ids, color? [4] }, icons?: { aspect }
 *                                       the selection outline (each id's bounds as
 *                                       twelve edges, or a cross for an entity without
 *                                       a mesh; default colour orange) and/or the icons
 *                                       of every light, camera and collider → "count",
 *                                       "lines": [[ax, ay, az, bx, by, bz, r, g, b, a]]
 *                                       — ready for fynkel_apple_viewport_debug_lines
 *   worldMatrix     ids                 → "matrices": { id: [16] } (FYN-T0003 for an
 *                                       id not in the world)
 *   setTransforms   transforms: [{ id, position [3], rotation [4] (x, y, z, w), scale [3] }]
 *                                       the gizmo's preview: the local transforms set
 *                                       in the world only (the document is untouched,
 *                                       stays clean) → "count"
 *   restoreTransforms ids               the transforms back to the document's (a
 *                                       cancelled drag) → "count"
 *   resync                              the incremental sync now (a no-op when the
 *                                       world already matches) → the sync report +
 *                                       "stats"
 *   reload                              the fallback and the user's Reload: the world
 *                                       emptied and the document loaded fresh →
 *                                       "stats", "diagnostics"
 *   recook [live]                       project-wide: cooks the project again, reopens
 *                                       the cache, points every preview's world at it
 *                                       and reloads each → "cook", "previews": [{
 *                                       handle, stats, diagnostics }]; `live: true`
 *                                       cooks the documents with unsaved edits as they
 *                                       are, their files untouched (Play)
 *   dump                                the world as `fynkel state dump` prints it, in
 *                                       document order (the gate's incremental-vs-fresh
 *                                       comparison) → "dump": { document, entityCount,
 *                                       entities: [{ id, name, parent, children, local,
 *                                       world, components }] }
 *   stats                               → "stats", "diagnostics"
 * FYNKEL_STATUS_INVALID_ARGUMENT for an unknown kind (FYN-T0004, the
 * suggestion lists them), a missing or malformed argument (FYN-T0003), a
 * handle without a preview (FYN-T0002). */
FynkelStatus fynkel_authoring_preview_call(FynkelAuthoringSession* session,
                                           uint32_t handle,
                                           const uint8_t* kind_utf8,
                                           uint32_t kind_len,
                                           const uint8_t* args_json_utf8,
                                           uint32_t args_len);

/* A live call on a RUNNING game's world (ADR-0021 P7, live Play mode) —
 * not a preview: no session, the world is the game's. Kinds:
 * - "entities" { since? } -> { tick, revision, entities: [{ id, entity,
 *   name, parent, spawned, layer, tags, children, types }] }, parents
 *   before children (`entity` the runtime handle, `children` a count,
 *   `types` the native component types). `revision` hashes the rows (this
 *   process only); given `since` equal to it the answer is { tick,
 *   revision, unchanged: true } without them. `id` is the
 *   authoring id for what a loaded scene spawned (a cooked prefab
 *   instance's children carry their derived ids), "runtime:<handle>" for
 *   what the game spawned or instantiated (`spawned` true);
 * - "entity" { id } -> { id, entity, name, parent, world, components: [{ type,
 *   data }] }, every native component as the scene document stores it;
 * - "set" { id, type, data } replaces or adds a native component from its
 *   document JSON; "remove" { id, type }; "rename" { id, name };
 *   "parent" { id, parent: id | null, index? } moves it (its local
 *   transform kept; `index` among the new siblings, last when absent);
 *   "destroy" { id } destroys it and its subtree; "spawn" { name?, parent?:
 *   id | null, index?, components: [{ type, data }], id?, scene? } makes
 *   an entity at once -> { ok, id } (with an `id` it joins a loaded scene
 *   under that authoring id: `scene`'s, its parent's, else the first);
 * - for the Scene view on the running world, the preview's own picking
 *   and overlays over every live entity (meshes read from the cache the
 *   world loaded): "pick" { ray: { origin, dir } } or { view, ndc } ->
 *   { ray, hits: [{ id, entity, distance, point, mesh }] } nearest first,
 *   each entity itself (a running prefab instance has no anchor);
 *   "pickRect" { view, min, max } -> { ids }; "lines" { outline?: { ids,
 *   color? }, icons?: { aspect } } -> { count, lines } (an id no longer
 *   alive draws nothing); "worldMatrix" { ids } -> { matrices: { id:
 *   16 floats } } for the ids still alive. `view` is the preview's shape.
 * A write applies at once, at the current tick, without stepping (it
 * shows while paused; no system runs). `out_view` gets the JSON result,
 * valid until the next query or mutating call on the world. The world
 * refusing a write is FYNKEL_STATUS_COMMANDS_REJECTED with { "ok": false,
 * "rejected": [{ code, message }] }; a malformed call, an unknown kind,
 * entity or component type is FYNKEL_STATUS_INVALID_ARGUMENT with
 * { "ok": false, "diagnostics": [...] } (FYN-T0003). */
FynkelStatus fynkel_world_live_call(FynkelWorld* world,
                                    const uint8_t* kind_utf8,
                                    uint32_t kind_len,
                                    const uint8_t* args_json_utf8,
                                    uint32_t args_len,
                                    FynkelBufferView* out_view);

/* UTF-8 JSON of the last call on this session (empty before any, or for
 * NULL). Valid until the next call on the session. */
FynkelBufferView fynkel_authoring_result(const FynkelAuthoringSession* session);
/* 1 after a caught panic (every call answers FYNKEL_STATUS_POISONED). */
int32_t fynkel_authoring_is_poisoned(const FynkelAuthoringSession* session);

#ifdef __cplusplus
}
#endif
#endif /* FYNKEL_AUTHORING_H */
