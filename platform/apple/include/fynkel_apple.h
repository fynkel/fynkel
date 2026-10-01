/* Fynkel macOS viewport ABI v12 (platform/apple). Pairs with fynkel.h: the
 * same dylib exports both. See platform/apple/README.md for the contract.
 * v3 added the M3-16 instrumentation fields of FynkelAppleViewportStats; v4
 * fynkel_apple_viewport_debug_lines and fynkel_apple_viewport_read_frame (the
 * editor's overlays and Save Frame, M4-06); v5 gpuAo (R2); v6
 * fynkel_apple_viewport_overlay (the gizmo overlay, ADR-0019); v7
 * fynkel_apple_viewport_set_view (a viewport's own camera over any world:
 * the editor's Scene view on a running game, ADR-0021 P7 L5); v8
 * fynkel_apple_viewport_outline (the selection outline); v9
 * fynkel_apple_viewport_extra_worlds (other worlds drawn into the frame:
 * Prefab Mode in context, multi-scene editing); v10 no call changed: every
 * viewport shares one GPU device (ADR-0030 §4). */
#ifndef FYNKEL_APPLE_H
#define FYNKEL_APPLE_H

#include <stdint.h>
#include "../../../crates/fynkel_native/include/fynkel.h"

#ifdef __cplusplus
extern "C" {
#endif

#define FYNKEL_APPLE_ABI_VERSION 12

typedef struct FynkelAppleViewport FynkelAppleViewport;

/* Slot completion signalling; 2 (Metal completion handler) is production. */
enum FynkelAppleSyncMode {
  FYNKEL_APPLE_SYNC_ASYNC = 0,
  FYNKEL_APPLE_SYNC_BLOCKING = 1,
  FYNKEL_APPLE_SYNC_METAL_COMPLETION = 2,
};

/* fynkel_apple_viewport_render: -1 = frame dropped (ring full), -2 = no world,
 * -3 = the viewport is broken (below). */
#define FYNKEL_APPLE_RENDER_DROPPED (-1)
#define FYNKEL_APPLE_RENDER_NO_WORLD (-2)
#define FYNKEL_APPLE_RENDER_BROKEN (-3)

typedef struct FynkelAppleCamera {
  float eye[3];
  float target[3];
  float up[3];
  float fov_y_radians;
  float near_plane;
  float far_plane;
} FynkelAppleCamera;

typedef struct FynkelAppleViewportStats {
  uint64_t frames_rendered;
  uint64_t frames_dropped_no_free_buffer;
  uint64_t frames_presented;
  uint64_t presents_without_new_frame;
  uint32_t cpu_p50_us, cpu_p95_us, cpu_p99_us;
  uint32_t gpu_complete_p50_us, gpu_complete_p95_us, gpu_complete_p99_us;
  uint64_t resizes;
  uint32_t resize_p50_us, resize_max_us;
  uint32_t present_lag_p50, present_lag_p95, present_lag_max;
  uint32_t acquire_wait_p50_us, acquire_wait_p95_us, acquire_wait_max_us;
  uint32_t instances_last_frame;
  uint32_t frame_ready_signals;
  /* Last frame: cooked meshes drawn, placeholder cubes, meshes missing
   * from the cache; assets that failed to load since the cache was set;
   * whether the scene's own Camera drove the view. */
  uint32_t meshes_last_frame, placeholders_last_frame, missing_last_frame;
  uint32_t asset_errors;
  uint32_t scene_camera;
  /* Last frame (M3-05): blend instances, lights kept / dropped, sun shadow
   * casters and the map side (0 when none), samples per pixel, the
   * internal render size. */
  uint32_t transparent_last_frame;
  uint32_t lights, lights_dropped;
  uint32_t shadow_casters, shadow_map;
  uint32_t msaa;
  uint32_t render_width, render_height;
  /* Last frame (M3-06): instances outside the camera frustum (not drawn),
   * mesh draw calls in the scene pass, draw calls in the shadow pass. */
  uint32_t culled_last_frame, draws_last_frame, shadow_draws_last_frame;
  /* Instrumentation (M3-16, blueprint §15.4), over the window: the world's
   * extract, the renderer's prepare (upload + cull) and encode (pass
   * recording) on the render call — cpu_* above is encode + submit —; the
   * GPU's pass times from Metal timestamps when the device samples them
   * (gpu_timestamps; the values trail the draw by three or four frames and
   * count only the frames read back, gpu_frames_timed): shadow, scene, post
   * (+ present), and the frame's span from the first pass's begin to the
   * last pass's end (the passes overlap on a tile-based GPU, so the sum
   * exceeds it); the renderer's GPU memory estimate. */
  uint32_t extract_p50_us, extract_p95_us, extract_p99_us;
  uint32_t prepare_p50_us, prepare_p95_us, prepare_p99_us;
  uint32_t encode_p50_us, encode_p95_us, encode_p99_us;
  uint32_t gpu_timestamps, gpu_frames_timed, gpu_frames_untimed;
  uint32_t gpu_shadow_p50_us, gpu_shadow_p95_us, gpu_shadow_p99_us;
  uint32_t gpu_ao_p50_us, gpu_ao_p95_us, gpu_ao_p99_us;
  uint32_t gpu_scene_p50_us, gpu_scene_p95_us, gpu_scene_p99_us;
  uint32_t gpu_post_p50_us, gpu_post_p95_us, gpu_post_p99_us;
  uint32_t gpu_frame_p50_us, gpu_frame_p95_us, gpu_frame_p99_us;
  uint64_t gpu_bytes;
} FynkelAppleViewportStats;

typedef void (*FynkelAppleFrameReadyFn)(void *userdata);

uint32_t fynkel_apple_abi_version(void);

/* Host-loop thread. Every viewport of the process draws with one shared GPU
 * device (v10); FYNKEL_VIEWPORT_DEVICE=own gives each its own. */
FynkelAppleViewport *fynkel_apple_viewport_create(uint32_t width, uint32_t height,
                                                  uint32_t buffer_count, int32_t sync_mode);
void fynkel_apple_viewport_destroy(FynkelAppleViewport *vp);
int32_t fynkel_apple_viewport_resize(FynkelAppleViewport *vp, uint32_t width, uint32_t height);
uint32_t fynkel_apple_viewport_buffer_count(const FynkelAppleViewport *vp);
const void *fynkel_apple_viewport_iosurface(const FynkelAppleViewport *vp, uint32_t index);
int32_t fynkel_apple_viewport_render(FynkelAppleViewport *vp, const FynkelWorld *world);
void fynkel_apple_viewport_set_camera(FynkelAppleViewport *vp, const FynkelAppleCamera *camera);

/* A viewport's own camera: the scene Camera component's projection. */
typedef struct {
  uint32_t projection;    /* 0 perspective, 1 orthographic */
  float fov_y;            /* radians */
  float ortho_height;     /* metres */
  float near_clip;
  float far_clip;
  uint32_t exposure_mode; /* 0 auto, 1 manual */
  float exposure;         /* EV100 (manual) */
  uint32_t culling_mask;  /* layer bits drawn */
} FynkelAppleViewCamera;

/* Draws every frame of this viewport through its own camera — at `world`
 * (16 floats, column-major) with `camera`'s projection — instead of the
 * rendered world's active camera; `world` NULL clears it. Returns 0, or -1
 * for a NULL viewport, a NULL camera with a world, or values that cannot
 * draw. Nothing in the world changes. Host-loop thread. */
int32_t fynkel_apple_viewport_set_view(FynkelAppleViewport *vp, const float *world,
                                       const FynkelAppleViewCamera *camera);
void fynkel_apple_viewport_set_acquire_wait_us(const FynkelAppleViewport *vp, uint32_t us);
void fynkel_apple_viewport_set_frame_ready_callback(const FynkelAppleViewport *vp,
                                                    FynkelAppleFrameReadyFn func, void *userdata);
void fynkel_apple_viewport_take_stats(const FynkelAppleViewport *vp, FynkelAppleViewportStats *out);
/* ABI 11 (foundation 4.3): a panic inside any viewport call is caught at
 * this boundary (it would abort the process). The viewport is broken from
 * then on: every call fails (render answers FYNKEL_APPLE_RENDER_BROKEN,
 * the others their failure value), except set_frame_ready_callback, which
 * still detaches, and destroy. `error` copies the panic's message (UTF-8,
 * cut at `cap`) and answers the bytes written, 0 while the viewport works.
 * `debug_panic` panics on purpose, for tests of a host's broken path. */
int32_t fynkel_apple_viewport_error(const FynkelAppleViewport *vp, uint8_t *out, uint32_t cap);
int32_t fynkel_apple_viewport_debug_panic(FynkelAppleViewport *vp);
/* Cooked assets (`fynkel cook` cache directory, UTF-8, not NUL-terminated;
 * NULL/empty clears). 0 on success, -1 when it is not a readable cache.
 * Rendering a world that loaded a scene from a cache adopts that world's
 * asset store, so hosts on loadProject/loadScene never call this. */
int32_t fynkel_apple_viewport_set_asset_cache(FynkelAppleViewport *vp,
                                              const uint8_t *dir_utf8, uint32_t dir_len);
/* Entities without a MeshRenderer draw as placeholder cubes (default on). */
void fynkel_apple_viewport_set_placeholders(FynkelAppleViewport *vp, int32_t enabled);
/* The quality profile (fynkel.project.json `quality.profiles`) the renderer
 * runs with, by name; NULL/empty = the project's default. Applied by the
 * next render. -1 when the store being drawn from has profiles and none
 * has that name (the default stays). */
int32_t fynkel_apple_viewport_set_quality(FynkelAppleViewport *vp,
                                          const uint8_t *name_utf8, uint32_t name_len);
/* Queues `count` world-space line segments for the NEXT render — ten floats
 * each, ax ay az bx by bz r g b a (metres; linear RGBA) — drawn by the
 * renderer's debug pass: depth-tested against the opaque scene, unlit,
 * cleared by every render (queue again before each frame). Returns the
 * segments queued (a non-finite segment is dropped), -1 for NULL. */
int32_t fynkel_apple_viewport_debug_lines(FynkelAppleViewport *vp,
                                          const float *lines, uint32_t count);
/* Queues the NEXT render's overlay (ADR-0019): drawn after post at the
 * target's resolution, never exposed. lines: 12 floats each — ax ay az bx by
 * bz r g b a width on_top (metres; linear RGBA; width in target pixels;
 * on_top != 0 draws over the scene instead of fading behind it). triangles:
 * 14 each — ax ay az bx by bz cx cy cz r g b a on_top. grid: NULL or 18
 * floats — minor, major, X-axis, Z-axis RGBA, cell (0 adaptive), fade (0
 * adaptive). Cleared by every render. Returns lines + triangles queued (a
 * non-finite one is dropped), -1 for NULL. */
int32_t fynkel_apple_viewport_overlay(FynkelAppleViewport *vp,
                                      const float *lines, uint32_t line_count,
                                      const float *triangles, uint32_t triangle_count,
                                      const float *grid);
/* Queues the NEXT render's selection outline: the silhouettes of what the
 * `selected` entities draw ringed in one colour, their `children`'s in
 * another, over everything (hidden parts too; overlay handles stay on top).
 * Entities are handle bits (the authoring preview's `entities` call, the
 * live world's `handles`); one in both lists is selected. style: NULL for
 * the default (orange, blue, 2 px) or 9 floats — selected RGBA, children
 * RGBA (linear), width in target pixels. Cleared by every render; a later
 * call replaces an earlier one. Returns the entities queued, -1 for a NULL
 * viewport, a NULL list with a count or a non-finite style. */
int32_t fynkel_apple_viewport_outline(FynkelAppleViewport *vp,
                                      const FynkelEntity *selected, uint32_t selected_count,
                                      const FynkelEntity *children, uint32_t children_count,
                                      const float *style);
/* Bit 0 of an extra world's flags: draw it greyed (Unity's Prefab Mode
 * context in "Gray").  */
#define FYNKEL_APPLE_EXTRA_WORLD_GRAY 1u
/* Queues other worlds for the NEXT render, drawn into the same frame as the
 * world `fynkel_apple_viewport_render` is given (the main world): Prefab
 * Mode in context (the scene around the prefab, transformed by the inverse
 * of the instance's world matrix) and multi-scene editing (the other loaded
 * scenes, identity). Each world is extracted by this call — call it right
 * before the render — so a world may be destroyed after it returns.
 * transforms: 16 floats per world (column-major), the extra world's frame
 * → the main world's; flags: one per world (FYNKEL_APPLE_EXTRA_WORLD_GRAY:
 * desaturated to its Rec. 709 luminance and halved; other bits ignored).
 * Their meshes share the frame's depth buffer and shadow maps (they cast
 * and receive), their point and spot lights join the frame's (placed by
 * the transform), their skinned meshes are posed as in their world; their
 * first directional light is the sun only when the main world has none;
 * the main world's camera (or the viewport's view), Environment, volumes
 * and post stay the frame's. They draw from the viewport's asset store
 * (the main world's cache: every preview of one authoring session shares
 * it). Never outlined. Cleared by every render; a later call replaces an
 * earlier one (count 0 clears). A NULL or poisoned world is skipped.
 * Returns the worlds queued, -1 for a NULL viewport, a NULL array with a
 * count or a non-finite transform (nothing is queued). */
int32_t fynkel_apple_viewport_extra_worlds(FynkelAppleViewport *vp,
                                           const FynkelWorld *const *worlds,
                                           const float *transforms,
                                           const uint32_t *flags, uint32_t count);
/* Copies the frame in ring slot `slot` (a render's return value) into `out`
 * as tightly packed RGBA8, top row first: width * height * 4 bytes at the
 * viewport's size. Blocks until the GPU is idle — a screenshot, never a
 * per-frame call. Returns the bytes written, -1 for NULL, a bad slot or a
 * `cap` smaller than the frame. */
int32_t fynkel_apple_viewport_read_frame(const FynkelAppleViewport *vp, int32_t slot,
                                         uint8_t *out, uint32_t cap);

/* Presenter (raster) thread: the only call it makes. */
int32_t fynkel_apple_viewport_acquire_presented(const FynkelAppleViewport *vp);

#ifdef __cplusplus
}
#endif
#endif
