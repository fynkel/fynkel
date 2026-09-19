# ADR-0014: Error reporting across the ABI

**Status:** Accepted  
**Date:** 2026-09-19  
**Related:** `docs/blueprint.md`

## Context

Panics must never unwind across the C ABI, and Dart needs to know which command in a batch failed.

## Decision

`step()` returns a status code and fills a diagnostics buffer of records `{command_index, code, entity_id, component_id, message}`. In debug builds the SDK throws `FynkelError` with those records; in release it logs and continues. Rust panics are caught at the ABI edge (`catch_unwind`), converted to a fatal status, and the world is marked poisoned; the host decides on restart.

## Consequences

Per-command result slots in the snapshot are not needed. Diagnostics codes are stable and shared with the CLI's `--json` output.
