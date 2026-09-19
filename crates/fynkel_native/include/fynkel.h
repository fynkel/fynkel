/*
 * Fynkel native ABI — version 0 (M0 prototype of the real contract).
 *
 * This header is the contract. Dart bindings are generated from it with
 * ffigen (ADR-0004); Rust implements it in crates/fynkel_native. The ABI is
 * designed by hand: batched buffers in, batched buffers out, one step() per
 * fixed tick, no per-property calls, no Rust layouts leaking through.
 *
 * Conventions
 * - All integers are little-endian; all records are fixed-size and
 *   4-byte aligned; buffers start with a FynkelBufferHeader (16 bytes).
 * - Input buffers are borrowed for the duration of the call only.
 * - Output views are owned by the world and valid until the next call that
 *   takes a non-const FynkelWorld*, or until the world is destroyed.
 * - Every function is safe to call with NULL handles (returns an error).
 * - Rust panics never cross this boundary: they become
 *   FYNKEL_STATUS_PANIC and poison the world (ADR-0014).
 * - Not thread-safe: a world must be driven from one thread at a time.
 */
#ifndef FYNKEL_H
#define FYNKEL_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define FYNKEL_ABI_VERSION 0u
#define FYNKEL_BUFFER_MAGIC 0x4b4e5946u /* "FYNK" */

typedef struct FynkelRuntime FynkelRuntime;
typedef struct FynkelWorld FynkelWorld;

/* Entity handle: (generation << 32) | slot. 0 is never valid. */
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
  FYNKEL_STATUS_OUT_OF_RESOURCES = 6
} FynkelStatus;

typedef enum FynkelBufferKind {
  FYNKEL_BUFFER_COMMANDS = 1,
  FYNKEL_BUFFER_SNAPSHOT = 2,
  FYNKEL_BUFFER_EVENTS = 3,
  FYNKEL_BUFFER_DIAGNOSTICS = 4
} FynkelBufferKind;

/* Command lanes (blueprint §5.1): simulation commands apply at the start of
 * the next fixed tick; presentation commands apply immediately on submit. */
typedef enum FynkelCommandLane {
  FYNKEL_LANE_SIMULATION = 0,
  FYNKEL_LANE_PRESENTATION = 1
} FynkelCommandLane;

typedef enum FynkelComponent {
  FYNKEL_COMPONENT_TRANSFORM = 1
} FynkelComponent;

typedef enum FynkelCommandKind {
  /* target = caller-chosen temp id (must be non-zero); payload = FynkelTransform.
   * Produces an FYNKEL_EVENT_SPAWNED event mapping temp id -> entity. */
  FYNKEL_COMMAND_SPAWN = 1,
  /* target = entity. */
  FYNKEL_COMMAND_DESTROY = 2,
  /* target = entity; payload = FynkelTransform. */
  FYNKEL_COMMAND_SET_TRANSFORM = 3
} FynkelCommandKind;

typedef enum FynkelEventKind {
  /* a = temp id, b = entity */
  FYNKEL_EVENT_SPAWNED = 1,
  /* a = entity */
  FYNKEL_EVENT_DESTROYED = 2
} FynkelEventKind;

typedef enum FynkelDiagnosticCode {
  FYNKEL_DIAG_UNKNOWN_COMMAND = 1,
  FYNKEL_DIAG_STALE_ENTITY = 2,
  FYNKEL_DIAG_INVALID_TEMP_ID = 3,
  FYNKEL_DIAG_INVALID_PAYLOAD = 4
} FynkelDiagnosticCode;

/* ---- Wire records ------------------------------------------------------- */

typedef struct FynkelBufferHeader {   /* 16 bytes, first in every buffer */
  uint32_t magic;                     /* FYNKEL_BUFFER_MAGIC */
  uint16_t version;                   /* FYNKEL_ABI_VERSION */
  uint16_t kind;                      /* FynkelBufferKind */
  uint32_t record_count;
  uint32_t byte_length;               /* including this header */
} FynkelBufferHeader;

typedef struct FynkelTransform {      /* 40 bytes */
  float position[3];
  float rotation[4];                  /* quaternion x, y, z, w */
  float scale[3];
} FynkelTransform;

typedef struct FynkelCommand {        /* 64 bytes */
  uint32_t kind;                      /* FynkelCommandKind */
  uint32_t flags;                     /* reserved, 0 */
  uint64_t target;                    /* entity or temp id, per kind */
  uint8_t payload[48];                /* per kind; FynkelTransform uses 40 */
} FynkelCommand;

typedef struct FynkelSnapshotTransform { /* 48 bytes */
  FynkelEntity entity;
  FynkelTransform transform;
} FynkelSnapshotTransform;

typedef struct FynkelEvent {          /* 32 bytes */
  uint32_t kind;                      /* FynkelEventKind */
  uint32_t tick_lo;                   /* low 32 bits of the tick that produced it */
  uint64_t a;
  uint64_t b;
  uint64_t c;
} FynkelEvent;

typedef struct FynkelDiagnostic {     /* 32 bytes; message bytes follow all records */
  uint32_t command_index;             /* index into the submitted batch */
  uint32_t code;                      /* FynkelDiagnosticCode */
  uint64_t entity;                    /* 0 if not applicable */
  uint32_t component;                 /* FynkelComponent, 0 if n/a */
  uint32_t message_offset;            /* byte offset from buffer start */
  uint32_t message_length;
  uint32_t lane;                      /* FynkelCommandLane */
} FynkelDiagnostic;

/* ---- Call structs ------------------------------------------------------- */

typedef struct FynkelBufferView {
  const uint8_t* data;                /* NULL when length == 0 */
  uint32_t length;
} FynkelBufferView;

typedef struct FynkelWorldDesc {
  uint32_t initial_entity_capacity;
  uint32_t flags;                     /* reserved */
} FynkelWorldDesc;

typedef struct FynkelStepDesc {
  float fixed_dt_seconds;
  uint32_t flags;                     /* reserved */
} FynkelStepDesc;

typedef struct FynkelStepResult {
  uint64_t tick;                      /* tick number after the step */
  uint32_t commands_applied;
  uint32_t commands_rejected;
  uint32_t snapshot_records;
  uint32_t event_records;
  uint32_t diagnostic_records;
  uint32_t live_entities;
} FynkelStepResult;

/* ---- Functions ---------------------------------------------------------- */

uint32_t fynkel_abi_version(void);
/* Fingerprint of the component/record schemas this library was built with.
 * The Dart side compares it with the one its generated codecs carry. */
uint64_t fynkel_schema_fingerprint(void);

FynkelRuntime* fynkel_runtime_create(void);
void fynkel_runtime_destroy(FynkelRuntime* runtime);

FynkelWorld* fynkel_world_create(FynkelRuntime* runtime, const FynkelWorldDesc* desc);
void fynkel_world_destroy(FynkelWorld* world);

/* Validates and stages a command buffer. Simulation-lane commands apply on
 * the next step(); presentation-lane commands apply now. Returns PROTOCOL
 * without applying anything if the buffer does not validate. */
FynkelStatus fynkel_world_submit_commands(FynkelWorld* world,
                                          uint32_t lane,
                                          const uint8_t* buffer,
                                          uint32_t length);

/* Applies staged simulation commands, advances one fixed tick, then
 * rebuilds the snapshot (subscribed entities only), event and diagnostics
 * buffers. Returns OK or COMMANDS_REJECTED on a normal step. */
FynkelStatus fynkel_world_step(FynkelWorld* world,
                               const FynkelStepDesc* desc,
                               FynkelStepResult* out_result);

/* Subscribe/unsubscribe `count` entities' `component` for snapshots. */
FynkelStatus fynkel_world_subscribe(FynkelWorld* world,
                                    uint32_t component,
                                    const FynkelEntity* entities,
                                    uint32_t count,
                                    int32_t enable);

/* Borrowed views; see ownership rules at the top. */
FynkelBufferView fynkel_world_snapshot(const FynkelWorld* world);
FynkelBufferView fynkel_world_events(const FynkelWorld* world);
FynkelBufferView fynkel_world_diagnostics(const FynkelWorld* world);

/* Non-zero once a panic or fatal error was caught; every mutating call then
 * returns FYNKEL_STATUS_POISONED. */
int32_t fynkel_world_is_poisoned(const FynkelWorld* world);

#ifdef __cplusplus
}
#endif
#endif /* FYNKEL_H */
