/* Fynkel macOS viewport ABI v1 (platform/apple). Pairs with fynkel.h: the
 * same dylib exports both. See platform/apple/README.md for the contract. */
#ifndef FYNKEL_APPLE_H
#define FYNKEL_APPLE_H

#include <stdint.h>
#include "../../../crates/fynkel_native/include/fynkel.h"

#ifdef __cplusplus
extern "C" {
#endif

#define FYNKEL_APPLE_ABI_VERSION 2

typedef struct FynkelAppleViewport FynkelAppleViewport;

/* Slot completion signalling; 2 (Metal completion handler) is production. */
enum FynkelAppleSyncMode {
  FYNKEL_APPLE_SYNC_ASYNC = 0,
  FYNKEL_APPLE_SYNC_BLOCKING = 1,
  FYNKEL_APPLE_SYNC_METAL_COMPLETION = 2,
};

/* fynkel_apple_viewport_render: -1 = frame dropped (ring full), -2 = no world. */
#define FYNKEL_APPLE_RENDER_DROPPED (-1)
#define FYNKEL_APPLE_RENDER_NO_WORLD (-2)

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
} FynkelAppleViewportStats;

typedef void (*FynkelAppleFrameReadyFn)(void *userdata);

uint32_t fynkel_apple_abi_version(void);

/* Host-loop thread. */
FynkelAppleViewport *fynkel_apple_viewport_create(uint32_t width, uint32_t height,
                                                  uint32_t buffer_count, int32_t sync_mode);
void fynkel_apple_viewport_destroy(FynkelAppleViewport *vp);
int32_t fynkel_apple_viewport_resize(FynkelAppleViewport *vp, uint32_t width, uint32_t height);
uint32_t fynkel_apple_viewport_buffer_count(const FynkelAppleViewport *vp);
const void *fynkel_apple_viewport_iosurface(const FynkelAppleViewport *vp, uint32_t index);
int32_t fynkel_apple_viewport_render(FynkelAppleViewport *vp, const FynkelWorld *world);
void fynkel_apple_viewport_set_camera(FynkelAppleViewport *vp, const FynkelAppleCamera *camera);
void fynkel_apple_viewport_set_acquire_wait_us(const FynkelAppleViewport *vp, uint32_t us);
void fynkel_apple_viewport_set_frame_ready_callback(const FynkelAppleViewport *vp,
                                                    FynkelAppleFrameReadyFn func, void *userdata);
void fynkel_apple_viewport_take_stats(const FynkelAppleViewport *vp, FynkelAppleViewportStats *out);
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

/* Presenter (raster) thread: the only call it makes. */
int32_t fynkel_apple_viewport_acquire_presented(const FynkelAppleViewport *vp);

#ifdef __cplusplus
}
#endif
#endif
