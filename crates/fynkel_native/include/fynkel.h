/*
 * Fynkel native ABI — version 1 (M1).
 *
 * This header is the contract. Dart bindings are generated from it with
 * ffigen (ADR-0004); Rust implements it in crates/fynkel_native over
 * fynkel_core. The ABI is designed by hand: batched buffers in, batched
 * buffers out, one step() per fixed tick, no per-property calls, no Rust
 * layouts leaking through.
 *
 * Record bodies inside buffers are NOT declared here: they are generated
 * from the Rust component schemas on both sides (ADR-0012), see
 * schemas/fynkel.schema.json. fynkel_schema_fingerprint() must equal the
 * fingerprint the Dart codecs were generated from.
 *
 * Conventions
 * - All integers are little-endian. Every buffer starts with a
 *   FynkelBufferHeader (24 bytes). Record bodies are laid out per the
 *   schema layout rule (natural alignment, padded to 8). Strings inside
 *   records are (offset, len) into the buffer's string tail, which starts
 *   at header.records_end.
 * - Input buffers are borrowed for the duration of the call only.
 * - Output views are owned by the world and valid until the next call that
 *   takes a non-const FynkelWorld*, or until the world is destroyed.
 * - Every function is safe to call with NULL handles (returns an error).
 * - Rust panics never cross this boundary: they become
 *   FYNKEL_STATUS_PANIC and poison the world (ADR-0014).
 * - Not thread-safe: a world must be driven from one thread at a time
 *   (ADR-0008).
 * - The engine creates no threads and owns no loop (ADR-0007): the host
 *   calls begin_frame, then step() as many times as the plan says.
 */
#ifndef FYNKEL_H
#define FYNKEL_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define FYNKEL_ABI_VERSION 1u
#define FYNKEL_BUFFER_MAGIC 0x4b4e5946u /* "FYNK" */
#define FYNKEL_BUFFER_HEADER_SIZE 24u

typedef struct FynkelRuntime FynkelRuntime;
typedef struct FynkelWorld FynkelWorld;

/* Entity handle: world id (8) | generation (24, never 0) | slot (32).
 * 0 is never valid. */
typedef uint64_t FynkelEntity;

typedef enum FynkelStatus {
  FYNKEL_STATUS_OK = 0,
  /* Some commands in the batch were rejected; see diagnostics. */
  FYNKEL_STATUS_COMMANDS_REJECTED = 1,
  FYNKEL_STATUS_INVALID_ARGUMENT = 2,
  /* Buffer header/record layout did not validate; nothing was applied. */
  FYNKEL_STATUS_PROTOCOL = 3,
  FYNKEL_STATUS_POISONED = 4,
  FYNKEL_STATUS_PANIC = 5,
  FYNKEL_STATUS_OUT_OF_RESOURCES = 6,
  /* ABI version or schema fingerprint mismatch. */
  FYNKEL_STATUS_INCOMPATIBLE = 7,
  /* A scene or project document could not be loaded; see
   * fynkel_world_load_report. */
  FYNKEL_STATUS_LOAD_FAILED = 8
} FynkelStatus;

typedef enum FynkelBufferKind {
  /* CommandHeader + body per command (kind = FynkelCommandKind). */
  FYNKEL_BUFFER_COMMANDS = 1,
  /* SnapshotTransformRecord per subscribed entity, handle order. */
  FYNKEL_BUFFER_SNAPSHOT = 2,
  /* EventHeader + body per event (kind = event kind). */
  FYNKEL_BUFFER_EVENTS = 3,
  /* DiagnosticRecord per diagnostic; messages in the string tail. */
  FYNKEL_BUFFER_DIAGNOSTICS = 4,
  /* Query response; record type depends on the query kind. */
  FYNKEL_BUFFER_QUERY = 5
} FynkelBufferKind;

/* Command lanes (blueprint §5.1, ADR-0008). */
typedef enum FynkelCommandLane {
  FYNKEL_LANE_SIMULATION = 0,
  FYNKEL_LANE_PRESENTATION = 1
} FynkelCommandLane;

/* Ids are the schema ids (schemas/fynkel.schema.json); listed for C hosts. */
typedef enum FynkelCommandKind {
  FYNKEL_COMMAND_SPAWN = 1,
  FYNKEL_COMMAND_DESTROY = 2,
  FYNKEL_COMMAND_SET_TRANSFORM = 3,
  FYNKEL_COMMAND_SET_PARENT = 4,
  FYNKEL_COMMAND_SET_NAME = 5
} FynkelCommandKind;

typedef enum FynkelComponent {
  FYNKEL_COMPONENT_TRANSFORM = 1,
  FYNKEL_COMPONENT_HIERARCHY = 2,
  FYNKEL_COMPONENT_NAME = 3
} FynkelComponent;

typedef enum FynkelQueryKind {
  /* entity -> one SnapshotTransformRecord (0 records if unknown). */
  FYNKEL_QUERY_TRANSFORM = 1,
  /* entity -> u64 handles of its children, deterministic order. */
  FYNKEL_QUERY_CHILDREN = 2,
  /* entity -> one u64: parent handle or 0. */
  FYNKEL_QUERY_PARENT = 3,
  /* entity -> one Name record (0 records if none). */
  FYNKEL_QUERY_NAME = 4,
  /* (no entity) -> u64 handles of every live entity, slot order. */
  FYNKEL_QUERY_ALL_ENTITIES = 5
} FynkelQueryKind;

/* ---- Buffer header ------------------------------------------------------ */

typedef struct FynkelBufferHeader {   /* 24 bytes, first in every buffer */
  uint32_t magic;                     /* FYNKEL_BUFFER_MAGIC */
  uint16_t version;                   /* FYNKEL_ABI_VERSION */
  uint16_t kind;                      /* FynkelBufferKind */
  uint32_t record_count;
  uint32_t byte_length;               /* whole buffer incl. header and tail */
  uint32_t records_end;               /* offset where the string tail starts */
  uint32_t request_id;                /* query responses echo the request id */
} FynkelBufferHeader;

/* ---- Call structs ------------------------------------------------------- */

typedef struct FynkelBufferView {
  const uint8_t* data;                /* NULL when length == 0 */
  uint32_t length;
} FynkelBufferView;

typedef struct FynkelWorldDesc {
  uint8_t world_id;                   /* 1..255; 0 = default (1) */
  uint8_t reserved[3];
  float fixed_dt_seconds;             /* 0 = default 1/60 */
  uint32_t max_steps_per_frame;       /* 0 = default 4 */
  float max_frame_dt_seconds;         /* 0 = default 0.25 */
  uint32_t flags;                     /* reserved */
} FynkelWorldDesc;

typedef struct FynkelFramePlan {
  uint32_t steps;                     /* fixed ticks to run this frame */
  float dropped_seconds;              /* > 0: MissedDeadline recorded */
  uint32_t clamped;                   /* frame dt exceeded max_frame_dt */
  float alpha;                        /* interpolation factor after the steps */
} FynkelFramePlan;

typedef struct FynkelSubmitResult {
  uint32_t accepted;                  /* staged (simulation) or applied (presentation) */
  uint32_t rejected;                  /* diagnostics describe each one */
} FynkelSubmitResult;

typedef struct FynkelStepResult {
  uint64_t tick;                      /* tick number after the step */
  uint32_t commands_applied;
  uint32_t commands_rejected;
  uint32_t snapshot_records;
  uint32_t event_records;             /* pending in the general lane */
  uint32_t diagnostic_records;
  uint32_t live_entities;
} FynkelStepResult;

typedef struct FynkelQueryDesc {
  uint32_t kind;                      /* FynkelQueryKind */
  uint32_t request_id;                /* echoed in the response header */
  FynkelEntity entity;                /* subject, per kind */
} FynkelQueryDesc;

/* Bridge statistics since world creation (blueprint §5.7, M1 gate). */
typedef struct FynkelWorldStats {
  uint64_t tick;
  uint64_t frame;
  uint64_t submit_calls;
  uint64_t step_calls;
  uint64_t query_calls;
  uint64_t bytes_in;                  /* command bytes received */
  uint64_t bytes_out;                 /* snapshot+event+diagnostic+query bytes produced */
  uint32_t live_entities;
  uint32_t subscriptions;
  uint32_t staged_commands;
  uint32_t poisoned;
} FynkelWorldStats;

/* ---- Functions ---------------------------------------------------------- */

uint32_t fynkel_abi_version(void);
/* Fingerprint of the record schemas this library was built with; the Dart
 * codecs carry the one they were generated from. */
uint64_t fynkel_schema_fingerprint(void);

FynkelRuntime* fynkel_runtime_create(void);
void fynkel_runtime_destroy(FynkelRuntime* runtime);

FynkelWorld* fynkel_world_create(FynkelRuntime* runtime, const FynkelWorldDesc* desc);
void fynkel_world_destroy(FynkelWorld* world);

/* Starts a host frame: accumulates dt, decides the number of fixed ticks,
 * clears last frame's presentation overrides (§5.1). */
FynkelStatus fynkel_world_begin_frame(FynkelWorld* world,
                                      float dt_seconds,
                                      FynkelFramePlan* out_plan);

/* Validates a command buffer. Simulation-lane commands are staged for the
 * next step(); presentation-lane commands apply now. Returns PROTOCOL
 * without applying anything if the buffer does not validate. */
FynkelStatus fynkel_world_submit_commands(FynkelWorld* world,
                                          uint32_t lane,
                                          const uint8_t* buffer,
                                          uint32_t length,
                                          FynkelSubmitResult* out_result);

/* One fixed tick: applies staged simulation commands, runs the native
 * phases, publishes the snapshot (subscribed entities only), events and
 * diagnostics. Returns OK or COMMANDS_REJECTED on a normal step. */
FynkelStatus fynkel_world_step(FynkelWorld* world, FynkelStepResult* out_result);

/* Subscribe/unsubscribe `count` entities' `component` for snapshots.
 * Only FYNKEL_COMPONENT_TRANSFORM is subscribable in M1. */
FynkelStatus fynkel_world_subscribe(FynkelWorld* world,
                                    uint32_t component,
                                    const FynkelEntity* entities,
                                    uint32_t count,
                                    int32_t enable);

/* Borrowed views; see ownership rules at the top. */
FynkelBufferView fynkel_world_snapshot(const FynkelWorld* world);
FynkelBufferView fynkel_world_diagnostics(const FynkelWorld* world);

/* Pull-only events (ADR-0008): moves pending general-lane events plus the
 * last tick's physics events into the view; a second call returns an empty
 * buffer until more events are produced. */
FynkelBufferView fynkel_world_drain_events(FynkelWorld* world);
/* Physics events of the tick just stepped, for afterPhysics; not drained. */
FynkelBufferView fynkel_world_after_physics_events(const FynkelWorld* world);

/* Explicit query; the response is valid until the next mutating call or
 * the next query. */
FynkelStatus fynkel_world_query(FynkelWorld* world,
                                const FynkelQueryDesc* desc,
                                FynkelBufferView* out_view);

FynkelStatus fynkel_world_stats(const FynkelWorld* world, FynkelWorldStats* out_stats);

uint64_t fynkel_world_tick(const FynkelWorld* world);
float fynkel_world_alpha(const FynkelWorld* world);

/* Non-zero once a panic or fatal error was caught; every mutating call then
 * returns FYNKEL_STATUS_POISONED. */
int32_t fynkel_world_is_poisoned(const FynkelWorld* world);

/* Documents (blueprint §7). Loads a `.scene.json` (`path`, UTF-8, not
 * NUL-terminated; prefab instances expand from `*.prefab.json` files next
 * to it and below) or a project directory's startup scene (every prefab in
 * the project's asset database is available) into the world: entities are
 * spawned immediately with their authoring ids kept for the report; other
 * components go through the command path and any rejection is in
 * fynkel_world_diagnostics. FYNKEL_STATUS_LOAD_FAILED when the document,
 * the project or the hierarchy has errors (the world is unchanged);
 * FYNKEL_STATUS_INVALID_ARGUMENT for NULL or non-UTF-8 input. */
FynkelStatus fynkel_world_load_scene(FynkelWorld* world,
                                     const uint8_t* path_utf8,
                                     uint32_t path_len);
FynkelStatus fynkel_world_load_project(FynkelWorld* world,
                                       const uint8_t* dir_utf8,
                                       uint32_t dir_len);
/* UTF-8 JSON describing the last load call on this world (empty before
 * any): {"ok": bool, "scene": {"id", "name", "path", "entities"} | null,
 * "project": {"name", "dir", "assets", "cache"} | null, "diagnostics":
 * [{"code", "severity", "message", "document"?, "pointer"?, ...}]}.
 * Valid until the next load call. */
FynkelBufferView fynkel_world_load_report(const FynkelWorld* world);

#ifdef __cplusplus
}
#endif
#endif /* FYNKEL_H */
