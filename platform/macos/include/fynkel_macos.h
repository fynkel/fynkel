/* Fynkel macOS viewport ABI v1 (platform/macos). Pairs with fynkel.h: the
 * same dylib exports both. See platform/macos/README.md for the contract. */
#ifndef FYNKEL_MACOS_H
#define FYNKEL_MACOS_H

#include <stdint.h>
#include "../../../crates/fynkel_native/include/fynkel.h"

#ifdef __cplusplus
extern "C" {
#endif

#define FYNKEL_MACOS_ABI_VERSION 1

typedef struct FynkelMacosViewport FynkelMacosViewport;

/* Slot completion signalling; 2 (Metal completion handler) is production. */
enum FynkelMacosSyncMode {
  FYNKEL_MACOS_SYNC_ASYNC = 0,
  FYNKEL_MACOS_SYNC_BLOCKING = 1,
  FYNKEL_MACOS_SYNC_METAL_COMPLETION = 2,
};

/* fynkel_macos_viewport_render: -1 = frame dropped (ring full), -2 = no world. */
#define FYNKEL_MACOS_RENDER_DROPPED (-1)
#define FYNKEL_MACOS_RENDER_NO_WORLD (-2)

typedef struct FynkelMacosCamera {
  float eye[3];
  float target[3];
  float up[3];
  float fov_y_radians;
  float near_plane;
  float far_plane;
} FynkelMacosCamera;

typedef struct FynkelMacosViewportStats {
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
} FynkelMacosViewportStats;

typedef void (*FynkelMacosFrameReadyFn)(void *userdata);

uint32_t fynkel_macos_abi_version(void);

/* Host-loop thread. */
FynkelMacosViewport *fynkel_macos_viewport_create(uint32_t width, uint32_t height,
                                                  uint32_t buffer_count, int32_t sync_mode);
void fynkel_macos_viewport_destroy(FynkelMacosViewport *vp);
int32_t fynkel_macos_viewport_resize(FynkelMacosViewport *vp, uint32_t width, uint32_t height);
uint32_t fynkel_macos_viewport_buffer_count(const FynkelMacosViewport *vp);
const void *fynkel_macos_viewport_iosurface(const FynkelMacosViewport *vp, uint32_t index);
int32_t fynkel_macos_viewport_render(FynkelMacosViewport *vp, const FynkelWorld *world);
void fynkel_macos_viewport_set_camera(FynkelMacosViewport *vp, const FynkelMacosCamera *camera);
void fynkel_macos_viewport_set_acquire_wait_us(const FynkelMacosViewport *vp, uint32_t us);
void fynkel_macos_viewport_set_frame_ready_callback(const FynkelMacosViewport *vp,
                                                    FynkelMacosFrameReadyFn func, void *userdata);
void fynkel_macos_viewport_take_stats(const FynkelMacosViewport *vp, FynkelMacosViewportStats *out);

/* Presenter (raster) thread: the only call it makes. */
int32_t fynkel_macos_viewport_acquire_presented(const FynkelMacosViewport *vp);

#ifdef __cplusplus
}
#endif
#endif
