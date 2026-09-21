/*
 * Fynkel authoring ABI — version 2 (M4-01, blueprint §19.2e item 1,
 * ADR-0016; version 2 added fynkel_authoring_operate and the assetDetail,
 * assetHistory and operations queries for item 5, the Project browser).
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
 * - The session never runs game code and never reads a cooked cache
 *   (ADR-0016/0018).
 */
#ifndef FYNKEL_AUTHORING_H
#define FYNKEL_AUTHORING_H

#include <stdint.h>
#include "fynkel.h"

#ifdef __cplusplus
extern "C" {
#endif

#define FYNKEL_AUTHORING_ABI_VERSION 2u

typedef struct FynkelAuthoringSession FynkelAuthoringSession;

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
 * first). Result: { "ok": true, "handle" }. */
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
 *   newDocument        kind, path, name?, shader?   a scene, prefab or material as
 *                                                  `Document::new` (kind's suffix added
 *                                                  when missing; name defaults to the
 *                                                  file's stem; a material's shader
 *                                                  validated against the project's)
 *   renameAsset        asset, path                 moves the file — and a source's
 *                                                  sidecar — so the id survives; `path`
 *                                                  ending in `/` or naming a folder keeps
 *                                                  the name; the kind (suffix, extension)
 *                                                  is kept; an open document follows
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
 *   undoAsset / redoAsset                          the asset history
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

/* UTF-8 JSON of the last call on this session (empty before any, or for
 * NULL). Valid until the next call on the session. */
FynkelBufferView fynkel_authoring_result(const FynkelAuthoringSession* session);
/* 1 after a caught panic (every call answers FYNKEL_STATUS_POISONED). */
int32_t fynkel_authoring_is_poisoned(const FynkelAuthoringSession* session);

#ifdef __cplusplus
}
#endif
#endif /* FYNKEL_AUTHORING_H */
