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

#define FYNKEL_ABI_VERSION 4u
#define FYNKEL_BUFFER_MAGIC 0x4b4e5946u /* "FYNK" */
#define FYNKEL_BUFFER_HEADER_SIZE 24u

typedef struct FynkelRuntime FynkelRuntime;
typedef struct FynkelWorld FynkelWorld;

/* Entity handle: world id (8) | generation (24, never 0) | slot (32).
 * 0 is never valid. */
typedef struct FynkelEntity {
  uint64_t world;
  uint32_t slot;
  uint32_t generation;
} FynkelEntity;

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
  FYNKEL_STATUS_LOAD_FAILED = 8,
  /* The build or the platform lacks what the call needs (an output device
   * for fynkel_world_audio_open); the world stays usable. */
  FYNKEL_STATUS_UNSUPPORTED = 9
} FynkelStatus;

typedef enum FynkelBufferKind {
  /* SnapshotTransformRecord per subscribed entity, handle order. */
  FYNKEL_BUFFER_SNAPSHOT = 2,
  /* EventHeader + body per event (kind = event kind). */
  FYNKEL_BUFFER_EVENTS = 3,
  /* DiagnosticRecord per diagnostic; messages in the string tail. */
  FYNKEL_BUFFER_DIAGNOSTICS = 4,
  /* Query response; record type depends on the query kind. */
  FYNKEL_BUFFER_QUERY = 5,
  /* InputEventRecord per raw control event (host -> world; fynkel_world_capture_input). */
  FYNKEL_BUFFER_INPUT_EVENTS = 6,
  /* InputActionRecord per bound action (world -> host; fynkel_world_input_view). */
  FYNKEL_BUFFER_INPUT_ACTIONS = 7,
  /* PhysicsQueryRecord per spatial query (host -> world;
   * fynkel_world_physics_query); answered in a QUERY buffer of
   * PhysicsHitRecords. */
  FYNKEL_BUFFER_PHYSICS_QUERIES = 8,
  /* ProjectionAnchorRecord per anchor (host -> world; fynkel_world_project);
   * answered in a QUERY buffer of ProjectedPointRecords (M3-13). */
  FYNKEL_BUFFER_PROJECTION_ANCHORS = 9
} FynkelBufferKind;

typedef enum FynkelComponent {
  FYNKEL_COMPONENT_TRANSFORM = 1,
  FYNKEL_COMPONENT_HIERARCHY = 2,
  FYNKEL_COMPONENT_NAME = 3,
  FYNKEL_COMPONENT_MESH_RENDERER = 4,
  FYNKEL_COMPONENT_CAMERA = 5,
  FYNKEL_COMPONENT_DIRECTIONAL_LIGHT = 6,
  FYNKEL_COMPONENT_POINT_LIGHT = 7,
  FYNKEL_COMPONENT_SPOT_LIGHT = 8,
  FYNKEL_COMPONENT_ENVIRONMENT = 9,
  FYNKEL_COMPONENT_RIGID_BODY = 10,
  FYNKEL_COMPONENT_COLLIDER = 11,
  FYNKEL_COMPONENT_CHARACTER_CONTROLLER = 12,
  FYNKEL_COMPONENT_THIRD_PERSON_CAMERA = 13,
  /* The native component catalogue (blueprint §3.5, M3-10). */
  FYNKEL_COMPONENT_ROTATOR = 14,
  FYNKEL_COMPONENT_OSCILLATE = 15,
  FYNKEL_COMPONENT_FOLLOW = 16,
  FYNKEL_COMPONENT_LOOK_AT_CAMERA = 17,
  FYNKEL_COMPONENT_LIFETIME_DESPAWN = 18,
  FYNKEL_COMPONENT_EMITTER = 19,
  /* A sensor collider that names an event for Dart (M3-12). */
  FYNKEL_COMPONENT_TRIGGER_ZONE = 20,
  /* Animation (M3-19): a clip player over the named nodes below it, and
   * the skin a skinned mesh deforms with. */
  FYNKEL_COMPONENT_ANIMATOR = 21,
  FYNKEL_COMPONENT_SKIN = 22,
  /* Audio (M3-11): a clip with its playback settings, and the ears. */
  FYNKEL_COMPONENT_AUDIO_SOURCE = 23,
  FYNKEL_COMPONENT_AUDIO_LISTENER = 24
} FynkelComponent;

/* Event ids are the schema ids (schemas/fynkel.schema.json). */
typedef enum FynkelEventKind {
  /* 1 retired: temporary-ID spawn acknowledgements. */
  FYNKEL_EVENT_DESTROYED = 2,
  FYNKEL_EVENT_PARENT_CHANGED = 3,
  FYNKEL_EVENT_MISSED_DEADLINE = 4,
  FYNKEL_EVENT_SCENE_READY = 5,
  /* M3-12: a scene's entities were unloaded (after their DESTROYEDs). */
  FYNKEL_EVENT_SCENE_UNLOADED = 6,
  /* M3-12: a fynkel.TriggerZone's sensor saw a collider enter / leave
   * (`fynkel.ev.Zone` body: zone, other, event name). */
  FYNKEL_EVENT_ZONE_ENTERED = 7,
  FYNKEL_EVENT_ZONE_EXITED = 8,
  /* M3-19: a non-looping clip reached its end (`fynkel.ev.AnimationEnded`
   * body: entity, clip). */
  FYNKEL_EVENT_ANIMATION_ENDED = 9,
  /* M3-19c: an animator's clock crossed a clip marker
   * (`fynkel.ev.AnimationMarker` body: entity, clip, name). */
  FYNKEL_EVENT_ANIMATION_MARKER = 10,
  /* An AudioSource's non-looping clip reached its end (M3-11)
   * (`fynkel.ev.PlaybackEnded` body: entity, clip). */
  FYNKEL_EVENT_PLAYBACK_ENDED = 11,
  /* ADR-0032: a layer of an Animator Controller entered a state
   * (`fynkel.ev.AnimatorStateEntered` body: entity, layer, state, from). */
  FYNKEL_EVENT_ANIMATOR_STATE_ENTERED = 12,
  /* ADR-0032: a layer stopped playing a state, its crossfade out ended
   * (`fynkel.ev.AnimatorStateExited` body: entity, layer, state). */
  FYNKEL_EVENT_ANIMATOR_STATE_EXITED = 13,
  FYNKEL_EVENT_COLLISION_ENTER = 0x100,
  FYNKEL_EVENT_COLLISION_STAY = 0x101,
  FYNKEL_EVENT_COLLISION_EXIT = 0x102,
  FYNKEL_EVENT_TRIGGER_ENTER = 0x103,
  FYNKEL_EVENT_TRIGGER_EXIT = 0x104
} FynkelEventKind;

typedef enum FynkelQueryKind {
  /* entity -> one SnapshotTransformRecord (0 records if unknown). */
  FYNKEL_QUERY_TRANSFORM = 1,
  /* entity -> FynkelEntity handles of its children, deterministic order. */
  FYNKEL_QUERY_CHILDREN = 2,
  /* entity -> one FynkelEntity: parent handle or all zero. */
  FYNKEL_QUERY_PARENT = 3,
  /* entity -> one Name record (0 records if none). */
  FYNKEL_QUERY_NAME = 4,
  /* (no entity) -> FynkelEntity handles of every live entity, slot order. */
  FYNKEL_QUERY_ALL_ENTITIES = 5,
  /* entity -> one framed record per native component it has, id order:
   * a ComponentHeader { component, size } then the component's body
   * (0 records if unknown). */
  FYNKEL_QUERY_COMPONENTS = 6,
  /* entity -> one BodyStateRecord (0 records without a RigidBody). */
  FYNKEL_QUERY_BODY = 7,
  /* (no entity) -> one EntityInfoRecord { entity, parent, name, layer,
   * tags, active } per live entity, slot order: the hierarchy, the names,
   * the tags and the active flags in one call. */
  FYNKEL_QUERY_ENTITIES = 8,
  /* entity -> one CharacterStateRecord { entity, grounded, sliding,
   * velocity, collision_flags } (0 records without a CharacterController)
   * (M3-09; collision_flags foundation 2.4). */
  FYNKEL_QUERY_CHARACTER = 9,
  /* (no entity) -> one SystemTimeRecord { system, entities, micros } per
   * SystemKind: the last step's per-phase wall time (M3-10). */
  FYNKEL_QUERY_SYSTEMS = 10,
  /* entity -> one AnimatorStateRecord { entity, time, duration, speed,
   * fade_remaining, playing, looping, clip } (0 records without an
   * Animator) (M3-19). */
  FYNKEL_QUERY_ANIMATOR = 11,
  /* entity -> one AudioStateRecord { entity, playing, paused, time,
   * duration, volume, pitch, looping, clip, bus, voice } (0 records
   * without an AudioSource) (M3-11). */
  FYNKEL_QUERY_AUDIO = 12,
  /* (no entity) -> one AudioStatsRecord: the audio engine's device,
   * voices and counters (M3-11). */
  FYNKEL_QUERY_AUDIO_STATS = 13,
  /* entity -> one AnimatorLayerStateRecord per layer of the Animator
   * Controller it runs (0 records without a running controller)
   * (ADR-0032). */
  FYNKEL_QUERY_ANIMATOR_CONTROLLER = 14,
  /* entity -> one AnimatorParameterRecord per parameter of the Animator
   * Controller it runs (0 records without a running controller)
   * (ADR-0032). */
  FYNKEL_QUERY_ANIMATOR_PARAMETERS = 15
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
  float fixed_dt_seconds;             /* 0 = default 1/60 */
  uint32_t max_steps_per_frame;       /* 0 = default 4 */
  float max_frame_dt_seconds;         /* 0 = default 0.25 */
  uint32_t flags;                     /* reserved */
} FynkelWorldDesc;

typedef struct FynkelFramePlan {
  uint32_t steps;                     /* fixed ticks to run this frame */
  float dropped_seconds;              /* > 0: MissedDeadline recorded */
  uint32_t clamped;                   /* scaled dt exceeded max_frame_dt */
  float alpha;                        /* interpolation factor after the steps */
  double real_delta;
  double gameplay_delta;
  double real_elapsed;
  double gameplay_elapsed;
  double clamped_seconds;             /* excluded by gameplay clamp, not catch-up */
  uint64_t frame;
} FynkelFramePlan;

typedef struct FynkelStepResult {
  uint64_t tick;                      /* tick number after the step */
  double simulation_elapsed;
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
  uint64_t step_calls;
  uint64_t query_calls;
  uint64_t bytes_in;                  /* command bytes received */
  uint64_t bytes_out;                 /* snapshot+event+diagnostic+query bytes produced */
  uint32_t live_entities;
  uint32_t subscriptions;
  uint32_t poisoned;
  /* Memory and the subscription budget (M3-16, blueprint §15.4 / C5). */
  uint64_t snapshot_bytes;            /* the last step's snapshot */
  uint64_t snapshot_peak_bytes;       /* the largest snapshot so far */
  uint32_t snapshot_budget;           /* fynkel_world_set_snapshot_budget; 0 = none */
  uint32_t reserved;
  uint64_t native_bytes;              /* Rust heap in use, the whole engine's */
  uint64_t resident_bytes;            /* process resident set; 0 where not reported */
  uint64_t residency_bytes;           /* CPU bytes of the resident assets */
} FynkelWorldStats;

/* ---- Functions ---------------------------------------------------------- */

uint32_t fynkel_abi_version(void);
/* Fingerprint of the record schemas this library was built with; the Dart
 * codecs carry the one they were generated from. */
uint64_t fynkel_schema_fingerprint(void);

/* Identity authority ABI 1: opaque process-lifetime storage, never freed.
 * Bootstrap once in the primary module. The host retains the pointer across
 * engine unload/replacement and attaches it to every additional engine module
 * BEFORE runtime, authoring or world creation. Attach is idempotent for the
 * same authority; changing a selected authority returns INVALID_ARGUMENT.
 * Never serialize the pointer or share it between processes. */
typedef struct FynkelIdentityAuthority FynkelIdentityAuthority;
const FynkelIdentityAuthority* fynkel_identity_authority(void);
FynkelStatus fynkel_identity_attach(const FynkelIdentityAuthority* authority);

FynkelRuntime* fynkel_runtime_create(void);
void fynkel_runtime_destroy(FynkelRuntime* runtime);

int32_t fynkel_world_contains(const FynkelWorld* world, FynkelEntity entity);
double fynkel_world_simulation_elapsed(const FynkelWorld* world);

FynkelWorld* fynkel_world_create(FynkelRuntime* runtime, const FynkelWorldDesc* desc, FynkelStatus* out_status);
void fynkel_world_destroy(FynkelWorld* world);

/* Starts a host frame: accepts real dt, samples scale, clamps gameplay once,
 * advances absolute host timelines and decides the number of fixed ticks,
 * advances the canonical frame clock. */
FynkelStatus fynkel_world_begin_frame(FynkelWorld* world,
                                      double dt_seconds,
                                      double time_scale,
                                      FynkelFramePlan* out_plan);

/* One fixed tick: delivers committed effects, runs the native
 * phases, publishes the snapshot (subscribed entities only), events and
 * diagnostics. Returns OK or COMMANDS_REJECTED on a normal step. */
FynkelStatus fynkel_world_step(FynkelWorld* world, FynkelStepResult* out_result);

/* After a completed manual fixed sequence, display its current pose without
 * interpolation lag. Does not advance clocks or change the accumulator.
 * Call before presentation; normal interpolation resumes on the next tick. */
FynkelStatus fynkel_world_snap_interpolation(FynkelWorld* world);

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

/* Input (blueprint §5.1, §19.2d item 7). The world's binding table comes
 * from the cooked project's inputActions on load. A host captures its raw
 * control events (keys, mouse buttons and movement, gamepad, touch
 * overlay) once per frame, before the ticks, as InputEventRecords; the
 * view is one InputActionRecord per action: held state every tick sees,
 * `pressed`/`released` edges the next fynkel_world_step consumes (a
 * catch-up tick sees held state only; a frame with no tick carries them),
 * `frame_pressed`/`frame_released` the current frame's raw edges for
 * update/lateUpdate. The view is valid until the next call or mutating
 * call. */
FynkelStatus fynkel_world_capture_input(FynkelWorld* world,
                                        const uint8_t* buffer,
                                        uint32_t length);
FynkelBufferView fynkel_world_input_view(FynkelWorld* world);

/* Pull-only events (ADR-0008): moves pending general-lane events plus the
 * last tick's physics events into the view; a second call returns an empty
 * buffer until more events are produced. */
FynkelBufferView fynkel_world_drain_events(FynkelWorld* world);
/* Physics events of the tick just stepped, for afterPhysics; not drained. */
FynkelBufferView fynkel_world_after_physics_events(const FynkelWorld* world);

/* Explicit query; the response is valid until the next mutating call or
 * the next query of the same kind (each kind answers into its own buffer,
 * so views of different kinds can be held together). */
FynkelStatus fynkel_world_query(FynkelWorld* world,
                                const FynkelQueryDesc* desc,
                                FynkelBufferView* out_view);

/* Spatial queries (M3-08): `buffer` is a PHYSICS_QUERIES buffer; the
 * response (a QUERY buffer of PhysicsHitRecords, query order, nearest
 * first within a query) is valid until the next physics query or
 * mutating call.
 * A framing error is FYNKEL_STATUS_PROTOCOL with an empty view. */
FynkelStatus fynkel_world_physics_query(FynkelWorld* world,
                                        const uint8_t* buffer,
                                        uint32_t length,
                                        FynkelBufferView* out_view);

/* Projected world anchors (M3-13, blueprint §10). At most this many
 * anchors project per request; the rest come back PROJECTED_DROPPED and
 * the world reports TOO_MANY_ANCHORS (15). */
#define FYNKEL_MAX_PROJECTION_ANCHORS 64u

/* ProjectedPointRecord.flags bits. */
#define FYNKEL_PROJECTED_IN_FRONT 1u   /* in front of the near plane; x/y meaningful */
#define FYNKEL_PROJECTED_ON_SCREEN 2u  /* in front and inside the viewport */
#define FYNKEL_PROJECTED_OCCLUDED 4u   /* a collider on the anchor's mask is in the way */
#define FYNKEL_PROJECTED_MISSING 8u    /* the entity is dead or has no transform */
#define FYNKEL_PROJECTED_NO_CAMERA 16u /* the world has no enabled camera */
#define FYNKEL_PROJECTED_DROPPED 32u   /* beyond FYNKEL_MAX_PROJECTION_ANCHORS */

typedef struct FynkelProjectionInfo {
  FynkelEntity camera;    /* the entity rendered from; 0 = no camera */
  uint32_t projected; /* anchors projected */
  uint32_t dropped;   /* anchors beyond the bound */
} FynkelProjectionInfo;

/* Projects a PROJECTION_ANCHORS buffer into a width x height viewport
 * (the frame's camera and interpolated poses, the renderer's matrices):
 * the response is a QUERY buffer of one ProjectedPointRecord per anchor
 * in request order (x/y in that viewport's pixels, origin top-left, y
 * down; depth metres along the view axis; flags above), valid until the
 * next projection or mutating call. out_info/out_view may be NULL. A framing
 * error is FYNKEL_STATUS_PROTOCOL with an empty view. */
FynkelStatus fynkel_world_project(FynkelWorld* world,
                                  uint32_t width,
                                  uint32_t height,
                                  const uint8_t* buffer,
                                  uint32_t length,
                                  FynkelProjectionInfo* out_info,
                                  FynkelBufferView* out_view);

/* ---- Audio (M3-11) ------------------------------------------------------ */

/* Immediate one-shot: one 40-byte PlaySound record followed by its string tail.
 * Not allowed inside a gameplay edit. Does not flush queued simulation effects. */
FynkelStatus fynkel_world_audio_play(FynkelWorld* world, const uint8_t* buffer, uint32_t length);

/* Opens the default output device and moves the world's mixer into its
 * callback: what a host that plays calls once after the world is created
 * (a scene may be loaded before or after). FYNKEL_STATUS_UNSUPPORTED when
 * the library was built without a device backend or no device could be
 * opened; the world reports `audio_device` in its diagnostics and keeps
 * playing silently on its null backend, so a game runs the same without
 * sound. Calling it again with a device open is a no-op. */
FynkelStatus fynkel_world_audio_open(FynkelWorld* world);

/* Pauses (suspended != 0) or resumes the output device: the host calls it
 * from its lifecycle (the app going to the background). A no-op without a
 * device. */
FynkelStatus fynkel_world_audio_suspend(FynkelWorld* world, uint32_t suspended);

/* Closes the output device; the world continues on the null backend. A
 * no-op without a device. */
FynkelStatus fynkel_world_audio_close(FynkelWorld* world);

FynkelStatus fynkel_world_stats(const FynkelWorld* world, FynkelWorldStats* out_stats);

/* The subscription budget (M3-16, blueprint C5): a ceiling in bytes on a
 * tick's snapshot. The first step whose snapshot exceeds it reports
 * `snapshot_budget` (diagnostic code 20, FYN-R0013) once — again only after
 * the snapshot came back under; the snapshot is still delivered whole.
 * 0 removes the ceiling. */
FynkelStatus fynkel_world_set_snapshot_budget(FynkelWorld* world, uint32_t bytes);

uint64_t fynkel_world_tick(const FynkelWorld* world);
float fynkel_world_alpha(const FynkelWorld* world);

/* Non-zero once a panic or fatal error was caught; every mutating call then
 * returns FYNKEL_STATUS_POISONED. */
int32_t fynkel_world_is_poisoned(const FynkelWorld* world);
/* The message (UTF-8) of the panic that poisoned the world, empty when
 * none did (foundation 4.1); the host reports it once and stops. */
FynkelBufferView fynkel_world_panic_message(const FynkelWorld* world);
/* Panics inside the world on purpose (Unity's Utils.ForceCrash), to test
 * a host's crash path: FYNKEL_STATUS_PANIC and the world is poisoned. */
FynkelStatus fynkel_world_debug_panic(FynkelWorld* world);

/* Cooked scenes (blueprint §6, §7; M3-03). A host reads the cache
 * `fynkel cook` wrote and never a source document. `cache_utf8` (UTF-8, not
 * NUL-terminated) is the cache directory (`<project>/.fynkel/cache`).
 * `fynkel_world_load_project` loads the startup scene the cache's index
 * names (a project naming none opens with no scene loaded and the world
 * left as it is: its input, layers, tags and gravity bound, `scene` null
 * in the report; ADR-0026); `fynkel_world_load_scene` loads the cooked scene with asset id
 * `scene_id_utf8` (36-char UUID text). Entities are spawned immediately
 * with their authoring ids kept for the report; other components go
 * through the command path and any rejection is in
 * fynkel_world_diagnostics. Both **replace** (M3-12): every loaded scene
 * is unloaded and every other entity destroyed first (DESTROYED events,
 * one SCENE_UNLOADED per scene, then the new scene's SCENE_READY), and
 * every residency token is dropped before the new scene's closure is
 * retained. The scene's closure of assets stays resident in the world's
 * store (a viewport rendering this world draws from it) until the scene
 * is unloaded, replaced, or the world destroyed. FYNKEL_STATUS_LOAD_FAILED
 * when the cache is missing or stale, the scene is not in it or the
 * hierarchy has errors (FYN-R#### diagnostics; the world is unchanged);
 * FYNKEL_STATUS_INVALID_ARGUMENT for NULL, non-UTF-8 or non-UUID input. */
FynkelStatus fynkel_world_load_project(FynkelWorld* world,
                                       const uint8_t* cache_utf8,
                                       uint32_t cache_len);
FynkelStatus fynkel_world_load_scene(FynkelWorld* world,
                                     const uint8_t* cache_utf8,
                                     uint32_t cache_len,
                                     const uint8_t* scene_id_utf8,
                                     uint32_t scene_id_len);
/* fynkel_world_load_scene with `flags` (M3-12): 0 replaces as above;
 * FYNKEL_LOAD_ADDITIVE keeps the world and refuses a scene id it already
 * holds (FYNKEL_STATUS_LOAD_FAILED, FYN-R0011). Other bits are
 * FYNKEL_STATUS_INVALID_ARGUMENT. The report's "scenes" lists what the
 * world holds afterwards. */
#define FYNKEL_LOAD_ADDITIVE 1u
FynkelStatus fynkel_world_load_scene_with(FynkelWorld* world,
                                          const uint8_t* cache_utf8,
                                          uint32_t cache_len,
                                          const uint8_t* scene_id_utf8,
                                          uint32_t scene_id_len,
                                          uint32_t flags);
/* Unloads a loaded scene (M3-12): its still-alive entities are destroyed
 * with their subtrees (DESTROYED events, then SCENE_UNLOADED with the
 * count) and its closure released from residency; entities gameplay
 * spawned or instantiated since stay. FYNKEL_STATUS_LOAD_FAILED with
 * FYN-R0010 when the world does not hold the scene. The report:
 * {"ok", "unloaded": {"id", "name", "entities"}, "scenes": [...],
 * "residency": {...}}. */
FynkelStatus fynkel_world_unload_scene(FynkelWorld* world,
                                       const uint8_t* scene_id_utf8,
                                       uint32_t scene_id_len);
/* DontDestroyOnLoad (ADR-0037): keeps the root `entity` and its subtree
 * across loads — a replace load leaves it, and so does unloading the scene
 * it came from; what it uses stays resident. Children made under it later
 * go with it; given a parent it stops being kept. The load report counts
 * the survivors ("persistent"). FYNKEL_STATUS_INVALID_ARGUMENT for an
 * entity that is not live or has a parent. */
FynkelStatus fynkel_world_dont_destroy_on_load(FynkelWorld* world,
                                               FynkelEntity entity);
/* Loading a scene in the background (ADR-0038). _async starts reading
 * the cooked scene `scene_id_utf8` from `cache_utf8` and making every
 * asset it uses resident on a background thread while the world runs on;
 * it writes a ticket (never 0). A cache that does not open is
 * FYNKEL_STATUS_LOAD_FAILED with the report. _progress writes the state
 * (FYNKEL_LOAD_LOADING with the share of assets loaded, FYNKEL_LOAD_READY
 * with 1, FYNKEL_LOAD_FAILED). _activate spawns a ready load's scene as
 * fynkel_world_load_scene_with does, with the same flags and report, from
 * memory; a failed load is FYNKEL_STATUS_LOAD_FAILED with its diagnostics.
 * Either way the ticket is spent. _cancel drops a load not activated.
 * A load still loading, an unknown ticket or unknown flags are
 * FYNKEL_STATUS_INVALID_ARGUMENT. Destroying the world drops its loads. */
#define FYNKEL_LOAD_LOADING 0u
#define FYNKEL_LOAD_READY 1u
#define FYNKEL_LOAD_FAILED 2u
FynkelStatus fynkel_world_load_scene_async(FynkelWorld* world,
                                           const uint8_t* cache_utf8,
                                           uint32_t cache_len,
                                           const uint8_t* scene_id_utf8,
                                           uint32_t scene_id_len,
                                           uint32_t* out_ticket);
FynkelStatus fynkel_world_load_progress(FynkelWorld* world,
                                        uint32_t ticket,
                                        uint32_t* out_state,
                                        float* out_progress);
FynkelStatus fynkel_world_load_activate(FynkelWorld* world,
                                        uint32_t ticket,
                                        uint32_t flags);
FynkelStatus fynkel_world_load_cancel(FynkelWorld* world, uint32_t ticket);
/* Input rebinds (M3-12; the `input.rebinds` of settings.json): `json_utf8`
 * is a JSON object {"<action>": ["<device>:<control>", ...], ...} whose
 * bindings replace the loaded project's for those actions (an empty
 * object clears every rebind). The world's action table is rebound at
 * once and the rebinds stay over every later load. The report's "input"
 * lists the actions (with "rebound": bool) and "bindingErrors" — a bad
 * binding is reported, never fatal. FYNKEL_STATUS_LOAD_FAILED before any
 * project is loaded; FYNKEL_STATUS_INVALID_ARGUMENT when the text is not
 * such an object. */
FynkelStatus fynkel_world_bind_actions(FynkelWorld* world,
                                       const uint8_t* json_utf8,
                                       uint32_t json_len);
/* Runtime prefab instantiation (M3-08b, blueprint §6 item 12). Spawns,
 * now, the cooked prefab `prefab_id_utf8` (36-char UUID text): its root
 * (ADR-0022) named `name_utf8` (empty: the prefab's name) with the given
 * local transform under `parent` (0 = root), its other entities under it
 * — the shape a scene instance expands to. The prefab comes from the cache
 * the last successful load opened (FYNKEL_STATUS_LOAD_FAILED before any);
 * its closure is retained with the scene's. Components go through the
 * command path; the root and the entities are in the report:
 * {"ok", "instance": {"prefab", "name", "path", "root": hex128 string,
 * "entities": n, "rejectedComponents", "assets", "behaviours": [...]},
 * "diagnostics": [...]}. FYNKEL_STATUS_LOAD_FAILED when the prefab is not
 * in the cache, does not decode (recook) or `parent` is not live (the
 * world is unchanged). */
typedef struct FynkelInstantiateDesc {
  const uint8_t* prefab_id_utf8;
  uint32_t prefab_id_len;
  const uint8_t* name_utf8;           /* NULL with name_len 0 for none */
  uint32_t name_len;
  FynkelEntity parent;                /* 0: a root */
  float position[3];
  float rotation[4];                  /* x, y, z, w */
  float scale[3];
} FynkelInstantiateDesc;
FynkelStatus fynkel_world_instantiate(FynkelWorld* world,
                                      const FynkelInstantiateDesc* desc);
/* A copy of a live object (ADR-0026 §4, Unity's `Instantiate(gameObject)`,
 * `GameObject.clone` in Dart). Spawns, now, a copy of the subtree under
 * `source`: every entity's name, own active flag, child order and
 * built-in components, as the world holds them (commands not yet stepped
 * are not in it). The copy's root is named `name_utf8` (empty: the
 * source's name) with the given local transform under `parent` (0 = a
 * root); the others keep their local transforms. Components go through
 * the command path, as an instantiated prefab's. No project needed. The
 * report: {"ok", "clone": {"source": hex128 string, "root": hex128 string, "entities":
 * [[source, copy], ...] (breadth first, the root first),
 * "rejectedComponents"}, "diagnostics": [...]}. FYNKEL_STATUS_LOAD_FAILED
 * when `source` or `parent` is not live or the transform is not finite
 * (the world is unchanged). */
typedef struct FynkelCloneDesc {
  FynkelEntity source;
  const uint8_t* name_utf8;           /* NULL with name_len 0 for none */
  uint32_t name_len;
  FynkelEntity parent;                /* 0: a root */
  float position[3];
  float rotation[4];                  /* x, y, z, w */
  float scale[3];
} FynkelCloneDesc;
FynkelStatus fynkel_world_clone(FynkelWorld* world, const FynkelCloneDesc* desc);
/* A material made at runtime (ADR-0026 §4, `world.createMaterial`):
 * `payload` is a MATL chunk payload — u32 version (1), u32 record size,
 * u32 tail length, u32 reserved, the `fynkel.PbrMaterial` wire record, its
 * string tail — drawn as the material `id` (16 bytes, big-endian UUID; one
 * the cache does not have, or a cooked material's to replace it) by every
 * renderer on the world's asset store until `payload_len` 0 removes it.
 * Kept across scene loads. FYNKEL_STATUS_LOAD_FAILED before any project is
 * loaded; FYNKEL_STATUS_INVALID_ARGUMENT for a payload that does not
 * decode (another build's record layout). */
FynkelStatus fynkel_world_set_material(FynkelWorld* world,
                                       const uint8_t* id,
                                       const uint8_t* payload,
                                       uint32_t payload_len);
/* A shader asset material made at runtime (ADR-0031,
 * `world.createShaderMaterial`): `json_utf8` is a material document as a
 * `.material.json` holds it, its `shader` a `.shader` asset's id — checked
 * against the shader's header as the cook checks it (types, ranges, alpha
 * modes, required textures), then drawn as the material `id` (16 bytes,
 * big-endian UUID; one the cache does not have, or a cooked material's to
 * replace it) by every renderer on the world's asset store until
 * `json_len` 0 removes it. Setting it again with new values alone rewrites
 * its uniform. Kept across scene loads. `*out` is empty, or the
 * diagnostics as a JSON array ([{"code", "severity", "message",
 * "pointer"?, ...}], warnings too), valid until the next call on this
 * world. FYNKEL_STATUS_LOAD_FAILED before any project is loaded or when
 * the cache has no such shader; FYNKEL_STATUS_INVALID_ARGUMENT when the
 * material does not fit its shader (the world's material is unchanged). */
FynkelStatus fynkel_world_set_shader_material(FynkelWorld* world,
                                              const uint8_t* id,
                                              const uint8_t* json_utf8,
                                              uint32_t json_len,
                                              FynkelBufferView* out);
/* A mesh made at runtime (ADR-0026 §4, `world.createMesh`): `payload` is a
 * static MESH chunk payload — u32 vertex count, u32 index count, 6 × f32
 * bounds (recomputed here), the 72-byte vertices (position, normal,
 * tangent, uv0, uv1, colour), the u32 triangle-list indices — drawn as the
 * mesh `id` (16 bytes, big-endian UUID, no sub-asset path: one the cache
 * does not have) by every renderer on the world's asset store until
 * `payload_len` 0 removes it; the `shape: mesh` colliders naming it are
 * rebuilt from it at the next step (MESH_UNAVAILABLE once removed). Kept
 * across scene loads. FYNKEL_STATUS_LOAD_FAILED before any project is
 * loaded; FYNKEL_STATUS_INVALID_ARGUMENT for a payload that does not
 * decode, has skin weights, no triangles, an index out of range or a
 * value that is not finite. */
FynkelStatus fynkel_world_set_mesh(FynkelWorld* world,
                                   const uint8_t* id,
                                   const uint8_t* payload,
                                   uint32_t payload_len);

/* A clip made at runtime (ADR-0033 § 2, `world.createClip`): `payload` is
 * an ANIM chunk payload — u32 channel count, f32 duration, per channel
 * the target node's name (u32 length + UTF-8, zero-padded to 4 bytes),
 * u32 property (0 translation, 1 rotation, 2 scale), u32 interpolation (0
 * step, 1 linear, 2 cubic spline), u32 key count, the key times (f32),
 * the values (f32, 3 or 4 per key, × 3 for a cubic spline); then
 * optionally u32 marker count and per marker f32 time + name (as above) —
 * registered as the clip `id` (16 bytes, big-endian UUID, no sub-asset
 * path: one the cache does not have) that `Animator`s, one-shots and
 * blend spaces play like a cooked one, until `payload_len` 0 removes it
 * (animators already playing it keep it). A changed clip plays from the
 * next tick. Kept across scene loads. FYNKEL_STATUS_LOAD_FAILED before
 * any project is loaded; FYNKEL_STATUS_INVALID_ARGUMENT for a payload
 * that does not decode or a clip that is not valid (keys out of order or
 * past the duration, a value that is not finite, markers out of order). */
FynkelStatus fynkel_world_set_clip(FynkelWorld* world,
                                   const uint8_t* id,
                                   const uint8_t* payload,
                                   uint32_t payload_len);

/* The cooked JSON of a document asset (ADR-0027 §6): the text of asset
 * `id` (16 bytes, big-endian UUID; no sub-asset) as the cook wrote it — a
 * data asset's `{ "$schema", "formatVersion", "id", "name", "type",
 * "version", "data" }`, or any other document kind's — read from the
 * world's cache (the one the last load opened) whether or not a loaded
 * scene retains it. `*out` is the UTF-8 text, valid until the next call
 * on this world. FYNKEL_STATUS_LOAD_FAILED before any project is loaded,
 * or for an id the cache does not have or that is not a document; `*out`
 * then holds the diagnostic as JSON ({"code", "severity", "message",
 * ...}). */
FynkelStatus fynkel_world_asset_json(FynkelWorld* world,
                                     const uint8_t* id,
                                     FynkelBufferView* out);

/* UTF-8 JSON describing the last load, unload, instantiate or bind call
 * on this world (empty before any): {"ok": bool, "mode": "replace" |
 * "additive"?, "scene": {"id", "name", "path", "key", "entities",
 * "rejectedComponents", "assets", "behaviours": [...]} | null,
 * "scenes": [{"id", "name", "entities"}]?, "unloaded": {...}?,
 * "instance": {...}?, "cache": {"dir", "profile", "project",
 * "startupScene"} | null, "residency": {"retained", "meshes", "textures",
 * "materials", "bytes"}?, "input": {"actions", "bindingErrors"}?,
 * "diagnostics": [{"code", "severity", "message", "document"?,
 * "pointer"?, ...}]}. Valid until the next such call. */
FynkelBufferView fynkel_world_load_report(const FynkelWorld* world);

/* F2 synchronous owned gameplay edit sessions, protocol version 1.
 * begin locks ordinary mutating/query calls until end. Such calls reject and
 * fail the edit. Snapshot/clock reads retain committed values. Destroy aborts.
 * Exchange framing: docs/design/foundation-gameplay-wire.md.
 * Errors return UTF-8 text; success returns operation-specific bytes.
 * Views expire at the next edit call. sequence is world-local and never reused.
 * End(commit=0) aborts; end(commit=1) validates and atomically publishes.
 */
/* Read-only lifecycle snapshot, outside edits: live count u32, followed by
 * (entity, name byte count u32, UTF-8 name). Errors return UTF-8 text. */
FynkelStatus fynkel_world_gameplay_state(FynkelWorld* world, FynkelBufferView* out_view);
/* Capture interpolation history before fixed callbacks; cancel an abandoned tick. */
FynkelStatus fynkel_world_gameplay_tick_prepare(FynkelWorld* world);
FynkelStatus fynkel_world_gameplay_tick_cancel(FynkelWorld* world);
FynkelStatus fynkel_world_gameplay_begin(FynkelWorld* world, uint64_t* out_sequence);
FynkelStatus fynkel_world_gameplay_exchange(FynkelWorld* world, uint64_t sequence,
    const uint8_t* data, uint32_t len, FynkelBufferView* out_view);
FynkelStatus fynkel_world_gameplay_end(FynkelWorld* world, uint64_t sequence,
    uint32_t commit, FynkelBufferView* out_view);

#ifdef __cplusplus
}
#endif
#endif /* FYNKEL_H */
