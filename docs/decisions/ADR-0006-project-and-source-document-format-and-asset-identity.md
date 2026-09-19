# ADR-0006: Project and source document format and asset identity

**Status:** Accepted  
**Date:** 2026-09-19  
**Related:** `docs/blueprint.md`

## Context

Scenes, prefabs, materials, and project files must be diffable, validatable, and editable by humans, the editor, the CLI, and agents alike.

## Decision

- Strict JSON (`.scene.json`, `.prefab.json`, `.material.json`, `.shader.json`) validated by JSON Schema draft 2020-12 plus semantic validation. One authoritative format; no JSON5.
- UUID-keyed entity/component maps; child ordering stored separately; format version and document id on every file.
- **Asset identity lives in `.meta.json` sidecars** next to each source asset, committed to version control, holding the stable id and import settings. The cache is derived and reconstructible.
- Cooked release assets use a **custom versioned container, one file per asset**, with lengths, checksums, and dependency ids. Pack files are a measured 1.x option.
- Prefab overrides target (prefab-local entity id, component id, field id); never array index or name.

## Alternatives considered

- Single `assets.manifest.json`: easier to query, conflicts on every change.
- Shipping source glTF/PNG in release: simplest, slowest loads; rejected for 1.0 but used during early development.

## Consequences

Every asset move must carry its sidecar; the CLI and editor enforce this. Merge conflicts are per-asset rather than in one manifest.
